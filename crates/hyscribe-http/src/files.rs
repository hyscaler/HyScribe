// SPDX-License-Identifier: AGPL-3.0-or-later

//! The default theme's and `wp-includes` files, with the
//! `Last-Modified` they have in the pinned release, an `ETag` from their
//! SHA-256, and conditional requests answered as Apache answers them.

use std::time::{Duration, UNIX_EPOCH};

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hyscribe_assets::{release_file, wp_includes};
use hyscribe_render::theme::twentytwentyfive_file;

const THEME: &str = "wp-content/themes/twentytwentyfive/";

/// The file at `path`, answering `headers`' preconditions at Unix time
/// `now`.
pub(crate) fn static_file(path: &str, headers: &HeaderMap, now: u64) -> Option<Response> {
    let path = path.strip_prefix('/').unwrap_or(path);
    let (content_type, bytes) = path
        .strip_prefix(THEME)
        .and_then(twentytwentyfive_file)
        .or_else(|| wp_includes(path.strip_prefix("wp-includes/")?))?;
    // Every served file is in the release (`theme_files_match_php`).
    let (modified, sha) = release_file(path).unwrap_or_default();
    let etag = format!("\"{sha}\"");
    let status = precondition(headers, &etag, modified, now);
    let mut response = match status {
        StatusCode::OK => bytes.into_response(),
        status => status.into_response(),
    };
    let set = response.headers_mut();
    if !content_type.is_empty() && status != StatusCode::NOT_MODIFIED {
        set.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    } else {
        set.remove(header::CONTENT_TYPE);
    }
    let last_modified = httpdate::fmt_http_date(UNIX_EPOCH + Duration::from_secs(modified));
    let values = [(header::LAST_MODIFIED, last_modified), (header::ETAG, etag)];
    for (name, value) in values
        .into_iter()
        .flat_map(|(name, value)| HeaderValue::try_from(value).map(|value| (name, value)))
    {
        set.insert(name, value);
    }
    Some(response)
}

/// Apache's `ap_meets_conditions()` for a GET or HEAD: 412 when `If-Match`
/// or `If-Unmodified-Since` fails, 304 when `If-None-Match` or
/// `If-Modified-Since` (a past date) matches and neither fails, else 200.
fn precondition(headers: &HeaderMap, etag: &str, modified: u64, now: u64) -> StatusCode {
    let value = |name| headers.get(name).and_then(|v| v.to_str().ok());
    let date = |name| {
        let time = httpdate::parse_http_date(value(name)?).ok()?;
        Some(time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()))
    };
    let listed = |list: &str, weak: bool| {
        list.split(',')
            .map(str::trim)
            .any(|tag| tag == "*" || tag == etag || weak && tag.strip_prefix("W/") == Some(etag))
    };
    let failed = value(header::IF_MATCH).is_some_and(|list| !listed(list, false))
        || date(header::IF_UNMODIFIED_SINCE).is_some_and(|since| since < modified);
    if failed {
        return StatusCode::PRECONDITION_FAILED;
    }
    let answers = [
        value(header::IF_NONE_MATCH).map(|list| listed(list, true)),
        date(header::IF_MODIFIED_SINCE).map(|since| since >= modified && since <= now),
    ];
    if answers.contains(&Some(false)) || answers == [None, None] {
        StatusCode::OK
    } else {
        StatusCode::NOT_MODIFIED
    }
}
