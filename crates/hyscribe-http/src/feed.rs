// SPDX-License-Identifier: AGPL-3.0-or-later

//! The RSS2 feeds: a list query's posts (`?feed=rss2`), and comments, the
//! site's (`?feed=comments-rss2`) or a post's (`?feed=rss2&p=1`), with
//! the caching headers `WP::send_headers()` gives a feed.

use std::future::Future;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderName, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hyscribe_db::DbError;
use hyscribe_model::{Comment, Post, PostQuery};
use hyscribe_phpser::md5;
use hyscribe_render::date::{self, Zone};
use hyscribe_render::feed::{atom, atom_comments, rdf, rss2, rss2_comments, rss092};

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
    format: Format,
    /// Where `redirect_canonical()` sends the feed, if elsewhere.
    pub(crate) redirect: Option<String>,
    /// The query found nothing past its first page: an empty feed of the
    /// 404 page, sent as a 404.
    not_found: bool,
}

/// A feed's format.
#[derive(Clone, Copy)]
pub(crate) enum Format {
    Rss2,
    Atom,
    Rdf,
    Rss092,
}

impl Format {
    /// `feed_content_type()`'s subtype.
    fn kind(self) -> &'static str {
        match self {
            Format::Rss2 | Format::Rss092 => "rss",
            Format::Atom => "atom",
            Format::Rdf => "rdf",
        }
    }
}

/// A comment feed's comments, newest first.
pub(crate) struct Comments {
    list: Vec<Comment>,
    /// `get_lastcommentmodified( 'GMT' )`.
    last: Option<String>,
    /// `is_single()`: a post, not a page, was asked for.
    single: bool,
    /// The posts of the comments, for a feed of many posts' comments.
    posts: Option<Vec<Post>>,
}

/// Whose comments a comment feed lists.
pub(crate) enum CommentsOf {
    /// The site's.
    Site,
    /// A post's or page's, and whether a post (not a page) was asked for.
    Post(u64, bool),
    /// Those on the posts a search or archive matches.
    Posts(Box<PostQuery>),
}

/// The feed of `page`, with `comments` for a comment feed.
pub(crate) async fn outcome(
    site: &Site,
    page: impl Future<Output = Result<Page, DbError>>,
    comments: Option<CommentsOf>,
    format: Format,
    redirect: Option<String>,
    not_found: bool,
) -> Result<Outcome, DbError> {
    let (page, last_post, comments) =
        tokio::join!(page, site.db.last_post_modified(), listed(site, comments));
    // Each read's error, if any, as `?` would give it.
    page.and_then(|page| {
        last_post.and_then(|last_post| {
            comments.map(|comments| {
                let feed = Feed {
                    last_post,
                    comments,
                    format,
                    redirect,
                    not_found,
                };
                feed_outcome(page, feed)
            })
        })
    })
}

/// `page`'s feed. A feed of many posts' comments lists the posts of those
/// comments, as `WP_Query::get_posts()` does.
fn feed_outcome(mut page: Page, mut feed: Feed) -> Outcome {
    if let Some(posts) = feed.comments.as_mut().and_then(|c| c.posts.take()) {
        page.request.posts = posts;
    }
    Outcome::Feed(Box::new(page), feed)
}

/// The comments `of` lists, none for a feed of posts. Not generic, unlike
/// [`outcome`], so its branches are counted once.
async fn listed(site: &Site, of: Option<CommentsOf>) -> Result<Option<Comments>, DbError> {
    let Some(of) = of else {
        return Ok(None);
    };
    let limit = absint(&site.option("posts_per_rss"));
    let single = matches!(of, CommentsOf::Post(_, true));
    let list = async {
        match &of {
            CommentsOf::Site => site.db.feed_comments(None, limit).await,
            CommentsOf::Post(post, _) => site.db.feed_comments(Some(*post), limit).await,
            CommentsOf::Posts(query) => site.db.archive_feed_comments(query, limit).await,
        }
    };
    let (list, last) = tokio::join!(list, site.db.last_comment_modified());
    let query = match of {
        CommentsOf::Site => Some(PostQuery {
            limit: u32::try_from(limit).unwrap_or(u32::MAX),
            ..PostQuery::default()
        }),
        CommentsOf::Post(..) => None,
        CommentsOf::Posts(query) => Some(*query),
    };
    let posts = match (query, &list) {
        (Some(query), Ok(list)) => {
            let include = Some(list.iter().map(|c| c.post_id).collect());
            site.db
                .posts(&PostQuery { include, ..query })
                .await
                .map(Some)
        }
        _ => Ok(None),
    };
    list.and_then(|list| {
        last.and_then(|last| {
            posts.map(|posts| {
                Some(Comments {
                    list,
                    last,
                    single,
                    posts,
                })
            })
        })
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
    let kind = feed.format.kind();
    let content_type = format!("application/{kind}+xml; charset={charset}");
    if let Some(location) = feed.redirect {
        let headers = [
            (header::CONTENT_TYPE, content_type),
            (header::LOCATION, location),
        ];
        return (StatusCode::MOVED_PERMANENTLY, headers, cache).into_response();
    }
    let rendering = Arc::clone(&site);
    let Page { request, title, .. } = page;
    let body = tokio::task::spawn_blocking(move || {
        let reads = Reads::new(&rendering, tokio::runtime::Handle::current());
        let last = feed.last_post.as_deref();
        let (theme, title) = (&rendering.theme, title.as_bytes());
        // RDF and RSS 0.92 list the query's posts, comment feed or not.
        let made = match (feed.format, &feed.comments) {
            (Format::Atom, Some(comments)) => {
                atom_comments(theme, &reads, &request, title, &comments.list, last)
            }
            (Format::Rss2, Some(comments)) => rss2_comments(
                theme,
                &reads,
                &request,
                title,
                comments.single,
                &comments.list,
                last,
            ),
            (Format::Atom, None) => atom(theme, &reads, &request, title, last),
            (Format::Rss2, None) => rss2(theme, &reads, &request, title, last),
            (Format::Rdf, _) => rdf(theme, &reads, &request, title, last),
            (Format::Rss092, _) => rss092(theme, &reads, &request, title, last),
        };
        reads.finish().map(|()| made)
    })
    .await;
    let status = if feed.not_found {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    response(finished(&site, body), status, content_type, cache)
}

/// The feed as the response, or the 500 page when rendering failed.
fn response(
    made: Result<Vec<u8>, Rendered>,
    status: StatusCode,
    content_type: String,
    cache: [(HeaderName, String); 2],
) -> Response {
    match made {
        Ok(feed) => (status, [(header::CONTENT_TYPE, content_type)], cache, feed).into_response(),
        Err(error) => html(error),
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
