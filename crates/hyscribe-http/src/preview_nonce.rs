// SPDX-License-Identifier: AGPL-3.0-or-later

//! `_show_post_preview()`, which WordPress runs on `init`: a request that
//! carries `preview_id` and `preview_nonce` without a valid nonce for the
//! current user dies with a 403, before any query runs.

use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use hyscribe_admin::die::page;
use hyscribe_phpser::{Value, isset};
use tokio::runtime::Handle;

use crate::login::{LoginReads, Names, cookie, cookies, joined, nocache, response};
use crate::{Site, now};

/// The refusal of a request whose preview nonce does not verify, else none.
pub(crate) async fn refuse(
    site: &Arc<Site>,
    headers: &HeaderMap,
    get: &Value,
    post: bool,
) -> Option<Response> {
    let (id, nonce) = (get.get_str("preview_id"), get.get_str("preview_nonce"));
    if !isset(id) || !isset(nonce) {
        return None;
    }
    let action = format!("post_preview_{}", id.map_or(0, Value::to_int));
    let nonce = nonce.map(Value::to_php_string).unwrap_or_default();
    let values = headers.get_all(header::COOKIE).iter();
    let jar = cookies(values.map(HeaderValue::as_bytes));
    let site = Arc::clone(site);
    let (now, handle) = (now(), Handle::current());
    let checked = tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&site, handle, now);
        let names = Names::new(&site);
        if let Some((user, token)) = reads.logged_in_user(cookie(&jar, &names.logged_in()), post) {
            reads.act_as(user, token);
        }
        (!reads.verify_nonce(&nonce, &action)).then(|| denied(&site))
    })
    .await;
    settled(checked)
}

/// The check's answer, or the 500 of a check that panicked.
fn settled(checked: Result<Option<Response>, tokio::task::JoinError>) -> Option<Response> {
    checked.unwrap_or_else(|failed| Some(joined(Err(failed))))
}

/// `wp_die( 'Sorry, you are not allowed to preview drafts.', 403 )`.
fn denied(site: &Site) -> Response {
    let body = page(
        site.public(),
        "WordPress &rsaquo; Error",
        "Sorry, you are not allowed to preview drafts.",
    );
    let mut headers = nocache();
    headers.push((header::CONTENT_TYPE, "text/html; charset=UTF-8".to_string()));
    response(StatusCode::FORBIDDEN, headers, body)
}

#[cfg(test)]
mod tests;
