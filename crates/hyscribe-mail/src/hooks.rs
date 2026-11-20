// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_mail()`'s hook points under WordPress's names, with its arguments
//! and in its order (ADR 0016): each default returns its value unchanged,
//! as WordPress does with nothing attached, and the sender's filters reach
//! the extensions (`hyscribe_model::hooks`). What a filter returns goes
//! through the same checks as the message it replaced.

use hyscribe_model::hooks::apply_filters;

use crate::Mail;
use crate::mailer::Outgoing;

/// The filters and actions of `wp_mail()`.
pub trait Hooks: Send + Sync {
    /// `wp_mail`: the message's arguments, first of all.
    fn wp_mail(&self, atts: Mail) -> Mail {
        atts
    }

    /// `pre_wp_mail`: a result other than none ends the call with it,
    /// nothing sent.
    fn pre_wp_mail(&self, _atts: &Mail) -> Option<bool> {
        None
    }

    /// `wp_mail_from`: the sender address. The address sent is always
    /// `MAIL_FROM`'s, so the result changes nothing.
    fn wp_mail_from(&self, from_email: Vec<u8>) -> Vec<u8> {
        apply_filters("wp_mail_from", from_email, &[])
    }

    /// `wp_mail_from_name`: the sender's name shown.
    fn wp_mail_from_name(&self, from_name: Vec<u8>) -> Vec<u8> {
        apply_filters("wp_mail_from_name", from_name, &[])
    }

    /// `wp_mail_content_type`: `text/plain` unless the headers say.
    fn wp_mail_content_type(&self, content_type: Vec<u8>) -> Vec<u8> {
        apply_filters("wp_mail_content_type", content_type, &[])
    }

    /// `wp_mail_charset`: `UTF-8`, the site's, unless the headers say.
    fn wp_mail_charset(&self, charset: Vec<u8>) -> Vec<u8> {
        apply_filters("wp_mail_charset", charset, &[])
    }

    /// The stand-in for `phpmailer_init`: given the message once checked,
    /// a result replaces the transport, as an API based sender would.
    fn phpmailer_init(&self, _message: &Outgoing) -> Option<bool> {
        None
    }

    /// `wp_mail_succeeded`: the message's arguments, once sent.
    fn wp_mail_succeeded(&self, _mail_data: &Mail) {}

    /// `wp_mail_failed`: the message's arguments and why it was not sent.
    fn wp_mail_failed(&self, _mail_data: &Mail, _error: &str) {}
}

/// Nothing attached.
pub struct NoHooks;

impl Hooks for NoHooks {}

#[cfg(test)]
mod tests;
