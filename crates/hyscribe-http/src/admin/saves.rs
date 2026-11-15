// SPDX-License-Identifier: AGPL-3.0-or-later

//! What hs-admin's actions write once they pass PHP's checks (ADR 0012,
//! MVP phase 3): each action's handler runs in one [`Write`], committed
//! when it answers and rolled back when it stops. An action with no
//! handler is answered 501; docs/ADMIN.md says how one is added.

use std::future::Future;
use std::pin::Pin;

use hyscribe_admin::actions::Asked;
use hyscribe_admin::posts::Answer;
use hyscribe_db::{DbError, Write};

pub(super) mod dashboard;
pub(super) mod screen;
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

/// What PHP does once an action's checks pass, reading and writing only
/// through the action's `Write`, so it sees its own writes.
pub(super) type Handler = for<'a, 'w> fn(&'a mut Write<'w>, &'a Asked<'a>) -> Saving<'a>;

/// The handler of `action` (as `hyscribe_admin::actions::action` names it)
/// on `screen` (hs-admin's path without `/hs-admin/`); none while not ported.
pub(super) fn handler(screen: &str, action: &[u8]) -> Option<Handler> {
    match (screen, action) {
        ("users", b"promote") => Some(users::promote),
        ("post", b"post-quickdraft-save") => Some(dashboard::quickdraft_save),
        ("admin-ajax", b"update-welcome-panel") => Some(dashboard::update_welcome_panel),
        ("admin-ajax", b"closed-postboxes") => Some(screen::closed_postboxes),
        ("admin-ajax", b"hidden-columns") => Some(screen::hidden_columns),
        ("admin-ajax", b"meta-box-order") => Some(screen::meta_box_order),
        (_, b"screen-options") => Some(screen::options),
        _ => None,
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Runs `handler` as one action in `write`, once begun: committed when
/// the handler answers, rolled back when it dies or fails.
pub(super) async fn act<'w>(
    write: Result<Write<'w>, DbError>,
    handler: Handler,
    asked: &Asked<'_>,
) -> Result<Answer, hyscribe_write::Error> {
    let mut write = write?;
    match handler(&mut write, asked).await {
        Ok(answer) => write.commit().await.map(|()| answer).map_err(Into::into),
        Err(Stop::Died(answer)) => Ok(answer),
        Err(Stop::Failed(err)) => Err(err),
    }
}

#[cfg(test)]
mod tests;
