// SPDX-License-Identifier: AGPL-3.0-or-later

//! What hs-admin's actions write once they pass PHP's checks (ADR 0012,
//! MVP phase 3): each action's handler runs in one [`Write`], committed
//! when it answers and rolled back when it stops. An action with no
//! handler is answered 501; docs/ADMIN.md says how one is added.

use std::future::Future;
use std::pin::Pin;

use axum::http::StatusCode;
use axum::response::Response;
use hyscribe_admin::actions::{ActionSite, Asked};
use hyscribe_admin::posts::{Answer, PAGE, POST, PostsSite};
use hyscribe_phpser::{Value, lookup};
use tokio::runtime::Handle;

use crate::login::LoginReads;
use hyscribe_admin::terms::{Added, Taxonomy, saved_row, term_added};
use hyscribe_db::{DbError, Write};
use hyscribe_kses::url_host;
use hyscribe_model::Term;

use super::{Admin, HTML, ajax_text};
use crate::redirect::wp_validate_redirect;

mod comments;
pub(super) mod dashboard;
mod options;
mod posts;
pub(super) mod screen;
mod terms;
mod user_edit;

pub(super) use user_edit::destroy_sessions;
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

/// What a handler reads of the site and the current user as the checks
/// read them, not seeing the action's writes; each call goes through [`read`].
pub(super) trait Acting: ActionSite {
    /// `admin_url( $path )`.
    fn admin_url(&self, path: &str) -> String;
    /// `get_comment_link()` of the comment `id`.
    fn comment_url(&self, id: u64) -> String;
    /// `user_can( $user, $cap )`.
    fn user_can(&self, user: u64, cap: &str) -> bool;
    /// `wp_create_nonce( $action )` for the current user.
    fn create_nonce(&self, action: &str) -> String;
}

impl Acting for Admin<'_> {
    fn admin_url(&self, path: &str) -> String {
        hyscribe_admin::assets::Urls::admin_url(self, path)
    }
    fn comment_url(&self, id: u64) -> String {
        let comment = hyscribe_admin::comments::CommentsSite::parent_comment(self, id);
        comment.map(|(link, _)| link).unwrap_or_default()
    }
    fn user_can(&self, user: u64, cap: &str) -> bool {
        hyscribe_admin::profile::ProfileSite::user_can(self, user, cap)
    }
    fn create_nonce(&self, action: &str) -> String {
        hyscribe_admin::page::PageSite::nonce(self, action)
    }
}

/// A call to the [`Acting`] site from a handler, which blocks on its reads.
pub(super) fn read<T>(call: impl FnOnce() -> T) -> T {
    tokio::task::block_in_place(call)
}

/// What PHP does once an action's checks pass, reading and writing the
/// database only through the action's `Write`, so it sees its own writes.
pub(super) type Handler<T = Answer> =
    for<'a, 'w> fn(&'a mut Write<'w>, &'a Asked<'a>, &'a dyn Acting) -> Saving<'a, T>;

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
    /// A comment's row of the comments list in `WP_Ajax_Response` XML.
    CommentRow(comments::Row),
    /// Quick Edit's answer: the posts list's row of the post saved.
    PostRow(u64),
}

/// The handler of `action` (as `hyscribe_admin::actions::action` names it)
/// on `screen` (hs-admin's path without `/hs-admin/`); none while not ported.
pub(super) fn handler(screen: &str, action: &[u8]) -> Option<Handler> {
    match (screen, action) {
        ("users", b"promote") => Some(users::promote),
        ("post", b"trash") => Some(posts::trash),
        ("post", b"untrash") => Some(posts::untrash),
        ("post", b"delete") => Some(posts::delete),
        ("users", b"dodelete") => Some(users::dodelete),
        ("users", b"resetpassword") => Some(users::resetpassword),
        ("user-new", b"createuser") => Some(user_edit::create),
        ("profile" | "user-edit", b"update") => Some(user_edit::update),
        ("admin-ajax", b"save-user-color-scheme") => Some(user_edit::color_scheme),
        ("admin-ajax", b"send-password-reset") => Some(user_edit::password_reset),
        ("edit-comments", _) => Some(comments::bulk),
        ("comment", b"editedcomment") => Some(comments::edited),
        ("comment", action) => comments::moderation(action),
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
        ("edit", _) => Some(posts::bulk),
        _ => None,
    }
}

/// The handler of the `admin-ajax.php` action `action`, if ported.
pub(super) fn ajax_handler(action: &[u8]) -> Option<Handler<Ajax>> {
    match action {
        b"add-tag" => Some(terms::ajax_add_tag),
        b"inline-save-tax" => Some(terms::inline_save_tax),
        b"delete-tag" => Some(terms::ajax_delete_tag),
        b"dim-comment" => Some(comments::dim),
        b"delete-comment" => Some(comments::delete),
        b"edit-comment" => Some(comments::quick_edit),
        b"replyto-comment" => Some(comments::reply),
        b"inline-save" => Some(posts::inline_save),
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
        Ajax::CommentRow(row) => (XML, row.sent(admin, asked)),
        Ajax::PostRow(id) => (HTML, post_row(admin, asked, id)),
    };
    ajax_text(StatusCode::OK, kind, body.into_bytes())
}

/// `wp_ajax_inline_save()`'s row of the post `id`, on the list `screen`
/// names, over the options as the action left them (sticky posts).
fn post_row(admin: &Admin, asked: &Asked, id: u64) -> String {
    let reloaded = Handle::current().block_on(admin.site.reloaded());
    let site = reloaded.as_ref().unwrap_or(admin.site);
    let mut reads = LoginReads::new(site, Handle::current(), admin.reads.now);
    reads.act_as(admin.user.id, admin.reads.session_token().to_vec());
    let fresh = Admin::new(&reads, site, admin.user.clone());
    let field = |name| lookup(asked.post, name).map_or_else(Vec::new, Value::to_php_string);
    let kind = match &field("screen")[..] {
        b"edit-page" => PAGE,
        _ => POST,
    };
    let filter = text(&field("post_status"));
    let excerpt = field("post_view") == b"excerpt";
    let post = PostsSite::post(&fresh, id);
    post.map_or_else(String::new, |post| {
        hyscribe_admin::posts::saved_row(&fresh, &post, kind, &filter, excerpt)
    })
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
    site: &dyn Acting,
) -> Result<T, hyscribe_write::Error> {
    let mut write = write?;
    match handler(&mut write, asked, site).await {
        Ok(answer) => write.commit().await.map(|()| answer).map_err(Into::into),
        Err(Stop::Died(answer)) => Ok(answer),
        Err(Stop::Failed(err)) => Err(err),
    }
}

#[cfg(test)]
mod tests;
