// SPDX-License-Identifier: AGPL-3.0-or-later

//! The default theme's and `wp-includes` files, with the
//! `Last-Modified` they have in the pinned release, an `ETag` from their
//! SHA-256, and conditional requests answered as Apache answers them.

use std::time::{Duration, UNIX_EPOCH};

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hyscribe_assets::{release_file, wp_includes};
use hyscribe_render::date::days_from_civil;
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
    let modified = modified as i64;
    let listed = |list: &str, weak: bool| {
        list.split(',')
            .map(str::trim)
            .any(|tag| tag == "*" || tag == etag || weak && tag.strip_prefix("W/") == Some(etag))
    };
    let failed = value(header::IF_MATCH).is_some_and(|list| !listed(list, false))
        || value(header::IF_UNMODIFIED_SINCE)
            .and_then(http_date)
            .is_some_and(|since| since < modified);
    if failed {
        return StatusCode::PRECONDITION_FAILED;
    }
    let answers = [
        value(header::IF_NONE_MATCH).map(|list| listed(list, true)),
        // A date Apache cannot read counts as one before the file's.
        value(header::IF_MODIFIED_SINCE).map(|since| {
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
fn http_date(text: &str) -> Option<i64> {
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
