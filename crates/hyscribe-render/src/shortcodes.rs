// SPDX-License-Identifier: AGPL-3.0-or-later

//! `do_shortcode()` over core's shortcodes: `[caption]` and `[wp_caption]`
//! (`img_caption_shortcode()`), `[gallery]` (`gallery_shortcode()`),
//! `[audio]` and `[video]`. A block theme has `html5` support for captions
//! and galleries, so only their HTML5 markup is made. `[playlist]` and
//! `[embed]` (`WP_Embed::run_shortcode()`) are not ported: they stay as
//! written, as a shortcode without a callback does. A shortcode inside an
//! HTML attribute stays as written too.

mod gallery;
mod media;

pub(crate) use gallery::wp_parse_id_list;

use hyscribe_kses::{esc_attr, sanitize_html_class};
use hyscribe_phpser::{Key, Value, is_empty_string};

use crate::formatting::{CORE_SHORTCODE_TAGS, find};
use crate::kses::wp_kses_post;
use crate::shortcode::{
    MAX_DEPTH, Shortcode, escape_brackets_in_elements, positions, replace_shortcodes,
    shortcode_atts, shortcode_parse_atts, tags_in, unescape_invalid_shortcodes,
};
use crate::template::Renderer;

/// The text of an attribute set by name.
pub(crate) fn att<'a>(atts: &'a [(String, Value)], name: &str) -> &'a Value {
    static NULL: Value = Value::Null;
    atts.iter()
        .find(|(known, _)| known == name)
        .map_or(&NULL, |(_, value)| value)
}

/// The value of attribute `name` as the shortcode gave it, if it did.
pub(crate) fn given<'a>(attr: &'a [(Key, Value)], name: &str) -> Option<&'a Value> {
    let key = Key::from_php_string(name.as_bytes().to_vec());
    attr.iter().find(|(known, _)| *known == key).map(|(_, v)| v)
}

/// `wp_validate_boolean( $value )`.
pub(crate) fn validate_boolean(value: &Value) -> bool {
    let text = value.to_php_string();
    !text.eq_ignore_ascii_case(b"false") && !is_empty_string(&text)
}

impl Renderer<'_> {
    /// `do_shortcode( $content )`.
    pub(crate) fn do_shortcode(&mut self, content: &[u8]) -> Vec<u8> {
        if self.shortcode_depth >= MAX_DEPTH {
            return content.to_vec();
        }
        let tags = tags_in(&CORE_SHORTCODE_TAGS, content);
        if tags.is_empty() {
            return content.to_vec();
        }
        // The brackets of HTML elements are escaped for the replacing, so
        // shortcodes in attributes stay as written.
        let content = escape_brackets_in_elements(content);
        self.shortcode_depth += 1;
        let content = replace_shortcodes(&content, &tags, &mut |shortcode| {
            self.do_shortcode_tag(shortcode)
        });
        self.shortcode_depth -= 1;
        unescape_invalid_shortcodes(&content)
    }

    /// `do_shortcode_tag( $m )`.
    fn do_shortcode_tag(&mut self, m: &Shortcode<'_>) -> Vec<u8> {
        if m.escaped() {
            return m.unwrapped().to_vec();
        }
        let hooks = self.context.site.hooks();
        let attr = shortcode_parse_atts(m.atts);
        if let Some(output) = hooks.pre_do_shortcode_tag(m.tag, &attr, m) {
            return output;
        }
        let output = match self.shortcode_output(m.tag, &attr, m.content) {
            Some(output) => output,
            // A tag with no callback: `[playlist]` and `[embed]`.
            None => return m.whole.to_vec(),
        };
        let open: &[u8] = if m.open { b"[" } else { b"" };
        let close: &[u8] = if m.close { b"]" } else { b"" };
        let output = [open, &output, close].concat();
        hooks.do_shortcode_tag(output, m.tag, &attr, m)
    }

    /// What the callback of shortcode `tag` returns, `None` for a tag
    /// without one.
    fn shortcode_output(
        &mut self,
        tag: &[u8],
        attr: &[(Key, Value)],
        content: Option<&[u8]>,
    ) -> Option<Vec<u8>> {
        Some(match tag {
            b"wp_caption" | b"caption" => self.img_caption_shortcode(attr, content),
            b"gallery" => self.gallery_shortcode(attr.to_vec()),
            b"audio" => self.wp_audio_shortcode(attr, content),
            b"video" => self.wp_video_shortcode(attr, content),
            _ => return None,
        })
    }

    /// `shortcode_atts( $pairs, $attr, $shortcode )`, filtered.
    pub(crate) fn shortcode_atts(
        &self,
        shortcode: &str,
        pairs: &[(&str, Value)],
        attr: &[(Key, Value)],
    ) -> Vec<(String, Value)> {
        let out = shortcode_atts(pairs, attr);
        self.context
            .site
            .hooks()
            .shortcode_atts(shortcode, out, pairs, attr)
    }

    /// `wp_unique_prefixed_id( $prefix )`.
    fn unique_prefixed_id(&mut self, prefix: &[u8]) -> Vec<u8> {
        let counter = self.prefixed_ids.entry(prefix.to_vec()).or_insert(0);
        *counter += 1;
        [prefix, counter.to_string().as_bytes()].concat()
    }

    /// `img_caption_shortcode( $attr, $content )` under `html5` support.
    pub(crate) fn img_caption_shortcode(
        &mut self,
        attr: &[(Key, Value)],
        content: Option<&[u8]>,
    ) -> Vec<u8> {
        let site = self.context.site;
        let hooks = site.hooks();
        let mut content = content.unwrap_or_default().to_vec();
        let mut attr = attr.to_vec();
        match given(&attr, "caption") {
            // New style: the caption follows the image and its link.
            None => {
                if let Some((image, caption)) = split_image(&content) {
                    let caption = Value::String(hyscribe_phpser::php_trim(&caption).to_vec());
                    attr.push((Key::String(b"caption".to_vec()), caption));
                    content = image;
                }
            }
            Some(caption) => {
                let caption = caption.to_php_string();
                if caption.contains(&b'<') {
                    let kept = wp_kses_post(&caption, &self.context.upload_host());
                    set_att(&mut attr, "caption", Value::String(kept));
                }
            }
        }
        let output = hooks.img_caption_shortcode(&attr, &content);
        if !is_empty_string(&output) {
            return output;
        }
        let pairs = [
            ("id", Value::String(Vec::new())),
            ("caption_id", Value::String(Vec::new())),
            ("align", Value::String(b"alignnone".to_vec())),
            ("width", Value::String(Vec::new())),
            ("caption", Value::String(Vec::new())),
            ("class", Value::String(Vec::new())),
        ];
        let atts = self.shortcode_atts("caption", &pairs, &attr);
        let width = att(&atts, "width").to_int();
        let caption = att(&atts, "caption").to_php_string();
        if width < 1 || is_empty_string(&caption) {
            return content;
        }
        let text = |name: &str| att(&atts, name).to_php_string();
        let (mut id, mut describedby, mut caption_id) =
            (String::new(), String::new(), String::new());
        let mut figure_id = Vec::new();
        if !is_empty_string(&text("id")) {
            let prefix = [&sanitize_html_class(&text("id"), b"")[..], b"-"].concat();
            let unique = self.unique_prefixed_id(&prefix);
            figure_id = unique.strip_suffix(b"-1").unwrap_or(&unique).to_vec();
            id = format!("id=\"{}\" ", esc_attr(&figure_id));
        }
        let mut caption_value = Vec::new();
        if !is_empty_string(&text("caption_id")) {
            let prefix = [&sanitize_html_class(&text("caption_id"), b"")[..], b"-"].concat();
            let unique = self.unique_prefixed_id(&prefix);
            caption_value = unique.strip_suffix(b"-1").unwrap_or(&unique).to_vec();
        } else if !is_empty_string(&figure_id) {
            let underscores: Vec<u8> = figure_id
                .iter()
                .map(|&b| if b == b'_' { b'-' } else { b })
                .collect();
            caption_value = [&b"caption-"[..], &underscores].concat();
        }
        if !is_empty_string(&caption_value) {
            caption_id = format!("id=\"{}\" ", esc_attr(&caption_value));
            describedby = format!("aria-describedby=\"{}\" ", esc_attr(&caption_value));
        }
        let class = [b"wp-caption ", &text("align")[..], b" ", &text("class")].concat();
        let class = hyscribe_phpser::php_trim(&class).to_vec();
        let caption_width = hooks.img_caption_shortcode_width(width, &atts, &content);
        let style = if caption_width != 0 {
            format!("style=\"width: {caption_width}px\" ")
        } else {
            String::new()
        };
        let inner = self.do_shortcode(&content);
        [
            format!(
                "<figure {id}{describedby}{style}class=\"{}\">",
                esc_attr(&class)
            )
            .as_bytes(),
            &inner,
            format!("<figcaption {caption_id}class=\"wp-caption-text\">").as_bytes(),
            &caption,
            b"</figcaption></figure>",
        ]
        .concat()
    }
}

/// `$attr[ $name ] = $value`: in place, or added last.
pub(crate) fn set_att(attr: &mut Vec<(Key, Value)>, name: &str, value: Value) {
    set_key(attr, Key::from_php_string(name.as_bytes().to_vec()), value);
}

/// `$attr[ $key ] = $value`: in place, or added last.
pub(crate) fn set_key(attr: &mut Vec<(Key, Value)>, key: Key, value: Value) {
    match attr.iter_mut().find(|(known, _)| *known == key) {
        Some(found) => found.1 = value,
        None => attr.push((key, value)),
    }
}

/// `preg_match( '#((?:<a [^>]+>\s*)?<img [^>]+>(?:\s*</a>)?)(.*)#is', $content )`:
/// the image with its link, and what follows.
fn split_image(content: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let lower = content.to_ascii_lowercase();
    let closers = positions(&lower, b">");
    // `[^>]+>` from `at`: a first character that is no `>`, then up to the
    // next `>`; the offset after it.
    let tag_end = |at: usize| {
        let close = closers.get(closers.partition_point(|&p| p < at + 1));
        let first = lower.get(at) != Some(&b'>');
        close.filter(|_| first).map(|close| close + 1)
    };
    let starts = |at: usize, opener: &[u8]| lower.get(at..).is_some_and(|r| r.starts_with(opener));
    let mut from = 0;
    while let Some(start) = find(&lower, from, b"<") {
        from = start + 1;
        let linked = starts(start, b"<a ")
            .then(|| tag_end(start + 3).map(|end| skip_space(&lower, end)))
            .flatten()
            .filter(|&at| starts(at, b"<img "));
        let Some(image_at) = linked.or_else(|| starts(start, b"<img ").then_some(start)) else {
            continue;
        };
        let Some(mut end) = tag_end(image_at + 5) else {
            continue;
        };
        let after = skip_space(&lower, end);
        if starts(after, b"</a>") {
            end = after + 4;
        }
        return Some((
            content.get(start..end).unwrap_or_default().to_vec(),
            content.get(end..).unwrap_or_default().to_vec(),
        ));
    }
    None
}

/// The offset after the whitespace `\s*` takes from `at`.
fn skip_space(text: &[u8], mut at: usize) -> usize {
    while text
        .get(at)
        .is_some_and(|&b| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'))
    {
        at += 1;
    }
    at
}

#[cfg(test)]
mod tests;
