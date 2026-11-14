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
use hyscribe_model::{Comment, CommentQuery, CommentRow, Post, Term, TermQuery, User, UserQuery};
use hyscribe_phpser::{Key, Value, addslashes, is_empty_string, parse_str};
use hyscribe_render::link::rest_url;
use hyscribe_render::permalinks::{Permalinks, author_posts_url, permalink};
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

    fn nonce(&self, action: &str) -> String {
        self.reads.nonce(action)
    }

    fn can(&self, user: u64, cap: &str, args: &[Value]) -> bool {
        let caps = || {
            self.caps
                .get_or_init(|| Capabilities::load(&self.reads, self.site.db.prefix(), self.user))
        };
        user != 0 && caps().has_cap(&self.reads, cap, args)
    }

    fn public_users(&self) -> bool {
        self.site.public_users
    }

    fn reads(&self) -> &dyn RenderSite {
        &self.reads.reads
    }

    fn theme(&self) -> &hyscribe_render::theme::Theme {
        &self.site.theme
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

    fn table_prefix(&self) -> String {
        self.site.db.prefix().into()
    }

    fn comments(&self, query: &CommentQuery) -> Vec<CommentRow> {
        self.reads.reads.wait(self.site.db.comment_query(query))
    }

    fn count_comments(&self, query: &CommentQuery) -> u64 {
        self.reads
            .reads
            .wait(self.site.db.count_comment_query(query))
    }

    fn comment_link(&self, comment: &Comment) -> String {
        let post = (comment.post_id != 0)
            .then(|| RenderSite::post(&self.reads.reads, comment.post_id))
            .flatten();
        let link = post.map(|post| permalink(&self.reads.reads, &post));
        let link = String::from_utf8_lossy(&link.unwrap_or_default()).into_owned();
        let links = Permalinks::of(&self.reads.reads);
        let link = if links.pretty() {
            links.trailing(&link)
        } else {
            link
        };
        format!("{link}#comment-{}", comment.id)
    }

    fn users(&self, query: &UserQuery) -> Vec<User> {
        self.reads.reads.wait(self.site.db.user_query(query))
    }

    fn count_users(&self, query: &UserQuery) -> u64 {
        self.reads.reads.wait(self.site.db.count_user_query(query))
    }

    fn user_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        CapabilityData::user_meta(&self.reads, id, key)
    }

    fn user_registered(&self, id: u64) -> String {
        self.reads.reads.wait(self.site.db.user_registered(id))
    }

    fn allcaps(&self, id: u64) -> Vec<(Key, Value)> {
        let user = Capabilities::load(&self.reads, self.site.db.prefix(), id);
        user.allcaps().to_vec()
    }

    fn author_link(&self, user: &User) -> String {
        let id = i64::try_from(user.id).unwrap_or_default();
        let link = author_posts_url(&self.reads.reads, id);
        String::from_utf8_lossy(&link).into_owned()
    }

    fn has_posts(&self, author: u64, types: &[(String, bool)]) -> bool {
        self.reads.reads.wait(self.site.db.has_posts(author, types))
    }

    fn posts_of_type(&self, post_type: &str) -> Vec<Post> {
        let reads = &self.reads.reads;
        reads.wait(self.site.db.posts_of_type(post_type))
    }

    fn revisions(&self, post: u64) -> Vec<u64> {
        self.reads.reads.wait(self.site.db.revisions(post))
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
    // A form's fields are read; any other body, and a form sent with
    // another method, only PHP reads.
    let unread_body = !asked.body.is_empty() && form.is_empty();
    let request = hyscribe_rest::Request {
        method: asked.method.as_str(),
        route: &asked.route,
        uri: asked
            .uri
            .path_and_query()
            .map_or(b"/", |uri| uri.as_str().as_bytes()),
        get: &get,
        post: &form,
        cookie_user: user,
        origin: header("origin"),
        nonce: header("x-wp-nonce"),
        method_override: header("x-http-method-override"),
        unread_body,
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
