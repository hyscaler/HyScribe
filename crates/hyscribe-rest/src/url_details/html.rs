// SPDX-License-Identifier: AGPL-3.0-or-later

//! The regexes `WP_REST_URL_Details_Controller` reads a page's head with,
//! matched by hand as PCRE matches them (the `i`, `s` and `U` modifiers,
//! back references to an optional quote), in linear time: where to look
//! next is looked up in tables, so hostile pages cannot make it quadratic.

use hyscribe_kses::{parse_url, wp_strip_all_tags};
use hyscribe_phpser::{is_empty_string, php_trim};
use hyscribe_render::formatting::html_entity_decode_quotes;

/// PCRE's `\s` without the `u` modifier.
fn space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Whether `text` at `at` starts with `word`, ASCII case insensitively.
fn at_word(text: &[u8], at: usize, word: &[u8]) -> bool {
    text.get(at..at + word.len())
        .is_some_and(|found| found.eq_ignore_ascii_case(word))
}

/// For each position, the next at or after it where `found` holds.
fn next(text: &[u8], found: impl Fn(usize) -> bool) -> Vec<Option<usize>> {
    let mut next = vec![None; text.len() + 1];
    for at in (0..text.len()).rev() {
        let after = next.get(at + 1).copied().flatten();
        if let Some(slot) = next.get_mut(at) {
            *slot = if found(at) { Some(at) } else { after };
        }
    }
    next
}

/// `next`'s entry at `at`.
fn lookup(next: &[Option<usize>], at: usize) -> Option<usize> {
    next.get(at).copied().flatten()
}

/// `strpos()`.
fn find(text: &[u8], needle: &[u8]) -> Option<usize> {
    text.windows(needle.len()).position(|w| w == needle)
}

/// `get_document_head()`: from `<head` for as many bytes as `</head>`, or
/// else `<body`, is from the start, then `</head>`; the whole page without
/// either.
pub(crate) fn document_head(html: &[u8]) -> Vec<u8> {
    let (Some(start), Some(end)) = (
        find(html, b"<head"),
        find(html, b"</head>").or_else(|| find(html, b"<body")),
    ) else {
        return html.to_vec();
    };
    let mut head: Vec<u8> = html.iter().skip(start).take(end).copied().collect();
    head.extend_from_slice(b"</head>");
    head
}

/// `prepare_metadata_for_output()`.
fn prepare(metadata: &[u8]) -> Vec<u8> {
    wp_strip_all_tags(&html_entity_decode_quotes(metadata), false)
}

/// `get_title()`: `#<title[^>]*>(.*?)<\s*/\s*title>#is`.
pub(crate) fn title(head: &[u8]) -> Vec<u8> {
    let found = (0..head.len())
        .find(|&at| at_word(head, at, b"<title"))
        .and_then(|open| Some(head.iter().skip(open).position(|&b| b == b'>')? + open + 1))
        .and_then(|from| {
            let close = (from..head.len()).find(|&at| closes_title(head, at))?;
            head.get(from..close)
        });
    match found {
        Some(title) if !is_empty_string(title) => prepare(php_trim(title)),
        _ => Vec::new(),
    }
}

/// `<\s*/\s*title>` at `at`.
fn closes_title(head: &[u8], at: usize) -> bool {
    let skip = |from: usize| from + head.iter().skip(from).take_while(|&&b| space(b)).count();
    if head.get(at) != Some(&b'<') {
        return false;
    }
    let slash = skip(at + 1);
    head.get(slash) == Some(&b'/') && at_word(head, skip(slash + 1), b"title>")
}

/// The quote at `at`, if one.
fn quote(text: &[u8], at: usize) -> Option<u8> {
    text.get(at).copied().filter(|b| matches!(b, b'"' | b'\''))
}

/// `get_icon()`: the first `<link>` whose `rel` is an icon, its `href`
/// made absolute on the URL's root, a `data:` URL as it is.
pub(crate) fn icon(head: &[u8], url: &[u8]) -> Vec<u8> {
    let Some(element) = icon_link(head) else {
        return Vec::new();
    };
    let href = href(element);
    if is_empty_string(href) {
        return Vec::new();
    }
    let icon = php_trim(href);
    let data = parse_url(icon).and_then(|parts| parts.scheme);
    if data.as_deref() == Some(b"data") {
        return icon.to_vec();
    }
    absolute(icon, url)
}

/// `#<link\s[^>]*rel=(?:[\"\']??)\s*(?:icon|shortcut icon|icon shortcut)\s*(?:[\"\']??)[^>]*\/?>#isU`:
/// the element up to the first `>` after a `rel=` naming an icon.
fn icon_link(head: &[u8]) -> Option<&[u8]> {
    let gt = next(head, |at| head.get(at) == Some(&b'>'));
    let rel = next(head, |at| {
        at_word(head, at, b"rel=") && names_icon(head, at + 4)
    });
    (0..head.len()).find_map(|at| {
        if !at_word(head, at, b"<link") || !head.get(at + 5).is_some_and(|&b| space(b)) {
            return None;
        }
        let end = lookup(&gt, at + 6)?;
        lookup(&rel, at + 6).filter(|&rel| rel < end)?;
        head.get(at..=end)
    })
}

/// After `rel=` at `at`: an optional quote, white space, then an icon.
fn names_icon(head: &[u8], at: usize) -> bool {
    let at = at + usize::from(quote(head, at).is_some());
    let at = at + head.iter().skip(at).take_while(|&&b| space(b)).count();
    [&b"icon"[..], b"shortcut icon", b"icon shortcut"]
        .iter()
        .any(|word| at_word(head, at, word))
}

/// `#href=([\"\']??)([^\" >]*?)\1[^>]*#isU`'s second group in `element`:
/// the value of its first `href=`, to its quote if it closes before a
/// space or `>`.
fn href(element: &[u8]) -> &[u8] {
    let Some(at) = (0..element.len()).find(|&at| at_word(element, at, b"href=")) else {
        return &[];
    };
    let rest = element.get(at + 5..).unwrap_or_default();
    let run = |from: usize| {
        let value = rest.get(from..).unwrap_or_default();
        let length = value.iter().take_while(|b| !b"\" >".contains(b)).count();
        (value, length)
    };
    if let Some(quote) = quote(rest, 0) {
        let (value, length) = run(1);
        if let Some(length) = (0..=length).rev().find(|&l| value.get(l) == Some(&quote)) {
            return value.get(..length).unwrap_or_default();
        }
    }
    let (value, length) = run(0);
    value.get(..length).unwrap_or_default()
}

/// One element of `get_meta_with_content_elements()`: the whole match and
/// its content.
pub(crate) struct Meta<'a> {
    element: &'a [u8],
    content: &'a [u8],
}

/// `get_meta_with_content_elements()`: `preg_match_all()` of
/// `#<meta\s[^>]*content=(["\']??)(.*)\1[^>]*\/?>#isU`.
pub(crate) fn metas(head: &[u8]) -> Vec<Meta<'_>> {
    let gt = next(head, |at| head.get(at) == Some(&b'>'));
    let content = next(head, |at| at_word(head, at, b"content="));
    let double = next(head, |at| head.get(at) == Some(&b'"'));
    let single = next(head, |at| head.get(at) == Some(&b'\''));
    let mut found = Vec::new();
    let mut at = 0;
    while at < head.len() {
        let start = at;
        at += 1;
        if !at_word(head, start, b"<meta") || !head.get(start + 5).is_some_and(|&b| space(b)) {
            continue;
        }
        let Some(end) = lookup(&gt, start + 6) else {
            break;
        };
        let Some(open) = lookup(&content, start + 6).filter(|&open| open < end) else {
            continue;
        };
        let value = open + 8;
        // The quoted value ends at the first such quote that a `>` follows.
        let closed = quote(head, value).and_then(|q| {
            let close = lookup(if q == b'"' { &double } else { &single }, value + 1)?;
            let end = lookup(&gt, close + 1)?;
            Some((head.get(value + 1..close).unwrap_or_default(), end))
        });
        let (content, end) = closed.unwrap_or((&[], end));
        found.push(Meta {
            element: head.get(start..=end).unwrap_or_default(),
            content,
        });
        at = end + 1;
    }
    found
}

/// `get_metadata_from_meta_element()`: the trimmed content of the first
/// element whose `attr` is one of `values`, quoted or not.
fn metadata<'a>(metas: &[Meta<'a>], attr: &str, values: &[&str]) -> &'a [u8] {
    let names = |element: &[u8]| {
        let attr = format!("{attr}=");
        (0..element.len())
            .filter(|&at| at_word(element, at, attr.as_bytes()))
            .any(|at| attr_is(element, at + attr.len(), values))
    };
    metas
        .iter()
        .find(|meta| names(meta.element))
        .map_or(&[][..], |meta| php_trim(meta.content))
}

/// `([\"\']??)\s*(?:values)\s*\1` at `at`: a quoted value is one of
/// `values` with white space around; an unquoted one starts with one.
fn attr_is(element: &[u8], at: usize, values: &[&str]) -> bool {
    let skip = |from: usize| from + element.iter().skip(from).take_while(|&&b| space(b)).count();
    match quote(element, at) {
        Some(q) => {
            let from = skip(at + 1);
            values.iter().any(|value| {
                at_word(element, from, value.as_bytes())
                    && element.get(skip(from + value.len())) == Some(&q)
            })
        }
        None => {
            let from = skip(at);
            values
                .iter()
                .any(|value| at_word(element, from, value.as_bytes()))
        }
    }
}

/// `get_description()`.
pub(crate) fn description(metas: &[Meta]) -> Vec<u8> {
    let description = metadata(metas, "name", &["description", "og:description"]);
    if description.is_empty() {
        return Vec::new();
    }
    prepare(description)
}

/// `get_image()`.
pub(crate) fn image(metas: &[Meta], url: &[u8]) -> Vec<u8> {
    let image = metadata(metas, "property", &["og:image", "og:image:url"]);
    if image.is_empty() {
        return Vec::new();
    }
    absolute(image, url)
}

/// `WP_Http::make_absolute_url()` of `relative` on the root of `url`, its
/// scheme and host, when it has both.
fn absolute(relative: &[u8], url: &[u8]) -> Vec<u8> {
    let parts = parse_url(url).unwrap_or_default();
    match (parts.scheme, parts.host) {
        (Some(scheme), Some(host)) => {
            let root = [&scheme[..], b"://", &host, b"/"].concat();
            hyscribe_fetch::make_absolute_url(relative, &root)
        }
        _ => relative.to_vec(),
    }
}

#[cfg(test)]
mod tests;
