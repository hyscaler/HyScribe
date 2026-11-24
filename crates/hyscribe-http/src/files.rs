// SPDX-License-Identifier: AGPL-3.0-or-later

//! The embedded themes' and `wp-includes` files, with the
//! `Last-Modified` they have in the pinned release, an `ETag` from their
//! SHA-256, and conditional requests answered as Apache answers them; and
//! the files of an installed theme's package, from its `public/`.
//! HyScribe's own files under `/hs-admin/` (ADR 0008) have an `ETag` only.

use std::time::{Duration, UNIX_EPOCH};

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hyscribe_assets::{hyscribe_file, release_file, wp_admin, wp_includes};
use hyscribe_phpser::urldecode;
use hyscribe_render::date::days_from_civil;
use hyscribe_render::theme::theme_file;

use crate::Site;
use crate::uploads::stays;

const THEMES: &str = "wp-content/themes/";

/// The types the web server sends files with, by extension, as for the
/// embedded themes' files; a file of another kind gets none.
const TYPES: [(&str, &str); 16] = [
    ("css", "text/css"),
    ("eot", "application/vnd.ms-fontobject"),
    ("gif", "image/gif"),
    ("html", "text/html"),
    ("jpg", "image/jpeg"),
    ("js", "text/javascript"),
    ("json", "application/json"),
    ("md", "text/markdown"),
    ("mp4", "video/mp4"),
    ("png", "image/png"),
    ("svg", "image/svg+xml"),
    ("ttf", "font/ttf"),
    ("txt", "text/plain"),
    ("webp", "image/webp"),
    ("woff", "font/woff"),
    ("woff2", "font/woff2"),
];

/// `/wp-content/themes/<slug>/<file>` of a theme installed as a package,
/// from its `public/`, as the web server serves a theme's static files,
/// with `Last-Modified` and an `ETag` from its size and time: a path that
/// is not a plain file inside `public/` (`..`, a hidden name, a link
/// leading out) and PHP source are not found.
pub(crate) fn installed_theme_file(
    site: &Site,
    path: &str,
    headers: &HeaderMap,
    now: u64,
) -> Option<Response> {
    let themes = site.extensions.as_ref()?.themes();
    let path = path
        .strip_prefix('/')
        .unwrap_or(path)
        .strip_prefix(THEMES)?;
    let (slug, file) = path.split_once('/')?;
    // Apache decodes the path, a `+` staying a `+`.
    let decoded = String::from_utf8(urldecode(file.replace('+', "%2B").as_bytes())).ok()?;
    let name = decoded.rsplit('/').next().unwrap_or_default();
    let extension = name
        .rsplit_once('.')
        .map_or("", |(_, e)| e)
        .to_ascii_lowercase();
    let php = extension.starts_with("php") || extension == "phtml" || extension == "phar";
    if php || !stays(slug) || !decoded.split('/').all(stays) {
        return None;
    }
    let public = themes.join(slug).join("public").canonicalize().ok()?;
    let file = public.join(&decoded).canonicalize().ok()?;
    let meta = std::fs::metadata(&file)
        .ok()
        .filter(|_| file.starts_with(&public))?;
    let bytes = std::fs::read(&file).ok().filter(|_| meta.is_file())?;
    let since = meta.modified().ok();
    let since = since.and_then(|at| at.duration_since(UNIX_EPOCH).ok());
    let modified = since.unwrap_or_default().as_secs();
    let etag = format!("\"{:x}-{:x}\"", meta.len(), modified);
    let kind = TYPES.iter().find(|(known, _)| *known == extension);
    let kind = kind.map_or("", |(_, kind)| kind);
    Some(conditional(headers, (kind, bytes), (Some(modified), etag), now))
}

/// The file at `path`, answering `headers`' preconditions at Unix time
/// `now`.
pub(crate) fn static_file(path: &str, headers: &HeaderMap, now: u64) -> Option<Response> {
    let path = path.strip_prefix('/').unwrap_or(path);
    if let Some((content_type, bytes, sha)) = path.strip_prefix("hs-admin/").and_then(hyscribe_file)
    {
        let etag = format!("\"{sha}\"");
        return Some(conditional(
            headers,
            (content_type, bytes),
            (None, etag),
            now,
        ));
    }
    let (content_type, bytes) = path
        .strip_prefix(THEMES)
        .and_then(|theme| theme.split_once('/'))
        .and_then(|(slug, file)| theme_file(slug, file))
        .or_else(|| wp_includes(path.strip_prefix("wp-includes/")?))
        .or_else(|| wp_admin(path.strip_prefix("hs-admin/")?))?;
    // Every served file is in the release (`theme_files_match_php`), the
    // admin's under `wp-admin`.
    let release = match path.strip_prefix("hs-admin/") {
        Some(admin) => format!("wp-admin/{admin}"),
        None => path.to_string(),
    };
    let (modified, sha) = release_file(&release).unwrap_or_default();
    let etag = format!("\"{sha}\"");
    Some(conditional(
        headers,
        (content_type, bytes),
        (Some(modified), etag),
        now,
    ))
}

/// A file of `content_type` (none if empty) last modified at Unix time
/// `modified`, if known, with the entity tag `etag`, answering `headers`'
/// preconditions at `now`.
pub(crate) fn conditional(
    headers: &HeaderMap,
    (content_type, bytes): (&str, impl IntoResponse),
    (modified, etag): (Option<u64>, String),
    now: u64,
) -> Response {
    let status = precondition(headers, &etag, modified, now);
    let mut response = match status {
        StatusCode::OK => bytes.into_response(),
        status => status.into_response(),
    };
    let set = response.headers_mut();
    let content_type = HeaderValue::try_from(content_type).ok();
    match content_type.filter(|kind| !kind.is_empty() && status != StatusCode::NOT_MODIFIED) {
        Some(kind) => set.insert(header::CONTENT_TYPE, kind),
        None => set.remove(header::CONTENT_TYPE),
    };
    let last_modified = modified
        .map(|modified| httpdate::fmt_http_date(UNIX_EPOCH + Duration::from_secs(modified)));
    let values = [
        (header::LAST_MODIFIED, last_modified),
        (header::ETAG, Some(etag)),
    ];
    for (name, value) in values.into_iter().flat_map(|(name, value)| {
        value
            .and_then(|value| HeaderValue::try_from(value).ok())
            .map(|value| (name, value))
    }) {
        set.insert(name, value);
    }
    response
}

/// Apache's `ap_meets_conditions()` for a GET or HEAD: 412 when `If-Match`
/// or `If-Unmodified-Since` fails, 304 when `If-None-Match` or
/// `If-Modified-Since` (a past date) matches and neither fails, else 200.
/// Without a modification time, the dates are not compared.
fn precondition(headers: &HeaderMap, etag: &str, modified: Option<u64>, now: u64) -> StatusCode {
    let value = |name| headers.get(name).and_then(|v| v.to_str().ok());
    let date = |name| value(name).filter(|_| modified.is_some());
    let modified = modified.unwrap_or_default() as i64;
    let listed = |list: &str, weak: bool| {
        list.split(',')
            .map(str::trim)
            .any(|tag| tag == "*" || tag == etag || weak && tag.strip_prefix("W/") == Some(etag))
    };
    let failed = value(header::IF_MATCH).is_some_and(|list| !listed(list, false))
        || date(header::IF_UNMODIFIED_SINCE)
            .and_then(http_date)
            .is_some_and(|since| since < modified);
    if failed {
        return StatusCode::PRECONDITION_FAILED;
    }
    let answers = [
        value(header::IF_NONE_MATCH).map(|list| listed(list, true)),
        // A date Apache cannot read counts as one before the file's.
        date(header::IF_MODIFIED_SINCE).map(|since| {
            http_date(since).is_some_and(|since| since >= modified && since <= now as i64)
        }),
    ];
    if answers.contains(&Some(false)) || answers == [None, None] {
        StatusCode::OK
    } else {
        StatusCode::NOT_MODIFIED
    }
}

/// `apr_date_parse_http()`: the Unix time of an RFC 1123, RFC 850 or
/// asctime date, its first word (the weekday) skipped and anything after
/// the time ignored; `None` for anything else or a time before 1970.
pub fn http_date(text: &str) -> Option<i64> {
    const MONTHS: [&[u8]; 12] = [
        b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov",
        b"Dec",
    ];
    let (_, rest) = text.trim_start().split_once(' ')?;
    let bytes = rest.as_bytes();
    let fits = |mask: &[u8]| {
        let padded = bytes.iter().chain(std::iter::repeat(&0));
        mask.iter().zip(padded).all(|(m, b)| match m {
            b'@' => b.is_ascii_uppercase(),
            b'$' => b.is_ascii_lowercase(),
            b'#' => b.is_ascii_digit(),
            b'~' => *b == b' ' || b.is_ascii_digit(),
            literal => b == literal,
        })
    };
    // Spaces read as 0, for asctime's padded day.
    let number = |at: usize, len: usize| {
        let digits = bytes.iter().skip(at).take(len);
        digits.fold(0, |n, b| n * 10 + i64::from(b.saturating_sub(b'0')))
    };
    // The year, the day, and where the month and time start.
    let (year, day, month, time) = if fits(b"## @$$ #### ##:##:## ") {
        (number(7, 4), number(0, 2), 3, 12)
    } else if fits(b"##-@$$-## ##:##:## ") {
        let year = number(7, 2);
        (
            if year < 70 { 2000 } else { 1900 } + year,
            number(0, 2),
            3,
            10,
        )
    } else if fits(b"@$$ ~# ##:##:## ####") {
        (number(16, 4), number(4, 2), 0, 7)
    } else if fits(b"# @$$ #### ##:##:## ") {
        (number(6, 4), number(0, 1), 2, 11)
    } else {
        return None;
    };
    let name = || bytes.iter().skip(month).take(3);
    let month = MONTHS.iter().position(|m| m.iter().eq(name()))? as i64 + 1;
    let (hour, minute, second) = (number(time, 2), number(time + 3, 2), number(time + 6, 2));
    let years = year - 1900;
    // February 29 only in leap years, by APR's own test.
    let leap = years & 3 == 0 && (years % 100 != 0 || years % 400 == 100);
    let last = match month {
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 31,
    };
    let valid = years >= 0 && (1..=last).contains(&day) && hour < 24 && minute < 60 && second < 62;
    let unix = days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second;
    (valid && unix >= 0).then_some(unix)
}

#[cfg(test)]
mod tests;
