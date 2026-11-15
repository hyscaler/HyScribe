// SPDX-License-Identifier: AGPL-3.0-or-later

//! What hs-admin's actions write once they pass PHP's checks (ADR 0012,
//! MVP phase 3): each action's handler runs in one [`Write`], committed
//! when it answers and rolled back when it stops. An action with no
//! handler is answered 501; docs/ADMIN.md says how one is added.

use std::future::Future;
use std::pin::Pin;

use hyscribe_admin::actions::{ActionSite, Asked};
use hyscribe_admin::posts::Answer;
use hyscribe_db::{DbError, Write};

mod comments;
mod users;

/// Why an action keeps nothing it wrote.
pub(super) enum Stop {
    /// PHP's `wp_die()` part way, with this answer.
    Died(Answer),
    /// A write function's error.
    Failed(hyscribe_write::Error),
}

impl From<hyscribe_write::Error> for Stop {
    fn from(err: hyscribe_write::Error) -> Stop {
        Stop::Failed(err)
    }
}

impl From<DbError> for Stop {
    fn from(err: DbError) -> Stop {
        Stop::Failed(err.into())
    }
}

/// A handler's work: the action's answer, or why it stopped.
pub(super) type Saving<'a> = Pin<Box<dyn Future<Output = Result<Answer, Stop>> + 'a>>;

/// What a handler reads of the site and the current user as the checks
/// read them, not seeing the action's writes; each call goes through [`read`].
pub(super) trait Acting: ActionSite {
    /// `admin_url( $path )`.
    fn admin_url(&self, path: &str) -> String;
    /// `get_comment_link()` of the comment `id`.
    fn comment_url(&self, id: u64) -> String;
}

impl Acting for super::Admin<'_> {
    fn admin_url(&self, path: &str) -> String {
        hyscribe_admin::assets::Urls::admin_url(self, path)
    }
    fn comment_url(&self, id: u64) -> String {
        let comment = hyscribe_admin::comments::CommentsSite::parent_comment(self, id);
        comment.map(|(link, _)| link).unwrap_or_default()
    }
}

/// A call to the [`Acting`] site from a handler, which blocks on its reads.
pub(super) fn read<T>(call: impl FnOnce() -> T) -> T {
    tokio::task::block_in_place(call)
}

/// What PHP does once an action's checks pass, reading and writing the
/// database only through the action's `Write`, so it sees its own writes.
pub(super) type Handler =
    for<'a, 'w> fn(&'a mut Write<'w>, &'a Asked<'a>, &'a dyn Acting) -> Saving<'a>;

/// The handler of `action` (as `hyscribe_admin::actions::action` names it)
/// on `screen` (hs-admin's path without `/hs-admin/`); none while not ported.
pub(super) fn handler(screen: &str, action: &[u8]) -> Option<Handler> {
    match (screen, action) {
        ("users", b"promote") => Some(users::promote),
        ("edit-comments", _) => Some(comments::bulk),
        ("comment", action) => comments::moderation(action),
        ("admin-ajax", b"dim-comment") => Some(comments::dim),
        ("admin-ajax", b"delete-comment") => Some(comments::delete),
        _ => None,
    }
}

/// Runs `handler` as one action in `write`, once begun: committed when
/// the handler answers, rolled back when it dies or fails.
pub(super) async fn act<'w>(
    write: Result<Write<'w>, DbError>,
    handler: Handler,
    asked: &Asked<'_>,
    site: &dyn Acting,
) -> Result<Answer, hyscribe_write::Error> {
    let mut write = write?;
    match handler(&mut write, asked, site).await {
        Ok(answer) => write.commit().await.map(|()| answer).map_err(Into::into),
        Err(Stop::Died(answer)) => Ok(answer),
        Err(Stop::Failed(err)) => Err(err),
    }
}

#[cfg(test)]
mod tests;
