// SPDX-License-Identifier: AGPL-3.0-or-later

//! The logged in user of a front end request, and what `WP_Query` shows
//! them: unpublished posts on their own (`get_posts()`'s check of a
//! singular query's post), previews, and the private posts lists add.
//! Without a valid cookie a request is a visitor's.

use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, header};
use hyscribe_auth::caps::{User as Capabilities, post_status_flags, post_type_cap};
use hyscribe_model::{Post, PostQuery};
use hyscribe_phpser::Value;
use tokio::runtime::Handle;

use crate::login::{LoginReads, cookies};
use crate::{Site, now};

/// The post types the front end lists, whose private posts a user may read.
const LISTED: [&str; 3] = ["post", "page", "attachment"];

/// The statuses registered `protected`: a user who may edit such a post
/// previews it.
const PROTECTED: [&str; 3] = ["draft", "pending", "future"];

/// A logged in user and their capabilities.
pub(crate) struct Viewer {
    /// What the render and the queries see of them.
    pub(crate) seen: hyscribe_model::Viewer,
    caps: Capabilities,
}

/// `wp_get_current_user()` on the front end, `None` for a visitor or a
/// cookie that does not verify.
pub(crate) async fn current(
    site: &Arc<Site>,
    headers: &HeaderMap,
    post: bool,
) -> Option<Arc<Viewer>> {
    let values = headers.get_all(header::COOKIE).iter();
    let jar = cookies(values.map(HeaderValue::as_bytes));
    if jar.is_empty() {
        return None;
    }
    let site = Arc::clone(site);
    let (now, handle) = (now(), Handle::current());
    let found = tokio::task::spawn_blocking(hyscribe_model::hooks::carry(move || {
        let reads = LoginReads::new(&site, handle, now);
        let (id, _) = reads.current_user(&jar, post)?;
        let caps = Capabilities::load(&reads, site.db.prefix(), id);
        let read_private = LISTED
            .iter()
            .filter(|kind| {
                let cap = post_type_cap(kind, "read_private_posts").unwrap_or_default();
                caps.has_cap(&reads, &cap, &[])
            })
            .map(|kind| (*kind).to_string())
            .collect();
        let seen = hyscribe_model::Viewer {
            user: id,
            read_private,
        };
        // A read that failed leaves the request a visitor's.
        let read = reads.reads.finish().is_ok();
        read.then(|| Arc::new(Viewer { seen, caps }))
    }))
    .await;
    found.ok().flatten()
}

impl Viewer {
    /// `current_user_can( $cap, $id )` on a blocking thread, false when a
    /// read fails.
    pub(crate) fn can_now(&self, site: &Site, handle: Handle, cap: &str, id: u64) -> bool {
        let reads = LoginReads::new(site, handle, now());
        let id = Value::Int(i64::try_from(id).unwrap_or(i64::MAX));
        let can = self.caps.has_cap(&reads, cap, &[id]);
        can && reads.reads.finish().is_ok()
    }

    /// [`Viewer::can_now`] from the runtime.
    async fn can(self: &Arc<Self>, site: &Arc<Site>, cap: &'static str, id: u64) -> bool {
        let (site, viewer, handle) = (Arc::clone(site), Arc::clone(self), Handle::current());
        let checked = tokio::task::spawn_blocking(hyscribe_model::hooks::carry(move || {
            viewer.can_now(&site, handle, cap, id)
        }))
        .await;
        checked.unwrap_or(false)
    }
}

/// What `get_posts()` makes of the one post a singular query found.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Seen {
    /// Left out: the query finds nothing.
    Hidden,
    Shown,
    /// Shown as `is_preview()`.
    Preview,
}

/// `get_posts()`'s check of a singular query's `post`: a public status
/// passes; a visitor sees nothing else; a protected one (a draft, pending
/// or scheduled post) is previewed by who may edit it, a private one shown
/// to who may read it, another registered one never, and an unregistered
/// one shown to who may edit it.
pub(crate) async fn seen(site: &Arc<Site>, viewer: Option<&Arc<Viewer>>, post: &Post) -> Seen {
    let flags = post_status_flags(&post.status);
    if flags.is_some_and(|(public, _)| public) {
        return Seen::Shown;
    }
    let Some(viewer) = viewer else {
        return Seen::Hidden;
    };
    let can = |cap| viewer.can(site, cap, post.id);
    let (cap, granted) = match flags {
        _ if PROTECTED.contains(&post.status.as_str()) => ("edit_post", Seen::Preview),
        Some((_, true)) => ("read_post", Seen::Shown),
        Some(_) => return Seen::Hidden,
        None => ("edit_post", Seen::Shown),
    };
    if can(cap).await {
        granted
    } else {
        Seen::Hidden
    }
}

/// Whether `redirect_canonical()` keeps a redirect to `post`, which it
/// drops for one the user cannot see: a private one they may read is kept.
pub(crate) async fn private_readable(
    site: &Arc<Site>,
    viewer: Option<&Arc<Viewer>>,
    post: &Post,
) -> bool {
    let private = post_status_flags(&post.status).is_some_and(|(_, private)| private);
    match viewer {
        Some(viewer) if private => viewer.can(site, "read_post", post.id).await,
        _ => false,
    }
}

/// `post` as the main query gives it: when previewed, a draft's or pending
/// post's date is now, and `_set_preview()` (the preview nonce verified)
/// puts the title, content and excerpt of its latest autosave in for a
/// user who may edit it (a failed read shows the post as saved).
pub(crate) async fn previewed(
    site: &Arc<Site>,
    viewer: Option<&Arc<Viewer>>,
    (mut post, seen): (Post, Seen),
    (preview, nonce): (bool, bool),
) -> Post {
    if seen == Seen::Preview && post.status != "future" {
        post.date = hyscribe_render::date::format_unix("Y-m-d H:i:s", now(), site.zone());
    }
    let Some(viewer) = viewer.filter(|_| nonce && (preview || seen == Seen::Preview)) else {
        return post;
    };
    if !viewer.can(site, "edit_post", post.id).await {
        return post;
    }
    let autosave = PostQuery {
        post_types: vec!["revision".into()],
        statuses: vec![hyscribe_model::StatusFilter {
            status: "inherit".into(),
            ..hyscribe_model::StatusFilter::default()
        }],
        parents: vec![post.id],
        names: vec![format!("{}-autosave-v1", post.id)],
        limit: 1,
        ..PostQuery::default()
    };
    let found = site.db.posts(&autosave).await.unwrap_or_default();
    if let Some(autosave) = found.into_iter().next() {
        post.title = autosave.title;
        post.content = autosave.content;
        post.excerpt = autosave.excerpt;
    }
    post
}

/// `post`, which the main query shows the user, as
/// `wp_force_plain_post_permalink()` sees it: a private one is shown only
/// to who may read it, so it links as a published one.
pub(crate) fn linked(post: &Post) -> Post {
    let status = if post.status == "private" {
        "publish"
    } else {
        &post.status
    };
    Post {
        status: status.to_string(),
        ..post.clone()
    }
}

/// `query`, a list's, as `WP_Query` makes it for the user: their readable
/// private posts in, and a search's password protected posts.
pub(crate) fn listing(mut query: PostQuery, viewer: Option<&Arc<Viewer>>) -> PostQuery {
    if let Some(viewer) = viewer {
        query.statuses = viewer.seen.statuses(&query.post_types);
        query.logged_in = true;
    }
    query
}
