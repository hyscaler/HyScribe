// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_audio_shortcode()` and `wp_video_shortcode()` with MediaElement as
//! their library, on the front end (`is_admin()` is false, and a block
//! theme sets no `$content_width`).

use std::sync::LazyLock;

use hyscribe_kses::{esc_attr, esc_html, esc_url, wp_parse_url};
use hyscribe_model::{OrderBy, Post, PostQuery, StatusFilter};
use hyscribe_phpser::{Key, Value, absint, add_query_arg, is_empty_string, remove_query_arg};

use super::{att, validate_boolean};
use crate::attachment::attachment_url;
use crate::attachment::prepend::{AUDIO, VIDEO_TYPES};
use crate::query_loop::query_limit;
use crate::template::{Renderer, replace};

/// Which player a shortcode makes.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Audio,
    Video,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Audio => "audio",
            Kind::Video => "video",
        }
    }

    /// `wp_get_audio_extensions()` or `wp_get_video_extensions()`, before
    /// their filters.
    fn extensions(self) -> Vec<String> {
        let list: Vec<&str> = match self {
            Kind::Audio => AUDIO.iter().map(|(ext, _)| *ext).collect(),
            Kind::Video => VIDEO_TYPES.iter().map(|(ext, _)| *ext).collect(),
        };
        list.into_iter().map(str::to_string).collect()
    }
}

/// The types of `wp_get_mime_types()` a visitor may upload, by their
/// extensions in `ext|ext` form, in order.
static MIME_TYPES: LazyLock<Vec<(Vec<String>, String)>> = LazyLock::new(|| {
    let json = include_bytes!("../../../../wordpress/wp-includes/mime-types.json");
    let types = hyscribe_phpser::json_decode(json).unwrap_or_default();
    let entries = types.as_array().unwrap_or_default();
    let split = |key: &Key| {
        let key = String::from_utf8_lossy(&key.to_php_string()).into_owned();
        key.split('|').map(str::to_string).collect()
    };
    let text = |value: &Value| String::from_utf8_lossy(&value.to_php_string()).into_owned();
    entries
        .iter()
        .map(|(key, mime)| (split(key), text(mime)))
        .collect()
});

/// `strtolower( wp_check_filetype( $file )['ext'] )`, empty for none: the
/// extension a name ends with, before one final line feed `$` allows.
fn extension(file: &[u8]) -> String {
    crate::attachment::prepend::extension(file.strip_suffix(b"\n").unwrap_or(file))
}

/// The type `wp_check_filetype( $file )` gives a file, none for a file of
/// no known extension.
fn mime_of(file: &[u8]) -> &'static str {
    let ext = extension(file);
    MIME_TYPES
        .iter()
        .find(|(extensions, _)| extensions.contains(&ext))
        .map_or("", |(_, mime)| mime.as_str())
}

impl Renderer<'_> {
    /// `wp_audio_shortcode( $attr, $content )`; nothing for `null`.
    pub(super) fn wp_audio_shortcode(
        &mut self,
        attr: &[(Key, Value)],
        content: Option<&[u8]>,
    ) -> Vec<u8> {
        let instance = self.audio_instances.get() + 1;
        self.audio_instances.set(instance);
        self.media_shortcode(Kind::Audio, attr, content.unwrap_or_default(), instance)
    }

    /// `wp_video_shortcode( $attr, $content )`; nothing for `null`.
    pub(super) fn wp_video_shortcode(
        &mut self,
        attr: &[(Key, Value)],
        content: Option<&[u8]>,
    ) -> Vec<u8> {
        self.video_instances += 1;
        let instance = self.video_instances;
        self.media_shortcode(Kind::Video, attr, content.unwrap_or_default(), instance)
    }

    /// The first attachment of `kind` attached to post `post_id`, as
    /// `get_attached_media()` lists them.
    fn attached_media(&self, kind: Kind, post_id: u64) -> Option<Post> {
        let query = PostQuery {
            post_types: vec!["attachment".into()],
            statuses: vec![StatusFilter {
                status: "inherit".into(),
                author: None,
            }],
            mime_types: vec![format!("{}/%", kind.name())],
            parents: vec![post_id],
            limit: query_limit(-1),
            order: vec![(OrderBy::MenuOrder, true)],
            ..PostQuery::default()
        };
        self.context.site.query(&query).into_iter().next()
    }

    fn media_shortcode(
        &mut self,
        kind: Kind,
        attr: &[(Key, Value)],
        content: &[u8],
        instance: u64,
    ) -> Vec<u8> {
        let site = self.context.site;
        let hooks = site.hooks();
        let name = kind.name();
        let post_id = self.context.post_id.filter(|id| site.post(*id).is_some());
        let post_id = post_id.unwrap_or(0);
        let overridden = hooks.media_shortcode_override(name, attr, content, instance);
        if !overridden.is_empty() {
            return overridden;
        }
        let mut types = hooks.media_extensions(name, kind.extensions());
        let text = |value: &str| Value::String(value.as_bytes().to_vec());
        let mut pairs: Vec<(&str, Value)> = vec![("src", text(""))];
        match kind {
            Kind::Audio => pairs.extend([
                ("loop", text("")),
                ("autoplay", text("")),
                ("muted", text("false")),
                ("preload", text("none")),
                ("class", text("wp-audio-shortcode")),
                ("style", text("width: 100%;")),
            ]),
            Kind::Video => pairs.extend([
                ("poster", text("")),
                ("loop", text("")),
                ("autoplay", text("")),
                ("muted", text("false")),
                ("preload", text("metadata")),
                ("width", Value::Int(640)),
                ("height", Value::Int(360)),
                ("class", text("wp-video-shortcode")),
            ]),
        }
        pairs.extend(types.iter().map(|ext| (ext.as_str(), text(""))));
        let mut atts = self.shortcode_atts(name, &pairs, attr);
        let get = |atts: &[(String, Value)], key: &str| att(atts, key).to_php_string();
        let mut vimeo = false;
        let mut youtube = false;
        let mut primary = false;
        if !is_empty_string(&get(&atts, "src")) {
            let src = get(&atts, "src");
            if kind == Kind::Video {
                vimeo = is_vimeo(&src);
                youtube = is_youtube(&src);
            }
            if !youtube && !vimeo && !types.contains(&extension(&src)) {
                let link = format!(
                    "<a class=\"wp-embedded-{name}\" href=\"{}\">{}</a>",
                    String::from_utf8_lossy(&esc_url(&src)),
                    esc_html(&src)
                );
                return link.into_bytes();
            }
            if vimeo {
                self.assets.enqueue_script("mediaelement-vimeo");
            }
            primary = true;
            types.insert(0, "src".into());
        } else {
            for ext in &types {
                let file = get(&atts, ext);
                if !is_empty_string(&file) && extension(&file) == *ext {
                    primary = true;
                }
            }
        }
        let mut attachment = None;
        if !primary {
            let Some(found) = self.attached_media(kind, post_id) else {
                return Vec::new();
            };
            let Some(url) = attachment_url(site, found.id).filter(|url| !is_empty_string(url))
            else {
                return Vec::new();
            };
            set_att(&mut atts, "src", Value::String(url));
            attachment = Some(found);
            types.insert(0, "src".into());
        }
        let library = hooks.media_shortcode_library(name, "mediaelement".into());
        let mediaelement = library == "mediaelement";
        if mediaelement {
            self.assets.enqueue_style("mediaelement");
            self.assets.enqueue_style("wp-mediaelement");
            self.assets.enqueue_script("wp-mediaelement");
            if kind == Kind::Video {
                self.assets.enqueue_script("mediaelement-vimeo");
            }
        }
        if mediaelement && kind == Kind::Video {
            let src = get(&atts, "src");
            if youtube {
                // Without the `feature` argument, over HTTPS.
                let src = https(&remove_query_arg(&[b"feature"], &src));
                set_att(&mut atts, "src", Value::String(src));
            } else if vimeo {
                let parts = wp_parse_url(&src).unwrap_or_default();
                let looped = if att(&atts, "loop").is_empty() {
                    "0"
                } else {
                    "1"
                };
                let bare = [
                    &b"https://"[..],
                    &parts.host.unwrap_or_default(),
                    &parts.path.unwrap_or_default(),
                ]
                .concat();
                let src = add_query_arg(&[(b"loop", Some(looped.as_bytes()))], &bare);
                set_att(&mut atts, "src", Value::String(src));
            }
        }
        let class = hooks.media_shortcode_class(name, get(&atts, "class"), &atts);
        set_att(&mut atts, "class", Value::String(class.clone()));
        let html = self.player_html(
            kind,
            &atts,
            &types,
            (post_id, instance),
            mediaelement,
            content,
        );
        hooks.media_shortcode(name, html, &atts, attachment.as_ref(), post_id, &library)
    }

    /// The `<audio>` or `<video>` element, with the sources of `types`.
    fn player_html(
        &self,
        kind: Kind,
        atts: &[(String, Value)],
        types: &[String],
        (post_id, instance): (u64, u64),
        mediaelement: bool,
        content: &[u8],
    ) -> Vec<u8> {
        let hooks = self.context.site.hooks();
        let name = kind.name();
        let flag = |key: &str| validate_boolean(att(atts, key));
        let class = att(atts, "class").to_php_string();
        let mut strings = Vec::new();
        // A video leaves out an empty class; an audio player keeps it.
        if kind == Kind::Audio || !is_empty_string(&class) {
            strings.push(format!("class=\"{}\"", esc_attr(&class)));
        }
        strings.push(format!("id=\"{name}-{post_id}-{instance}\""));
        if kind == Kind::Video {
            for key in ["width", "height"] {
                let value = absint(att(atts, key));
                if value != 0 {
                    strings.push(format!("{key}=\"{value}\""));
                }
            }
            let poster = esc_url(&att(atts, "poster").to_php_string());
            if !is_empty_string(&poster) {
                strings.push(format!("poster=\"{}\"", esc_attr(&poster)));
            }
        }
        for key in ["loop", "autoplay", "muted"] {
            if flag(key) {
                strings.push(key.to_string());
            }
        }
        let preload = att(atts, "preload").to_php_string();
        if !is_empty_string(&preload)
            && [&b"none"[..], b"metadata", b"auto"].contains(&&preload[..])
        {
            strings.push(format!("preload=\"{}\"", esc_attr(&preload)));
        }
        if kind == Kind::Audio {
            let style = att(atts, "style").to_php_string();
            strings.push(format!("style=\"{}\"", esc_attr(&style)));
        }
        let mut html = format!("<{name} {} controls=\"controls\">", strings.join(" ")).into_bytes();
        let mut fileurl = Vec::new();
        for ext in types {
            let file = att(atts, ext).to_php_string();
            if is_empty_string(&file) {
                continue;
            }
            if fileurl.is_empty() {
                fileurl = file.clone();
            }
            let mime = match (ext.as_str(), &file) {
                ("src", _) if is_youtube(&file) => "video/youtube",
                ("src", _) if is_vimeo(&file) => "video/vimeo",
                _ => mime_of(&file),
            };
            let url = add_query_arg(&[(b"_", Some(instance.to_string().as_bytes()))], &file);
            html.extend(
                format!(
                    "<source type=\"{mime}\" src=\"{}\" />",
                    String::from_utf8_lossy(&esc_url(&url))
                )
                .into_bytes(),
            );
        }
        if kind == Kind::Video && !is_empty_string(content) {
            let content = if content.contains(&b'\n') {
                // `str_replace( array( "\r\n", "\n", "\t" ), '', $content )`.
                let content = replace(content, b"\r\n", b"");
                replace(&replace(&content, b"\n", b""), b"\t", b"")
            } else {
                content.to_vec()
            };
            html.extend_from_slice(hyscribe_phpser::php_trim(&content));
        }
        if mediaelement {
            let link = String::from_utf8_lossy(&esc_url(&fileurl)).into_owned();
            let fallback = format!("<a href=\"{link}\">{link}</a>").into_bytes();
            html.extend(hooks.wp_mediaelement_fallback(fallback, &fileurl));
        }
        html.extend_from_slice(format!("</{name}>").as_bytes());
        if kind == Kind::Video {
            let width = att(atts, "width");
            let rule = if width.is_empty() {
                String::new()
            } else {
                format!("width: {}px;", width.to_int())
            };
            return [
                format!("<div style=\"{rule}\" class=\"wp-video\">").as_bytes(),
                &html,
                b"</div>",
            ]
            .concat();
        }
        html
    }
}

/// `$atts[ $name ] = $value`, in place.
fn set_att(atts: &mut [(String, Value)], name: &str, value: Value) {
    for (_, found) in atts.iter_mut().filter(|(known, _)| known == name) {
        *found = value.clone();
    }
}

/// `preg_match( '#^https?://(?:www\.)?(?:youtube\.com/watch|youtu\.be/)#', $src )`.
fn is_youtube(src: &[u8]) -> bool {
    let rest = src
        .strip_prefix(b"https://")
        .or_else(|| src.strip_prefix(b"http://"));
    let rest = rest.map(|rest| rest.strip_prefix(b"www.").unwrap_or(rest));
    rest.is_some_and(|rest| {
        rest.starts_with(b"youtube.com/watch") || rest.starts_with(b"youtu.be/")
    })
}

/// `preg_match( '#^https?://(.+\.)?vimeo\.com/.*#', $src )`.
fn is_vimeo(src: &[u8]) -> bool {
    let Some(rest) = src
        .strip_prefix(b"https://")
        .or_else(|| src.strip_prefix(b"http://"))
    else {
        return false;
    };
    if rest.starts_with(b"vimeo.com/") {
        return true;
    }
    // `.+\.` then the host: one character at least, and no line feed.
    let needle = b".vimeo.com/";
    rest.windows(needle.len()).enumerate().any(|(at, window)| {
        window == needle && at > 0 && !rest.get(..at).unwrap_or_default().contains(&b'\n')
    })
}

/// `set_url_scheme( $url, 'https' )` of a URL that begins with `http://` or
/// `https://`.
fn https(url: &[u8]) -> Vec<u8> {
    let plain = url.strip_prefix(b"http://");
    let rest = plain.or_else(|| url.strip_prefix(b"https://"));
    [&b"https://"[..], rest.unwrap_or_default()].concat()
}
