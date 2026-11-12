// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin's screens at `/hs-admin/` (ADR 0008): `auth_redirect()` before
//! any of them. The screens themselves are not ported yet.

use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::Response;
use hyscribe_admin::login::Session;
use hyscribe_phpser::{Value, urlencode};
use tokio::runtime::Handle;

use crate::login::{LoginReads, Names, cookie, cookies, is_ssl, joined, response};
use crate::redirect::wp_sanitize_redirect;
use crate::{Site, now};

/// Whether `path` is an hs-admin screen: its files have an extension and
/// are served, or missing, as Apache serves `wp-admin`'s.
pub(crate) fn is_admin(path: &str) -> bool {
    let screen = path
        .rsplit('/')
        .next()
        .is_none_or(|name| !name.contains('.'));
    (path == "/hs-admin" || path.starts_with("/hs-admin/")) && screen
}

/// The answer to a request for an admin screen.
pub(crate) async fn respond(
    site: &Arc<Site>,
    uri: Uri,
    headers: HeaderMap,
    post: bool,
) -> Response {
    let site = Arc::clone(site);
    let (now, handle) = (now(), Handle::current());
    joined(
        tokio::task::spawn_blocking(move || answer(&site, &uri, &headers, post, now, handle)).await,
    )
}

/// A `302` to `location`, as `wp_redirect()` sends it.
fn redirect(mut headers: Vec<(header::HeaderName, String)>, location: &[u8]) -> Response {
    let location = String::from_utf8_lossy(&wp_sanitize_redirect(location)).into_owned();
    headers.push((
        header::HeaderName::from_static("x-redirect-by"),
        "WordPress".into(),
    ));
    headers.push((header::LOCATION, location));
    response(StatusCode::FOUND, headers, Vec::new())
}

/// A logged in user's answer: over HTTPS if their `use_ssl` asks for it.
fn admitted(https: bool, use_ssl: bool, here: &[u8]) -> Response {
    if !https && use_ssl {
        redirect(Vec::new(), &[&b"https://"[..], here].concat())
    } else {
        response(StatusCode::NOT_IMPLEMENTED, Vec::new(), Vec::new())
    }
}

fn answer(
    site: &Site,
    uri: &Uri,
    headers: &HeaderMap,
    post: bool,
    now: i64,
    handle: Handle,
) -> Response {
    let names = Names::new(site);
    let https = is_ssl(site);
    let cookies = cookies(
        headers
            .get_all(header::COOKIE)
            .iter()
            .map(HeaderValue::as_bytes),
    );
    let reads = LoginReads::new(site, handle, now);
    let session = reads.session(cookie(&cookies, &names.auth(https)), post, https);
    // `$_SERVER['HTTP_HOST'] . $_SERVER['REQUEST_URI']`.
    let host = headers
        .get(header::HOST)
        .map_or(&b""[..], HeaderValue::as_bytes);
    let requested = uri.path_and_query().map_or("", |p| p.as_str());
    let here = [host, requested.as_bytes()].concat();
    let answer = match session {
        Session::User(id) => {
            let use_ssl = reads
                .user_option(id, "use_ssl")
                .is_some_and(|v| !Value::String(v).is_empty());
            admitted(https, use_ssl, &here)
        }
        _ => {
            // `wp_login_url( $redirect, true )` after `nocache_headers()`.
            let scheme: &[u8] = if https { b"https://" } else { b"http://" };
            let back = urlencode(&[scheme, &here].concat());
            let login = format!(
                "{}{}?redirect_to={}&reauth=1",
                site.option("siteurl"),
                crate::login::PATH,
                String::from_utf8_lossy(&back)
            );
            let nocache = vec![
                (header::EXPIRES, "Wed, 11 Jan 1984 05:00:00 GMT".to_string()),
                (
                    header::CACHE_CONTROL,
                    "no-cache, must-revalidate, max-age=0, no-store, private".to_string(),
                ),
            ];
            redirect(nocache, login.as_bytes())
        }
    };
    match reads.reads.finish() {
        Ok(()) => answer,
        Err(err) => crate::html(crate::database_error(site, &err)),
    }
}

#[cfg(test)]
mod tests;
