// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_db::Connection;
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;
use hyscribe_write::options::update_option;

use super::*;
use crate::admin::saves::tests::{SITE, settled, writer};

fn view(settings: bool, rules: bool) -> View {
    View {
        synced: Synced::default(),
        welcome: None,
        draft: false,
        settings,
        rules,
        editor: None,
    }
}

/// A Settings screen's view fails at the statement that fails while it
/// refreshes the rules or deletes the notices it showed, and so writes
/// nothing.
#[tokio::test]
async fn a_settings_view_fails_with_its_statement() {
    let writer = writer().await;
    let view = view(true, true);
    let ctx = context(1, SITE, SITE, Default::default());
    let saved = settled(async |budget| {
        let mut write = writer.begin().await;
        if let Ok(write) = &mut write {
            write.fail_after(budget);
        }
        written(write, &ctx, &view, true).await
    });
    assert!(saved.await.is_ok());
}

/// `options` set as the Permalinks screen's save and the Reading save set
/// them.
async fn set(write: &mut Write<'_>, options: &[(&str, &str)]) {
    for (name, value) in options {
        let value = Value::String(value.as_bytes().to_vec());
        update_option(write, name, &value, None).await.unwrap();
    }
}

/// The Permalinks screen's view after `setup`, against the write oracle's
/// `golden`: the rules PHP stores.
async fn viewed_after(golden: &str, setup: impl AsyncFnOnce(&mut Write<'_>)) {
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        setup(&mut write).await;
        write.commit().await.unwrap();
        Ok::<_, String>(())
    };
    let ctx = context(1, SITE, SITE, Default::default());
    let permalinks = view(false, true);
    let action = async |conn: &mut Connection, ()| {
        let viewed = written(writer.begin_on(conn).await, &ctx, &permalinks, true);
        let (write, _) = viewed.await.map_err(|err| format!("{err:?}"))?;
        write.commit().await.map_err(|err| err.to_string())
    };
    let report = check_writes(&fixture_database(), golden, setup, action).await;
    report.unwrap();
}

/// A view under each structure the screen offers, the rules left as the
/// default install keeps them, and after a structure change.
#[tokio::test]
async fn the_permalinks_view_refreshes_the_rules_as_php() {
    for (golden, structure, category_base, tag_base) in [
        ("day", "/%year%/%monthnum%/%day%/%postname%/", "", ""),
        ("month", "/%year%/%monthnum%/%postname%/", "", ""),
        ("numeric", "/archives/%post_id%", "", ""),
        ("postname", "/%postname%/", "", ""),
        (
            "custom",
            "/%category%/%author%/%postname%/",
            "/topics",
            "/labels",
        ),
    ] {
        let options = [
            ("permalink_structure", structure),
            ("category_base", category_base),
            ("tag_base", tag_base),
        ];
        let golden = format!("admin-permalink-view-{golden}");
        viewed_after(&golden, async |write| set(write, &options).await).await;
    }
    // A static front page's rules, stored by the Reading save, then post names.
    viewed_after("admin-permalink-view-changed", async |write| {
        set(write, &[("permalink_structure", "/archives/%post_id%")]).await;
        set(write, &[("show_on_front", "page"), ("page_on_front", "2")]).await;
        flush_rewrite_rules(write).await.unwrap();
        set(write, &[("permalink_structure", "/%postname%/")]).await;
    })
    .await;
}

/// The block editor's first view writes before its page, which reads what
/// it wrote, and answers 501 while a preload is not ported; what the write
/// layer does not port answers 501 too.
#[tokio::test(flavor = "multi_thread")]
async fn the_block_editor_s_view_writes_before_its_page() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (site, _) = crate::admin::tests::logged_in(&db.name, dir.path()).await;
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let mut reads = crate::login::LoginReads::new(&site, handle, crate::now());
        reads.act_as(1, Vec::new());
        let user = reads.reads.wait(site.db.user(1)).unwrap();
        let mut admin = Admin::new(&reads, &site, user);
        let post = admin.full_post(1).unwrap();
        let gone = Post {
            id: 999,
            post_type: "post".into(),
            ..Post::default()
        };
        let cases = [
            (Target::New("post".into()), true, StatusCode::OK),
            (
                Target::Edit(Box::new(gone)),
                true,
                StatusCode::NOT_IMPLEMENTED,
            ),
            (Target::Edit(Box::new(post)), false, StatusCode::OK),
            (
                Target::New("nope".into()),
                true,
                StatusCode::NOT_IMPLEMENTED,
            ),
        ];
        for (target, answered, status) in cases {
            let view = View {
                editor: Some(target),
                ..view(false, false)
            };
            let browser = hyscribe_admin::admin_bar::Browser::default();
            let response = viewed(&mut admin, &writer, view, &|admin| {
                let request = hyscribe_admin::page::Request {
                    canonical: "http://127.0.0.1:8081/hs-admin/post-new",
                    origin: "http://127.0.0.1:8081",
                    uri: "/hs-admin/post-new",
                    browser: &browser,
                };
                let preloads = |_: &[(&'static str, String)]| answered.then_some(Value::Null);
                // Unanswered, the preloads are the REST API's.
                let answer = match answered {
                    true => super::super::block_editor_with(admin, &request, &preloads),
                    false => super::super::block_editor(admin, &request),
                };
                answer.map_or_else(super::super::not_ported, |answer| {
                    super::super::served(admin, answer)
                })
            });
            assert_eq!(response.status(), status);
        }
        assert_eq!(admin.full_post(4).unwrap().status, "auto-draft");
    })
    .await
    .unwrap();
}

/// In primary mode the editor's first view writes, then serves its page
/// with the preloads PHP's has: the paths `post-editor.tsv` lists.
#[tokio::test(flavor = "multi_thread")]
async fn the_block_editor_s_page_is_served() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = crate::admin::tests::logged_in(&db.name, dir.path()).await;
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary)
        .await
        .unwrap();
    let site = std::sync::Arc::new(
        std::sync::Arc::try_unwrap(site)
            .ok()
            .unwrap()
            .with_writer(writer),
    );
    let golden = include_str!("../../../../hyscribe-admin/tests/golden/post-editor.tsv");
    let php_paths = |case: &str| {
        let row = golden
            .lines()
            .find(|line| line.starts_with(&format!("{case}\tpreloads\t")))
            .unwrap();
        let tokens: Vec<&str> = row.rsplit('\t').next().unwrap().split(' ').collect();
        let pairs = tokens.chunks(2).map(|pair| pair.join(" "));
        // PHP's global styles post was 6, the scratch database's is 5.
        let pairs = pairs.map(|path| path.replace("/global-styles/6", "/global-styles/5"));
        pairs.collect::<Vec<String>>()
    };
    for (path, case) in [
        (
            "/hs-admin/post-new?post_type=page",
            "new-page-administrator",
        ),
        (
            "/hs-admin/post?post=1&action=edit",
            "edit-post-administrator",
        ),
    ] {
        let response = crate::admin::tests::send(&site, "GET", path, &cookies, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let page = crate::admin::tests::page_of(response).await;
        let marker = "wp.apiFetch.createPreloadingMiddleware( ";
        let json = page
            .split_once(marker)
            .unwrap()
            .1
            .split(" ) );")
            .next()
            .unwrap();
        let memo = hyscribe_phpser::json_decode(json.as_bytes()).unwrap();
        let mut ours = Vec::new();
        for (key, value) in memo.as_array().unwrap() {
            let key = String::from_utf8(key.to_php_string()).unwrap();
            match key.as_str() {
                "OPTIONS" => ours.extend(value.as_array().unwrap().iter().map(|(path, _)| {
                    format!("OPTIONS {}", String::from_utf8_lossy(&path.to_php_string()))
                })),
                _ => ours.push(format!("GET {key}")),
            }
        }
        // The new page's ID is the scratch database's.
        let php: Vec<String> = php_paths(case)
            .into_iter()
            .map(|p| p.replace("/pages/11", "/pages/4"))
            .collect();
        assert_eq!(ours, php, "{path}");
    }
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    let mut write = writer.begin().await.unwrap();
    assert_eq!(write.post(4).await.unwrap().unwrap().post_type, "page");
    let lock = write
        .meta_value(hyscribe_db::Meta::Post, 1, "_edit_lock")
        .await
        .unwrap();
    assert!(lock.is_some());
}

/// A database that fails before the editor's view begins ends it.
#[tokio::test]
async fn a_failed_begin_ends_the_editor_s_view() {
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    write.fail_after(0);
    let err = write.option_value("home").await.unwrap_err();
    let ctx = context(1, SITE, SITE, Default::default());
    let target = Target::New("post".into());
    let written = editor_written(Err(err), &ctx, &target, ("twentytwentyfive", "")).await;
    assert!(matches!(written, Err(Error::Db(_))));
}
