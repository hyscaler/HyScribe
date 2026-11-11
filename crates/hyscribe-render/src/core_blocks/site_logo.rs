// SPDX-License-Identifier: AGPL-3.0-or-later

//! The site logo block, `render_block_core_site_logo()` over
//! `get_custom_logo()`: the logo image at its full size, or scaled to the
//! block's width, linked home.

use hyscribe_html::TagProcessor;
use hyscribe_kses::{esc_html, esc_url};
use hyscribe_phpser::{Maybe, Value, is_empty_string, maybe_unserialize};

use super::{Block, Context};
use crate::attachment::{ImageOptions, attachment_is_image, wp_get_attachment_image};
use crate::formatting::{CORE_SHORTCODE_TAGS, convert_chars, find, wptexturize};

/// `render_block_core_site_logo()`.
pub(super) fn site_logo(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let width = block.attribute("width").filter(|width| !width.is_empty());
    // The filter the block puts on `wp_get_attachment_image_src`.
    let resize = |image_width: i64, image_height: i64| {
        let width = width.map_or(0.0, Value::to_float);
        let height = width / (image_width as f64 / image_height as f64);
        (width as i64, height as i64)
    };
    let options = ImageOptions {
        resize: width.map(|_| &resize as &dyn Fn(i64, i64) -> (i64, i64)),
        ..ImageOptions::default()
    };
    let logo = custom_logo(context, &options);
    if logo.is_empty() {
        return Vec::new();
    }
    let is_link = block.attribute("isLink").is_some_and(|v| !v.is_empty());
    let logo = if !is_link {
        unlink(&logo)
    } else if block
        .attribute("linkTarget")
        .map(Value::to_php_string)
        .as_deref()
        == Some(b"_blank")
    {
        // The link is ours, with `rel="home"`, as the block checks for.
        let mut tags = TagProcessor::new(&logo);
        tags.next_tag(Some(b"a"), None);
        tags.set_attribute(b"aria-label", b"(Home link, opens in a new tab)");
        tags.set_attribute(b"target", b"_blank");
        tags.get_updated_html().to_vec()
    } else {
        logo
    };
    let class: &[u8] = if width.is_none() {
        b"is-default-size"
    } else {
        b""
    };
    [
        format!("<div {}>", block.wrapper(context, class)).as_bytes(),
        &logo,
        b"</div>",
    ]
    .concat()
}

/// `get_custom_logo()` for a theme without `unlink-homepage-logo`.
fn custom_logo(context: &Context<'_>, options: &ImageOptions<'_>) -> Vec<u8> {
    let site = context.site;
    let id = u64::try_from(logo_id(context).to_int()).unwrap_or(0);
    let image = site
        .post(id)
        .is_some_and(|post| attachment_is_image(site, &post));
    if !image {
        return Vec::new();
    }
    let mut attr: Vec<(&str, Vec<u8>)> =
        vec![("class", b"custom-logo".to_vec()), ("loading", Vec::new())];
    let alt = site
        .post_meta(id, "_wp_attachment_image_alt")
        .unwrap_or_default();
    if is_empty_string(&alt) {
        // `get_bloginfo( 'name', 'display' )`.
        let name = site.option("blogname").unwrap_or_default();
        let name = convert_chars(&wptexturize(&name, &CORE_SHORTCODE_TAGS));
        attr.push(("alt", esc_html(&name).into_bytes()));
    }
    let image = wp_get_attachment_image(site, &context.loading, id, "full", &attr, options);
    if image.is_empty() {
        return image;
    }
    let request = context.request;
    let current = !request.is_paged() && request.is_site_home(site);
    let home = esc_url(&[&site.home_url()[..], b"/"].concat());
    [
        b"<a href=\"".as_slice(),
        &home,
        b"\" class=\"custom-logo-link\" rel=\"home\"",
        if current {
            b" aria-current=\"page\""
        } else {
            b""
        },
        b">",
        &image,
        b"</a>",
    ]
    .concat()
}

/// The `site_logo` option, which `_override_custom_logo_theme_mod()` puts
/// before the theme's `custom_logo` mod.
fn logo_id(context: &Context<'_>) -> Value {
    if let Some(logo) = context.site.option("site_logo") {
        return Value::String(logo);
    }
    let name = format!("theme_mods_{}", context.theme.slug());
    let mods = context.site.option(&name).unwrap_or_default();
    match maybe_unserialize(&mods) {
        Maybe::Value(mods) => mods.get_str("custom_logo").cloned().unwrap_or(Value::Null),
        _ => Value::Null,
    }
}

/// `preg_replace( '#<a.*?>(.*?)</a>#i', '\1', $logo )` for the logo's own
/// markup, which has no line breaks.
fn unlink(logo: &[u8]) -> Vec<u8> {
    let lower = logo.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(start) = find(&lower, at, b"<a") {
        let opened = find(&lower, start + 2, b">");
        let closed = opened.and_then(|end| Some((end, find(&lower, end, b"</a>")?)));
        let Some((end, close)) = closed else {
            break;
        };
        out.extend_from_slice(logo.get(at..start).unwrap_or_default());
        out.extend_from_slice(logo.get(end + 1..close).unwrap_or_default());
        at = close + 4;
    }
    out.extend_from_slice(logo.get(at..).unwrap_or_default());
    out
}

#[cfg(test)]
mod tests;
