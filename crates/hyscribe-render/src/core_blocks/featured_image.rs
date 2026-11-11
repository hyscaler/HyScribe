// SPDX-License-Identifier: AGPL-3.0-or-later

//! The featured image block, `render_block_core_post_featured_image()`:
//! the post's thumbnail through `wp_get_attachment_image()`, or the first
//! image of its content, sized, linked and overlaid as set.

use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_kses::{esc_attr, esc_url, safecss_filter_attr, strip_tags};
use hyscribe_phpser::{Key, Value, is_empty_string, php_trim};

use super::avatar::border_attributes;
use super::{Block, Context};
use crate::attachment::{post_thumbnail_id, wp_get_attachment_image};
use crate::post::the_title;
use crate::style::get_styles;

/// `render_block_core_post_featured_image()`.
pub(super) fn post_featured_image(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(id) = context.post_id else {
        return Vec::new();
    };
    let truthy = |name| block.attribute(name).is_some_and(|v| !v.is_empty());
    let text = |name| {
        block
            .attribute(name)
            .map(Value::to_php_string)
            .unwrap_or_default()
    };
    let is_link = truthy("isLink");
    let size = block.attribute("sizeSlug").map_or_else(
        || "post-thumbnail".to_string(),
        |v| String::from_utf8_lossy(&v.to_php_string()).into_owned(),
    );
    let (border_class, border_style) = border_attributes(block);
    let mut attr: Vec<(&str, Vec<u8>)> = Vec::new();
    if !border_class.is_empty() {
        attr.push(("class", border_class.clone().into_bytes()));
    }
    if !border_style.is_empty() {
        attr.push(("style", border_style.clone()));
    }
    let post = context.site.post(id);
    if is_link {
        let title = post.as_ref().map(the_title).unwrap_or_default();
        let alt = if is_empty_string(&title) {
            format!("Untitled post {id}").into_bytes()
        } else {
            php_trim(&strip_tags(&title)).to_vec()
        };
        attr.push(("alt", alt));
    }
    let extra = extra_styles(block);
    if !extra.is_empty() {
        match attr.iter_mut().find(|(name, _)| *name == "style") {
            Some((_, style)) => style.extend_from_slice(extra.as_bytes()),
            None => attr.push(("style", extra.into_bytes())),
        }
    }
    let thumbnail = post_thumbnail_id(context.site, Some(id));
    let mut image = match &post {
        Some(_) => wp_get_attachment_image(
            context.site,
            &context.loading,
            thumbnail,
            &size,
            &attr,
            true,
        ),
        None => Vec::new(),
    };
    if image.is_empty() && truthy("useFirstImageFromPost") {
        let content = post
            .as_ref()
            .map(|p| p.content.as_slice())
            .unwrap_or_default();
        let style = attr
            .iter()
            .find(|(name, _)| *name == "style")
            .map(|(_, v)| v.as_slice());
        image = first_image(content, style.unwrap_or_default());
    }
    if image.is_empty() {
        return Vec::new();
    }
    let overlay = overlay(block, &border_class, &border_style);
    let figure = if is_link {
        let href = post
            .as_ref()
            .map(|p| context.permalink(p))
            .unwrap_or_default();
        let rel = match block.attribute("rel").filter(|rel| !rel.is_empty()) {
            Some(rel) => format!("rel=\"{}\"", esc_attr(&rel.to_php_string())),
            None => String::new(),
        };
        [
            format!(
                "<a href=\"{}\" target=\"{}\" {rel}>",
                String::from_utf8_lossy(&esc_url(&href)),
                esc_attr(&text("linkTarget"))
            )
            .as_bytes(),
            &image,
            overlay.as_bytes(),
            b"</a>",
        ]
        .concat()
    } else {
        [&image[..], overlay.as_bytes()].concat()
    };
    [
        format!("<figure {}>", block.wrapper(context, b"")).as_bytes(),
        &figure,
        b"</figure>",
    ]
    .concat()
}

/// The image's own sizing styles: aspect ratio, height, width, scale and
/// shadow.
fn extra_styles(block: &Block<'_>) -> String {
    let declaration = |property: &str, value: &Value| {
        let css = [property.as_bytes(), b":", &value.to_php_string()].concat();
        format!("{};", esc_attr(&safecss_filter_attr(&css)))
    };
    // Set, and not null or the empty string.
    let given = |name| {
        block
            .attribute(name)
            .filter(|v| !matches!(v, Value::String(s) if s.is_empty()))
    };
    let mut styles = String::new();
    if let Some(ratio) = block.attribute("aspectRatio").filter(|v| !v.is_empty()) {
        if ratio.to_php_string() != b"auto" {
            styles.push_str(&declaration("aspect-ratio", ratio));
        }
        styles.push_str("width:100%;");
    }
    match (given("height"), given("width")) {
        (Some(height), _) => styles.push_str(&declaration("height", height)),
        (None, Some(_)) => styles.push_str("height:auto;"),
        (None, None) => {}
    }
    if let Some(width) = given("width") {
        styles.push_str(&declaration("width", width));
    }
    if let Some(scale) = block.attribute("scale").filter(|v| !v.is_empty()) {
        styles.push_str(&declaration("object-fit", scale));
    }
    if let Some(shadow) = block
        .attributes
        .get_path(&["style", "shadow"])
        .filter(|v| !v.is_empty())
    {
        let shadow = Value::Array(vec![(Key::String(b"shadow".to_vec()), shadow.clone())]);
        styles.push_str(&String::from_utf8_lossy(&get_styles(&shadow, false).css));
    }
    styles
}

/// `get_block_core_post_featured_image_overlay_element_markup()`.
fn overlay(block: &Block<'_>, border_class: &str, border_style: &[u8]) -> String {
    let text = |name| {
        block
            .attribute(name)
            .filter(|v| !v.is_empty())
            .map(|v| String::from_utf8_lossy(&v.to_php_string()).into_owned())
    };
    let Some(dim) = text("dimRatio") else {
        return String::new();
    };
    let mut classes = vec!["wp-block-post-featured-image__overlay".to_string()];
    let mut styles = Vec::new();
    if !border_class.is_empty() {
        classes.push(border_class.to_string());
    }
    if !border_style.is_empty() {
        styles.push(String::from_utf8_lossy(border_style).into_owned());
    }
    classes.push("has-background-dim".into());
    classes.push(format!("has-background-dim-{dim}"));
    if let Some(color) = text("overlayColor") {
        classes.push(format!("has-{color}-background-color"));
    }
    let (gradient, custom_gradient) = (text("gradient"), text("customGradient"));
    if gradient.is_some() || custom_gradient.is_some() {
        classes.push("has-background-gradient".into());
    }
    if let Some(gradient) = gradient {
        classes.push(format!("has-{gradient}-gradient-background"));
    }
    if let Some(gradient) = custom_gradient {
        styles.push(format!("background-image: {gradient};"));
    }
    if let Some(color) = text("customOverlayColor") {
        styles.push(format!("background-color: {color};"));
    }
    format!(
        "<span class=\"{}\" style=\"{}\" aria-hidden=\"true\"></span>",
        esc_attr(classes.join(" ").as_bytes()),
        esc_attr(&safecss_filter_attr(styles.join(" ").as_bytes()))
    )
}

/// The content's first `<img>`, rebuilt with its attributes and `style`
/// added to its own.
fn first_image(content: &[u8], style: &[u8]) -> Vec<u8> {
    let mut found = TagProcessor::new(content);
    if !found.next_tag(Some(b"img"), None) {
        return Vec::new();
    }
    let mut image = TagProcessor::new(b"<img>");
    image.next_tag(None, None);
    for name in found
        .get_attribute_names_with_prefix(b"")
        .unwrap_or_default()
    {
        match found.get_attribute(&name) {
            Some(Attribute::Value(value)) => image.set_attribute(&name, &value),
            _ => image.set_boolean_attribute(&name),
        };
    }
    if !style.is_empty() {
        let merged = match image.get_attribute(b"style") {
            Some(Attribute::Value(own)) if !own.is_empty() => {
                // `rtrim( $existing_style, ';' )`.
                let kept = own.len() - own.iter().rev().take_while(|b| **b == b';').count();
                [own.get(..kept).unwrap_or_default(), b";", style].concat()
            }
            _ => style.to_vec(),
        };
        image.set_attribute(b"style", &merged);
    }
    image.get_updated_html().to_vec()
}

#[cfg(test)]
mod tests;
