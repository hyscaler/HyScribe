// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin's concatenating loaders, `load-styles.php` and
//! `load-scripts.php`, at `/hs-admin/load-styles` and `/hs-admin/load-scripts`.

use std::sync::OnceLock;
use std::time::{Duration, UNIX_EPOCH};

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hyscribe_assets::{wp_admin, wp_includes};
use hyscribe_phpser::{Key, Value, json_decode, md5, parse_str};

const DEPENDENCIES: &[u8] = include_bytes!("../../../wordpress/wp-includes/dependencies.json");

/// The WordPress version the loaders report, `$wp_version`.
const WP_VERSION: &str = "7.1.3";

/// A year, the loaders' `$expires_offset`.
const EXPIRES: u64 = 31_536_000;

/// Which loader a path is.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Loader {
    Styles,
    Scripts,
}

impl Loader {
    /// The loader at `path`, if any.
    pub(crate) fn at(path: &str) -> Option<Loader> {
        match path {
            "/hs-admin/load-styles" => Some(Loader::Styles),
            "/hs-admin/load-scripts" => Some(Loader::Scripts),
            _ => None,
        }
    }
}

fn dependencies() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| json_decode(DEPENDENCIES).unwrap_or(Value::Null))
}

/// `$_GET['load']`: a list's parts in key order, joined, then only the
/// characters handles use, split on commas, each handle once.
fn handles(query: &Value) -> Vec<String> {
    let load = match query.get_str("load") {
        Some(Value::Array(parts)) => {
            let mut parts = parts.clone();
            parts.sort_by(|(a, _), (b, _)| compare_keys(a, b));
            parts
                .iter()
                .map(|(_, part)| part.to_php_string())
                .collect::<Vec<_>>()
                .concat()
        }
        Some(load) => load.to_php_string(),
        None => Vec::new(),
    };
    let kept: Vec<u8> = load
        .into_iter()
        .filter(|b| b.is_ascii_alphanumeric() || matches!(b, b',' | b'_' | b'-'))
        .collect();
    let mut seen = std::collections::HashSet::new();
    String::from_utf8_lossy(&kept)
        .split(',')
        .filter(|handle| seen.insert(handle.to_string()))
        .map(str::to_string)
        .collect()
}

/// `ksort()`'s order: integer keys by value, before string keys.
fn compare_keys(a: &Key, b: &Key) -> std::cmp::Ordering {
    match (a, b) {
        (Key::Int(a), Key::Int(b)) => a.cmp(b),
        (Key::Int(_), Key::String(_)) => std::cmp::Ordering::Less,
        (Key::String(_), Key::Int(_)) => std::cmp::Ordering::Greater,
        (Key::String(a), Key::String(b)) => a.cmp(b),
    }
}

/// `str_replace()` over bytes.
fn replace(haystack: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(haystack.len());
    let mut rest = haystack;
    while let Some(at) = rest.windows(from.len()).position(|w| w == from) {
        out.extend_from_slice(rest.get(..at).unwrap_or_default());
        out.extend_from_slice(to);
        rest = rest.get(at + from.len()..).unwrap_or_default();
    }
    out.extend_from_slice(rest);
    out
}

/// The file at a registered `src`, `get_file()`: empty when there is none.
fn file(src: &[u8]) -> &'static [u8] {
    let src = String::from_utf8_lossy(src);
    let found = match (
        src.strip_prefix("/wp-admin/"),
        src.strip_prefix("/wp-includes/"),
    ) {
        (Some(path), _) => wp_admin(path),
        (_, Some(path)) => wp_includes(path),
        _ => None,
    };
    found.map_or(&[][..], |(_, bytes)| bytes)
}

/// The loader's response to a request with `raw_query`.
pub(crate) fn load(
    loader: Loader,
    raw_query: Option<&str>,
    headers: &HeaderMap,
    now: u64,
) -> Response {
    let query = parse_str(raw_query.unwrap_or_default().as_bytes());
    let handles = handles(&query);
    let kind = match loader {
        Loader::Styles => "styles",
        Loader::Scripts => "scripts",
    };
    let registered = dependencies().get_str(kind).cloned().unwrap_or(Value::Null);
    let items: Vec<(&String, &Value)> = handles
        .iter()
        .filter_map(|handle| Some((handle, registered.get_str(handle)?)))
        .collect();
    // `get_etag()`: a missing version is WordPress's, `false` none.
    let mut etag = format!("WP:{WP_VERSION};");
    for (handle, item) in &items {
        let ver = item.get_str("ver").filter(|ver| **ver != Value::Null);
        let ver = ver.map_or(WP_VERSION.into(), |ver| {
            String::from_utf8_lossy(&ver.to_php_string()).into_owned()
        });
        etag.push_str(&format!("{handle}:{ver};"));
    }
    let etag = format!("W/\"{}\"", md5(etag.as_bytes()));
    let asked = headers.get(header::IF_NONE_MATCH).map(|v| {
        v.as_bytes()
            .iter()
            .filter(|b| **b != b'\\')
            .copied()
            .collect::<Vec<u8>>()
    });
    if asked.as_deref() == Some(etag.as_bytes()) {
        return StatusCode::NOT_MODIFIED.into_response();
    }
    let rtl = query.get_str("dir") == Some(&Value::String(b"rtl".to_vec()));
    let mut out = Vec::new();
    for (_, item) in items {
        let src = item
            .get_str("src")
            .filter(|src| !src.is_empty())
            .map(Value::to_php_string);
        // Styles skip a handle without a file; scripts add its newline.
        let Some(src) = src.or_else(|| (loader == Loader::Scripts).then(Vec::new)) else {
            continue;
        };
        let mirrored = rtl && item.get_str("rtl").is_some_and(|rtl| !rtl.is_empty());
        let path = if mirrored {
            String::from_utf8_lossy(&src)
                .replace(".min.css", "-rtl.min.css")
                .into_bytes()
        } else {
            src.clone()
        };
        let content = [file(&path), b"\n"].concat();
        if loader == Loader::Scripts {
            out.extend(content);
        } else if src.starts_with(b"/wp-includes/css/") {
            // Relative URLs from `wp-includes/css`, as seen from the admin.
            let content = replace(&content, b"../images/", b"../wp-includes/images/");
            let content = replace(&content, b"../js/tinymce/", b"../wp-includes/js/tinymce/");
            out.extend(replace(&content, b"../fonts/", b"../wp-includes/fonts/"));
        } else {
            out.extend(replace(&content, b"../images/", b"images/"));
        }
    }
    let content_type = match loader {
        Loader::Styles => "text/css; charset=UTF-8",
        Loader::Scripts => "application/javascript; charset=UTF-8",
    };
    let expires = httpdate::fmt_http_date(UNIX_EPOCH + Duration::from_secs(now + EXPIRES));
    let mut response = out.into_response();
    let set = response.headers_mut();
    for (name, value) in [
        (header::ETAG, etag),
        (header::CONTENT_TYPE, content_type.to_string()),
        (header::EXPIRES, expires),
        (header::CACHE_CONTROL, format!("public, max-age={EXPIRES}")),
    ] {
        // Every value here is ASCII.
        set.insert(
            name,
            HeaderValue::try_from(value).unwrap_or(HeaderValue::from_static("")),
        );
    }
    response
}
