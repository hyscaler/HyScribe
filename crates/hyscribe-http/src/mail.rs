// SPDX-License-Identifier: AGPL-3.0-or-later

//! The mails of hs-admin's actions and the comment form (ADR 0016): each
//! built by `hyscribe-mail` inside its action, sent once it commits, and
//! dropped when it rolls back.

use std::cell::RefCell;
use std::net::IpAddr;
use std::sync::Arc;

use hyscribe_config::Constants;
use hyscribe_db::{DbError, Write};
use hyscribe_mail::{
    Comment, Mail, Mailer, Post, PostAuthor, User, email_changed, notify_moderator,
    notify_postauthor, password_changed,
};
use hyscribe_phpser::Value;
use hyscribe_write::caps::Caps;
use hyscribe_write::users::Updated;

/// `retrieve_password()`'s error when `wp_mail()` fails.
pub(crate) const EMAIL_FAILURE: &str = concat!(
    "<strong>Error:</strong> The email could not be sent. Your site may not be correctly ",
    "configured to send emails. <a href=\"https://wordpress.org/documentation/article/",
    "reset-your-password/\">Get support for resetting your password</a>."
);

/// A mail an action sends once it commits.
#[derive(Debug, PartialEq)]
pub(crate) enum Queued {
    /// A mail of the kind named, sent in the background: PHP ignores the
    /// result.
    Background(&'static str, Mail),
    /// `retrieve_password()`'s mail to the account, asked for from the
    /// client address: waited for, within the password reset limits.
    Reset(Mail, u64, Option<IpAddr>),
}

/// The mails an action has queued, and the transport that sends them.
pub(crate) struct Outbox {
    queued: RefCell<Vec<Queued>>,
    mailer: Arc<Mailer>,
}

impl Outbox {
    /// An empty outbox for `mailer`.
    pub(crate) fn new(mailer: Arc<Mailer>) -> Outbox {
        Outbox {
            queued: RefCell::default(),
            mailer,
        }
    }

    /// Queues `mail` until the action commits.
    pub(crate) fn queue(&self, mail: Queued) {
        self.queued.borrow_mut().push(mail);
    }

    /// Drops what an action that rolled back queued.
    pub(crate) fn clear(&self) {
        self.queued.borrow_mut().clear();
    }

    /// Sends what the committed action queued: how many mails it waited
    /// for, and how many of those were sent.
    pub(crate) async fn send(&self) -> (usize, usize) {
        let queued = self.queued.take();
        let (mut waited, mut sent) = (0, 0);
        for mail in queued {
            match mail {
                Queued::Background(kind, mail) => {
                    self.mailer.wp_mail(mail, kind, false).await;
                }
                Queued::Reset(mail, account, address) => {
                    waited += 1;
                    let reset = self.mailer.retrieve_password(mail, account, address).await;
                    sent += usize::from(reset);
                }
            }
        }
        (waited, sent)
    }
}

/// What the mails read of the site, as the action has left it.
pub(crate) struct MailSite {
    name: Vec<u8>,
    home: Vec<u8>,
    site: Vec<u8>,
    admin: Vec<u8>,
    admin_email: Vec<u8>,
}

impl MailSite {
    /// The site at `home`, its title, address and admin email read in
    /// `write`; `admin_url()` is hs-admin's under the site address.
    pub(crate) async fn read(write: &mut Write<'_>, home: &str) -> Result<MailSite, DbError> {
        let name = write.option_value("blogname").await?.unwrap_or_default();
        let admin_email = write.option_value("admin_email").await?.unwrap_or_default();
        let site = match write.configured("siteurl") {
            Some(url) => url.as_bytes().to_vec(),
            None => write.option_value("siteurl").await?.unwrap_or_default(),
        };
        let site = site
            .strip_suffix(b"/")
            .map_or_else(|| site.clone(), <[u8]>::to_vec);
        Ok(MailSite {
            name,
            home: home.as_bytes().to_vec(),
            admin: [&site[..], b"/hs-admin/"].concat(),
            site,
            admin_email,
        })
    }

    /// The site as the messages take it.
    pub(crate) fn site(&self) -> hyscribe_mail::Site<'_> {
        hyscribe_mail::Site {
            name: &self.name,
            home: &self.home,
            site: &self.site,
            admin: &self.admin,
            admin_email: &self.admin_email,
        }
    }
}

/// `wp_update_user()`'s notices to the user of `updated` on the site at
/// `home`, queued in `outbox`: the password's change, then the address's.
pub(crate) async fn queue_changes(
    write: &mut Write<'_>,
    home: &str,
    updated: &Updated,
    outbox: &Outbox,
) -> Result<(), DbError> {
    let site = MailSite::read(write, home).await?;
    let user = User {
        login: &updated.login,
        email: &updated.email,
    };
    let password = updated
        .password_changed
        .then(|| password_changed(&site.site(), &user));
    let address = updated
        .new_email
        .as_ref()
        .map(|new| email_changed(&site.site(), &user, new));
    for mail in password.into_iter().chain(address) {
        outbox.queue(Queued::Background("wp_update_user", mail));
    }
    Ok(())
}

/// An option as `get_option()` tests it: set and not empty.
async fn option_on(write: &mut Write<'_>, name: &str) -> Result<bool, DbError> {
    let value = write.option_value(name).await?;
    Ok(value.is_some_and(|value| !Value::String(value).is_empty()))
}

/// A comment just saved: its ID, the current user, the post's permalink
/// and the comment's link.
pub(crate) struct Commented<'a> {
    pub(crate) id: u64,
    pub(crate) user: u64,
    pub(crate) permalink: &'a [u8],
    pub(crate) link: &'a [u8],
}

/// `comment_post`'s notices of the comment `saved`, each with its kind:
/// `wp_new_comment_notify_moderator()`'s for a held comment,
/// `wp_new_comment_notify_postauthor()`'s for an approved one. The
/// commenter's host is their address (ADR 0016).
pub(crate) async fn comment_notices(
    write: &mut Write<'_>,
    site: &MailSite,
    saved: &Commented<'_>,
    constants: Constants,
) -> Result<Vec<(&'static str, Mail)>, DbError> {
    // The action has just written both.
    let row = write.comment(saved.id).await?.unwrap_or_default();
    let post = write.post(row.post_id).await?;
    let (title, post_author) = post
        .map(|post| (post.title, post.author))
        .unwrap_or_default();
    let author = match write.user(post_author).await? {
        Some((_, user)) => {
            let caps = Caps::load(write, post_author, Some(row.post_id), constants).await?;
            let on = [Value::Int(i64::try_from(row.post_id).unwrap_or_default())];
            let can_edit = caps.can("edit_post", &on);
            Some((user.email, caps.can("read_post", &on), can_edit))
        }
        None => None,
    };
    let author = author
        .as_ref()
        .map(|(email, can_read, can_edit)| PostAuthor {
            email,
            can_read: *can_read,
            can_edit_comment: *can_edit,
            // A reply's parent is on the same post.
            can_edit_parent: *can_edit,
        });
    let comment = Comment {
        id: saved.id,
        kind: row.comment_type.as_bytes(),
        author: &row.author,
        author_email: &row.author_email,
        author_url: &row.author_url,
        author_ip: &row.author_ip,
        author_domain: &row.author_ip,
        content: &row.content,
        parent: row.parent,
        user_id: row.user_id,
    };
    let post = Post {
        title: &title,
        author: post_author,
        permalink: saved.permalink,
    };
    let trash = constants.empty_trash_days != 0;
    let mut notices = Vec::new();
    if row.approved == "0" && option_on(write, "moderation_notify").await? {
        let [_, waiting, ..] = write.comment_counts(0).await?;
        let mails = notify_moderator(
            &site.site(),
            &comment,
            &post,
            author.as_ref(),
            waiting,
            trash,
        );
        notices.extend(mails.into_iter().map(|mail| ("wp_notify_moderator", mail)));
    }
    if row.approved == "1" && option_on(write, "comments_notify").await? {
        let (user, link) = (saved.user, saved.link);
        let mail = site.site();
        let notice = notify_postauthor(
            &mail,
            &comment,
            &post,
            author.as_ref(),
            user,
            link,
            b"",
            trash,
        );
        notices.extend(notice.map(|mail| ("wp_notify_postauthor", mail)));
    }
    Ok(notices)
}

#[cfg(test)]
pub(crate) mod tests;
