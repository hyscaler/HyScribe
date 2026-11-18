// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Embed`: URLs alone on their line or in their paragraph turned into
//! embeds (`autoembed()`), each through `WP_Embed::shortcode()` with its
//! cache in the post's `_oembed_` meta, written as PHP writes it in primary
//! mode only. A page view's `usecache` serves a cached embed however old,
//! so `oembed_ttl` only matters when a post is saved. While fetches are
//! off, an uncached URL stays as it is, as PHP leaves one whose fetch
//! failed, and nothing is written.
//!
//! Not ported: the audio and video handlers (a URL ending in a media
//! extension stays as it is), the `oembed_cache` posts of content without
//! a post, and embeds of the site's own posts; none of them fetch.

mod oembed;

use hyscribe_kses::url_host;
use hyscribe_phpser::{Key, Value, is_empty_string, md5, urlencode};

use crate::autop::replace_in_html_tags;
use crate::outbound::Size;
use crate::template::{Renderer, replace};

use oembed::{Got, wp_oembed_get};

/// What a failed embed caches.
const UNKNOWN: &[u8] = b"{{unknown}}";

/// `wp_embed_defaults()` without a `$content_width`.
const DEFAULTS: Size = Size {
    width: 500,
    height: 750,
};

/// `wp_get_audio_extensions()` and `wp_get_video_extensions()`.
const MEDIA: [&[u8]; 10] = [
    b"mp3", b"ogg", b"flac", b"m4a", b"wav", b"mp4", b"m4v", b"webm", b"ogv", b"flv",
];

/// A pattern's match at a position: the ends of its leading part, its URL
/// and the whole.
type Found = Option<(usize, usize, usize)>;

/// PCRE's `\s`.
pub(crate) fn is_space(byte: u8) -> bool {
    matches!(byte, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

/// The length of `https?://` at the start of `text`, ignoring case.
fn scheme(text: &[u8]) -> Option<usize> {
    [&b"http://"[..], b"https://"]
        .into_iter()
        .find(|scheme| {
            text.get(..scheme.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(scheme))
        })
        .map(<[u8]>::len)
}

/// The end of `https?://[^\s<>"]+` at `at` in `text`, ignoring case.
fn url_end(text: &[u8], at: usize) -> Option<usize> {
    let rest = text.get(at..).unwrap_or_default();
    let scheme = scheme(rest)?;
    let length = rest
        .get(scheme..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&b| !is_space(b) && !matches!(b, b'<' | b'>' | b'"'))
        .count();
    (length > 0).then_some(at + scheme + length)
}

/// The run of `\s` from `at`.
fn spaces(text: &[u8], at: usize) -> usize {
    let run = text.get(at..).unwrap_or_default();
    at + run.iter().take_while(|&&b| is_space(b)).count()
}

/// `|^(\s*)(https?://[^\s<>"]+)(\s*)$|im` at `start`, a line's start: the
/// ends of the leading space, the URL and the match.
fn alone(text: &[u8], start: usize) -> Found {
    let before = start.checked_sub(1).and_then(|before| text.get(before));
    let line = before.is_none_or(|&b| b == b'\n');
    let lead = spaces(text, start);
    let url = url_end(text, lead).filter(|_| line)?;
    let trail = spaces(text, url);
    // `$` holds at the end or before a newline; the greedy run gives back
    // what follows its last newline otherwise.
    let end = match text.get(trail) {
        None | Some(b'\n') => trail,
        Some(_) => {
            let run = text.get(url..trail).unwrap_or_default();
            url + run.iter().rposition(|&b| b == b'\n')?
        }
    };
    Some((lead, url, end))
}

/// `|(<p(?: [^>]*)?>\s*)(https?://[^\s<>"]+)(\s*<\/p>)|i` at `start`.
fn paragraph(text: &[u8], start: usize) -> Found {
    let rest = text.get(start..).unwrap_or_default();
    if !rest.get(..2)?.eq_ignore_ascii_case(b"<p") {
        return None;
    }
    let open = match rest.get(2)? {
        b'>' => start + 3,
        b' ' => {
            let attributes = rest.get(3..).unwrap_or_default();
            start + 4 + attributes.iter().position(|&b| b == b'>')?
        }
        _ => return None,
    };
    let lead = spaces(text, open);
    let url = url_end(text, lead)?;
    let close = spaces(text, url);
    let end = text.get(close..).unwrap_or_default();
    let closed = end
        .get(..4)
        .is_some_and(|tag| tag.eq_ignore_ascii_case(b"</p>"));
    closed.then_some((lead, url, close + 4))
}

/// The video ID `#https?://(www.)?youtube\.com/(?:v|embed)/([^/]+)#i`
/// finds first in `url`.
fn youtube_id(url: &[u8]) -> Option<&[u8]> {
    (0..url.len()).find_map(|at| {
        let rest = url.get(at..).unwrap_or_default();
        let after = rest.get(scheme(rest)?..).unwrap_or_default();
        // `(www.)?` takes four bytes when what follows still matches.
        let www = after
            .get(..3)
            .is_some_and(|w| w.eq_ignore_ascii_case(b"www"));
        let tries = [after.get(4..).filter(|_| www), Some(after)];
        tries.into_iter().flatten().find_map(|rest| {
            let host = rest
                .get(..12)
                .filter(|h| h.eq_ignore_ascii_case(b"youtube.com/"))?;
            let path = rest.get(host.len()..).unwrap_or_default();
            let lower = path.to_ascii_lowercase();
            let skip = [&b"v/"[..], b"embed/"]
                .into_iter()
                .find(|p| lower.starts_with(p))?;
            let id = path.get(skip.len()..).unwrap_or_default();
            let id = id.split(|&b| b == b'/').next().unwrap_or_default();
            (!id.is_empty()).then_some(id)
        })
    })
}

/// `/<blockquote\s[^>]*?wp-embedded-content/`
fn embedded_quote(html: &[u8]) -> bool {
    html.windows(11).enumerate().any(|(at, w)| {
        let tag = html.get(at + 12..).unwrap_or_default();
        let inside = tag.split(|&b| b == b'>').next().unwrap_or_default();
        w == b"<blockquote"
            && html.get(at + 11).is_some_and(|&b| is_space(b))
            && inside.windows(19).any(|w| w == b"wp-embedded-content")
    })
}

impl Renderer<'_> {
    /// `WP_Embed::autoembed()`.
    pub(crate) fn autoembed(&mut self, content: &[u8]) -> Vec<u8> {
        const BREAK: &[u8] = b"<!-- wp-line-break -->";
        let content = replace_in_html_tags(content, b"\n", BREAK);
        // PHP first looks for `#(^|\s|>)https?://#i`, which only saves work.
        let lines = self.each(&content, alone);
        let paragraphs = self.each(&lines, paragraph);
        replace(&paragraphs, BREAK, b"\n")
    }

    /// `preg_replace_callback()` of a pattern `matcher` finds at a position,
    /// with `autoembed_callback()`.
    fn each(&mut self, text: &[u8], matcher: fn(&[u8], usize) -> Found) -> Vec<u8> {
        let mut out = Vec::with_capacity(text.len());
        let mut copied = 0;
        // A match ends past its URL, so the next search starts later.
        while let Some((lead, url, end)) = (copied..text.len()).find_map(|at| matcher(text, at)) {
            out.extend(text.get(copied..lead).unwrap_or_default());
            out.extend(self.embed(text.get(lead..url).unwrap_or_default()));
            out.extend(text.get(url..end).unwrap_or_default());
            copied = end;
        }
        out.extend(text.get(copied..).unwrap_or_default());
        out
    }

    /// `WP_Embed::shortcode( array(), $url )` with `linkifunknown` off: the
    /// embed, or the URL when there is none.
    fn embed(&mut self, url: &[u8]) -> Vec<u8> {
        let url = replace(url, b"&amp;", b"&");
        if let Some(handled) = self.handler(&url) {
            return handled;
        }
        let site = self.context.site;
        let home = url_host(&site.home_url());
        let own = home.is_some() && url_host(&url) == home;
        let (Some(post), false) = (self.context.post_id, own) else {
            return url;
        };
        let attr = Value::Array(vec![
            (Key::String(b"width".to_vec()), Value::Int(DEFAULTS.width)),
            (Key::String(b"height".to_vec()), Value::Int(DEFAULTS.height)),
        ]);
        let suffix = md5(&[&url[..], &hyscribe_phpser::encode(&attr)].concat());
        let key = format!("_oembed_{suffix}");
        let cache = site.post_meta(post, &key).unwrap_or_default();
        if cache == UNKNOWN {
            return url;
        }
        if !is_empty_string(&cache) {
            return self.embed_html(cache, &url, post);
        }
        match wp_oembed_get(site, &url, DEFAULTS) {
            Got::Off => url,
            Got::Html(html) => {
                let now = self.context.request.now.to_string();
                site.update_post_meta(post, &key, &html);
                site.update_post_meta(post, &format!("_oembed_time_{suffix}"), now.as_bytes());
                self.embed_html(html, &url, post)
            }
            Got::Failed => {
                site.update_post_meta(post, &key, UNKNOWN);
                url
            }
        }
    }

    /// `apply_filters( 'embed_oembed_html', ... )` with core's
    /// `wp_maybe_enqueue_oembed_host_js()`.
    fn embed_html(&mut self, html: Vec<u8>, url: &[u8], post: u64) -> Vec<u8> {
        let hooks = self.context.site.hooks();
        let html = hooks.embed_oembed_html(html, url, DEFAULTS, post);
        if embedded_quote(&html) {
            self.assets.enqueue_script("wp-embed");
        }
        html
    }

    /// `get_embed_handler_html()`: the YouTube embed URL handler's embed of
    /// the video's page; a media URL, whose handlers are not ported, as is.
    fn handler(&mut self, url: &[u8]) -> Option<Vec<u8>> {
        if let Some(id) = youtube_id(url) {
            let page = [&b"https://youtube.com/watch?v="[..], &urlencode(id)].concat();
            return Some(self.autoembed(&page));
        }
        // `#^https?://.+?\.(mp3|...)$#i`
        let start = scheme(url).unwrap_or_default();
        let body = url.get(start..).unwrap_or_default().to_ascii_lowercase();
        let media = MEDIA.iter().any(|extension| {
            let dot = body.len().saturating_sub(extension.len() + 1);
            dot > 0 && body.ends_with(extension) && body.get(dot) == Some(&b'.')
        });
        media.then(|| url.to_vec())
    }
}

#[cfg(test)]
mod tests;
