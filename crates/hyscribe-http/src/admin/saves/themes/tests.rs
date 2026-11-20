// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Themes screen's activation against what PHP answers
//! (`reference/admin-saves/cases.list`), on a copy of the fixture its writer
//! writes, and the next load's follow-ups on a site that switched theme.

use std::sync::Arc;

use hyscribe_config::Mode;
use hyscribe_db::{Connection, Writer};
use hyscribe_phpser::{Key, Value};
use hyscribe_testkit::writes::check_writes;
use hyscribe_testkit::{fixture_database, hs_admin_page, unhex};
use hyscribe_write::options::{delete_option, update_option};

use crate::admin::saves::posts::tests::unnonced;
use crate::admin::saves::tests::{
    AFTER, ANSWERS, CASES, Request, SITE, answered, case_row, hs, redirected, turn, writer,
};
use crate::admin::tests::{SAVES, location, logged_in, page_of, send, session_nonce, unwritable};

/// The database the answers are compared on: `wordpress_saves` loaded with
/// the fixture.
async fn copy() -> hyscribe_config::Database {
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    db
}

/// `text` with the ten characters after each `_wpnonce=` of `to` where
/// `from` has its own.
fn nonces_of(from: &str, to: &str) -> String {
    let at = |text: &str| text.find("_wpnonce=").map(|at| at + 9);
    match (at(from), at(to)) {
        (Some(php), Some(ours)) => format!(
            "{}{}{}",
            from.get(..php).unwrap(),
            to.get(ours..ours + 10).unwrap(),
            nonces_of(from.get(php + 10..).unwrap(), to.get(ours + 10..).unwrap())
        ),
        _ => from.to_string(),
    }
}

/// A site on `wordpress_saves`, loaded afresh, logged in, in primary mode.
async fn site(dir: &std::path::Path) -> (Arc<crate::Site>, String) {
    let config = copy().await;
    let (site, cookies) = logged_in("wordpress_saves", dir).await;
    let writer = Writer::open(&config, Mode::Primary).await.unwrap();
    let site = Arc::try_unwrap(site).ok().unwrap().with_writer(writer);
    (Arc::new(site), cookies)
}

/// Each activation of `reference/admin-saves/cases.list` answers as PHP
/// does, and the Themes screen after it, on the next load, is PHP's.
#[tokio::test(flavor = "multi_thread")]
async fn the_activations_answer_as_php() {
    let _saves = SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    for case in [
        "themes-activate-tt4",
        "themes-activate-tt3",
        "themes-activate-active",
        "themes-activate-missing",
    ] {
        let (site, cookies) = site(dir.path()).await;
        let [_, _, target, _, action, field, ..] = case_row(CASES, case).unwrap()[..] else {
            panic!("{case}");
        };
        let nonce = session_nonce(&site, &cookies, action).await;
        let uri = format!("/hs-admin/{}&{field}={nonce}", hs(target));
        let response = send(&site, "GET", &uri, &cookies, None).await;
        let [_, status, php_location, php_body] = case_row(ANSWERS, case).unwrap()[..] else {
            panic!("{case}");
        };
        assert_eq!(response.status().as_str(), status, "{case}");
        let ours = (response
            .headers()
            .get(axum::http::header::LOCATION)
            .is_some())
        .then(|| location(&response).to_string());
        let php = hs(&nonces_of(php_location, &format!("_wpnonce={nonce}")));
        assert_eq!(ours.as_deref().unwrap_or_default(), php, "{case}");
        let php = hs_admin_page(&String::from_utf8(unhex(php_body)).unwrap());
        assert_eq!(page_of(response).await, php, "{case}");
        let Some(to) = ours else {
            continue;
        };
        let [_, status, php_body] = case_row(AFTER, case).unwrap()[..] else {
            panic!("{case}");
        };
        let now = crate::admin::saves::themes::checked(site.reloaded().await.unwrap()).await;
        assert!(
            now.options
                .get("theme_switched")
                .is_none_or(|v| v.is_empty())
        );
        let response = send(
            &Arc::new(now),
            "GET",
            to.trim_start_matches(SITE),
            &cookies,
            None,
        )
        .await;
        assert_eq!(response.status().as_str(), status, "{case}");
        let php = hs_admin_page(&String::from_utf8(unhex(php_body)).unwrap());
        let page = page_of(response).await;
        // Times, like nonces, are ten digits.
        let php = unnonced(&php).replace("%time%", "%nonce%");
        let page = unnonced(&page);
        let at = page.bytes().zip(php.bytes()).position(|(a, b)| a != b);
        let near = |text: &str| {
            text.get(at.unwrap_or(0).saturating_sub(100)..)
                .unwrap_or_default()
                .chars()
                .take(300)
                .collect::<String>()
        };
        assert!(
            page == php,
            "{case}\n  php:  {:?}\n  ours: {:?}",
            near(&php),
            near(&page)
        );
    }
}

/// What a site that never switched, one without a writer, one on a theme it
/// does not have, and one whose database takes no write, is as it was.
#[tokio::test(flavor = "multi_thread")]
async fn the_next_load_leaves_other_sites_alone() {
    let _saves = SAVES.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let (site, _) = site(dir.path()).await;
    let site = Arc::try_unwrap(site).ok().unwrap();
    let switched = |site: &crate::Site, value: &str| {
        let mut options = site.options.clone();
        options.insert("theme_switched".into(), value.as_bytes().to_vec());
        options
    };
    for value in ["", "0"] {
        let mut idle = site.reloaded().await.unwrap();
        idle.options = switched(&site, value);
        let idle = crate::admin::saves::themes::checked(idle).await;
        assert_eq!(
            idle.options.get("theme_switched").unwrap(),
            value.as_bytes()
        );
    }
    // Another theme than the embedded ones.
    let mut foreign = site.reloaded().await.unwrap();
    foreign.options = switched(&site, "twentytwentyfour");
    foreign
        .options
        .insert("stylesheet".into(), b"gone".to_vec());
    let foreign = crate::admin::saves::themes::checked(foreign).await;
    assert_eq!(
        foreign.options.get("theme_switched").unwrap(),
        b"twentytwentyfour"
    );
    // No writer.
    let mut shadow = site.reloaded().await.unwrap();
    shadow.writer = None;
    shadow.options = switched(&site, "twentytwentyfour");
    let shadow = crate::admin::saves::themes::checked(shadow).await;
    assert_eq!(
        shadow.options.get("theme_switched").unwrap(),
        b"twentytwentyfour"
    );
    // A database that takes no write.
    let site = unwritable(Arc::new(site), "missing_").await;
    let mut stuck = site.reloaded().await.unwrap();
    stuck.options = switched(&site, "twentytwentyfour");
    let stuck = crate::admin::saves::themes::checked(stuck).await;
    assert_eq!(
        stuck.options.get("theme_switched").unwrap(),
        b"twentytwentyfour"
    );
}

/// The activation writes PHP's rows, as one action, and every statement of
/// it can fail: nothing is kept then.
#[tokio::test(flavor = "multi_thread")]
async fn the_activation_writes_php_s_rows() {
    let _turn = turn().await;
    let writer = writer().await;
    let request = Request {
        screen: "themes",
        action: b"activate",
        get: "action=activate&stylesheet=twentytwentyfour",
        post: "",
        user: 1,
    };
    // The site every oracle case starts from (`$themes_base`).
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        for prefix in [
            "_site_transient_wp_theme_files_patterns-",
            "_site_transient_timeout_wp_theme_files_patterns-",
        ] {
            for (name, _) in write.options_starting(prefix).await.unwrap() {
                delete_option(&mut write, &name).await.unwrap();
            }
        }
        delete_option(&mut write, "theme_mods_twentytwentyfive")
            .await
            .unwrap();
        let css = (Key::String(b"custom_css_post_id".to_vec()), Value::Int(-1));
        let mods = Value::Array(vec![css]);
        update_option(&mut write, "theme_mods_twentytwentyfive", &mods, None)
            .await
            .unwrap();
        write.commit().await.unwrap();
        Ok::<(), String>(())
    };
    let action = async |conn: &mut Connection, ()| {
        let to = redirected(&writer, conn, &request).await;
        assert_eq!(to, format!("{SITE}/hs-admin/themes?activated=true"));
        Ok(())
    };
    let report = check_writes(
        &fixture_database(),
        "admin-themes-activate-twentytwentyfour",
        setup,
        action,
    )
    .await;
    report.unwrap();
}

/// A theme the site does not have is refused as PHP refuses it, a switch
/// from one is not ported.
#[tokio::test(flavor = "multi_thread")]
async fn themes_that_do_not_exist_are_refused() {
    use hyscribe_admin::posts::Answer;
    let _turn = turn().await;
    let writer = writer().await;
    let _ = check_writes(
        &fixture_database(),
        "admin-themes-activate-twentytwentyfour",
        async |_: &mut Connection| Ok::<(), String>(()),
        async |conn: &mut Connection, ()| {
            let missing = Request {
                screen: "themes",
                action: b"activate",
                get: "action=activate&stylesheet=nope",
                post: "",
                user: 1,
            };
            let died = answered(&writer, conn, &missing).await;
            assert!(
                matches!(died, Ok(Answer::Denied(message)) if message.contains("does not exist"))
            );
            // The site's own theme is not one it has.
            let mut write = writer.begin_on(conn).await.unwrap();
            update_option(
                &mut write,
                "stylesheet",
                &Value::String(b"gone".to_vec()),
                None,
            )
            .await
            .unwrap();
            write.commit().await.unwrap();
            let request = Request {
                get: "action=activate&stylesheet=twentytwentyfour",
                ..missing
            };
            let refused = answered(&writer, conn, &request).await;
            assert!(matches!(refused, Err(hyscribe_write::Error::Unported(_))));
            Ok(())
        },
    )
    .await;
}
