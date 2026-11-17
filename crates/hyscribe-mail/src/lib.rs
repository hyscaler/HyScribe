// SPDX-License-Identifier: AGPL-3.0-or-later

//! The emails WordPress 7.1.3 sends, each built as its function hands it to
//! `wp_mail()`: recipient, subject, body and headers, with every filter at
//! its default, in English. Sending is the transport's (ADR 0016); the
//! caller reads the site, the users and the capabilities, and makes the
//! keys. PHP is the oracle: `reference/mail/golden.sh` records every message.
//!
//! Where each one will be called, once the transport is decided:
//! - [`retrieve_password`]: hs-admin's password reset for users (bulk and
//!   Ajax, `hyscribe-http`'s `saves/users.rs` and `saves/user_edit.rs`) and
//!   the lost password form (`login/reset.rs`, which builds it already)
//! - [`new_user_to_admin`], [`new_user_to_user`]: Add User
//!   (`edit_user_created_user`) and registration (`register_new_user`)
//! - [`email_change_request`]: one's own profile save (`saves/user_edit.rs`)
//! - [`new_admin_email`], [`admin_email_changed`]: the General settings save
//!   (`saves/options.rs`) and the `adminhash` confirmation
//! - [`password_changed`], [`email_changed`]: `wp_update_user()` of another
//!   user's edit and of a confirmed email change; [`password_reset_to_admin`]:
//!   `after_password_reset` (`login/reset.rs`, which builds it already)
//! - [`notify_postauthor`], [`notify_moderator`]: a new comment
//!   (`comment_post`) and a comment approved in the admin
//!   (`comment_unapproved_to_approved`)
//! - [`user_request`], [`request_confirmed`], [`erasure_fulfilled`],
//!   [`export_ready`]: the personal data requests' screens and confirmation

mod comments;
mod decode;
mod privacy;
mod users;

pub use comments::{Comment, Post, PostAuthor, notify_moderator, notify_postauthor};
pub use decode::wp_specialchars_decode;
pub use privacy::{Action, erasure_fulfilled, export_ready, request_confirmed, user_request};
pub use users::{
    User, admin_email_changed, email_change_request, email_changed, new_admin_email,
    new_user_to_admin, new_user_to_user, password_changed, password_reset_to_admin,
    retrieve_password,
};

use hyscribe_kses::str_replace;

/// One `wp_mail()` call's arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mail {
    /// The recipient's address.
    pub to: Vec<u8>,
    /// The subject.
    pub subject: Vec<u8>,
    /// The plain text body.
    pub message: Vec<u8>,
    /// Headers, one per `\n` terminated line; empty for `wp_mail()`'s own.
    pub headers: Vec<u8>,
}

/// What every message reads of the site.
#[derive(Debug, Clone, Copy)]
pub struct Site<'a> {
    /// The `blogname` option as stored, which `esc_html()` encoded.
    pub name: &'a [u8],
    /// `home_url()`.
    pub home: &'a [u8],
    /// `site_url()` in the login scheme, without a trailing slash.
    pub site: &'a [u8],
    /// `admin_url()`, ending in `/wp-admin/`.
    pub admin: &'a [u8],
    /// The `admin_email` option.
    pub admin_email: &'a [u8],
}

impl Site<'_> {
    /// `wp_specialchars_decode( get_option( 'blogname' ), ENT_QUOTES )`.
    fn title(&self) -> Vec<u8> {
        wp_specialchars_decode(self.name, true)
    }
}

/// `[%s] $what`, the subject of most messages.
fn subject(title: &[u8], what: &[u8]) -> Vec<u8> {
    [b"[", title, b"] ", what].concat()
}

/// `str_replace()` of each `###NAME###` placeholder in turn.
fn fill(text: &[u8], values: &[(&[u8], &[u8])]) -> Vec<u8> {
    values.iter().fold(text.to_vec(), |text, (name, value)| {
        str_replace(&text, &[b"###", *name, b"###"].concat(), value)
    })
}

/// A message to `to` with no headers of its own.
fn mail(to: &[u8], subject: Vec<u8>, message: Vec<u8>) -> Mail {
    Mail {
        to: to.to_vec(),
        subject,
        message,
        headers: Vec::new(),
    }
}

#[cfg(test)]
mod tests;
