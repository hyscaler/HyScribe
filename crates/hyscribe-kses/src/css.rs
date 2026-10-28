// SPDX-License-Identifier: AGPL-3.0-or-later

//! `safecss_filter_attr()` with the default `safe_style_css` list.

use crate::php::{
    contains, empty, first_from, is_space, positions, rest, scan, splice, str_replace, trim,
};
use crate::protocol::{ALLOWED_PROTOCOLS, wp_kses_bad_protocol, wp_kses_no_null};

/// The default `safe_style_css` list, duplicates included.
const SAFE_STYLE_CSS: [&str; 191] = [
    "background",
    "background-color",
    "background-image",
    "background-position",
    "background-repeat",
    "background-size",
    "background-attachment",
    "background-blend-mode",
    "border",
    "border-radius",
    "border-width",
    "border-color",
    "border-style",
    "border-right",
    "border-right-color",
    "border-right-style",
    "border-right-width",
    "border-bottom",
    "border-bottom-color",
    "border-bottom-left-radius",
    "border-bottom-right-radius",
    "border-bottom-style",
    "border-bottom-width",
    "border-bottom-right-radius",
    "border-bottom-left-radius",
    "border-left",
    "border-left-color",
    "border-left-style",
    "border-left-width",
    "border-top",
    "border-top-color",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-top-style",
    "border-top-width",
    "border-top-left-radius",
    "border-top-right-radius",
    "border-spacing",
    "border-collapse",
    "caption-side",
    "columns",
    "column-count",
    "column-fill",
    "column-gap",
    "column-rule",
    "column-span",
    "column-width",
    "display",
    "color",
    "filter",
    "font",
    "font-family",
    "font-size",
    "font-style",
    "font-variant",
    "font-weight",
    "letter-spacing",
    "line-height",
    "text-align",
    "text-decoration",
    "text-indent",
    "text-transform",
    "white-space",
    "height",
    "min-height",
    "max-height",
    "width",
    "min-width",
    "max-width",
    "margin",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "margin-top",
    "margin-block-start",
    "margin-block-end",
    "margin-inline-start",
    "margin-inline-end",
    "padding",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "padding-top",
    "padding-block-start",
    "padding-block-end",
    "padding-inline-start",
    "padding-inline-end",
    "flex",
    "flex-basis",
    "flex-direction",
    "flex-flow",
    "flex-grow",
    "flex-shrink",
    "flex-wrap",
    "gap",
    "column-gap",
    "row-gap",
    "grid-template-columns",
    "grid-auto-columns",
    "grid-column-start",
    "grid-column-end",
    "grid-column",
    "grid-column-gap",
    "grid-template-rows",
    "grid-auto-rows",
    "grid-row-start",
    "grid-row-end",
    "grid-row",
    "grid-row-gap",
    "grid-gap",
    "justify-content",
    "justify-items",
    "justify-self",
    "align-content",
    "align-items",
    "align-self",
    "clear",
    "cursor",
    "direction",
    "float",
    "list-style-type",
    "object-fit",
    "object-position",
    "opacity",
    "overflow",
    "vertical-align",
    "writing-mode",
    "position",
    "top",
    "right",
    "bottom",
    "left",
    "z-index",
    "box-shadow",
    "aspect-ratio",
    "container-type",
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-opacity",
    "stroke-width",
    "color-interpolation",
    "color-interpolation-filters",
    "paint-order",
    "stop-color",
    "stop-opacity",
    "flood-color",
    "flood-opacity",
    "lighting-color",
    "marker",
    "marker-end",
    "marker-mid",
    "marker-start",
    "clip-path",
    "clip-rule",
    "mask",
    "mask-type",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "x",
    "y",
    "d",
    "alignment-baseline",
    "baseline-shift",
    "dominant-baseline",
    "glyph-orientation-horizontal",
    "glyph-orientation-vertical",
    "text-anchor",
    "unicode-bidi",
    "word-spacing",
    "font-size-adjust",
    "font-stretch",
    "color-rendering",
    "image-rendering",
    "shape-rendering",
    "text-rendering",
    "vector-effect",
    "transform",
    "transform-origin",
    "pointer-events",
    "visibility",
    "--*",
];

/// The properties whose `url()` values are checked.
const URL_DATA_TYPES: [&str; 14] = [
    "background",
    "background-image",
    "cursor",
    "filter",
    "list-style",
    "list-style-image",
    "clip-path",
    "fill",
    "marker",
    "marker-end",
    "marker-mid",
    "marker-start",
    "mask",
    "stroke",
];

/// The properties whose gradients are allowed.
const GRADIENT_DATA_TYPES: [&str; 2] = ["background", "background-image"];

/// The functions whose calls are removed before the final check.
const FUNCTIONS: [&str; 36] = [
    "var",
    "calc",
    "min",
    "max",
    "minmax",
    "clamp",
    "repeat",
    "matrix",
    "matrix3d",
    "perspective",
    "rotate",
    "rotate3d",
    "rotateX",
    "rotateY",
    "rotateZ",
    "scale",
    "scale3d",
    "scaleX",
    "scaleY",
    "scaleZ",
    "skew",
    "skewX",
    "skewY",
    "translate",
    "translate3d",
    "translateX",
    "translateY",
    "translateZ",
    "circle",
    "ellipse",
    "inset",
    "path",
    "polygon",
    "rect",
    "shape",
    "xywh",
];

/// The JIT stack frames one match may use: PHP gives PCRE's JIT a 192 KiB
/// stack, and a longer gradient or function call fails the regex, which
/// drops the declaration. Measured on PHP 8.4, with the frame counts in
/// [`gradient`] and [`call`].
const FRAME_LIMIT: usize = 8192;

/// Whether `list` holds `name`.
fn listed(list: &[&str], name: &[u8]) -> bool {
    list.iter().any(|item| item.as_bytes() == name)
}

/// `^--[a-zA-Z0-9-_]+$`: a custom property.
fn is_custom_property(name: &[u8]) -> bool {
    name.strip_prefix(b"--").is_some_and(|tail| {
        !tail.is_empty()
            && tail
                .iter()
                .all(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    })
}

/// The `url\([^)]+\)` matches in `value`.
fn urls(value: &[u8]) -> Vec<&[u8]> {
    let closes = positions(value, |at| at.first() == Some(&b')'));
    scan(value.len(), |i| {
        if !rest(value, i).starts_with(b"url(") {
            return None;
        }
        let close = first_from(&closes, i)?;
        (close > i + 4).then_some((close + 1 - i, ()))
    })
    .into_iter()
    .map(|(start, end, ())| value.get(start..end).unwrap_or_default())
    .collect()
}

/// Group 2 of `^url\(\s*(['"]?)(.*)(\g1)\s*\)$` for a `urls()` match's
/// `inner` text: after leading space, a quoted string closed by the last
/// non-space byte, or else everything.
fn url_body(inner: &[u8]) -> &[u8] {
    let unspaced = rest(inner, inner.iter().take_while(|&&b| is_space(b)).count());
    if let [quote @ (b'\'' | b'"'), body @ ..] = unspaced
        && let Some(last) = body.iter().rposition(|&b| !is_space(b))
        && body.get(last) == Some(quote)
    {
        return body.get(..last).unwrap_or_default();
    }
    unspaced
}

/// Whether every `url()` in `value` holds a URL with an allowed protocol;
/// `test` loses each one checked.
fn urls_allowed(value: &[u8], test: &mut Vec<u8>) -> bool {
    for found in urls(value) {
        let body = url_body(found.get(4..found.len() - 1).unwrap_or_default());
        let url = trim(body);
        if empty(body) || empty(url) || wp_kses_bad_protocol(url, &ALLOWED_PROTOCOLS) != url {
            return false;
        }
        *test = str_replace(test, found, b"");
    }
    true
}

/// The length of the gradient at the start of `text` (`(?:repeating-)?
/// (?:linear|radial|conic)-gradient\((?:[^()]|\([^()]*\))*\)`) and its JIT
/// frames: two, one more for `repeating-`, and one per byte or group
/// inside.
fn gradient(text: &[u8]) -> Option<(usize, usize)> {
    let (tail, mut frames) = match text.strip_prefix(b"repeating-") {
        Some(tail) => (tail, 3),
        None => (text, 2),
    };
    let kind = ["linear", "radial", "conic"]
        .iter()
        .find(|kind| tail.starts_with(kind.as_bytes()))?;
    let body = rest(tail, kind.len()).strip_prefix(b"-gradient(")?;
    let mut at = 0;
    for _ in 0..body.len() {
        match body.get(at)? {
            b')' => return Some((text.len() - body.len() + at + 1, frames)),
            b'(' => {
                let inner = rest(body, at + 1);
                let close = inner.iter().position(|&b| b == b'(' || b == b')')?;
                if inner.get(close) == Some(&b'(') {
                    return None;
                }
                at += close + 2;
            }
            _ => at += 1,
        }
        frames += 1;
    }
    None
}

/// The length of the call to an allowed function at offset `i` of `text`
/// (`\b(?:var|calc|...)(\((?:[^()]|(?1))*\))`) and its JIT frames: four,
/// one per byte inside and three per nested group. `None` without a call;
/// `Some(None)` for a call whose parentheses never balance.
fn call(text: &[u8], i: usize) -> Option<Option<(usize, usize)>> {
    let word = |b: &u8| b.is_ascii_alphanumeric() || *b == b'_';
    if i.checked_sub(1).and_then(|j| text.get(j)).is_some_and(word) {
        return None;
    }
    let tail = rest(text, i);
    let name = FUNCTIONS.iter().find(|name| {
        tail.strip_prefix(name.as_bytes())
            .is_some_and(|t| t.first() == Some(&b'('))
    })?;
    let group = rest(tail, name.len());
    let mut depth = 0usize;
    let mut frames = 1;
    for (at, &byte) in group.iter().enumerate() {
        match byte {
            b'(' => {
                depth += 1;
                frames += 3;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(Some((name.len() + at + 1, frames)));
                }
            }
            _ => frames += 1,
        }
    }
    Some(None)
}

/// The test string without its function calls, or `None` where a call
/// keeps its parenthesis (it never closes) or overflows the JIT stack:
/// either way the declaration is dropped.
fn without_calls(test: &[u8]) -> Option<Vec<u8>> {
    let mut dropped = false;
    let found = scan(test.len(), |i| {
        // The rest no longer matters, and scanning it again for every
        // unbalanced call would be quadratic.
        if dropped {
            return None;
        }
        let found = call(test, i)?;
        dropped |= found.is_none_or(|(_, frames)| frames > FRAME_LIMIT);
        found.map(|(len, _)| (len, Vec::new()))
    });
    (!dropped).then(|| splice(test, &found))
}

/// Whether one `;`-separated declaration (trimmed) is kept.
fn allowed(declaration: &[u8]) -> bool {
    let mut test = declaration.to_vec();
    if let Some(colon) = declaration.iter().position(|&b| b == b':') {
        let name = trim(declaration.get(..colon).unwrap_or_default());
        let value = rest(declaration, colon + 1);
        let (url, gradients) = if is_custom_property(name) {
            let value = trim(value);
            (value.starts_with(b"url("), contains(value, b"-gradient("))
        } else if listed(&SAFE_STYLE_CSS, name) {
            (
                listed(&URL_DATA_TYPES, name),
                listed(&GRADIENT_DATA_TYPES, name),
            )
        } else {
            return false;
        };
        if url && !urls_allowed(value, &mut test) {
            return false;
        }
        if gradients {
            let found = scan(test.len(), |i| gradient(rest(&test, i)));
            if found.iter().any(|&(_, _, frames)| frames > FRAME_LIMIT) {
                return false;
            }
            let found: Vec<Vec<u8>> = found
                .iter()
                .map(|&(start, end, _)| test.get(start..end).unwrap_or_default().to_vec())
                .collect();
            for gradient in found {
                test = str_replace(&test, &gradient, b"");
            }
        }
    }
    without_calls(&test).is_some_and(|test| {
        !test
            .iter()
            .any(|b| matches!(b, b'\\' | b'(' | b'&' | b'=' | b'}'))
            && !contains(&test, b"/*")
    })
}

/// `safecss_filter_attr()`: the declarations of a `style` attribute's
/// (decoded) value that use allowed properties and no unsafe CSS, joined
/// by `;`. The result still needs HTML escaping.
pub fn safecss_filter_attr(css: &[u8]) -> Vec<u8> {
    let css: Vec<u8> = wp_kses_no_null(css)
        .into_iter()
        .filter(|&b| !matches!(b, b'\n' | b'\r' | b'\t'))
        .collect();
    let mut out = Vec::with_capacity(css.len());
    for declaration in trim(&css).split(|&b| b == b';') {
        if declaration.is_empty() {
            continue;
        }
        let declaration = trim(declaration);
        if allowed(declaration) {
            if !out.is_empty() {
                out.push(b';');
            }
            out.extend_from_slice(declaration);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filter(css: &str) -> String {
        String::from_utf8(safecss_filter_attr(css.as_bytes())).unwrap()
    }

    fn kept(css: &str) -> bool {
        filter(css) == css
    }

    #[test]
    fn keeps_allowed_properties() {
        assert_eq!(filter("color:red;COLOR:red;margin:0"), "color:red;margin:0");
        assert_eq!(filter(" ; color : red ; ;"), "color : red;");
        assert_eq!(filter("a:b;;c"), "c");
        assert_eq!(filter("color:red;;color:blue"), "color:red;color:blue");
        assert_eq!(filter("\0wid\nth:1\rpx\t"), "width:1px");
        assert_eq!(filter(":1px;--*:red;foo"), "--*:red;foo");
        assert_eq!(filter(""), "");
    }

    #[test]
    fn custom_properties() {
        assert!(kept("--x-y_Z9:1"));
        assert!(!kept("--:1"));
        assert!(!kept("--a b:1"));
        assert!(!kept("-x:1"));
        assert!(kept("--x: url(a)"));
        assert!(!kept("--x: url(javascript:a)"));
        assert!(!kept("--x:a url(javascript:a)x"));
        assert!(!kept("--x:a url(javascript:a)"));
        assert!(kept("--x:linear-gradient(red,blue)"));
        assert!(kept("--x:linear-gradient(red,(blue))"));
    }

    #[test]
    fn rejects_unsafe_characters() {
        for css in [
            "width:a\\b",
            "width:a(b",
            "width:a&b",
            "width:a=b",
            "width:a}b",
            "width:a/*b",
        ] {
            assert_eq!(filter(css), "", "{css}");
        }
        assert!(kept("width:a/b*c)"));
        assert!(kept("width:a{b<c>"));
    }

    #[test]
    fn checks_urls() {
        assert!(kept("background:url( 'a b' ) url(\"x')"));
        assert!(kept("background:url(\"a\" )"));
        assert!(kept("cursor:url(x),auto"));
        assert!(kept("background:url(http://x/?a=b&c=d)"));
        assert!(!kept("background:url('')x)"));
        assert!(!kept("background:url('')"));
        assert!(!kept("background:url( 0 )"));
        assert!(!kept("background:url( '0' )"));
        assert!(!kept("background:url( \"\" )"));
        assert!(!kept("background:url(javascript:alert(1))"));
        assert!(!kept("background:url( \"javascript:x\")"));
        assert!(!kept("background:url(a) url(data:x)"));
        assert!(!kept("background:url( )"));
        assert!(kept("background:calc(url())"));
        assert!(kept("background:calc(url()) url(a)"));
        assert!(!kept("background:url(a"));
        assert!(!kept("background:URL(a)"));
        assert!(kept("width:calc(url(javascript:x))"));
        assert!(kept("background:var(--a,URL(javascript:x))"));
    }

    #[test]
    fn url_bodies_follow_the_regex() {
        assert_eq!(url_body(b"  'a b'  "), b"a b");
        assert_eq!(url_body(b"'a' b'"), b"a' b");
        assert_eq!(url_body(b"\"a'"), b"\"a'");
        assert_eq!(url_body(b"'"), b"'");
        assert_eq!(url_body(b"'  "), b"'  ");
        assert_eq!(url_body(b"''"), b"");
        assert_eq!(url_body(b" a "), b"a ");
    }

    #[test]
    fn allows_gradients_in_backgrounds() {
        assert!(kept("background:linear-gradient(to right,rgb(1,2,3),#fff)"));
        assert!(kept(
            "background-image:repeating-radial-gradient(red,blue)conic-gradient(red)"
        ));
        assert!(kept("background:url(a.png),linear-gradient(red,blue)"));
        assert!(kept("background:linear-gradient(a)x linear-gradient(a)"));
        assert!(kept("background:calc(linear-gradient(((a))))"));
        assert!(!kept("color:linear-gradient(red,blue)"));
        assert!(!kept("background:linear-gradient(red,blue"));
        assert!(!kept("background:linear-gradient((a)"));
        assert!(!kept("background:linear-gradient((ab"));
        assert!(!kept("background:linear-gradient(rgb(calc(1)))"));
        assert!(!kept("background:repeating-gradient(red)"));
        assert!(!kept("background:Linear-gradient(red)"));
        assert!(kept("background:linear-gradient(linear-gradient(a))"));
    }

    #[test]
    fn removes_gradients_like_str_replace() {
        // Removing the first gradient joins `linear-` and `gradient(b)`
        // into a copy of the second, which goes too.
        assert!(kept(
            "background:linear-linear-gradient(a)gradient(b) linear-gradient(b)"
        ));
    }

    #[test]
    fn strips_allowed_function_calls() {
        assert!(kept("width:calc(1px+calc(2px))calc(3px) var(--x)"));
        assert!(kept(
            "transform:rotateX(1deg) scale3d(1,1,1) translateZ(1px)"
        ));
        assert!(kept("clip-path:xywh(1px 2px 3px 4px)"));
        assert!(kept("clip-path:shape(from 0 0)"));
        assert!(kept("width:-calc(1px)"));
        assert!(kept("width:\u{e9}calc(1px)"));
        assert!(!kept("width:xcalc(1px)"));
        assert!(!kept("width:_calc(1px)"));
        assert!(!kept("width:9calc(1px)"));
        assert!(!kept("width:Calc(1px)"));
        assert!(!kept("width:calc (1px)"));
        assert!(!kept("width:calc(1px"));
        assert!(!kept("width:calc(1px) calc((2px)"));
        assert!(!kept("width:calc((1px) var(x)"));
        assert!(!kept("width:attr(x)"));
        // Removing a call can join `/` and `*` into a comment.
        assert!(!kept("width:a/calc(1)*b"));
        assert!(kept("calc(1px)"));
    }

    #[test]
    fn drops_what_overflows_the_jit_stack() {
        let wrap = |prefix: &str, unit: &str, n: usize, suffix: &str| {
            format!("{prefix}{}{suffix}", unit.repeat(n))
        };
        assert!(kept(&wrap("width:calc(", "a", 8188, ")")));
        assert!(!kept(&wrap("width:calc(", "a", 8189, ")")));
        assert!(kept(&wrap("width:calc(", "(a)", 2047, ")")));
        assert!(!kept(&wrap("width:calc(", "(a)", 2048, ")")));
        assert!(kept(&wrap("background:linear-gradient(", "a", 8190, ")")));
        assert!(!kept(&wrap("background:linear-gradient(", "a", 8191, ")")));
        assert!(kept(&wrap(
            "background:repeating-conic-gradient(",
            "(b)",
            8189,
            ")"
        )));
        assert!(!kept(&wrap(
            "background:repeating-conic-gradient(",
            "(b)",
            8190,
            ")"
        )));
    }
}
