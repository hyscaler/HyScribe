// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_notify_postauthor()` and `wp_notify_moderator()`
//! (`wp-includes/pluggable.php`).

use crate::{Mail, Site, mail, wp_specialchars_decode};
use hyscribe_kses::url_host;
use hyscribe_phpser::is_empty_string;
use hyscribe_render::number_format;

/// The comment a notice is about, as stored.
#[derive(Debug, Clone, Copy)]
pub struct Comment<'a> {
    /// `comment_ID`.
    pub id: u64,
    /// `comment_type`: `trackback`, `pingback`, `note` or a comment.
    pub kind: &'a [u8],
    /// `comment_author`.
    pub author: &'a [u8],
    /// `comment_author_email`.
    pub author_email: &'a [u8],
    /// `comment_author_url`.
    pub author_url: &'a [u8],
    /// `comment_author_IP`.
    pub author_ip: &'a [u8],
    /// `gethostbyaddr()` of the address when it is one (which gives the
    /// address back when the lookup fails), else empty.
    pub author_domain: &'a [u8],
    /// `comment_content`.
    pub content: &'a [u8],
    /// `comment_parent`.
    pub parent: u64,
    /// `user_id`.
    pub user_id: u64,
}

/// The post commented on.
#[derive(Debug, Clone, Copy)]
pub struct Post<'a> {
    /// `post_title`.
    pub title: &'a [u8],
    /// `post_author`.
    pub author: u64,
    /// `get_permalink()`.
    pub permalink: &'a [u8],
}

/// The post's author, when that user exists, and what they may do.
#[derive(Debug, Clone, Copy)]
pub struct PostAuthor<'a> {
    /// `user_email`.
    pub email: &'a [u8],
    /// `user_can( $author, 'read_post', $post )`.
    pub can_read: bool,
    /// `user_can( $author, 'edit_comment', $comment )`.
    pub can_edit_comment: bool,
    /// `user_can( $author, 'edit_comment', $comment->comment_parent )`.
    pub can_edit_parent: bool,
}

/// The line naming who wrote the comment: `Website` for a trackback or
/// pingback, `Author` otherwise.
fn author_line(comment: &Comment<'_>, label: &[u8]) -> Vec<u8> {
    [
        label,
        b": ",
        comment.author,
        b" (IP address: ",
        comment.author_ip,
        b", ",
        comment.author_domain,
        b")\r\n",
    ]
    .concat()
}

/// A moderation link of the admin: `comment.php?action=$action&c=$id`.
fn action_link(site: &Site<'_>, label: &[u8], action: &[u8], id: u64) -> Vec<u8> {
    let id = id.to_string();
    [
        label,
        b": ",
        site.admin,
        b"comment.php?action=",
        action,
        b"&c=",
        id.as_bytes(),
        b"#wpbody-content\r\n",
    ]
    .concat()
}

/// `Trash it` while `EMPTY_TRASH_DAYS` is not 0, else `Delete it`.
fn trash_link(site: &Site<'_>, id: u64, trash: bool) -> Vec<u8> {
    if trash {
        action_link(site, b"Trash it", b"trash", id)
    } else {
        action_link(site, b"Delete it", b"delete", id)
    }
}

/// The word for a trackback or pingback, in lower and title case.
fn ping_kind(kind: &[u8]) -> Option<(&'static [u8], &'static [u8])> {
    match kind {
        b"trackback" => Some((b"trackback", b"Trackback")),
        b"pingback" => Some((b"pingback", b"Pingback")),
        _ => None,
    }
}

/// `wp_notify_postauthor()`'s notice to the post's `author`, or none:
/// no author with an address, a comment the author wrote, a comment the
/// author (`current_user`) moderated, or a post the author may not read.
/// `comment_link` is `get_comment_link()`, `edit_link` the post's
/// `get_edit_post_link()` for the current user (for notes), and `trash`
/// whether `EMPTY_TRASH_DAYS` is not 0.
#[allow(clippy::too_many_arguments)]
pub fn notify_postauthor(
    site: &Site<'_>,
    comment: &Comment<'_>,
    post: &Post<'_>,
    author: Option<&PostAuthor<'_>>,
    current_user: u64,
    comment_link: &[u8],
    edit_link: &[u8],
    trash: bool,
) -> Option<Mail> {
    let author = author.filter(|author| {
        !is_empty_string(author.email)
            && comment.user_id != post.author
            && current_user != post.author
            && author.can_read
    })?;
    let title = site.title();
    let content = wp_specialchars_decode(comment.content, false);
    let host = url_host(site.home).unwrap_or_default();
    let host = host.strip_prefix(b"www.").unwrap_or(&host);
    let from = if comment.author.is_empty() {
        &title[..]
    } else {
        comment.author
    };
    let mut headers = [
        &b"From: \""[..],
        from,
        b"\" <wordpress@",
        host,
        b">\nContent-Type: text/plain; charset=\"UTF-8\"\n",
    ]
    .concat();
    if !comment.author_email.is_empty() {
        let email = comment.author_email;
        let reply_to = if comment.author.is_empty() {
            [&b"Reply-To: "[..], email, b"\n"].concat()
        } else {
            [&b"Reply-To: \""[..], email, b"\" <", email, b">\n"].concat()
        };
        headers.extend_from_slice(&reply_to);
    }

    let on_post = [&b" on your post \""[..], post.title, b"\"\r\n"].concat();
    let note = comment.kind == b"note";
    let (mut message, what): (_, &[u8]) = if let Some((kind, what)) = ping_kind(comment.kind) {
        let message = [
            &b"New "[..],
            kind,
            &on_post,
            &author_line(comment, b"Website"),
            b"URL: ",
            comment.author_url,
            b"\r\nComment: \r\n",
            &content,
            b"\r\n\r\nYou can see all ",
            kind,
            b"s on this post here:\r\n",
        ]
        .concat();
        (message, what)
    } else if note {
        let text = if is_empty_string(&content) {
            &b"resolved/reopened"[..]
        } else {
            &content
        };
        let message = [
            &b"New note"[..],
            &on_post,
            &author_line(comment, b"Author"),
            b"Email: ",
            comment.author_email,
            b"\r\nNote: \r\n",
            text,
            b"\r\n\r\nYou can see all notes on this post here:\r\n",
        ]
        .concat();
        (message, b"Note")
    } else {
        let mut message = [
            &b"New comment"[..],
            &on_post,
            &author_line(comment, b"Author"),
            b"Email: ",
            comment.author_email,
            b"\r\nURL: ",
            comment.author_url,
            b"\r\n",
        ]
        .concat();
        if comment.parent != 0 && author.can_edit_parent {
            message.extend(action_link(
                site,
                b"In reply to",
                b"editcomment",
                comment.parent,
            ));
        }
        message.extend_from_slice(b"Comment: \r\n");
        message.extend_from_slice(&content);
        message.extend_from_slice(b"\r\n\r\nYou can see all comments on this post here:\r\n");
        (message, b"Comment")
    };
    if note {
        message.extend_from_slice(&[edit_link, b"\r\n"].concat());
    } else {
        message.extend_from_slice(
            &[
                post.permalink,
                b"#comments\r\n\r\nPermalink: ",
                comment_link,
                b"\r\n",
            ]
            .concat(),
        );
        if author.can_edit_comment {
            message.extend(trash_link(site, comment.id, trash));
            message.extend(action_link(site, b"Spam it", b"spam", comment.id));
        }
    }
    let subject = [b"[", &title[..], b"] ", what, b": \"", post.title, b"\""].concat();
    Some(Mail {
        to: author.email.to_vec(),
        subject: wp_specialchars_decode(&subject, false),
        message,
        headers,
    })
}

/// `wp_notify_moderator()`'s notices, while `moderation_notify` is on: to
/// the admin email, and to the post's `author` too when they may edit the
/// comment and have another address. `waiting` counts the comments
/// awaiting approval; `trash` is as for [`notify_postauthor`].
pub fn notify_moderator(
    site: &Site<'_>,
    comment: &Comment<'_>,
    post: &Post<'_>,
    author: Option<&PostAuthor<'_>>,
    waiting: u64,
    trash: bool,
) -> Vec<Mail> {
    let mut emails = vec![site.admin_email];
    if let Some(author) = author.filter(|author| {
        author.can_edit_comment
            && !is_empty_string(author.email)
            && !author.email.eq_ignore_ascii_case(site.admin_email)
    }) {
        emails.push(author.email);
    }
    let title = site.title();
    let content = wp_specialchars_decode(comment.content, false);
    let kind = ping_kind(comment.kind);
    let mut message = [
        &b"A new "[..],
        kind.map_or(&b"comment"[..], |(kind, _)| kind),
        b" on the post \"",
        post.title,
        b"\" is waiting for your approval\r\n",
        post.permalink,
        b"\r\n\r\n",
    ]
    .concat();
    if let Some((_, kind)) = kind {
        message.extend(
            [
                &author_line(comment, b"Website")[..],
                b"URL: ",
                comment.author_url,
                b"\r\n",
                kind,
                b" excerpt: \r\n",
            ]
            .concat(),
        );
    } else {
        message.extend(
            [
                &author_line(comment, b"Author")[..],
                b"Email: ",
                comment.author_email,
                b"\r\nURL: ",
                comment.author_url,
                b"\r\n",
            ]
            .concat(),
        );
        if comment.parent != 0 {
            message.extend(action_link(
                site,
                b"In reply to",
                b"editcomment",
                comment.parent,
            ));
        }
        message.extend_from_slice(b"Comment: \r\n");
    }
    message.extend_from_slice(&content);
    message.extend_from_slice(b"\r\n\r\n");
    message.extend(action_link(site, b"Approve it", b"approve", comment.id));
    message.extend(trash_link(site, comment.id, trash));
    message.extend(action_link(site, b"Spam it", b"spam", comment.id));
    let count = number_format(waiting);
    let waiting = if waiting == 1 {
        [&b"Currently "[..], count.as_bytes(), b" comment is waiting"].concat()
    } else {
        [
            &b"Currently "[..],
            count.as_bytes(),
            b" comments are waiting",
        ]
        .concat()
    };
    message.extend(
        [
            &waiting[..],
            b" for approval. Please visit the moderation panel:\r\n",
            site.admin,
            b"edit-comments.php?comment_status=moderated#wpbody-content\r\n",
        ]
        .concat(),
    );
    let subject = [
        b"[",
        &title[..],
        b"] Please moderate: \"",
        post.title,
        b"\"",
    ]
    .concat();
    let subject = wp_specialchars_decode(&subject, false);
    emails
        .into_iter()
        .map(|email| mail(email, subject.clone(), message.clone()))
        .collect()
}
