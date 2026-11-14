// SPDX-License-Identifier: AGPL-3.0-or-later

//! The REST API (hyscribe-rest, ADR 0013) at `?rest_route=` and, with
//! pretty permalinks, `/wp-json/`: the request read, the logged in cookie
//! checked against HyScribe's sessions, and the answer sent.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::sync::Arc;

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, HeaderValue, Method, Uri, header};
use axum::response::Response;
use hyscribe_admin::login::Site as LoginSite;
use hyscribe_auth::caps::{CapabilityData, User as Capabilities};
use hyscribe_model::{Post, Term, TermQuery};
use hyscribe_phpser::{Value, addslashes, is_empty_string, parse_str};
use hyscribe_render::link::rest_url;
use hyscribe_render::site::Site as RenderSite;
use tokio::runtime::Handle;

use crate::login::{LoginReads, Names, cookie, cookies, joined};
use crate::{Site, now, trackback};

/// The `rest_route` query var, `rest_api_loaded()` serves the request when
/// it is not empty: `$_GET`'s, slashed as WordPress slashes it, else the
/// rewrite rules'. Any other value than a string is not ported.
pub(crate) fn route(uri: &Uri, query: &HashMap<String, String>) -> Option<Option<Vec<u8>>> {
    let get = parse_str(uri.query().unwrap_or_default().as_bytes());
    let route = match get.get_str("rest_route") {
        Some(Value::String(route)) => addslashes(route),
        Some(_) => return Some(None),
        None => query.get("rest_route")?.as_bytes().to_vec(),
    };
    (!is_empty_string(&route)).then_some(Some(route))
}

/// The request as the API reads it.
pub(crate) struct Asked {
    pub(crate) method: Method,
    pub(crate) uri: Uri,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
    pub(crate) route: Vec<u8>,
}

/// The answer to a REST request, read on a blocking thread.
pub(crate) async fn respond(site: &Arc<Site>, asked: Asked) -> Response {
    let site = Arc::clone(site);
    let (now, handle) = (now(), Handle::current());
    joined(tokio::task::spawn_blocking(move || answer(&site, &asked, now, handle)).await)
}

/// The site and user as the API reads them.
struct Rest<'a> {
    reads: LoginReads<'a>,
    site: &'a Site,
    /// The user of the logged in cookie, 0 for none.
    user: u64,
    caps: OnceCell<Capabilities>,
}

impl hyscribe_rest::Site for Rest<'_> {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        CapabilityData::option(&self.reads, name)
    }

    fn rest_url(&self, path: &str) -> String {
        let url = rest_url(self.site.home.as_bytes(), &self.site.permalinks(), path);
        String::from_utf8_lossy(&url).into_owned()
    }

    fn verify_nonce(&self, nonce: &[u8]) -> bool {
        self.reads.verify_nonce(nonce, "wp_rest")
    }

    fn create_nonce(&self) -> String {
        self.reads.nonce("wp_rest")
    }

    fn can(&self, user: u64, cap: &str, args: &[Value]) -> bool {
        let caps = || {
            self.caps
                .get_or_init(|| Capabilities::load(&self.reads, self.site.db.prefix(), self.user))
        };
        user != 0 && caps().has_cap(&self.reads, cap, args)
    }

    fn terms(&self, query: &TermQuery) -> Vec<Term> {
        RenderSite::term_query(&self.reads.reads, query)
    }

    fn count_terms(&self, query: &TermQuery) -> u64 {
        self.reads.reads.wait(self.site.db.term_query_count(query))
    }

    fn term_link(&self, taxonomy: &str, term: &Term) -> String {
        let link = hyscribe_render::permalinks::term_link(&self.reads.reads, taxonomy, term);
        String::from_utf8_lossy(&link).into_owned()
    }

    fn post(&self, id: u64) -> Option<Post> {
        RenderSite::post(&self.reads.reads, id)
    }
}

fn answer(site: &Site, asked: &Asked, now: i64, handle: Handle) -> Response {
    let header = |name: &str| asked.headers.get(name).map(HeaderValue::as_bytes);
    let cookies = cookies(
        asked
            .headers
            .get_all(header::COOKIE)
            .iter()
            .map(HeaderValue::as_bytes),
    );
    let post = asked.method == Method::POST;
    let mut reads = LoginReads::new(site, handle, now);
    let logged_in = cookie(&cookies, &Names::new(site).logged_in());
    let (user, token) = reads.logged_in_user(logged_in, post).unwrap_or_default();
    reads.act_as(user, token);
    let rest = Rest {
        reads,
        site,
        user,
        caps: OnceCell::new(),
    };
    let entries = |value: Value| value.as_array().unwrap_or_default().to_vec();
    let get = entries(parse_str(asked.uri.query().unwrap_or_default().as_bytes()));
    let form = if post {
        entries(trackback::form(&asked.headers, &asked.body))
    } else {
        Vec::new()
    };
    let request = hyscribe_rest::Request {
        method: asked.method.as_str(),
        route: &asked.route,
        get: &get,
        post: &form,
        cookie_user: user,
        origin: header("origin"),
        nonce: header("x-wp-nonce"),
        method_override: header("x-http-method-override"),
    };
    let served = hyscribe_rest::serve(&rest, &request);
    let Rest { reads, .. } = rest;
    if let Err(err) = reads.reads.finish() {
        return crate::html(crate::database_error(site, &err));
    }
    let response = served.headers.into_iter().fold(
        Response::builder().status(served.status),
        |response, (name, value)| response.header(name, value),
    );
    // Every header is the site's URL escaped, a request header's value or
    // fixed text, which HTTP carries.
    response.body(Body::from(served.body)).unwrap_or_default()
}

#[cfg(test)]
mod tests;
