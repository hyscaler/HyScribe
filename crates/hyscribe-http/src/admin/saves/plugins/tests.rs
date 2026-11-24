// SPDX-License-Identifier: AGPL-3.0-or-later

//! The activation and deactivation against the write oracle
//! (`reference/writes/actions-plugins.php`), with the test extension's
//! manifest installed in a folder of the test's own.

use std::path::{Path, PathBuf};

use hyscribe_db::{Connection, Writer};
use hyscribe_ext::installed::APPROVED;
use hyscribe_phpser::{Key, Value};
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;
use hyscribe_write::options::{Autoload, update_option};

use super::super::handler;
use super::super::tests::unread::Unread;
use super::super::tests::{SITE, acted, asked, fields, turn, writer};
use super::*;

use crate::admin::saves::posts::tests::unnonced;
use crate::extensions::tests::{FILE, MANIFEST, install};

/// A plugins folder with the test extension installed, and its own folder.
fn plugins() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let folder = install(dir.path());
    (dir, folder)
}

fn list(files: &[&str]) -> Value {
    let files = files
        .iter()
        .map(|file| Value::String(file.as_bytes().to_vec()));
    Value::Array((0..).map(Key::Int).zip(files).collect())
}

/// `recently_activated` as the cases that have one start from.
fn recent() -> Value {
    Value::Array(vec![
        (Key::String(FILE.into()), Value::Int(4_102_444_800)),
        (
            Key::String(b"old/old.php".to_vec()),
            Value::Int(1_000_000_000),
        ),
    ])
}

/// `action` of the test extension from `active` and `recently`, as the
/// golden `admin-plugins-<golden>`, answering a redirect to `done`, on a
/// site whose extensions are in `dir`, if it has any.
async fn case(
    writer: &Writer,
    golden: &str,
    (active, recently): (&[&str], Option<Value>),
    (action, done): (&'static [u8], &str),
    dir: Option<&Path>,
) {
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        update_option(&mut write, "active_plugins", &list(active), None)
            .await
            .unwrap();
        if let Some(recently) = &recently {
            update_option(
                &mut write,
                "recently_activated",
                recently,
                Some(Autoload::Off),
            )
            .await
            .unwrap();
        }
        write.commit().await.unwrap();
        Ok::<(), String>(())
    };
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap();
    let action = async |conn: &mut Connection, ()| {
        let get = fields(&format!(
            "action={}&plugin={FILE}&plugin_status=all&paged=1&s=",
            text(action)
        ));
        let asked = Asked {
            now,
            ..asked("/hs-admin/plugins", &get, &[])
        };
        let handler = handler("plugins", action).unwrap();
        let site = Unread(
            Default::default(),
            false,
            dir.map(|dir| Runtime::new(dir, dir)),
        );
        let answer = acted(writer, conn, (handler, &site), &asked).await;
        let to = format!("{SITE}/hs-admin/plugins?{done}=true&plugin_status=all&paged=1&s=");
        assert_eq!(answer.map_err(|e| format!("{e:?}"))?, Answer::Redirect(to));
        Ok(())
    };
    let golden = format!("admin-plugins-{golden}");
    check_writes(&fixture_database(), &golden, setup, action)
        .await
        .unwrap();
}

/// Each case writes PHP's rows, as one action whose every statement can
/// fail; activation approves the extension, deactivation takes it back.
#[tokio::test(flavor = "multi_thread")]
async fn the_actions_write_php_s_rows() {
    let _turn = turn().await;
    let writer = writer().await;
    let (dir, folder) = plugins();
    let path = dir.path();
    std::fs::write(folder.join(hyscribe_ext::faults::FILE), "nonsense").unwrap();
    case(
        &writer,
        "activate",
        (&[], None),
        (b"activate", "activate"),
        Some(path),
    )
    .await;
    assert!(folder.join(APPROVED).is_file());
    assert!(!folder.join(hyscribe_ext::faults::FILE).exists());
    let other = ["wordfence/wordfence.php"];
    case(
        &writer,
        "activate-sorted",
        (&other, None),
        (b"activate", "activate"),
        Some(path),
    )
    .await;
    case(
        &writer,
        "activate-recent",
        (&[], Some(recent())),
        (b"activate", "activate"),
        Some(path),
    )
    .await;
    case(
        &writer,
        "deactivate",
        (&[FILE], None),
        (b"deactivate", "deactivate"),
        Some(path),
    )
    .await;
    assert!(!folder.join(APPROVED).exists());
    let both = [FILE, "wordfence/wordfence.php"];
    case(
        &writer,
        "deactivate-kept",
        (&both, None),
        (b"deactivate", "deactivate"),
        Some(path),
    )
    .await;
    let inactive = (&[][..], Some(recent()));
    case(
        &writer,
        "deactivate-inactive",
        inactive,
        (b"deactivate", "deactivate"),
        Some(path),
    )
    .await;
    // A site without extensions runs none, and writes the same rows.
    let deactivate = (b"deactivate".as_slice(), "deactivate");
    case(&writer, "deactivate", (&[FILE], None), deactivate, None).await;
}

/// The answer of `action` on `get` on a site whose plugins are in `dir`.
async fn answer(
    (writer, conn): (&Writer, &mut Connection),
    action: &'static [u8],
    get: &str,
    dir: Option<PathBuf>,
) -> Result<Answer, hyscribe_write::Error> {
    let get = fields(&format!("action={}&{get}", text(action)));
    let asked = asked("/hs-admin/plugins", &get, &[]);
    let site = Unread(
        Default::default(),
        false,
        dir.map(|dir| Runtime::new(&dir, &dir)),
    );
    acted(
        writer,
        conn,
        (handler("plugins", action).unwrap(), &site),
        &asked,
    )
    .await
}

/// What PHP dies of, or cannot go on with, writes nothing; an approval
/// that cannot be written or taken back stops the action, its rows rolled
/// back.
#[tokio::test(flavor = "multi_thread")]
async fn refusals_write_nothing() {
    let _turn = turn().await;
    let writer = writer().await;
    let (dir, folder) = plugins();
    let path = Some(dir.path().to_path_buf());
    let none = async |_: &mut Connection| Ok::<(), String>(());
    let action = async |conn: &mut Connection, ()| {
        let invalid = answer(
            (&writer, &mut *conn),
            b"activate",
            "plugin=..%2Fx.php",
            path.clone(),
        )
        .await;
        assert!(matches!(invalid, Ok(Answer::Died("Invalid plugin path."))));
        let missing = answer(
            (&writer, &mut *conn),
            b"activate",
            "plugin=missing%2Fmissing.php",
            path.clone(),
        )
        .await;
        assert!(matches!(
            missing,
            Ok(Answer::Died("Plugin file does not exist."))
        ));
        let nowhere = answer(
            (&writer, &mut *conn),
            b"activate",
            &format!("plugin={FILE}"),
            None,
        )
        .await;
        assert!(matches!(
            nowhere,
            Ok(Answer::Died("Plugin file does not exist."))
        ));
        let get = format!("plugin={FILE}&from=import");
        let imported = answer((&writer, &mut *conn), b"activate", &get, path.clone()).await;
        assert!(matches!(imported, Err(hyscribe_write::Error::Unported(_))));
        // Active already, an activation writes only `recently_activated`;
        // a deactivation needs no `active_plugins`, nor an extension.
        let mut write = writer.begin_on(conn).await.unwrap();
        update_option(&mut write, "active_plugins", &list(&[FILE]), None)
            .await
            .unwrap();
        write.commit().await.unwrap();
        let again = answer(
            (&writer, &mut *conn),
            b"activate",
            &format!("plugin={FILE}"),
            path.clone(),
        )
        .await;
        assert!(matches!(again, Ok(Answer::Redirect(_))));
        let mut write = writer.begin_on(conn).await.unwrap();
        let text = Value::String(b"x".to_vec());
        update_option(&mut write, "active_plugins", &text, None)
            .await
            .unwrap();
        write.commit().await.unwrap();
        let unread = answer(
            (&writer, &mut *conn),
            b"activate",
            &format!("plugin={FILE}"),
            path.clone(),
        )
        .await;
        assert!(matches!(unread, Err(hyscribe_write::Error::Unported(_))));
        let mut write = writer.begin_on(conn).await.unwrap();
        hyscribe_write::options::delete_option(&mut write, "active_plugins")
            .await
            .unwrap();
        write.commit().await.unwrap();
        let none = answer(
            (&writer, &mut *conn),
            b"deactivate",
            &format!("plugin={FILE}"),
            None,
        )
        .await;
        assert!(matches!(none, Ok(Answer::Redirect(_))));
        // A fault record that cannot be cleared.
        std::fs::create_dir(folder.join(hyscribe_ext::faults::FILE)).unwrap();
        let get = format!("plugin={FILE}");
        let uncleared = answer((&writer, &mut *conn), b"activate", &get, path.clone()).await;
        assert!(matches!(uncleared, Err(hyscribe_write::Error::Refused(_))));
        std::fs::remove_dir(folder.join(hyscribe_ext::faults::FILE)).unwrap();
        // The approval the activation above wrote, made a folder.
        std::fs::remove_file(folder.join(APPROVED)).unwrap();
        std::fs::create_dir(folder.join(APPROVED)).unwrap();
        let unapproved = answer(
            (&writer, &mut *conn),
            b"activate",
            &format!("plugin={FILE}"),
            path.clone(),
        )
        .await;
        assert!(matches!(unapproved, Err(hyscribe_write::Error::Refused(_))));
        let unrevoked = answer(
            (&writer, &mut *conn),
            b"deactivate",
            &format!("plugin={FILE}"),
            path.clone(),
        )
        .await;
        assert!(matches!(unrevoked, Err(hyscribe_write::Error::Refused(_))));
        Ok(())
    };
    // Nothing is written: the golden of one that writes tells no rows apart.
    let checked = check_writes(
        &fixture_database(),
        "admin-plugins-deactivate-inactive",
        none,
        action,
    )
    .await;
    assert!(checked.unwrap_err().contains("recently_activated"));
}

/// A view keeps the plugins of the last week, as PHP's does; every
/// statement of it can fail, and nothing is kept then.
#[tokio::test(flavor = "multi_thread")]
async fn a_view_keeps_the_last_week() {
    let _turn = turn().await;
    let writer = writer().await;
    let stale = Value::Array(vec![
        (Key::String(FILE.into()), Value::Int(4_102_444_800)),
        (
            Key::String(b"old/old.php".to_vec()),
            Value::Int(1_000_000_000),
        ),
        (
            Key::String(b"x/x.php".to_vec()),
            Value::String(b"soon".to_vec()),
        ),
    ]);
    for (golden, stored) in [("view", None), ("view-recent", Some(stale))] {
        let setup = async |conn: &mut Connection| {
            if let Some(stored) = &stored {
                let mut write = writer.begin_on(conn).await.unwrap();
                let off = Some(Autoload::Off);
                update_option(&mut write, "recently_activated", stored, off)
                    .await
                    .unwrap();
                write.commit().await.unwrap();
            }
            Ok::<(), String>(())
        };
        let action = async |conn: &mut Connection, ()| {
            let kept = super::super::tests::settled(async |budget| {
                let mut write = writer.begin_on(conn).await?;
                write.fail_after(budget);
                keep_recent(&mut write, 2_000_000_000).await?;
                write.commit().await.map_err(hyscribe_write::Error::from)
            });
            kept.await.map_err(|err| format!("{err:?}"))
        };
        let golden = format!("admin-plugins-{golden}");
        check_writes(&fixture_database(), &golden, setup, action)
            .await
            .unwrap();
    }
    // A value the screen does not read is left as it is.
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        let text = Value::String(b"x".to_vec());
        update_option(&mut write, "recently_activated", &text, None)
            .await
            .unwrap();
        write.commit().await.unwrap();
        Ok::<(), String>(())
    };
    let action = async |conn: &mut Connection, ()| {
        let mut write = writer.begin_on(conn).await.unwrap();
        keep_recent(&mut write, 0).await.unwrap();
        write.commit().await.map_err(|err| err.to_string())
    };
    let none = check_writes(&fixture_database(), "admin-plugins-view", setup, action).await;
    assert!(none.unwrap_err().contains("recently_activated"));
}

/// `reference/plugins/golden.sh`'s answers to the actions: a case, its
/// status, its `Location`, its body in hexadecimal and the options after.
const ACTIONS: &str = include_str!("../../../../tests/golden/plugin-actions.tsv");

/// `reference/plugins/golden.sh`'s views of the screen.
const SCREENS: &str = include_str!("../../../../../hyscribe-admin/tests/golden/plugins.tsv");

/// The recorded page of the view `variant` with `query`, as hs-admin
/// prints it, its plugins folder named as HyScribe names it.
fn screen(variant: &str, query: &str) -> String {
    let start = format!("{variant}\t{query}\t");
    let row = SCREENS.lines().find(|line| line.starts_with(&start));
    let page = row.unwrap().rsplit('\t').next().unwrap();
    let page = String::from_utf8(hyscribe_testkit::unhex(page)).unwrap();
    hyscribe_testkit::hs_admin_page(&page)
        .replace("/var/www/html/wp-content/plugins", "hyscribe/plugins/")
}

/// `page` with its nonces and HyScribe's own part of its rows taken out.
fn compared(page: &str) -> String {
    let mut out = String::new();
    let mut rest = page;
    while let Some((before, after)) = rest.split_once("<div class=\"hyscribe-grants ") {
        out.push_str(before);
        rest = after.split_once("</div>").map_or("", |(_, tail)| tail);
    }
    // Times, like nonces, are ten digits.
    crate::admin::saves::posts::tests::unnonced(&(out + rest)).replace("%time%", "%nonce%")
}

/// Fails with where `ours` first differs from `php`.
fn same(php: &str, ours: &str, what: &str) {
    let at = php.bytes().zip(ours.bytes()).position(|(a, b)| a != b);
    let at = at.unwrap_or(php.len().min(ours.len()));
    let near = |text: &str| {
        text.get(at.saturating_sub(80)..)
            .unwrap_or_default()
            .chars()
            .take(240)
            .collect::<String>()
    };
    assert!(
        php == ours,
        "{what}\n  php:  {:?}\n  ours: {:?}",
        near(php),
        near(ours)
    );
}

/// The recorded answer of `case`: its fields after its name.
fn action_row(case: &str) -> Vec<&'static str> {
    let start = format!("{case}\t");
    let row = ACTIONS.lines().find(|line| line.starts_with(&start));
    row.unwrap().split('\t').skip(1).collect()
}

/// hs-admin on `database` with the test extension installed, logged in as
/// user 1, with `writer` if any.
async fn served(
    database: &str,
    writer: Option<Writer>,
    dir: &Path,
) -> (std::sync::Arc<crate::Site>, String, tempfile::TempDir) {
    let (plugins, _) = plugins();
    let (site, cookies) = crate::admin::tests::logged_in(database, dir).await;
    let site = std::sync::Arc::try_unwrap(site).ok().unwrap();
    let site = site
        .with_extensions(plugins.path(), plugins.path())
        .with_writer(writer);
    (std::sync::Arc::new(site), cookies, plugins)
}

/// The screen lists the installed extension as PHP lists the plugin it
/// stands in for, but for what it asks for, shown before activation.
#[tokio::test(flavor = "multi_thread")]
async fn the_screen_lists_installed_extensions() {
    use crate::admin::tests::{page_of, send};
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies, _plugins) = served("wordpress", None, dir.path()).await;
    for query in ["", "?plugin_status=search"] {
        let uri = format!("/hs-admin/plugins{query}");
        let response = send(&site, "GET", &uri, &cookies, None).await;
        assert_eq!(response.status(), 200);
        let page = page_of(response).await;
        assert_eq!(page.contains("hyscribe-grants"), query.is_empty());
        let php = screen("x13inactive", query);
        same(&compared(&php), &compared(&page), query);
    }
    let response = send(&site, "GET", "/hs-admin/plugins?s=x", &cookies, None).await;
    assert_eq!(response.status(), 501);
}

/// The refusals and the deaths of `reference/plugins/actions.list`, as PHP
/// answers them, writing nothing.
#[tokio::test(flavor = "multi_thread")]
async fn refused_actions_answer_as_php() {
    use crate::admin::tests::{page_of, send, session_nonce, unwritable};
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies, plugins) = served("wordpress", None, dir.path()).await;
    let site = unwritable(site, "wp_").await;
    let test = "hyscribe-test%2Fhyscribe-test.php";
    let cases = [
        (
            "activate-bad-nonce",
            format!("action=activate&plugin={test}"),
            None,
        ),
        (
            "activate-missing",
            "action=activate&plugin=missing%2Fmissing.php".to_string(),
            Some("activate-plugin_missing/missing.php"),
        ),
        (
            "activate-invalid",
            "action=activate&plugin=..%2Fx.php".to_string(),
            Some("activate-plugin_../x.php"),
        ),
        (
            "deactivate-bad-nonce",
            format!("action=deactivate&plugin={test}"),
            None,
        ),
        (
            "delete",
            format!("action=delete-selected&checked%5B0%5D={test}"),
            Some("bulk-plugins"),
        ),
    ];
    for (case, query, action) in cases {
        let nonce = match action {
            Some(action) => session_nonce(&site, &cookies, action).await,
            None => "0123456789".into(),
        };
        let uri =
            format!("/hs-admin/plugins?{query}&plugin_status=all&paged=1&s=&_wpnonce={nonce}");
        let response = send(&site, "GET", &uri, &cookies, None).await;
        let [status, _, body, ..] = action_row(case)[..] else {
            panic!("{case}");
        };
        assert_eq!(response.status().as_str(), status, "{case}");
        let php = String::from_utf8(hyscribe_testkit::unhex(body)).unwrap();
        let php = hyscribe_testkit::hs_admin_page(&php);
        same(&php, &page_of(response).await, case);
    }
    drop(plugins);
}

/// Activating and deactivating through hs-admin answer PHP's redirects, the
/// screen after an activation is PHP's whole, and `active_plugins` holds
/// the plugin the extension stands in for, as PHP reads it.
#[tokio::test(flavor = "multi_thread")]
async fn activation_and_deactivation_answer_as_php() {
    use crate::admin::tests::{SAVES, location, page_of, send, session_nonce};
    let _saves = SAVES.lock().await;
    let config = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&config)
        .await
        .unwrap();
    let writer = Writer::open(&config, hyscribe_config::Mode::Primary)
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies, _plugins) = served("wordpress_saves", writer, dir.path()).await;
    for action in ["activate", "deactivate"] {
        let nonce = session_nonce(&site, &cookies, &format!("{action}-plugin_{FILE}")).await;
        let uri = format!(
            "/hs-admin/plugins?action={action}&plugin=hyscribe-test%2Fhyscribe-test.php&plugin_status=all&paged=1&s=&_wpnonce={nonce}"
        );
        let response = send(&site, "GET", &uri, &cookies, None).await;
        let [status, php_location, _, active, _] = action_row(action)[..] else {
            panic!("{action}");
        };
        assert_eq!(response.status().as_str(), status, "{action}");
        let to = location(&response).to_string();
        let php_location = php_location.replace("/wp-admin/plugins.php", "/hs-admin/plugins");
        assert_eq!(to, php_location);
        let reloaded = std::sync::Arc::new(site.reloaded().await.unwrap());
        let stored = reloaded.options.get("active_plugins").cloned();
        assert_eq!(format!("{} on", text(&stored.unwrap_or_default())), active);
        if action == "activate" {
            // Through the router, which runs the active extension: its pages
            // are in the menu.
            let response = live(&reloaded, to.trim_start_matches(SITE), &cookies).await;
            let page = page_of(response).await;
            let php = screen("x13active", "?activate=true&plugin_status=all&paged=1&s=");
            assert!(!page.contains("hyscribe-grants"));
            same(&compared(&php), &compared(&page), "activated");
        }
    }
}

/// An activation whose export fails writes nothing of its own (PHP's
/// writes nothing, as the write oracle says of it), and answers PHP's
/// redirect to the screen's error.
#[tokio::test(flavor = "multi_thread")]
async fn a_failed_activation_writes_nothing() {
    let _turn = turn().await;
    let writer = writer().await;
    let (dir, folder) = plugins();
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        let trap = Value::String(b"trap".to_vec());
        update_option(&mut write, "ext_hooks", &trap, None)
            .await
            .unwrap();
        write.commit().await.unwrap();
        Ok::<(), String>(())
    };
    let action = async |conn: &mut Connection, ()| {
        let get = format!("plugin={FILE}");
        let path = Some(dir.path().to_path_buf());
        let failed = answer((&writer, &mut *conn), b"activate", &get, path).await;
        let to = format!(
            "{SITE}/hs-admin/plugins?error=true&plugin=hyscribe-test%2Fhyscribe-test.php&_error_nonce=nonce-plugin-activation-error_{FILE}"
        );
        assert!(matches!(failed, Ok(Answer::Redirect(at)) if at == to));
        // Its component refused: the same answer, nothing run.
        std::fs::write(folder.join("extension.wasm"), b"not wasm").unwrap();
        let get = format!("plugin={FILE}");
        let path = Some(dir.path().to_path_buf());
        let refused = answer((&writer, &mut *conn), b"activate", &get, path).await;
        assert!(matches!(refused, Ok(Answer::Redirect(at)) if at == to));
        Ok(())
    };
    let checked = check_writes(
        &fixture_database(),
        "admin-plugins-deactivate-inactive",
        setup,
        action,
    )
    .await;
    let differences = checked.unwrap_err();
    assert!(differences.contains("recently_activated"), "{differences}");
    assert!(!differences.contains("ext_active"), "{differences}");
    assert!(!differences.contains("active_plugins"), "{differences}");
    assert!(!folder.join(APPROVED).exists());
}

/// A request through the router that reloads the site, as `serve` serves it.
async fn live(
    site: &std::sync::Arc<crate::Site>,
    uri: &str,
    cookies: &str,
) -> axum::response::Response {
    let request = axum::http::Request::builder()
        .uri(uri)
        .header(axum::http::header::HOST, "127.0.0.1:8081")
        .header(axum::http::header::COOKIE, cookies)
        .body(axum::body::Body::empty())
        .unwrap();
    tower::ServiceExt::oneshot(crate::live(site.clone()), request)
        .await
        .unwrap()
}

/// The body tag of `page`.
fn body_tag(page: &str) -> String {
    let start = page.find("<body class=\"").unwrap();
    let tag = page.get(start..).unwrap();
    tag.get(..=tag.find('>').unwrap()).unwrap().to_string()
}

/// `reference/plugins/golden.sh`'s body tag of the front end `path` in
/// `variant`: the page hooks on (`x13hooks`), or set to trap (`x13fail`),
/// which is the page they do not change.
fn php_body(variant: &str, path: &str) -> String {
    let golden = include_str!("../../../../tests/golden/extension-pages.tsv");
    let start = format!("{variant}\t{path}\t");
    let row = golden
        .lines()
        .find(|line| line.starts_with(&start))
        .unwrap();
    String::from_utf8(hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap())).unwrap()
}

/// The test extension installed and activated changes the front end and
/// hs-admin as its hooks say, as the plugin it stands in for changes PHP's,
/// each request in an instance of its own, two at once too; three traps
/// disable it until it is activated again; deactivated, it changes nothing,
/// and an activation whose export traps leaves it inactive as PHP leaves a
/// plugin whose activation ends in a fatal error.
#[tokio::test(flavor = "multi_thread")]
async fn the_extension_runs_as_its_hooks_say() {
    use crate::admin::tests::{SAVES, location, page_of, session_nonce};
    let _saves = SAVES.lock().await;
    let config = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&config)
        .await
        .unwrap();
    let open = async || {
        Writer::open(&config, hyscribe_config::Mode::Primary)
            .await
            .unwrap()
    };
    let writer = open().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies, plugins) = served("wordpress_saves", open().await, dir.path()).await;
    let folder = plugins.path().join("hyscribe-test");
    let act = async |action: &str| {
        let nonce = session_nonce(&site, &cookies, &format!("{action}-plugin_{FILE}")).await;
        let uri = format!(
            "/hs-admin/plugins?action={action}&plugin=hyscribe-test%2Fhyscribe-test.php&plugin_status=all&paged=1&s=&_wpnonce={nonce}"
        );
        location(&live(&site, &uri, &cookies).await).to_string()
    };
    let front = async |path: &str| body_tag(&page_of(live(&site, path, "").await).await);
    let option = async |name: &str| {
        let mut write = writer.begin().await.unwrap();
        write.option_value(name).await.unwrap()
    };
    let hooks = async |value: &str| {
        let mut write = writer.begin().await.unwrap();
        let value = Value::String(value.as_bytes().to_vec());
        update_option(&mut write, "ext_hooks", &value, None)
            .await
            .unwrap();
        write.commit().await.unwrap();
    };
    let done =
        |action| format!("{SITE}/hs-admin/plugins?{action}=true&plugin_status=all&paged=1&s=");
    assert_eq!(act("activate").await, done("activate"));
    assert_eq!(option("ext_active").await.as_deref(), Some(&b"1"[..]));
    assert_eq!(front("/").await, php_body("x13fail", "/"));
    hooks("on").await;
    for path in ["/", "/?p=1"] {
        assert_eq!(front(path).await, php_body("x13hooks", path), "{path}");
    }
    let page = page_of(live(&site, "/hs-admin/plugins", &cookies).await).await;
    same(
        &compared(&screen("x13hooks", "")),
        &compared(&page),
        "hooks",
    );
    let (home, single) = tokio::join!(front("/"), front("/?p=1"));
    assert_eq!(
        (home, single),
        (php_body("x13hooks", "/"), php_body("x13hooks", "/?p=1"))
    );
    hooks("trap").await;
    for _ in 0..3 {
        assert_eq!(front("/").await, php_body("x13fail", "/"));
    }
    let record = hyscribe_ext::faults::read(&folder).unwrap();
    assert!(record.disabled().is_some(), "{record:?}");
    hooks("on").await;
    assert_eq!(front("/").await, php_body("x13fail", "/"));
    // Activating again enables it again; nothing else is run.
    assert_eq!(act("activate").await, done("activate"));
    assert!(!folder.join(hyscribe_ext::faults::FILE).exists());
    assert_eq!(front("/").await, php_body("x13hooks", "/"));
    assert_eq!(act("deactivate").await, done("deactivate"));
    assert_eq!(option("ext_active").await, None);
    assert_eq!(front("/").await, php_body("x13fail", "/"));
    // As the screen PHP's activation sends to was recorded from.
    let mut write = writer.begin().await.unwrap();
    hyscribe_write::options::delete_option(&mut write, "recently_activated")
        .await
        .unwrap();
    write.commit().await.unwrap();
    hooks("trap").await;
    let to = act("activate").await;
    let [_, php_location, ..] = action_row("activate-fails")[..] else {
        panic!("activate-fails");
    };
    let php_location = php_location.replace("/wp-admin/plugins.php", "/hs-admin/plugins");
    assert_eq!(unnonced(&to), unnonced(&php_location));
    let active = option("active_plugins").await.unwrap_or_default();
    assert_eq!(text(&active), "a:0:{}");
    assert!(!folder.join(APPROVED).exists());
    let page = page_of(live(&site, to.trim_start_matches(SITE), &cookies).await).await;
    let query = php_location.split_once("plugins").unwrap().1;
    same(
        &compared(&screen("x13inactive", query)),
        &compared(&page),
        "failed",
    );
}

/// An extension asking for more than the defaults is listed with its limits.
#[test]
fn limits_asked_for_are_listed() {
    let text = format!("{MANIFEST}\n[limits]\ncall_ms = 100\n");
    let installed = hyscribe_ext::installed::Installed {
        manifest: hyscribe_ext::Manifest::parse(text.as_bytes()).unwrap(),
        folder: PathBuf::new(),
        approved: false,
        disabled: false,
    };
    let plugin = crate::admin::listed(installed, None);
    assert_eq!(plugin.limits, Some((100, 250, 16)));
    assert_eq!(plugin.file, FILE);
}

/// Keeps in `folder` a key list of test roots 1 and 2 naming key 10 for the
/// index, and an index of `serial` expiring at `expires` that lists the
/// test extension with `latest` as its newest version.
fn keep_registry(folder: &Path, (serial, expires): (u64, u64), latest: &str) {
    use hyscribe_testkit::minisign::{Key, key_list, sign_keys};
    std::fs::create_dir_all(folder).unwrap();
    let keys = [(&Key(10), r#""index""#, expires - 2000, expires + 10)];
    let list = key_list(1, expires + 10, &keys, &[]);
    for (n, signature) in sign_keys(&list, 1, expires + 10, &[&Key(1), &Key(2)])
        .iter()
        .enumerate()
    {
        let file = folder.join(format!("keys.json.{}.minisig", n + 1));
        std::fs::write(file, signature).unwrap();
    }
    std::fs::write(folder.join("keys.json"), list).unwrap();
    let sdk = hyscribe_ext::manifest::SDK.last().unwrap();
    let index = format!(
        r#"{{"serial":{serial},"expires":{expires},"extensions":[{{"id":"hyscribe-test","kind":"plugin","latest":{{"{sdk}":"{latest}"}},"sha256":"{}","size":10}}],"revoked":[]}}"#,
        "0".repeat(64)
    );
    let comment = format!("hyscribe-index serial={serial} expires={expires}");
    let signature = Key(10).sign(index.as_bytes(), &comment);
    std::fs::write(folder.join("index.json.minisig"), signature).unwrap();
    std::fs::write(folder.join("index.json"), index).unwrap();
}

/// The kept registry index offers a newer version in PHP's update row,
/// with the command that installs it; a revoked version gets an error row,
/// an unsigned package says so, and an index out of date gives no rows
/// and a notice. Rendering writes nothing to `registry/`.
#[tokio::test(flavor = "multi_thread")]
async fn the_screen_shows_what_the_kept_registry_says() {
    use crate::admin::tests::{page_of, send};
    use hyscribe_testkit::minisign::Key as Signer;
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies, plugins) = served("wordpress", None, dir.path()).await;
    let registry = dir.path().join("registry");
    let now = u64::try_from(crate::now()).unwrap();
    keep_registry(&registry, (2, now + 1000), "0.2.0");
    let roots = (1..=3).map(|seed| Signer(seed).public());
    let roots = roots.filter_map(|key| hyscribe_ext::signature::PublicKey::parse(&key));
    let site = std::sync::Arc::try_unwrap(site).ok().unwrap();
    let site = std::sync::Arc::new(site.with_registry(&registry, roots.collect()));
    let folder = plugins.path().join("hyscribe-test");
    let sha256 = "a".repeat(64);
    let origin = |text: String| std::fs::write(folder.join("origin.txt"), text).unwrap();
    origin(format!("registry {} {sha256}\n", Signer(10).id()));
    let page =
        async || page_of(send(&site, "GET", "/hs-admin/plugins", &cookies, None).await).await;
    let offered = page().await;
    assert!(
        offered.contains("<tr class=\"inactive update\""),
        "{offered}"
    );
    assert!(offered.contains("<tr class=\"plugin-update-tr\" id=\"hyscribe-test-update\""));
    assert!(offered.contains("View version 0.2.0 details</a>. Install it on the server with <code>hyscribe plugin update hyscribe-test</code>."));
    assert!(!offered.contains("out of date"));
    origin(format!(
        "registry {} {sha256}\nrevoked 1 a <b>bad</b> build\n",
        Signer(10).id()
    ));
    let revoked = page().await;
    assert!(revoked.contains("notice-error notice-alt\"><p>The registry revoked version 0.1.0 of HyScribe Test: <em>a &lt;b&gt;bad&lt;/b&gt; build</em>."));
    origin(format!("unsigned {sha256}\n"));
    let unsigned = page().await;
    assert!(unsigned.contains("<p>Unsigned package, installed from a local file"));
    assert!(!unsigned.contains("hyscribe-test-update"));
    origin(format!("trusted {} {sha256}\n", Signer(4).id()));
    let trusted = page().await;
    assert!(trusted.contains("<p>Signed by a key this site trusts.</p>"));
    origin(format!("registry {} {sha256}\n", Signer(10).id()));
    keep_registry(&registry, (3, now - 1), "0.2.0");
    let stale = page().await;
    assert!(stale.contains("The update information from the plugin registry is out of date"));
    assert!(!stale.contains("hyscribe-test-update"));
    assert!(!registry.join("state").exists());
    std::fs::remove_dir_all(&registry).unwrap();
    let none = page().await;
    assert!(!none.contains("out of date") && !none.contains("hyscribe-test-update"));
}

/// The test extension's admin pages, activated, as PHP serves the plugin's
/// to the administrator: in the menu, under their URLs, their callbacks'
/// output inside hs-admin's frame.
#[tokio::test(flavor = "multi_thread")]
async fn the_extension_s_admin_pages_are_php_s() {
    use crate::admin::tests::{SAVES, page_of, session_nonce};
    let _saves = SAVES.lock().await;
    let config = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&config)
        .await
        .unwrap();
    let open = async || {
        Writer::open(&config, hyscribe_config::Mode::Primary)
            .await
            .unwrap()
    };
    let writer = open().await.unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies, plugins) = served("wordpress_saves", open().await, dir.path()).await;
    let nonce = session_nonce(&site, &cookies, &format!("activate-plugin_{FILE}")).await;
    let uri = format!(
        "/hs-admin/plugins?action=activate&plugin=hyscribe-test%2Fhyscribe-test.php&plugin_status=all&paged=1&s=&_wpnonce={nonce}"
    );
    assert_eq!(live(&site, &uri, &cookies).await.status(), 302);
    let mut write = writer.begin().await.unwrap();
    let title = Value::String(b"Hello World".to_vec());
    update_option(&mut write, "ext_title", &title, None)
        .await
        .unwrap();
    write.commit().await.unwrap();
    let golden = include_str!("../../../../tests/golden/extension-admin.tsv");
    let mut seen = 0;
    // User 1 as each role in turn, as the golden lists them.
    for asked_as in ["administrator", "editor"] {
        crate::admin::media::tests::as_role(&config, asked_as).await;
        for line in golden.lines() {
            let [_, path, role, status, page] = line.split('\t').collect::<Vec<_>>()[..] else {
                panic!("{line}");
            };
            if role != asked_as {
                continue;
            }
            seen += 1;
            let php = String::from_utf8(hyscribe_testkit::unhex(page)).unwrap();
            let php = hyscribe_testkit::hs_admin_page(&php);
            let asked = format!("/hs-admin/{}", path.replacen(".php", "", 1));
            let response = live(&site, &asked, &cookies).await;
            assert_eq!(response.status().as_str(), status, "{role} {path}");
            same(&compared(&php), &compared(&page_of(response).await), path);
        }
    }
    crate::admin::media::tests::as_role(&config, "administrator").await;
    assert_eq!(seen, 10);
    // A page no active extension has, and one asked with more, are not served.
    for asked in [
        "/hs-admin/admin?page=nope",
        "/hs-admin/admin?page=hyscribe-test-top&x=1",
    ] {
        assert_eq!(live(&site, asked, &cookies).await.status(), 501, "{asked}");
    }
    // Under another menu, its parent is that menu's, whose page it is not:
    // refused, as `user_can_access_admin_page()` refuses it.
    let refused = live(&site, "/hs-admin/tools?page=hyscribe-test", &cookies).await;
    assert_eq!(refused.status(), 403);
    // A site in a folder serves the page below it, and still refuses it
    // to the editor.
    let folder = "http://127.0.0.1:8081/blog";
    let (site, cookies) =
        crate::admin::tests::logged_in_at("wordpress_saves", dir.path(), folder).await;
    let site = std::sync::Arc::try_unwrap(site).ok().unwrap();
    let site = site.with_extensions(plugins.path(), plugins.path());
    let site = std::sync::Arc::new(site.with_writer(open().await));
    let asked = "/blog/hs-admin/admin?page=hyscribe-test-admin";
    let response = live(&site, asked, &cookies).await;
    assert_eq!(response.status(), 200);
    assert!(
        page_of(response)
            .await
            .contains("<p id=\"ext-admin\">true</p>")
    );
    let outside = live(&site, "/hs-admin/admin?page=hyscribe-test-admin", &cookies).await;
    assert!(!page_of(outside).await.contains("ext-admin"));
    crate::admin::media::tests::as_role(&config, "editor").await;
    assert_eq!(live(&site, asked, &cookies).await.status(), 403);
    let asked = "/blog/hs-admin/upload?page=hyscribe-test-admin";
    assert_eq!(live(&site, asked, &cookies).await.status(), 500);
    crate::admin::media::tests::as_role(&config, "administrator").await;
}
