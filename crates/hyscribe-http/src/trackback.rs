// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp-trackback.php`, reached by its own path or through the template
//! loader's `tb` var. Trackbacks are closed (docs/SECURITY_MODEL.md), so a
//! POST that carries one is told so and nothing is stored.

use std::collections::HashMap;

use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use hyscribe_db::DbError;
use hyscribe_kses::esc_url_raw;
use hyscribe_phpser::{Value, is_empty_string, parse_str};
use hyscribe_render::formatting::sanitize_text_field;

use crate::query::date_query;
use crate::{Outcome, Site, bad_p, database_error, html, request_uri, route, viewable};

/// Whether the path is `wp-trackback.php`'s, path info included.
pub(crate) fn is_file(path: &str) -> bool {
    path.strip_prefix("/wp-trackback.php")
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
}

/// `wp-trackback.php` run as a file: its own `wp()`, with no template
/// loader, so no redirect, favicon, robots.txt, feed or sitemap, and the
/// status of the query's 404. Every redirect the query would get comes
/// from a 404, which the query's own (a negative or array `p`, or an
/// invalid date) sends with a 200.
pub(crate) async fn outcome(
    site: &Site,
    uri: &Uri,
    query: &HashMap<String, String>,
) -> Result<Outcome, DbError> {
    let (singular, status) = match route(site, "/", uri.query(), query).await? {
        Outcome::Page(page) | Outcome::Feed(page, _) => (singular(&page.request), page.status),
        _ => (None, StatusCode::NOT_FOUND),
    };
    let status = if bad_p(query, uri.query()) || date_query(query).invalid {
        StatusCode::OK
    } else {
        status
    };
    Ok(Outcome::Trackback(post_id(uri, singular), status))
}

/// The post of a singular query, 0 for none found.
pub(crate) fn singular(request: &hyscribe_render::site::Request) -> Option<i64> {
    request
        .singular
        .then(|| request.post.map_or(0, u64::cast_signed))
}

/// `wp-trackback.php`'s post ID: a singular query's post, else none for a
/// set `tb_id`, else the request URI's last segment as an integer.
pub(crate) fn post_id(uri: &Uri, singular: Option<i64>) -> i64 {
    if let Some(id) = singular {
        return id;
    }
    let get = parse_str(uri.query().unwrap_or_default().as_bytes());
    if get.get_str("tb_id").is_some_and(|v| !v.is_empty()) {
        return 0;
    }
    let uri = request_uri(uri);
    let last = uri.rsplit(|&b| b == b'/').next().unwrap_or_default();
    Value::String(last.to_vec()).to_int()
}

/// `$_POST`, which PHP fills from a URL-encoded form body only.
pub(crate) fn form(headers: &HeaderMap, body: &[u8]) -> Value {
    let urlencoded = headers
        .get(header::CONTENT_TYPE)
        .and_then(|t| t.to_str().ok())
        .and_then(|t| t.split(';').next())
        .is_some_and(|t| {
            t.trim()
                .eq_ignore_ascii_case("application/x-www-form-urlencoded")
        });
    parse_str(if urlencoded { body } else { b"" })
}

/// The reply to a trackback request with `form` as `$_POST`. A UTF-7
/// charset ends it with nothing, no post ID gets an error, no trackback
/// fields the post's link, a URL and a title that trackbacks are closed,
/// and anything else nothing. Only a published post or page is linked,
/// where WordPress links any post, telling drafts and private posts apart
/// from IDs with none.
pub(crate) async fn respond(site: &Site, id: i64, status: StatusCode, form: &Value) -> Response {
    let field = |name| {
        form.get_str(name)
            .map(Value::to_php_string)
            .unwrap_or_default()
    };
    let nothing = (status, [(header::CONTENT_TYPE, "text/html; charset=UTF-8")]);
    let charset: Vec<u8> = sanitize_text_field(&field("charset"))
        .to_ascii_uppercase()
        .into_iter()
        .filter(|b| !matches!(b, b',' | b' '))
        .collect();
    if charset == b"UTF-7" {
        return nothing.into_response();
    }
    if id == 0 {
        return error(status, "I really need an ID for this to work.");
    }
    let given = |text: Vec<u8>| !is_empty_string(&text);
    let url = given(esc_url_raw(&field("url")));
    let title = given(sanitize_text_field(&field("title")));
    if url && title {
        return error(status, "Sorry, trackbacks are closed for this item.");
    }
    if url || title || given(sanitize_text_field(&field("blog_name"))) {
        return nothing.into_response();
    }
    let post = match u64::try_from(id) {
        Ok(id) => site.db.post(id).await,
        Err(_) => Ok(None),
    };
    match post {
        Ok(Some(post)) if viewable(&post, &["post", "page"]) => (
            StatusCode::FOUND,
            [(header::LOCATION, site.permalink(&post))],
        )
            .into_response(),
        // `wp_redirect()` of no link sends nothing.
        Ok(_) => nothing.into_response(),
        Err(err) => html(&database_error(site, &err)),
    }
}

/// `trackback_response()` of an error.
fn error(status: StatusCode, message: &str) -> Response {
    let body = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<response>\n<error>1</error>\n<message>{message}</message>\n</response>"
    );
    let content_type = [(header::CONTENT_TYPE, "text/xml; charset=UTF-8")];
    (status, content_type, body).into_response()
}
