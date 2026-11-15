// SPDX-License-Identifier: AGPL-3.0-or-later

//! What hs-admin's actions write once they pass PHP's checks (ADR 0012,
//! MVP phase 3): each action's handler runs in one [`Write`], committed
//! when it answers and rolled back when it stops. An action with no
//! handler is answered 501; docs/ADMIN.md says how one is added.

use std::future::Future;
use std::pin::Pin;

use axum::http::StatusCode;
use axum::response::Response;
use hyscribe_admin::actions::Asked;
use hyscribe_admin::posts::Answer;
use hyscribe_admin::terms::{Added, Taxonomy, saved_row, term_added};
use hyscribe_db::{DbError, Write};
use hyscribe_kses::url_host;
use hyscribe_model::Term;

use super::{Admin, HTML, ajax_text};
use crate::redirect::wp_validate_redirect;

pub(super) mod dashboard;
mod options;
pub(super) mod screen;
mod terms;
mod users;

/// Why an action keeps nothing it wrote.
pub(super) enum Stop<T = Answer> {
    /// PHP's `wp_die()` part way, with this answer.
    Died(T),
    /// A write function's error.
    Failed(hyscribe_write::Error),
}

impl<T> From<hyscribe_write::Error> for Stop<T> {
    fn from(err: hyscribe_write::Error) -> Stop<T> {
        Stop::Failed(err)
    }
}

impl<T> From<DbError> for Stop<T> {
    fn from(err: DbError) -> Stop<T> {
        Stop::Failed(err.into())
    }
}

/// A handler's work: the action's answer, or why it stopped.
pub(super) type Saving<'a, T = Answer> = Pin<Box<dyn Future<Output = Result<T, Stop<T>>> + 'a>>;

/// What PHP does once an action's checks pass, reading and writing only
/// through the action's `Write`, so it sees its own writes.
pub(super) type Handler<T = Answer> =
    for<'a, 'w> fn(&'a mut Write<'w>, &'a Asked<'a>) -> Saving<'a, T>;

/// What an `admin-ajax.php` action answers, sent once committed.
#[derive(Debug, PartialEq)]
pub(super) enum Ajax {
    /// `wp_die( $text )`, or the text echoed before `wp_die()`.
    Text(String),
    /// `WP_Ajax_Response::send()`'s XML.
    Xml(String),
    /// The terms list's row of a term saved (`single_row()`).
    TermRow(Taxonomy, Term),
    /// `wp_ajax_add_tag()`'s answer for the term added.
    TermAdded(Taxonomy, Added),
}

/// The handler of `action` (as `hyscribe_admin::actions::action` names it)
/// on `screen` (hs-admin's path without `/hs-admin/`); none while not ported.
pub(super) fn handler(screen: &str, action: &[u8]) -> Option<Handler> {
    match (screen, action) {
        ("users", b"promote") => Some(users::promote),
        ("options", b"update") => Some(options::update),
        ("options", b"adminhash") => Some(options::adminhash),
        ("options", b"dismiss") => Some(options::dismiss),
        ("options-permalink", b"") => Some(options::permalinks),
        ("options-privacy", b"create-privacy-page") => Some(options::create_privacy_page),
        ("edit-tags", b"add-tag") => Some(terms::add_tag),
        ("edit-tags", b"delete") => Some(terms::delete),
        ("edit-tags", b"bulk-delete") => Some(terms::bulk_delete),
        ("edit-tags", b"editedtag") => Some(terms::edited_tag),
        ("post", b"post-quickdraft-save") => Some(dashboard::quickdraft_save),
        ("admin-ajax", b"update-welcome-panel") => Some(dashboard::update_welcome_panel),
        ("admin-ajax", b"closed-postboxes") => Some(screen::closed_postboxes),
        ("admin-ajax", b"hidden-columns") => Some(screen::hidden_columns),
        ("admin-ajax", b"meta-box-order") => Some(screen::meta_box_order),
        (_, b"screen-options") => Some(screen::options),
        _ => None,
    }
}

/// The handler of the `admin-ajax.php` action `action`, if ported.
pub(super) fn ajax_handler(action: &[u8]) -> Option<Handler<Ajax>> {
    match action {
        b"add-tag" => Some(terms::ajax_add_tag),
        b"inline-save-tax" => Some(terms::inline_save_tax),
        b"delete-tag" => Some(terms::ajax_delete_tag),
        _ => None,
    }
}

/// `wp_get_referer()`.
fn referer(asked: &Asked) -> Option<Vec<u8>> {
    let host = url_host(asked.home.as_bytes()).unwrap_or_default();
    asked.wp_get_referer(&|location| wp_validate_redirect(location, &host, b""))
}

/// The response of an Ajax action's `answer`, its rows read once committed.
pub(super) fn ajax_served(admin: &Admin, asked: &Asked, answer: Ajax) -> Response {
    const XML: &str = "text/xml; charset=UTF-8";
    // The rows' edit links come back to `wp_get_referer()`, empty when false.
    let referer = || String::from_utf8_lossy(&referer(asked).unwrap_or_default()).into_owned();
    let (kind, body) = match answer {
        Ajax::Text(text) => (HTML, text),
        Ajax::Xml(xml) => (XML, xml),
        Ajax::TermRow(taxonomy, term) => (HTML, saved_row(admin, taxonomy, &term, &referer())),
        Ajax::TermAdded(taxonomy, added) => (XML, term_added(admin, taxonomy, &added, &referer())),
    };
    ajax_text(StatusCode::OK, kind, body.into_bytes())
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Runs `handler` as one action in `write`, once begun: committed when
/// the handler answers, rolled back when it dies or fails.
pub(super) async fn act<'w, T>(
    write: Result<Write<'w>, DbError>,
    handler: Handler<T>,
    asked: &Asked<'_>,
) -> Result<T, hyscribe_write::Error> {
    let mut write = write?;
    match handler(&mut write, asked).await {
        Ok(answer) => write.commit().await.map(|()| answer).map_err(Into::into),
        Err(Stop::Died(answer)) => Ok(answer),
        Err(Stop::Failed(err)) => Err(err),
    }
}

#[cfg(test)]
mod tests;
