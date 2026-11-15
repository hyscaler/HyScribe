// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_filter_content_tags()` in the `the_content` and
//! `widget_block_content` contexts: the images and iframes of post content
//! or a block widget get their dimensions, `srcset` and `sizes` from the
//! attachment a `wp-image-N` class names, and the loading attributes.

use std::collections::HashSet;
use std::num::NonZeroU64;

use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_kses::esc_attr;
use hyscribe_phpser::{Value, isset, wp_basename};

use crate::attachment::{edit_hash, metadata, srcset, upload_baseurl};
use crate::formatting::find;
use crate::media::{Filter, Given, Loading};
use crate::site::Site;
use crate::template::replace;

/// [`wp_filter_content_tags`] in or outside the main loop, on its own: the
/// entry point for fuzzing.
pub fn filter(site: &dyn Site, content: &[u8], in_the_loop: bool) -> Vec<u8> {
    wp_filter_content_tags(site, &Loading::in_the_loop(in_the_loop), content)
}

/// `wp_filter_content_tags( $content, 'the_content' )`, which the excerpts
/// here share.
pub(crate) fn wp_filter_content_tags(
    site: &dyn Site,
    loading: &Loading,
    content: &[u8],
) -> Vec<u8> {
    filter_content_tags(site, loading, content, Filter::Content)
}

/// `wp_filter_content_tags( $content, $context )` for the context of
/// `filter`.
pub(crate) fn filter_content_tags(
    site: &dyn Site,
    loading: &Loading,
    content: &[u8],
    filter: Filter,
) -> Vec<u8> {
    let matches = tags(content);
    let mut content = content.to_vec();
    let mut done: HashSet<&[u8]> = HashSet::new();
    for (tag, img) in matches {
        // Each distinct tag once; `str_replace()` covers its repeats.
        if !done.insert(tag) {
            continue;
        }
        let filtered = if img {
            image(site, loading, tag, filter)
        } else {
            iframe(loading, tag, filter)
        };
        if filtered != tag {
            content = replace(&content, tag, &filtered);
        }
    }
    content
}

/// `/<(img|iframe)\s[^>]+>/` over `content`, in order: each tag and whether
/// it is an image.
fn tags(content: &[u8]) -> Vec<(&[u8], bool)> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(start) = find(content.get(at..).unwrap_or_default(), 0, b"<").map(|i| at + i) {
        let rest = content.get(start + 1..).unwrap_or_default();
        let name = [&b"img"[..], b"iframe"]
            .into_iter()
            .find(|n| rest.starts_with(n));
        let after = name.map_or(0, <[u8]>::len) + 1;
        let space = rest
            .get(after - 1)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'));
        let close = rest
            .get(after..)
            .and_then(|r| r.iter().position(|b| *b == b'>'));
        // The pattern needs a character before `>`; `<img >` has nothing to
        // filter either way.
        match (name, space, close) {
            (Some(name), true, Some(len)) => {
                let end = start + 1 + after + len + 1;
                found.push((content.get(start..end).unwrap_or_default(), name == b"img"));
                at = end;
            }
            _ => at = start + 1,
        }
    }
    found
}

/// The first `pattern` capture: `name="` then what the predicate accepts.
fn attribute<'a>(
    tag: &'a [u8],
    prefix: &[u8],
    quotes: &[u8],
    allowed: fn(&u8) -> bool,
) -> Option<&'a [u8]> {
    let mut at = 0;
    while let Some(start) = find(tag.get(at..).unwrap_or_default(), 0, prefix).map(|i| at + i) {
        let after = start + prefix.len();
        let quote = tag.get(after).filter(|q| quotes.contains(q));
        // Either quote may close what either opened, as the patterns allow.
        if quote.is_some() {
            let value = tag.get(after + 1..).unwrap_or_default();
            let len = value.iter().take_while(|b| allowed(b)).count();
            if len > 0 && value.get(len).is_some_and(|b| quotes.contains(b)) {
                return value.get(..len);
            }
        }
        at = start + 1;
    }
    None
}

/// A whole number attribute such as ` width="300"`.
fn number(tag: &[u8], prefix: &[u8], quotes: &[u8]) -> Option<i64> {
    attribute(tag, prefix, quotes, u8::is_ascii_digit).map(|v| Value::String(v.to_vec()).to_int())
}

/// `src="…"` up to its query string.
fn source(tag: &[u8]) -> &[u8] {
    let src = attribute(tag, b"src=", b"\"", |b| *b != b'"').unwrap_or_default();
    src.split(|b| *b == b'?').next().unwrap_or_default()
}

fn image(site: &dyn Site, loading: &Loading, tag: &[u8], filter: Filter) -> Vec<u8> {
    let mut image = tag.to_vec();
    if let Some(id) = image_id(tag).map(NonZeroU64::get) {
        if find(&image, 0, b" width=").is_none() && find(&image, 0, b" height=").is_none() {
            image = add_width_and_height(site, &image, id);
        }
        if find(&image, 0, b" srcset=").is_none() {
            image = add_srcset_and_sizes(site, &image, id);
        }
    }
    let image = add_loading(loading, &image, filter);
    auto_sizes(&image)
}

/// The attachment `wp-image-N`, in any case, names.
fn image_id(tag: &[u8]) -> Option<NonZeroU64> {
    let lower = tag.to_ascii_lowercase();
    let mut at = 0;
    while let Some(start) =
        find(lower.get(at..).unwrap_or_default(), 0, b"wp-image-").map(|i| at + i)
    {
        let digits: Vec<u8> = lower
            .iter()
            .skip(start + 9)
            .take_while(|b| b.is_ascii_digit())
            .copied()
            .collect();
        if !digits.is_empty() {
            return NonZeroU64::new(digits.iter().fold(0u64, |n, d| {
                n.saturating_mul(10).saturating_add(u64::from(d - b'0'))
            }));
        }
        at = start + 1;
    }
    None
}

/// `wp_image_src_get_dimensions()`.
fn dimensions(src: &[u8], meta: &Value) -> Option<(i64, i64)> {
    let int = |v: &Value, key| v.get_str(key).map_or(0, Value::to_int);
    let file = meta.get_str("file").filter(|file| isset(Some(file)));
    let file = file.map(|file| wp_basename(&file.to_php_string()));
    if file.is_some_and(|file| find(src, 0, &file).is_some()) {
        return Some((int(meta, "width"), int(meta, "height")));
    }
    let name = wp_basename(src);
    let sizes = meta.get_str("sizes").and_then(Value::as_array)?;
    sizes
        .iter()
        .find(|(_, size)| size.get_str("file").map(Value::to_php_string).as_ref() == Some(&name))
        .map(|(_, size)| (int(size, "width"), int(size, "height")))
}

/// `wp_img_tag_add_width_and_height_attr()`.
fn add_width_and_height(site: &dyn Site, image: &[u8], id: u64) -> Vec<u8> {
    let src = source(image);
    let size = metadata(site, id).and_then(|meta| dimensions(src, &meta));
    let Some((mut width, mut height)) = size.filter(|(w, h)| !src.is_empty() && *w != 0 && *h != 0)
    else {
        return image.to_vec();
    };
    // `style="width: 300px;"` scales the height to match.
    if let Some(styled) = styled_width(image).filter(|w| *w != 0) {
        height = (height as f64 * styled as f64 / width as f64).round() as i64;
        width = styled;
    }
    replace(
        image,
        b"<img",
        format!("<img width=\"{width}\" height=\"{height}\"").as_bytes(),
    )
}

/// `/style="width:\s*(\d+)px;"/`.
fn styled_width(image: &[u8]) -> Option<i64> {
    let mut at = 0;
    while let Some(start) =
        find(image.get(at..).unwrap_or_default(), 0, b"style=\"width:").map(|i| at + i)
    {
        let rest = image.get(start + 13..).unwrap_or_default();
        let rest = rest
            .get(
                rest.iter()
                    .take_while(|b| b.is_ascii_whitespace() || **b == b'\x0b')
                    .count()..,
            )
            .unwrap_or_default();
        let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
        if digits > 0 && rest.get(digits..).is_some_and(|r| r.starts_with(b"px;\"")) {
            return Some(Value::String(rest.get(..digits).unwrap_or_default().to_vec()).to_int());
        }
        at = start + 1;
    }
    None
}

/// `wp_img_tag_add_srcset_and_sizes_attr()`, `wp_image_add_srcset_and_sizes()`.
fn add_srcset_and_sizes(site: &dyn Site, image: &[u8], id: u64) -> Vec<u8> {
    let Some(meta) =
        metadata(site, id).filter(|m| m.get_str("sizes").is_some_and(|s| !s.is_empty()))
    else {
        return image.to_vec();
    };
    let src = source(image);
    let file = meta
        .get_str("file")
        .map(Value::to_php_string)
        .unwrap_or_default();
    let edited = edit_hash(&file).is_some_and(|hash| find(&wp_basename(src), 0, hash).is_none());
    if src.is_empty() || edited {
        return image.to_vec();
    }
    let given = (
        number(image, b" width=", b"\""),
        number(image, b" height=", b"\""),
    );
    let size = match given {
        (Some(w), Some(h)) if w != 0 && h != 0 => Some((w, h)),
        _ => dimensions(src, &meta),
    };
    let Some((width, height)) = size else {
        return image.to_vec();
    };
    let made = srcset(
        (width, height),
        src,
        &meta,
        &upload_baseurl(&|name| site.option(name).unwrap_or_default()),
    );
    let has_sizes = find(image, 0, b" sizes=").is_some();
    let attr = match made {
        Some(srcset) if has_sizes => format!(" srcset=\"{}\"", esc_attr(srcset.as_bytes())),
        // A `srcset` means a width, so `sizes` has one.
        Some(srcset) => format!(
            " srcset=\"{0}\" sizes=\"(max-width: {1}px) 100vw, {1}px\"",
            esc_attr(srcset.as_bytes()),
            width.unsigned_abs()
        ),
        _ => return image.to_vec(),
    };
    // `/<img ([^>]+?)[\/ ]*>/`
    let Some(start) = find(image, 0, b"<img ") else {
        return image.to_vec();
    };
    let inner = image.get(start + 5..).unwrap_or_default();
    // A matched tag ends in `>` and has a `src`, so there is something to keep.
    let close = inner.iter().position(|b| *b == b'>').unwrap_or(inner.len());
    let attributes = inner.get(..close).unwrap_or_default();
    let kept = attributes.len()
        - attributes
            .iter()
            .rev()
            .take_while(|b| matches!(b, b'/' | b' '))
            .count();
    [
        image.get(..start + 5).unwrap_or_default(),
        attributes.get(..kept).unwrap_or_default(),
        attr.as_bytes(),
        b" />",
        inner.get(close + 1..).unwrap_or_default(),
    ]
    .concat()
}

/// `wp_img_tag_add_loading_optimization_attrs()`.
fn add_loading(loading: &Loading, image: &[u8], filter: Filter) -> Vec<u8> {
    let letters = |prefix: &[u8]| attribute(image, prefix, b"\"'", u8::is_ascii_alphabetic);
    let (loading_value, fetchpriority, decoding) = (
        letters(b" loading="),
        letters(b" fetchpriority="),
        letters(b" decoding="),
    );
    let given = Given {
        width: number(image, b" width=", b"\"'"),
        height: number(image, b" height=", b"\"'"),
        loading: loading_value,
        fetchpriority,
    };
    let optimization = loading.optimization(true, &given, filter);
    if find(image, 0, b" src=\"").is_none() {
        return image.to_vec();
    }
    let insert = |image: Vec<u8>, name: &str, value: Option<&str>| match value {
        Some(value) => replace(
            &image,
            b"<img",
            format!("<img {name}=\"{value}\"").as_bytes(),
        ),
        None => image,
    };
    let mut image = image.to_vec();
    if decoding.is_none() {
        image = insert(image, "decoding", optimization.decoding);
    }
    if find(&image, 0, b" width=\"").is_none() || find(&image, 0, b" height=\"").is_none() {
        return image;
    }
    if loading_value.is_none() {
        image = insert(image, "loading", optimization.loading);
    }
    if fetchpriority.is_none() {
        image = insert(image, "fetchpriority", optimization.fetchpriority);
    }
    image
}

/// `wp_img_tag_add_auto_sizes()`: a lazy image with a width and sizes
/// sizes itself first.
fn auto_sizes(image: &[u8]) -> Vec<u8> {
    let mut tags = TagProcessor::new(image);
    if !tags.next_tag(Some(b"IMG"), None) {
        return image.to_vec();
    }
    let value = |tags: &mut TagProcessor, name: &[u8]| match tags.get_attribute(name) {
        Some(Attribute::Value(value)) => Some(value),
        _ => None,
    };
    let trimmed = |v: &[u8]| v.trim_ascii().to_ascii_lowercase();
    let lazy = value(&mut tags, b"loading").is_some_and(|l| trimmed(&l) == b"lazy");
    let width = value(&mut tags, b"width").is_some_and(|w| !w.is_empty());
    let Some(sizes) = value(&mut tags, b"sizes").filter(|_| lazy && width) else {
        return image.to_vec();
    };
    let first = sizes.split(|b| *b == b',').next().unwrap_or_default();
    if trimmed(first) == b"auto" {
        return image.to_vec();
    }
    tags.set_attribute(b"sizes", &[b"auto, ", &sizes[..]].concat());
    tags.get_updated_html().to_vec()
}

/// `wp_iframe_tag_add_loading_attr()` for an iframe without `loading`.
fn iframe(loading: &Loading, tag: &[u8], filter: Filter) -> Vec<u8> {
    if find(tag, 0, b" loading=").is_some() {
        return tag.to_vec();
    }
    // Sizes only matter for being known, not for their values.
    let known = |name: &[u8]| find(tag, 0, name).map(|_| 100);
    let given = Given {
        width: known(b" width=\""),
        height: known(b" height=\""),
        ..Given::default()
    };
    let optimization = loading.optimization(false, &given, filter);
    let complete = [&b" src=\""[..], b" width=\"", b" height=\""]
        .iter()
        .all(|n| find(tag, 0, n).is_some());
    match optimization.loading.filter(|_| complete) {
        Some(value) => replace(
            tag,
            b"<iframe",
            format!("<iframe loading=\"{value}\"").as_bytes(),
        ),
        None => tag.to_vec(),
    }
}

#[cfg(test)]
mod tests;
