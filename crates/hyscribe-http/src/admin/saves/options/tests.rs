// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Settings saves against the write oracle (`reference/writes/actions-options.php`)
//! and the answers `reference/admin-saves/golden.sh` recorded.

use hyscribe_db::{Connection, Writer};
use hyscribe_phpser::{Key, Value, parse_str};
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;
use hyscribe_write::options::{Autoload, add_option, update_option};

use super::super::tests::unread::Unread;
use super::super::tests::{ANSWERS, CASES, SITE, acted, case_row, hs, settled, writer};
use super::super::{Handler, Stop, handler};
use super::*;
use crate::mail::tests::mailed;

/// The fixture's home URL, which the write oracle's site keeps.
const HOME: &str = "http://default-install.test";

/// One test of the module at a time: their actions write the same options.
static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn form(pairs: &[(&str, &str)]) -> Vec<(Key, Value)> {
    let pair = |(name, value): &(&str, &str)| {
        let key = Key::String(name.as_bytes().to_vec());
        (key, Value::String(value.as_bytes().to_vec()))
    };
    pairs.iter().map(pair).collect()
}

/// A request to hs-admin's `screen` past its checks, as user 1.
fn asked<'a>(screen: &'a str, get: &'a [(Key, Value)], post: &'a [(Key, Value)]) -> Asked<'a> {
    Asked {
        home: HOME,
        ..super::super::tests::asked(screen, get, post)
    }
}

/// The handler of `screen` (hs-admin's path) for `asked`.
fn screen_handler(screen: &str, asked: &Asked) -> Handler {
    let name = screen.trim_start_matches("/hs-admin/");
    handler(name, &hyscribe_admin::actions::action(name, asked)).unwrap()
}

/// The handler of `asked` on `screen`, as [`acted`].
async fn saved(
    writer: &Writer,
    conn: &mut Connection,
    screen: &str,
    asked: &Asked<'_>,
) -> Result<Answer, hyscribe_write::Error> {
    acted(
        writer,
        conn,
        (screen_handler(screen, asked), &Unread::default()),
        asked,
    )
    .await
}

/// A save in `screen` of `post` (and `get`) after `setup`, checked against
/// the write oracle's `golden`, answering with a redirect to `to`.
async fn check(
    golden: &str,
    setup: impl AsyncFnOnce(&mut Write<'_>),
    screen: &str,
    get: &[(Key, Value)],
    post: &[(Key, Value)],
    to: &str,
) {
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        setup(&mut write).await;
        write.commit().await.unwrap();
        Ok::<_, String>(())
    };
    let action = async |conn: &mut Connection, ()| {
        let answer = saved(&writer, conn, screen, &asked(screen, get, post)).await;
        match answer {
            Ok(Answer::Redirect(location)) if location == to => Ok(()),
            Ok(Answer::Redirect(location)) => Err(format!("{golden} went to {location}")),
            _ => Err(format!("{golden} did not redirect")),
        }
    };
    check_writes(&fixture_database(), golden, setup, action)
        .await
        .unwrap();
}

/// A save of the options page `page`, from its screen.
fn page_form(page: &str, pairs: &[(&str, &str)]) -> Vec<(Key, Value)> {
    let referer = format!("/hs-admin/options-{page}");
    let mut post = form(&[
        ("option_page", page),
        ("action", "update"),
        ("_wp_http_referer", &referer),
    ]);
    post.extend(form(pairs));
    post
}

async fn postname(write: &mut Write<'_>) {
    let structure = Value::String(b"/%postname%/".to_vec());
    update_option(write, "permalink_structure", &structure, None)
        .await
        .unwrap();
}

async fn pending(write: &mut Write<'_>) {
    let pending = Value::Array(form(&[
        ("hash", "0123456789abcdef0123456789abcdef"),
        ("newemail", "new@example.test"),
    ]));
    add_option(write, "adminhash", &pending, Some(Autoload::Off))
        .await
        .unwrap();
    let email = Value::String(b"new@example.test".to_vec());
    add_option(write, "new_admin_email", &email, None)
        .await
        .unwrap();
}

async fn none(_: &mut Write<'_>) {}

/// Pretty permalinks on a site no longer fresh.
async fn used(write: &mut Write<'_>) {
    postname(write).await;
    let used = Value::String(b"0".to_vec());
    update_option(write, "fresh_site", &used, None)
        .await
        .unwrap();
}

async fn migrating(write: &mut Write<'_>) {
    let required = Value::Bool(true);
    let option = "https_migration_required";
    update_option(write, option, &required, None).await.unwrap();
}

/// Every Settings save of the write oracle writes PHP's rows: the options
/// sanitized, the notices kept for the screen after, a pending admin email
/// with its key, the rewrite rules of a new home or front page, and a new
/// privacy policy draft.
#[tokio::test]
async fn saves_write_php_s_rows() {
    let _one = ONE.lock().await;
    let back = |page: &str| format!("/hs-admin/options-{page}?settings-updated=true");
    let post = page_form(
        "general",
        &[
            ("blogname", "An \"Oracle\" <b>site</b> & co"),
            ("blogdescription", " Just a tagline "),
            ("siteurl", "http://default-install.test"),
            ("home", "https://default-install.test/"),
            ("new_admin_email", "Oracle@Example.test"),
            ("users_can_register", "1"),
            ("default_role", "editor"),
            ("WPLANG", ""),
            ("timezone_string", "UTC+5.5"),
            ("date_format", "\\c\\u\\s\\t\\o\\m"),
            ("date_format_custom", "j F Y"),
            ("time_format", "H:i"),
            ("time_format_custom", "g:i A"),
            ("start_of_week", "0"),
            ("site_icon", "0"),
        ],
    );
    let options = "/hs-admin/options";
    check(
        "admin-options-general",
        used,
        options,
        &[],
        &post,
        &back("general"),
    )
    .await;
    // The new admin email is asked to confirm, by its key.
    let mails = mailed();
    assert_eq!(mails.len(), 1);
    assert!(mails[0].to.eq_ignore_ascii_case(b"oracle@example.test"));
    assert!(mails[0].subject.ends_with(b"] New Admin Email Address"));
    let message = String::from_utf8(mails[0].message.clone()).unwrap();
    assert!(
        message.contains("/hs-admin/options.php?adminhash="),
        "{message}"
    );
    assert!(message.contains("(admin)"), "{message}");
    let post = page_form(
        "general",
        &[
            ("blogname", "HyScribe Default Install"),
            ("siteurl", "default-install.test"),
            ("home", "http://example.test"),
            ("new_admin_email", "not an email"),
            ("timezone_string", "Mars/Base"),
            ("date_format", "F j, Y"),
            ("time_format", "g:i a"),
            ("start_of_week", "1"),
            ("default_role", "king"),
        ],
    );
    let golden = "admin-options-general-errors";
    check(golden, migrating, options, &[], &post, &back("general")).await;
    assert!(mailed().is_empty());
    let post = page_form(
        "writing",
        &[
            ("default_category", "2"),
            ("default_post_format", "aside"),
            ("mailserver_url", "mail.example.test<b>x</b>"),
            ("mailserver_port", "-995"),
            ("mailserver_login", "oracle@example.test"),
            ("mailserver_pass", "a <secret>"),
            ("default_email_category", "1"),
            (
                "ping_sites",
                "https://rpc.pingomatic.com/\n\n  http://example.test/x  \njavascript:alert(1)",
            ),
        ],
    );
    check(
        "admin-options-writing",
        none,
        options,
        &[],
        &post,
        &back("writing"),
    )
    .await;
    let post = page_form(
        "reading",
        &[
            ("posts_per_page", "-7"),
            ("posts_per_rss", "0"),
            ("rss_use_excerpt", "1"),
            ("show_on_front", "page"),
            ("page_on_front", "2"),
            ("page_for_posts", "0"),
            ("blog_public", "0"),
        ],
    );
    check(
        "admin-options-reading",
        postname,
        options,
        &[],
        &post,
        &back("reading"),
    )
    .await;
    let post = page_form(
        "discussion",
        &[
            ("default_comment_status", "open"),
            ("require_name_email", "1"),
            ("comment_max_links", "-3"),
            ("moderation_keys", "spam\n spam \n\nviagra"),
            ("disallowed_keys", "\n"),
            ("close_comments_days_old", "14.5"),
            ("thread_comments", "1"),
            ("thread_comments_depth", "7"),
            ("comments_per_page", "x"),
            ("default_comments_page", "oldest"),
            ("comment_order", "desc"),
            ("avatar_rating", "PG"),
            ("avatar_default", "identicon"),
        ],
    );
    let golden = "admin-options-discussion";
    check(golden, none, options, &[], &post, &back("discussion")).await;
    let post = page_form(
        "media",
        &[
            ("thumbnail_size_w", "200"),
            ("thumbnail_size_h", "x"),
            ("medium_size_w", "300"),
            ("medium_size_h", "300"),
            ("large_size_w", "2048"),
            ("large_size_h", "1024.9"),
        ],
    );
    check(
        "admin-options-media",
        none,
        options,
        &[],
        &post,
        &back("media"),
    )
    .await;
    let post = page_form("privacy", &[("blog_public", "0")]);
    check(
        "admin-options-privacy",
        none,
        options,
        &[],
        &post,
        &back("privacy"),
    )
    .await;
    let post = form(&[
        ("selection", "custom"),
        ("permalink_structure", "archives//%post_id%#"),
        ("category_base", "topics/ "),
        ("tag_base", "#"),
    ]);
    let to = format!("{HOME}/hs-admin/options-permalink?settings-updated=true");
    let screen = "/hs-admin/options-permalink";
    check("admin-options-permalink", none, screen, &[], &post, &to).await;
    let general = format!("{HOME}/hs-admin/options-general?updated=true");
    let get = form(&[("adminhash", "0123456789abcdef0123456789abcdef")]);
    check(
        "admin-options-adminhash",
        pending,
        options,
        &get,
        &[],
        &general,
    )
    .await;
    // The old address is told of the change.
    let mails = mailed();
    let to: Vec<&[u8]> = mails.iter().map(|mail| &mail.to[..]).collect();
    assert_eq!(to, [&b"admin@default-install.test"[..]]);
    assert!(mails[0].subject.ends_with(b"] Admin Email Changed"));
    let get = form(&[("dismiss", "new_admin_email")]);
    check(
        "admin-options-dismiss",
        pending,
        options,
        &get,
        &[],
        &general,
    )
    .await;
    assert!(mailed().is_empty());
}

/// The new privacy policy page, whose ID the editor link carries.
#[tokio::test]
async fn the_privacy_policy_draft_is_php_s() {
    let _one = ONE.lock().await;
    let writer = writer().await;
    let action = async |conn: &mut Connection, ()| {
        let post = form(&[("action", "create-privacy-page")]);
        let screen = "/hs-admin/options-privacy";
        let answer = saved(&writer, conn, screen, &asked(screen, &[], &post)).await;
        let Ok(Answer::Redirect(to)) = answer else {
            return Err("no redirect".into());
        };
        let edit = format!("{HOME}/hs-admin/post?post=");
        let id = to
            .strip_prefix(&edit)
            .and_then(|to| to.strip_suffix("&action=edit"));
        match id.map(str::parse::<u64>) {
            Some(Ok(_)) => Ok(()),
            _ => Err(to),
        }
    };
    let setup = async |_: &mut Connection| Ok::<_, String>(());
    let report = check_writes(&fixture_database(), "admin-privacy-create", setup, action).await;
    report.unwrap();
}

/// The Settings saves of `reference/admin-saves/golden.sh` answer as PHP
/// did on the oracle, whose configuration sets the addresses; its new
/// policy page's ID is the oracle's, so that one is checked above.
#[tokio::test]
async fn saves_answer_as_php() {
    let _one = ONE.lock().await;
    let urls = vec![("home", SITE.to_string()), ("siteurl", SITE.to_string())];
    let writer = writer().await.with_urls(urls);
    let mut answered = 0;
    for row in ANSWERS.lines().filter(|row| row.starts_with("options-")) {
        let [case, _, php_location, _] = row.split('\t').collect::<Vec<_>>()[..] else {
            panic!("{row}");
        };
        let fields = case_row(CASES, case).unwrap();
        let (target, body) = (hs(fields[2]), hs(fields[3]));
        let (path, query) = target.split_once('?').unwrap_or((&target, ""));
        let get = parse_str(query.as_bytes()).as_array().unwrap().to_vec();
        let body = if body == "-" { "" } else { &body };
        let post = parse_str(body.as_bytes()).as_array().unwrap().to_vec();
        let screen = format!("/hs-admin/{path}");
        let asked = super::super::tests::asked(&screen, &get, &post);
        let handler = screen_handler(&screen, &asked);
        let mut write = writer.begin().await.unwrap();
        let Ok(Answer::Redirect(to)) = handler(&mut write, &asked, &Unread::default()).await else {
            panic!("{case}: no redirect");
        };
        assert_eq!(to, hs(php_location), "{case}");
        answered += 1;
    }
    assert_eq!(answered, 13);
}

/// What a save of `post` (and `get`) on `screen` answers after `setup`, in
/// one action that is rolled back: the redirect, or `None` when not ported.
async fn answer(
    setup: &[(&str, Value)],
    screen: &str,
    get: &[(Key, Value)],
    post: &[(Key, Value)],
) -> Option<String> {
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    for (option, value) in setup {
        update_option(&mut write, option, value, None)
            .await
            .unwrap();
    }
    let asked = asked(screen, get, post);
    match screen_handler(screen, &asked)(&mut write, &asked, &Unread::default()).await {
        Ok(Answer::Redirect(to)) => Some(to),
        Err(Stop::Failed(Error::Unported(_))) => None,
        Err(Stop::Failed(err)) => panic!("{screen}: {err:?}"),
        _ => panic!("{screen}: neither a redirect nor unported"),
    }
}

/// What PHP stops on, or goes on to a screen not ported after, answers 501;
/// what an old site's lists add is saved.
#[tokio::test]
async fn unported_saves() {
    let _one = ONE.lock().await;
    let text = |text: &str| Value::String(text.as_bytes().to_vec());
    let options = "/hs-admin/options";
    let general = |pairs: &[(&str, &str)]| page_form("general", pairs);
    let unregistered = form(&[("action", "update"), ("page_options", "blogname")]);
    assert_eq!(answer(&[], options, &[], &unregistered).await, None);
    let mut zones = general(&[]);
    zones.push((
        Key::String(b"timezone_string".to_vec()),
        Value::Array(form(&[("0", "x")])),
    ));
    assert_eq!(answer(&[], options, &[], &zones).await, None);
    // An unknown zone keeps a stored one; an empty list is no zone.
    let back = Some("/hs-admin/options-general?settings-updated=true".to_string());
    let london = [("timezone_string", text("Europe/London"))];
    let mars = general(&[("timezone_string", "Mars/Base")]);
    assert_eq!(answer(&london, options, &[], &mars).await, back);
    let mut empty = general(&[]);
    empty.push((
        Key::String(b"timezone_string".to_vec()),
        Value::Array(Vec::new()),
    ));
    assert_eq!(answer(&[], options, &[], &empty).await, back);
    let named = general(&[("timezone_string", "Europe/London")]);
    assert_eq!(answer(&[], options, &[], &named).await, back);
    // A list where PHP saves any value is saved serialized.
    let mut listed = page_form("discussion", &[]);
    listed.push((
        Key::String(b"comments_notify".to_vec()),
        Value::Array(form(&[("0", "1")])),
    ));
    let discussed = "/hs-admin/options-discussion?settings-updated=true";
    assert_eq!(
        answer(&[], options, &[], &listed).await.as_deref(),
        Some(discussed)
    );
    let language = [("WPLANG", text("de_DE"))];
    let english = general(&[("WPLANG", "")]);
    assert_eq!(answer(&language, options, &[], &english).await, None);
    let latin = [("blog_charset", text("ISO-8859-1"))];
    let reading = page_form("reading", &[]);
    assert_eq!(answer(&latin, options, &[], &reading).await, None);
    let old = [("initial_db_version", text("30000"))];
    let writing = page_form("writing", &[("use_smilies", "1")]);
    let back = "/hs-admin/options-writing?settings-updated=true";
    assert_eq!(
        answer(&old, options, &[], &writing).await.as_deref(),
        Some(back)
    );
    // A pending change without its key, and a key that is not it.
    let keyless = [("adminhash", Value::Array(form(&[("newemail", "a@b.test")])))];
    let get = form(&[("adminhash", "0123")]);
    assert_eq!(answer(&keyless, options, &get, &[]).await, None);
    let keyed = [(
        "adminhash",
        Value::Array(form(&[("hash", "4567"), ("newemail", "a@b.test")])),
    )];
    let not_updated = format!("{HOME}/hs-admin/options-general?updated=false");
    assert_eq!(answer(&keyed, options, &get, &[]).await, Some(not_updated));
    let permalinks = "/hs-admin/options-permalink";
    let base = form(&[("category_base", "topics")]);
    assert_eq!(answer(&[], permalinks, &[], &base).await, None);
    let mut listed = form(&[("tag_base", "")]);
    listed.push((
        Key::String(b"permalink_structure".to_vec()),
        Value::Array(Vec::new()),
    ));
    assert_eq!(answer(&[], permalinks, &[], &listed).await, None);
}

/// The error a save stopped with; PHP's `wp_die()` fails the test.
fn failed(stop: Stop) -> Error {
    match stop {
        Stop::Failed(err) => err,
        Stop::Died(_) => panic!("died"),
    }
}

/// A save's statements fail one at a time: each answers the database's error.
#[tokio::test]
async fn saves_fail_at_each_statement() {
    let _one = ONE.lock().await;
    let writer = writer().await;
    let saves = [
        (
            "/hs-admin/options",
            page_form("general", &[("home", "https://x.test")]),
        ),
        (
            "/hs-admin/options",
            page_form("reading", &[("page_on_front", "2")]),
        ),
        (
            "/hs-admin/options-permalink",
            form(&[("permalink_structure", "/%postname%/")]),
        ),
        (
            "/hs-admin/options-permalink",
            form(&[("permalink_structure", "/archives/")]),
        ),
        (
            "/hs-admin/options-privacy",
            form(&[("action", "create-privacy-page")]),
        ),
    ];
    for (screen, post) in saves {
        let asked = asked(screen, &[], &post);
        let handler = screen_handler(screen, &asked);
        let saved = settled(async |budget| {
            let mut write = writer.begin().await?;
            write.fail_after(budget);
            handler(&mut write, &asked, &Unread::default())
                .await
                .map_err(failed)
        });
        assert!(saved.await.is_ok(), "{screen}");
    }
    for get in [
        form(&[("adminhash", "0123456789abcdef0123456789abcdef")]),
        form(&[("dismiss", "new_admin_email")]),
    ] {
        let asked = asked("/hs-admin/options", &get, &[]);
        let handler = screen_handler("options", &asked);
        let saved = settled(async |budget| {
            let mut write = writer.begin().await?;
            let pending = Value::Array(form(&[
                ("hash", "0123456789abcdef0123456789abcdef"),
                ("newemail", "new@example.test"),
            ]));
            update_option(&mut write, "adminhash", &pending, None).await?;
            write.fail_after(budget);
            handler(&mut write, &asked, &Unread::default())
                .await
                .map_err(failed)
        });
        assert!(saved.await.is_ok());
    }
}

/// Sites installed before database 32453 also save the Writing screen's
/// formatting options; later ones do not.
#[tokio::test]
async fn old_sites_save_formatting() {
    let _one = ONE.lock().await;
    let writer = writer().await;
    let post = page_form("writing", &[("use_smilies", "0")]);
    let asked = asked("/hs-admin/options", &[], &post);
    for (initial, smilies) in [("32452", "0"), ("32453", "1")] {
        let mut write = writer.begin().await.unwrap();
        let version = Value::String(initial.as_bytes().to_vec());
        update_option(&mut write, "initial_db_version", &version, None)
            .await
            .unwrap();
        let saved = update(&mut write, &asked, &Unread::default()).await;
        assert!(matches!(saved, Ok(Answer::Redirect(_))), "{initial}");
        let stored = write.option_value("use_smilies").await.unwrap();
        assert_eq!(stored.as_deref(), Some(smilies.as_bytes()), "{initial}");
    }
}

/// The rest of the requests' shapes: an offset west of UTC, a site that
/// pings nobody, a base PHP stops on, and a key that cannot be made.
#[tokio::test]
async fn other_requests() {
    let _one = ONE.lock().await;
    let text = |text: &str| Value::String(text.as_bytes().to_vec());
    let options = "/hs-admin/options";
    let west = page_form("general", &[("timezone_string", "UTC-3")]);
    let general = "/hs-admin/options-general?settings-updated=true";
    assert_eq!(
        answer(&[], options, &[], &west).await.as_deref(),
        Some(general)
    );
    let private = [("blog_public", text("0"))];
    let writing = page_form("writing", &[]);
    let back = "/hs-admin/options-writing?settings-updated=true";
    assert_eq!(
        answer(&private, options, &[], &writing).await.as_deref(),
        Some(back)
    );
    let mut base = form(&[("permalink_structure", "/%postname%/")]);
    base.push((Key::String(b"tag_base".to_vec()), Value::Array(Vec::new())));
    let permalinks = "/hs-admin/options-permalink";
    assert_eq!(answer(&[], permalinks, &[], &base).await, None);
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let email = text("new@example.test");
    let failed = new_admin_email(&mut write, &email, Err(std::io::Error::other("no"))).await;
    assert!(matches!(failed, Err(Error::Random(_))));
}
