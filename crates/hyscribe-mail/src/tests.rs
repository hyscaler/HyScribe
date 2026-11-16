// SPDX-License-Identifier: AGPL-3.0-or-later

//! Every message against PHP WordPress's, recorded by
//! `reference/mail/golden.sh` over the cases of `reference/mail/run.php`.

use super::*;
use hyscribe_kses::str_replace;
use hyscribe_testkit::unhex;

const HOME: &[u8] = b"http://127.0.0.1:8081";
const ADMIN: &[u8] = b"http://127.0.0.1:8081/wp-admin/";
const ADMIN_EMAIL: &[u8] = b"admin@default-install.test";
const ODD: User<'static> = User {
    login: b"Tom O. Neil.",
    email: b"o'neil@example.test",
};
const NEW_EMAIL: &[u8] = b"new-address@example.test";
const PERMALINK: &[u8] = b"http://127.0.0.1:8081/?p=1";
const COMMENT_LINK: &[u8] = b"http://127.0.0.1:8081/?p=1#comment-1";
const EDIT_LINK: &[u8] = b"http://127.0.0.1:8081/wp-admin/post.php?post=1&action=edit";
/// A privacy request's ID, which the goldens mask.
const REQUEST: u64 = 12;

/// The site as each `blog` variant of `run.php` stores its title.
fn site(blog: &str) -> Site<'static> {
    let name: &[u8] = match blog {
        "plain" => b"HyScribe Default Install",
        "odd" => b"Tom &amp; Jerry&#039;s &lt;b&gt; &#0039; &#X26; &#x00027; &amp;lt; 100% &quot;q&quot; &#034;",
        _ => b"",
    };
    Site {
        name,
        home: HOME,
        site: HOME,
        admin: ADMIN,
        admin_email: ADMIN_EMAIL,
    }
}

/// `run.php`'s comment 1 on post 1, changed by `fields`.
fn comment(fields: impl FnOnce(&mut Comment<'static>)) -> Comment<'static> {
    let mut comment = Comment {
        id: 1,
        kind: b"comment",
        author: b"A \"Reader\"",
        author_email: b"reader@example.test",
        author_url: b"https://reader.example.test/a?b=1&c=2",
        author_ip: b"127.0.0.1",
        author_domain: b"localhost",
        content: b"A &lt;b&gt;bold&lt;/b&gt; &amp;amp; &quot;quoted&quot; &#039;s\nline two",
        parent: 0,
        user_id: 0,
    };
    fields(&mut comment);
    comment
}

/// Post 1 as `run.php` titles it, by `author`.
fn post(author: u64) -> Post<'static> {
    Post {
        title: b"Hello &amp; \"world\" &lt;i&gt; &#039;s",
        author,
        permalink: PERMALINK,
    }
}

/// The admin (user 1), an editor or a subscriber as the post's author.
fn author(role: &str) -> PostAuthor<'static> {
    let editor = role != "subscriber";
    PostAuthor {
        email: match role {
            "admin" => ADMIN_EMAIL,
            "editor" => b"editor@example.test",
            _ => b"subscriber@example.test",
        },
        can_read: true,
        can_edit_comment: editor,
        can_edit_parent: editor,
    }
}

/// The messages of `run.php`'s case `name` for the site `site`.
fn ours(name: &str, site: &Site<'_>) -> Vec<Mail> {
    let admin = author("admin");
    let editor = author("editor");
    let subscriber = author("subscriber");
    let postauthor = |comment: Comment<'_>, author: &PostAuthor<'_>, current_user: u64| {
        let post = post(if author.email == ADMIN_EMAIL { 1 } else { 2 });
        let edit = if current_user == 1 { EDIT_LINK } else { b"" };
        let trash = !name.ends_with("-no-trash");
        notify_postauthor(
            site,
            &comment,
            &post,
            Some(author),
            current_user,
            COMMENT_LINK,
            edit,
            trash,
        )
        .into_iter()
        .collect()
    };
    let moderator = |comment: Comment<'_>, author: &PostAuthor<'_>, waiting: u64| {
        let post = post(if author.email == ADMIN_EMAIL { 1 } else { 2 });
        let trash = !name.ends_with("-no-trash");
        notify_moderator(site, &comment, &post, Some(author), waiting, trash)
    };
    match name {
        "retrieve-password" => vec![retrieve_password(site, &ODD, b"%key%", b"en_US", Some(b"192.0.2.1"))],
        "retrieve-password-logged-in" => vec![retrieve_password(site, &ODD, b"%key%", b"en_US", None)],
        "retrieve-password-no-address" => vec![retrieve_password(site, &ODD, b"%key%", b"en_US", Some(b""))],
        "new-user-both" => vec![new_user_to_admin(site, &ODD), new_user_to_user(site, &ODD, b"%key%")],
        "new-user-admin" => vec![new_user_to_admin(site, &ODD)],
        "new-user-user" => vec![new_user_to_user(site, &ODD, b"%key%")],
        "email-change-request" => vec![email_change_request(site, ODD.login, NEW_EMAIL, b"%hash%")],
        "new-admin-email" => vec![new_admin_email(site, b"admin", b"new-admin@example.test", b"%hash%")],
        "admin-email-changed" => {
            admin_email_changed(site, ADMIN_EMAIL, b"new-admin@example.test").into_iter().collect()
        }
        "admin-email-changed-default" => {
            admin_email_changed(site, b"you@example.com", b"new-admin@example.test").into_iter().collect()
        }
        "password-changed" => vec![password_changed(site, &ODD)],
        "email-changed" => vec![email_changed(site, &ODD, NEW_EMAIL)],
        "password-and-email-changed" => {
            vec![password_changed(site, &ODD), email_changed(site, &ODD, NEW_EMAIL)]
        }
        "password-reset-admin" => password_reset_to_admin(site, &ODD).into_iter().collect(),
        "password-reset-admin-self" => {
            let admin = User { login: b"admin", email: ADMIN_EMAIL };
            password_reset_to_admin(site, &admin).into_iter().collect()
        }
        "postauthor-comment" | "postauthor-comment-no-trash" => postauthor(comment(|_| {}), &admin, 0),
        "postauthor-reply" => postauthor(comment(|c| c.parent = 1), &admin, 0),
        "postauthor-anonymous" => postauthor(
            comment(|c| {
                c.author = b"";
                c.author_ip = b"";
                c.author_domain = b"";
            }),
            &admin,
            0,
        ),
        "postauthor-no-email" => postauthor(comment(|c| c.author_email = b""), &admin, 0),
        "postauthor-anonymous-no-email" => postauthor(
            comment(|c| {
                c.author = b"";
                c.author_email = b"";
            }),
            &admin,
            0,
        ),
        "postauthor-trackback" => postauthor(comment(|c| c.kind = b"trackback"), &admin, 0),
        "postauthor-pingback" => postauthor(comment(|c| c.kind = b"pingback"), &admin, 0),
        "postauthor-note" => postauthor(
            comment(|c| {
                c.kind = b"note";
                c.user_id = 1;
            }),
            &editor,
            1,
        ),
        "postauthor-note-resolved" => postauthor(
            comment(|c| {
                c.kind = b"note";
                c.content = b"";
            }),
            &editor,
            0,
        ),
        "postauthor-subscriber" => postauthor(comment(|c| c.parent = 1), &subscriber, 0),
        "postauthor-own-comment" => postauthor(comment(|c| c.user_id = 1), &admin, 0),
        "postauthor-own-moderation" => postauthor(comment(|_| {}), &admin, 1),
        "moderator-comment" | "moderator-comment-no-trash" => moderator(comment(|_| {}), &admin, 1),
        "moderator-reply" => moderator(comment(|c| c.parent = 1), &admin, 1),
        "moderator-trackback" => moderator(comment(|c| c.kind = b"trackback"), &admin, 1),
        "moderator-pingback" => moderator(
            comment(|c| {
                c.kind = b"pingback";
                c.author_ip = b"";
                c.author_domain = b"";
            }),
            &admin,
            1,
        ),
        "moderator-editor-two-waiting" => moderator(comment(|_| {}), &editor, 2),
        "moderator-subscriber" => moderator(comment(|_| {}), &subscriber, 1),
        "user-request-export" => vec![user_request(site, ODD.email, Action::Export, REQUEST, b"%key%")],
        "user-request-erase" => vec![user_request(site, ODD.email, Action::Erase, REQUEST, b"%key%")],
        "request-confirmed-export" => vec![request_confirmed(site, ODD.email, Action::Export)],
        "request-confirmed-erase" => vec![request_confirmed(site, ODD.email, Action::Erase)],
        "erasure-fulfilled" => vec![erasure_fulfilled(site, ODD.email, b"")],
        "erasure-fulfilled-policy" => {
            vec![erasure_fulfilled(site, ODD.email, b"http://127.0.0.1:8081/?page_id=3")]
        }
        "export-ready" => vec![export_ready(
            site,
            ODD.email,
            b"%date%",
            b"http://127.0.0.1:8081/wp-content/uploads/wp-personal-data-exports/wp-personal-data-file-abc123.zip",
        )],
        _ => panic!("no case {name}"),
    }
}

/// A field as `run.php` prints it: escaped, with the request ID masked.
fn printed(field: &[u8]) -> String {
    let field = str_replace(
        field,
        format!("request_id={REQUEST}").as_bytes(),
        b"request_id=%request%",
    );
    let mut out = String::new();
    for c in String::from_utf8(field).unwrap().chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// One golden line of a case's message, or `-` for none.
fn lines(label: &str, mails: &[Mail]) -> Vec<String> {
    if mails.is_empty() {
        return vec![format!("{label}\t-")];
    }
    mails
        .iter()
        .map(|mail| {
            let fields = [&mail.to, &mail.subject, &mail.message, &mail.headers];
            let fields: Vec<String> = fields.iter().map(|field| printed(field)).collect();
            format!("{label}\t{}", fields.join("\t"))
        })
        .collect()
}

#[test]
fn messages_match_php() {
    let golden = include_str!("../tests/golden/mail.tsv");
    let mut labels: Vec<&str> = golden
        .lines()
        .filter_map(|line| line.split('\t').next())
        .collect();
    labels.dedup();
    let mut ours_lines = Vec::new();
    for label in &labels {
        let (name, blog) = label.split_once('/').unwrap();
        ours_lines.extend(lines(label, &ours(name, &site(blog))));
    }
    let theirs: Vec<&str> = golden.lines().collect();
    for (ours, theirs) in ours_lines.iter().zip(&theirs) {
        assert_eq!(ours, theirs);
    }
    assert_eq!(ours_lines.len(), theirs.len());
    assert_eq!((labels.len(), theirs.len()), (85, 91));
}

#[test]
fn decoding_matches_php() {
    let golden = include_str!("../tests/golden/decode.tsv");
    for line in golden.lines() {
        let [text, none, quotes] =
            <[&str; 3]>::try_from(line.split('\t').collect::<Vec<_>>()).unwrap();
        let text = unhex(text);
        assert_eq!(wp_specialchars_decode(&text, false), unhex(none), "{line}");
        assert_eq!(wp_specialchars_decode(&text, true), unhex(quotes), "{line}");
    }
    assert_eq!(golden.lines().count(), 19);
}
