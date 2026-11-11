// SPDX-License-Identifier: AGPL-3.0-or-later

//! The media and text block, `render_block_core_media_text()`: the global
//! post's featured image put into the media figure.

use hyscribe_html::TagProcessor;
use hyscribe_kses::{esc_url, strip_tags};
use hyscribe_phpser::{Value, float_to_string, php_trim};

use crate::attachment::image_downsize;
use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::formatting::find;
use crate::template::Renderer;

impl Renderer<'_> {
    /// `render_block_core_media_text()`.
    pub(crate) fn media_text(&mut self, block: &Block) -> Vec<u8> {
        let content = self.saved_markup(block);
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/media-text", parsed);
        let attribute = |name| attributes.get_str(name).filter(|v| **v != Value::Null);
        if attribute("useFeaturedImage") == Some(&Value::Bool(false)) {
            return content;
        }
        let site = self.context.site;
        let post = self.context.post_id.or(self.context.request.post);
        let thumbnail = post
            .and_then(|id| site.post(id))
            .and_then(|post| site.post_meta(post.id, "_thumbnail_id"))
            .map_or(0, |id| {
                u64::try_from(Value::String(id).to_int()).unwrap_or(0)
            });
        let Some((url, _, _)) = image_downsize(site, thumbnail, "post-thumbnail") else {
            return content;
        };
        let right = attribute("mediaPosition")
            .map(Value::to_php_string)
            .as_deref()
            == Some(b"right");
        let fill = attribute("imageFill").is_some_and(|v| !v.is_empty());
        let focal = attribute("focalPoint").map_or_else(
            || "50% 50%".to_string(),
            |point| {
                let percent = |axis| {
                    float_to_string(
                        (point.get_str(axis).map_or(0.0, Value::to_float) * 100.0).round(),
                    )
                };
                format!("{}% {}%", percent("x"), percent("y"))
            },
        );
        let id =
            String::from_utf8_lossy(&self.unique_id("wp-block-media-text__media-")).into_owned();
        let mut tags = TagProcessor::new(&content);
        while tags.next_tag(Some(b"div"), Some(b"wp-block-media-text")) {
            if fill {
                tags.remove_class(b"is-image-fill");
                tags.add_class(b"is-image-fill-element");
            }
        }
        let content = tags.get_updated_html().to_vec();
        // The first media figure, or with the media on the right the last.
        let media = |tags: &mut TagProcessor| {
            tags.next_tag(Some(b"figure"), Some(b"wp-block-media-text__media"))
        };
        let mut count = 0;
        let mut counter = TagProcessor::new(&content);
        while media(&mut counter) {
            count += 1;
        }
        let wanted = if right { count } else { count.min(1) };
        let mut tags = TagProcessor::new(&content);
        for _ in 0..wanted {
            media(&mut tags);
        }
        // Without a figure the processor is on no tag, and this does nothing.
        tags.set_attribute(b"id", id.as_bytes());
        let content = with_image_tag(tags.get_updated_html(), &id);
        let mut tags = TagProcessor::new(&content);
        // PHP's query has no `id` key, so this is the first figure.
        if !tags.next_tag(Some(b"figure"), None) {
            return content;
        }
        tags.remove_attribute(b"id");
        if !tags.next_tag(Some(b"img"), Some(b"wp-block-media-text__featured_image")) {
            return content;
        }
        tags.set_attribute(b"src", &esc_url(&url));
        let size =
            attribute("mediaSizeSlug").map_or_else(|| b"full".to_vec(), Value::to_php_string);
        let class = [format!("wp-image-{thumbnail} size-").as_bytes(), &size].concat();
        tags.set_attribute(b"class", &class);
        let alt = site
            .post_meta(thumbnail, "_wp_attachment_image_alt")
            .unwrap_or_default();
        tags.set_attribute(b"alt", php_trim(&strip_tags(&alt)));
        if fill {
            tags.set_attribute(b"style", format!("object-position:{focal};").as_bytes());
        }
        tags.get_updated_html().to_vec()
    }
}

/// `/(<figure\s+id="ID"\s+class="wp-block-media-text__media"\s*>)/`, with
/// the featured image's tag put after each match.
fn with_image_tag(content: &[u8], id: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = find(content, at, b"<figure") {
        let rest = content.get(start + 7..).unwrap_or_default();
        let end = skip(rest, 1)
            .and_then(|r| r.strip_prefix(format!("id=\"{id}\"").as_bytes()))
            .and_then(|r| skip(r, 1))
            .and_then(|r| r.strip_prefix(b"class=\"wp-block-media-text__media\""))
            .and_then(|r| skip(r, 0))
            .and_then(|r| r.strip_prefix(b">"))
            .map(|r| content.len() - r.len());
        match end {
            Some(end) => {
                out.extend_from_slice(content.get(at..end).unwrap_or_default());
                out.extend_from_slice(b"<img class=\"wp-block-media-text__featured_image\">");
                at = end;
            }
            None => {
                out.extend_from_slice(content.get(at..start + 7).unwrap_or_default());
                at = start + 7;
            }
        }
    }
    out.extend_from_slice(content.get(at..).unwrap_or_default());
    out
}

/// `\s+` (`min` 1) or `\s*` (`min` 0) at the start of `rest`, and what
/// follows it.
fn skip(rest: &[u8], min: usize) -> Option<&[u8]> {
    let n = rest
        .iter()
        .take_while(|b| b.is_ascii_whitespace() || **b == b'\x0b')
        .count();
    (n >= min).then(|| rest.get(n..).unwrap_or_default())
}

#[cfg(test)]
mod tests;
