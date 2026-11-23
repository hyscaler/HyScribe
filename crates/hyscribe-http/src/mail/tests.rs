// SPDX-License-Identifier: AGPL-3.0-or-later

//! A recording transport for the handlers' tests, and the outbox and the
//! comment notices against the fixture.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use hyscribe_mail::{Hooks, Outgoing};

use super::*;

/// Hooks that keep every message `wp_mail()` is handed and answer for the
/// transport, sending unless told to fail.
#[derive(Default)]
pub(crate) struct Recorder {
    mails: Mutex<Vec<Mail>>,
    failing: AtomicBool,
}

impl Hooks for Recorder {
    fn wp_mail(&self, atts: Mail) -> Mail {
        self.mails.lock().unwrap().push(atts.clone());
        atts
    }
    fn phpmailer_init(&self, _: &Outgoing) -> Option<bool> {
        Some(!self.failing.load(Ordering::Relaxed))
    }
}

thread_local! {
    static RECORDED: (Arc<Recorder>, &'static Outbox) = {
        let recorder = Arc::new(Recorder::default());
        let off = hyscribe_config::Mail::default();
        let (mailer, _) = Mailer::new(&off, b"http://example.test", recorder.clone());
        (recorder, Box::leak(Box::new(Outbox::new(Arc::new(mailer)))))
    };
}

/// This test's outbox, on its thread, through the recorder.
pub(crate) fn outbox() -> &'static Outbox {
    RECORDED.with(|(_, outbox)| *outbox)
}

impl Recorder {
    /// The messages handed to `wp_mail()` so far, taken.
    pub(crate) fn taken(&self) -> Vec<Mail> {
        self.mails.lock().unwrap().drain(..).collect()
    }
}

/// The messages this test's outbox has sent or tried to, taken.
pub(crate) fn mailed() -> Vec<Mail> {
    RECORDED.with(|(recorder, _)| recorder.taken())
}

/// Makes this test's sends fail, or succeed again.
pub(crate) fn failing(failing: bool) {
    RECORDED.with(|(recorder, _)| recorder.failing.store(failing, Ordering::Relaxed));
}

/// `mails` as `reference/mail/run.php` prints those of its save `case`,
/// a line each, or `case/plain\t-` for none.
pub(crate) fn printed(case: &str, mails: &[Mail]) -> Vec<String> {
    let label = format!("{case}/plain");
    let escaped = |field: &[u8]| {
        let field = String::from_utf8_lossy(field).replace('\\', "\\\\");
        let field = field.replace('\t', "\\t").replace('\n', "\\n");
        field.replace('\r', "\\r")
    };
    let line = |mail: &Mail| {
        let fields = [&mail.to, &mail.subject, &mail.message, &mail.headers];
        let fields: Vec<String> = fields.into_iter().map(|field| escaped(field)).collect();
        format!("{label}\t{}", fields.join("\t"))
    };
    match mails {
        [] => vec![format!("{label}\t-")],
        mails => mails.iter().map(line).collect(),
    }
}

/// What PHP's save `case` hands `wp_mail()`, as `reference/mail/golden.sh`
/// recorded it.
pub(crate) fn php_mails(case: &str) -> Vec<String> {
    let label = format!("{case}/plain\t");
    include_str!("../../tests/golden/mail-flows.tsv")
        .lines()
        .filter(|line| line.starts_with(&label))
        .map(str::to_string)
        .collect()
}

fn mail(subject: &[u8]) -> Mail {
    Mail {
        to: b"a@example.test".to_vec(),
        subject: subject.to_vec(),
        ..Mail::default()
    }
}

#[tokio::test]
async fn the_outbox_sends_what_it_holds_once() {
    let outbox = outbox();
    outbox.queue(Queued::Background("test", mail(b"dropped")));
    outbox.clear();
    outbox.queue(Queued::Background("test", mail(b"one")));
    outbox.queue(Queued::Reset(mail(b"two"), 1, None));
    failing(true);
    outbox.queue(Queued::Reset(mail(b"three"), 2, None));
    assert_eq!(outbox.send().await, (2, 0));
    failing(false);
    assert_eq!(outbox.send().await, (0, 0));
    outbox.queue(Queued::Reset(mail(b"four"), 1, None));
    assert_eq!(outbox.send().await, (1, 1));
    let subjects: Vec<Vec<u8>> = mailed().into_iter().map(|mail| mail.subject).collect();
    assert_eq!(subjects, [&b"one"[..], b"two", b"three", b"four"]);
}

/// `comment_post`'s notices of the fixture's comment 1 on post 1, by
/// user 1, as its status, the options and the post's author make them.
#[tokio::test]
async fn comment_notices_follow_the_status_and_the_options() {
    use hyscribe_config::Mode;
    use sqlx::Connection as _;
    let db = hyscribe_testkit::fixture_database();
    let writer = hyscribe_db::Writer::open(&db, Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let mut tx = conn.begin().await.unwrap();
    let mut notices = async |sql: &'static str| {
        if !sql.is_empty() {
            sqlx::query(sql).execute(&mut *tx).await.unwrap();
        }
        let mut write = writer.begin_on(&mut tx).await.unwrap();
        let site = MailSite::read(&mut write, "http://127.0.0.1:8081")
            .await
            .unwrap();
        let comment = Commented {
            id: 1,
            user: 0,
            permalink: b"http://127.0.0.1:8081/?p=1",
            link: b"http://127.0.0.1:8081/?p=1#comment-1",
        };
        let constants = Constants::default();
        let notices = comment_notices(&mut write, &site, &comment, constants)
            .await
            .unwrap();
        let shown = |(kind, mail): (&str, Mail)| {
            let subject = String::from_utf8(mail.subject).unwrap();
            let trash = mail.message.windows(9).any(|w| w == b"Trash it:");
            format!(
                "{kind} {} {subject} {trash}",
                String::from_utf8(mail.to).unwrap()
            )
        };
        notices.into_iter().map(shown).collect::<Vec<_>>()
    };
    let admin = "admin@default-install.test";
    let title = "[HyScribe Default Install]";
    let approved = format!("wp_notify_postauthor {admin} {title} Comment: \"Hello world!\" true");
    assert_eq!(notices("").await, [approved]);
    let off = "UPDATE wp_options SET option_value = '' WHERE option_name = 'comments_notify'";
    assert!(notices(off).await.is_empty());
    let held = "UPDATE wp_comments SET comment_approved = '0' WHERE comment_ID = 1";
    let moderate =
        format!("wp_notify_moderator {admin} {title} Please moderate: \"Hello world!\" true");
    assert_eq!(notices(held).await, std::slice::from_ref(&moderate));
    // Without its author the post's moderator is the admin email alone.
    let orphan = "UPDATE wp_posts SET post_author = 999 WHERE ID = 1";
    assert_eq!(notices(orphan).await, [moderate]);
    let off = "UPDATE wp_options SET option_value = '0' WHERE option_name = 'moderation_notify'";
    assert!(notices(off).await.is_empty());
    let spam = "UPDATE wp_comments SET comment_approved = 'spam' WHERE comment_ID = 1";
    assert!(notices(spam).await.is_empty());
    // `WP_SITEURL` wins over the option, untrailingslashed.
    let urls = vec![("siteurl", "https://configured.test/".to_string())];
    let writer = writer.with_urls(urls);
    let mut write = writer.begin_on(&mut tx).await.unwrap();
    let site = MailSite::read(&mut write, "https://home.test")
        .await
        .unwrap();
    let site = site.site();
    assert_eq!(
        (site.site, site.admin),
        (
            &b"https://configured.test"[..],
            &b"https://configured.test/hs-admin/"[..]
        )
    );
    assert_eq!(
        (site.home, site.name),
        (&b"https://home.test"[..], &b"HyScribe Default Install"[..])
    );
    drop(write);
    tx.rollback().await.unwrap();
}
