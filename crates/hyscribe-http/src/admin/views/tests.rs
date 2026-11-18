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
        site_editor: false,
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
/// it wrote, and answers 501 while a preload is not ported or unanswered; what the write
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
            (Target::New("post".into()), Some(true), StatusCode::OK),
            (
                Target::Edit(Box::new(gone)),
                Some(true),
                StatusCode::NOT_IMPLEMENTED,
            ),
            (Target::Edit(Box::new(post)), None, StatusCode::OK),
            (
                Target::New("nope".into()),
                Some(true),
                StatusCode::NOT_IMPLEMENTED,
            ),
            (
                Target::New("post".into()),
                Some(false),
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
                // Without an answer given, the preloads are the REST API's.
                let answer = match answered {
                    Some(answered) => {
                        let preloads =
                            |_: &[(&'static str, String)]| answered.then_some(Value::Null);
                        super::super::block_editor_with(admin, &request, &preloads)
                    }
                    None => super::super::block_editor(admin, &request),
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

/// The editor's view writes only once the preloads it does not write are
/// answered: all but the global styles post's and the active theme's.
#[tokio::test(flavor = "multi_thread")]
async fn the_editor_s_view_waits_for_its_preloads() {
    let dir = tempfile::tempdir().unwrap();
    let (site, _) = crate::admin::tests::logged_in(&fixture_database().name, dir.path()).await;
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let mut reads = crate::login::LoginReads::new(&site, handle, crate::now());
        reads.act_as(1, Vec::new());
        let user = reads.reads.wait(site.db.user(1)).unwrap();
        let admin = Admin::new(&reads, &site, user);
        let target = Target::New("page".into());
        for answered in [true, false] {
            let preloads = |paths: &[(&'static str, String)]| {
                let paths: Vec<&str> = paths.iter().map(|(_, path)| path.as_str()).collect();
                assert!(paths.contains(&"/wp/v2/pages/0?context=edit"));
                assert!(!paths.iter().any(|path| path.contains("/wp/v2/themes?")));
                assert!(!paths.iter().any(|path| path.contains("/global-styles/0")));
                answered.then_some(Value::Null)
            };
            assert_eq!(answered_before(&admin, &target, &preloads), answered);
        }
    })
    .await
    .unwrap();
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
    let mut write = writer.begin().await.unwrap();
    write.fail_after(0);
    let err = write.option_value("home").await.unwrap_err();
    let written = site_editor_written(Err(err), &ctx, ("twentytwentyfive", "")).await;
    assert!(matches!(written, Err(Error::Db(_))));
}

/// The paths of the page's preload memo, `OPTIONS` ones prefixed.
fn preloaded_paths(page: &str) -> Vec<String> {
    let marker = "wp.apiFetch.createPreloadingMiddleware( ";
    let json = page.split_once(marker).unwrap().1;
    let json = json.split(" ) );").next().unwrap();
    let memo = hyscribe_phpser::json_decode(json.as_bytes()).unwrap();
    let mut paths = Vec::new();
    for (key, value) in memo.as_array().unwrap() {
        let key = String::from_utf8(key.to_php_string()).unwrap();
        match key.as_str() {
            "OPTIONS" => paths.extend(value.as_array().unwrap().iter().map(|(path, _)| {
                format!("OPTIONS {}", String::from_utf8_lossy(&path.to_php_string()))
            })),
            _ => paths.push(format!("GET {key}")),
        }
    }
    paths
}

/// In primary mode the site editor's first view writes the custom CSS
/// lookup and the theme's global styles post, then serves its page with
/// the preloads PHP's has; later views write nothing more. What it does
/// not port answers 501 and writes nothing, as does a view whose writes
/// the write layer does not port.
#[tokio::test(flavor = "multi_thread")]
async fn the_site_editor_s_page_is_served() {
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
    let send = async |path: &str| {
        let response = crate::admin::tests::send(&site, "GET", path, &cookies, None).await;
        let status = response.status();
        (status, crate::admin::tests::page_of(response).await)
    };
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    // Arguments that are not strings are not ported.
    let (status, _) = send("/hs-admin/site-editor?p[]=/").await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    let mut write = writer.begin().await.unwrap();
    assert!(write.post(4).await.unwrap().is_none());
    drop(write);
    let golden = include_str!("../../../../hyscribe-admin/tests/golden/site-editor.tsv");
    for (path, case) in [
        ("/hs-admin/site-editor", "se-first"),
        ("/hs-admin/site-editor", "se-again"),
        ("/hs-admin/site-editor?p=/page/2&canvas=edit", "se-page-2"),
        (
            "/hs-admin/site-editor?p=/&postId=1&postType=post",
            "se-post",
        ),
    ] {
        let (status, page) = send(path).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        let row = golden
            .lines()
            .find(|line| line.starts_with(&format!("{case}\tpreloads\t")))
            .unwrap();
        let tokens: Vec<&str> = row.rsplit('\t').next().unwrap().split(' ').collect();
        let php: Vec<String> = tokens.chunks(2).map(|pair| pair.join(" ")).collect();
        assert_eq!(preloaded_paths(&page), php, "{path}");
    }
    // Nor is a post neither a post nor a page: the global styles post.
    let (status, _) = send("/hs-admin/site-editor?p=/&postId=4").await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
    let mut write = writer.begin().await.unwrap();
    let styles = write.post(4).await.unwrap().unwrap();
    assert_eq!(styles.post_type, "wp_global_styles");
    assert!(write.post(5).await.unwrap().is_none());
    let mods = write.option_value("theme_mods_twentytwentyfive").await;
    let lookup: &[u8] = b"a:1:{s:18:\"custom_css_post_id\";i:-1;}";
    assert_eq!(mods.unwrap().as_deref(), Some(lookup));
    let unported = Value::String(b"x".to_vec());
    update_option(&mut write, "theme_mods_twentytwentyfive", &unported, None)
        .await
        .unwrap();
    write.commit().await.unwrap();
    let (status, _) = send("/hs-admin/site-editor").await;
    assert_eq!(status, StatusCode::NOT_IMPLEMENTED);
}

/// The site editor's view writes only once the preloads it does not write
/// are answered, for a user who may edit theme options, past its redirects.
#[tokio::test(flavor = "multi_thread")]
async fn the_site_editor_s_view_waits_for_its_preloads() {
    let dir = tempfile::tempdir().unwrap();
    let (site, _) = crate::admin::tests::logged_in(&fixture_database().name, dir.path()).await;
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let mut reads = crate::login::LoginReads::new(&site, handle, crate::now());
        reads.act_as(1, Vec::new());
        let user = reads.reads.wait(site.db.user(1)).unwrap();
        let admin = Admin::new(&reads, &site, user);
        for answered in [true, false] {
            let preloads = |paths: &[(&'static str, String)]| {
                assert!(
                    !paths
                        .iter()
                        .any(|(_, path)| path.contains("/wp/v2/themes?"))
                );
                assert!(
                    !paths
                        .iter()
                        .any(|(_, path)| path.contains("/global-styles/0"))
                );
                answered.then_some(Value::Null)
            };
            assert_eq!(site_editor_before(&admin, "", &preloads), answered);
            assert!(!site_editor_before(&admin, "postType=page", &preloads));
            assert!(!site_editor_before(&admin, "p[]=/", &preloads));
        }
        let nobody = Admin::new(&reads, &site, hyscribe_model::User::default());
        assert!(!site_editor_before(&nobody, "", &|_| Some(Value::Null)));
        // The page, once the view found the global styles post: none for
        // arguments not ported or a preload not answered.
        let mut admin = admin;
        admin.styles = Some(4);
        let browser = hyscribe_admin::admin_bar::Browser::default();
        let request = hyscribe_admin::page::Request {
            canonical: "http://127.0.0.1:8081/hs-admin/site-editor",
            origin: "http://127.0.0.1:8081",
            uri: "/hs-admin/site-editor",
            browser: &browser,
        };
        let with = |query: &str, answered: bool| {
            let preloads = |_: &[(&'static str, String)]| answered.then_some(Value::Null);
            super::super::site_editor_with(&admin, query, &request, &preloads)
        };
        assert!(with("p[]=/", true).is_none());
        assert!(with("", false).is_none());
        assert!(matches!(
            with("", true),
            Some(hyscribe_admin::posts::Answer::Page(_))
        ));
    })
    .await
    .unwrap();
}

/// An admin request's scheduling as the write oracle's `cron-schedule-events-fixture`
/// has it on the default install, whose first admin request this is, and a
/// `cron` option older than version 2 left as it is (PHP upgrades it).
#[tokio::test]
async fn an_admin_request_schedules_the_cron_events_as_php_does() {
    let writer = writer().await;
    let action = async |conn: &mut Connection, ()| {
        settled(async |budget| {
            let mut write = writer.begin_on(conn).await;
            if let Ok(write) = &mut write {
                write.fail_after(budget);
            }
            schedule(write, crate::now()).await
        })
        .await
        .map_err(|err| format!("{err:?}"))
    };
    let setup = async |_: &mut Connection| Ok::<_, String>(());
    let golden = "cron-schedule-events-fixture";
    check_writes(&fixture_database(), golden, setup, action)
        .await
        .unwrap();
    let mut write = writer.begin().await.unwrap();
    let old = Value::Array(Vec::new());
    update_option(&mut write, "cron", &old, None).await.unwrap();
    assert!(schedule(Ok(write), 0).await.is_ok());
}

/// A logged in request to any admin screen, the page served or not,
/// schedules the events before it; one without a login does not.
#[tokio::test(flavor = "multi_thread")]
async fn an_admin_screen_schedules_the_cron_events_first() {
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
    let events = async || {
        let cron = site.db.option("cron").await.unwrap().unwrap();
        let cron = String::from_utf8(cron).unwrap();
        ["wp_scheduled_delete", "delete_expired_transients"].map(|hook| cron.contains(hook))
    };
    assert_eq!(events().await, [false, false]);
    let send = crate::admin::tests::send;
    let anonymous = send(&site, "GET", "/hs-admin/profile", "", None).await;
    assert_eq!(anonymous.status(), StatusCode::FOUND);
    assert_eq!(events().await, [false, false]);
    let nowhere = send(&site, "GET", "/hs-admin/nowhere", &cookies, None).await;
    assert_eq!(nowhere.status(), StatusCode::NOT_IMPLEMENTED);
    assert_eq!(events().await, [true, true]);
}
