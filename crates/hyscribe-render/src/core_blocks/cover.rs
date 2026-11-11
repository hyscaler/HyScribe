// SPDX-License-Identifier: AGPL-3.0-or-later

//! The cover block, `render_block_core_cover()`: the post's featured image
//! as its background, put before the inner container. An embedded video
//! background needs `wp_oembed_get()`, a fetch from another site, which is
//! not made (docs/SECURITY_MODEL.md), so it keeps its saved markup as PHP
//! does when that fetch fails.

use hyscribe_html::TagProcessor;
use hyscribe_kses::esc_url;
use hyscribe_phpser::Value;

use super::{Block, Context};
use crate::attachment::{
    attachment_alt, focal_position, image_downsize, post_thumbnail_id, wp_get_attachment_image,
};
use crate::blocks::is_pcre_space;
use crate::formatting::is_word;

/// `render_block_core_cover()`.
pub(super) fn cover(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let content = block.content;
    let attribute = |name| block.attribute(name);
    let text = |name| attribute(name).map(Value::to_php_string);
    let truthy = |name| attribute(name).is_some_and(|v| !v.is_empty());
    let image_background = text("backgroundType").as_deref() == Some(b"image");
    if !image_background || attribute("useFeaturedImage") == Some(&Value::Bool(false)) {
        return content.to_vec();
    }
    let position = attribute("focalPoint").map(focal_position);
    // The global post, which a post template sets.
    let thumbnail = post_thumbnail_id(context.site, context.post_id);
    let image = if !(truthy("hasParallax") || truthy("isRepeated")) {
        let mut attr: Vec<(&str, Vec<u8>)> = vec![
            ("class", b"wp-block-cover__image-background".to_vec()),
            ("data-object-fit", b"cover".to_vec()),
        ];
        if let Some(position) = &position {
            attr.push(("data-object-position", position.clone().into_bytes()));
            attr.push(("style", format!("object-position:{position};").into_bytes()));
        }
        let size = text("sizeSlug").unwrap_or_else(|| b"post-thumbnail".to_vec());
        let size = String::from_utf8_lossy(&size);
        wp_get_attachment_image(
            context.site,
            &context.loading,
            thumbnail,
            &size,
            &attr,
            true,
        )
    } else {
        let size = text("sizeSlug").unwrap_or_else(|| b"full".to_vec());
        let Some((url, _, _)) =
            image_downsize(context.site, thumbnail, &String::from_utf8_lossy(&size))
        else {
            return content.to_vec();
        };
        let mut tags = TagProcessor::new(b"<div></div>");
        tags.next_tag(None, None);
        let alt = attachment_alt(context.site, thumbnail);
        if !hyscribe_phpser::is_empty_string(&alt) {
            tags.set_attribute(b"role", b"img");
            tags.set_attribute(b"aria-label", &alt);
        }
        tags.add_class(b"wp-block-cover__image-background");
        tags.add_class(format!("wp-image-{thumbnail}").as_bytes());
        for flag in ["hasParallax", "isRepeated"] {
            if truthy(flag) {
                tags.add_class(if flag == "hasParallax" {
                    b"has-parallax"
                } else {
                    b"is-repeated"
                });
            }
        }
        let style = format!(
            "background-position:{};background-image:url({});",
            position.as_deref().unwrap_or("50% 50%"),
            String::from_utf8_lossy(&esc_url(&url))
        );
        tags.set_attribute(b"style", style.as_bytes());
        tags.get_updated_html().to_vec()
    };
    match inner_container(content) {
        Some(at) => [
            content.get(..at).unwrap_or_default(),
            &image,
            content.get(at..).unwrap_or_default(),
        ]
        .concat(),
        None => content.to_vec(),
    }
}

/// `/<div\b[^>]+wp-block-cover__inner-container[\s|"][^>]*>/U`: where the
/// first such tag starts, in one pass: the earliest `<div` since the last
/// `>` is the one a match starts at.
fn inner_container(content: &[u8]) -> Option<usize> {
    const CLASS: &[u8] = b"wp-block-cover__inner-container";
    // A match ends at a `>`, and neither the class nor the byte after it is
    // one, so the scan can stop at the last.
    let last_close = content.iter().rposition(|b| *b == b'>')?;
    let mut opener = None;
    for (at, byte) in content.iter().enumerate().take(last_close) {
        let rest = content.get(at..).unwrap_or_default();
        if *byte == b'>' {
            opener = None;
        } else if opener.is_none()
            && rest.starts_with(b"<div")
            && !content.get(at + 4).copied().is_some_and(is_word)
        {
            opener = Some(at);
        }
        // `\b` keeps the class from following `<div` at once, so `[^>]+`
        // holds.
        let after = content.get(at + CLASS.len()).copied();
        let ends = after.is_some_and(|b| is_pcre_space(b) || matches!(b, b'|' | b'"'));
        if opener.is_some() && rest.starts_with(CLASS) && ends {
            return opener;
        }
    }
    None
}

#[cfg(test)]
mod tests;
