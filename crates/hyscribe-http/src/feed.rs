// SPDX-License-Identifier: AGPL-3.0-or-later

//! The RSS2 feeds: a list query's posts (`?feed=rss2`), and comments, the
//! site's (`?feed=comments-rss2`) or a post's (`?feed=rss2&p=1`), with
//! the caching headers `WP::send_headers()` gives a feed.

use std::future::Future;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hyscribe_db::DbError;
use hyscribe_model::Comment;
use hyscribe_phpser::md5;
use hyscribe_render::date::{self, Zone};
use hyscribe_render::feed::{atom, rss2, rss2_comments};

use crate::query::absint;
use crate::reads::Reads;
use crate::{Outcome, Page, Rendered, Site, finished, html};

/// The months of an HTTP date.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// What a feed lists besides its page's posts.
pub(crate) struct Feed {
    /// `get_lastpostmodified( 'GMT' )`.
    last_post: Option<String>,
    /// For a comment feed, its comments.
    comments: Option<Comments>,
    /// An Atom feed rather than RSS2.
    atom: bool,
}

/// A comment feed's comments, newest first.
pub(crate) struct Comments {
    list: Vec<Comment>,
    /// `get_lastcommentmodified( 'GMT' )`.
    last: Option<String>,
    /// `is_single()`: a post, not a page, was asked for.
    single: bool,
}

/// The feed of `page`, with `comments` its comment feed: of the post given,
/// or the site's, and whether a post (not a page) was asked for.
pub(crate) async fn outcome(
    site: &Site,
    page: impl Future<Output = Result<Page, DbError>>,
    comments: Option<(Option<u64>, bool)>,
    atom: bool,
) -> Result<Outcome, DbError> {
    let limit = absint(&site.option("posts_per_rss"));
    let listed = async {
        let Some((post, single)) = comments else {
            return Ok(None);
        };
        let (list, last) = tokio::join!(
            site.db.feed_comments(post, limit),
            site.db.last_comment_modified()
        );
        list.and_then(|list| last.map(|last| Some(Comments { list, last, single })))
    };
    let (page, last_post, comments) = tokio::join!(page, site.db.last_post_modified(), listed);
    // Each read's error, if any, as `?` would give it.
    page.and_then(|page| {
        let feed = |last_post, comments| {
            Outcome::Feed(
                Box::new(page),
                Feed {
                    last_post,
                    comments,
                    atom,
                },
            )
        };
        last_post.and_then(|last_post| comments.map(|comments| feed(last_post, comments)))
    })
}

/// The feed of `page`, or 304 when the client's copy is current.
pub(crate) async fn respond(
    site: Arc<Site>,
    page: Page,
    feed: Feed,
    headers: &HeaderMap,
) -> Response {
    let changed = match &feed.comments {
        Some(comments) => feed.last_post.clone().max(comments.last.clone()),
        None => feed.last_post.clone(),
    };
    let last_modified = last_modified(changed.as_deref(), page.request.now);
    let etag = format!("\"{}\"", md5(last_modified.as_bytes()));
    let charset = site.option("blog_charset");
    let cache = [
        (header::LAST_MODIFIED, last_modified.clone()),
        (header::ETAG, etag.clone()),
    ];
    if not_modified(headers, &last_modified, &etag) {
        return (StatusCode::NOT_MODIFIED, cache).into_response();
    }
    let kind = if feed.atom { "atom" } else { "rss" };
    let rendering = Arc::clone(&site);
    let Page { request, title, .. } = page;
    let body = tokio::task::spawn_blocking(move || {
        let reads = Reads::new(&rendering, tokio::runtime::Handle::current());
        let last = feed.last_post.as_deref();
        let made = match &feed.comments {
            Some(comments) => rss2_comments(
                &rendering.theme,
                &reads,
                &request,
                title.as_bytes(),
                comments.single,
                &comments.list,
                last,
            ),
            None if feed.atom => atom(&rendering.theme, &reads, &request, title.as_bytes(), last),
            None => rss2(&rendering.theme, &reads, &request, title.as_bytes(), last),
        };
        reads.finish().map(|()| made)
    })
    .await;
    response(finished(&site, body), kind, &charset, cache)
}

/// The feed as the response, or the 500 page when rendering failed.
fn response(
    made: Result<Vec<u8>, Rendered>,
    kind: &str,
    charset: &str,
    cache: [(HeaderName, String); 2],
) -> Response {
    match made {
        Ok(feed) => (
            StatusCode::OK,
            [(
                header::CONTENT_TYPE,
                format!("application/{kind}+xml; charset={charset}"),
            )],
            cache,
            feed,
        )
            .into_response(),
        Err(error) => html(&error),
    }
}

/// The `Last-Modified` of a feed: the latest post's time, or `now` on a
/// site without posts.
fn last_modified(last: Option<&str>, now: i64) -> String {
    let format = "D, d M Y H:i:s";
    let time = match last {
        Some(last) => date::format(format, last, Zone::Utc),
        None => date::format_unix(format, now, Zone::Utc),
    };
    format!("{time} GMT")
}

/// Whether the client's `If-Modified-Since` and `If-None-Match` show it
/// has this version, as `WP::send_headers()` decides: both must agree when
/// both are sent, either will do otherwise.
fn not_modified(headers: &HeaderMap, last_modified: &str, etag: &str) -> bool {
    let header = |name| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map_or("", str::trim)
    };
    let (client_modified, client_etag) = (
        header(header::IF_MODIFIED_SINCE),
        header(header::IF_NONE_MATCH),
    );
    let since = http_date(client_modified).unwrap_or(0);
    let fresh = http_date(last_modified).is_some_and(|ours| since >= ours);
    let same = client_etag == etag;
    if !client_modified.is_empty() && !client_etag.is_empty() {
        fresh && same
    } else {
        fresh || same
    }
}

/// `strtotime()` for an RFC 1123 date (`Tue, 27 Oct 2026 08:08:15 GMT`),
/// the form WordPress sends and clients echo.
fn http_date(text: &str) -> Option<i64> {
    let parts: Vec<&str> = text.split(' ').collect();
    let [_, day, month, year, time, "GMT"] = parts.as_slice() else {
        return None;
    };
    let month = MONTHS.iter().position(|m| m == month)? + 1;
    date::strtotime(&format!("{year}-{month:02}-{day:0>2} {time}"))
}

#[cfg(test)]
mod tests;
