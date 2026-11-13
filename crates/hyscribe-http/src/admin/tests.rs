// SPDX-License-Identifier: AGPL-3.0-or-later

//! These tests read the default install fixture from a live MariaDB:
//! start it with `reference/default-install/db.sh up`.

use axum::body::Body;
use hyscribe_config::Secret;
use tower::ServiceExt;

use super::*;
use crate::router;
use crate::tests::oracle_site;

async fn get(site: &Arc<Site>, uri: &str, host: &str, cookie: &str) -> Response {
    let mut request = axum::http::Request::builder()
        .uri(uri)
        .header(header::HOST, host);
    if !cookie.is_empty() {
        request = request.header(header::COOKIE, cookie);
    }
    router(Arc::clone(site))
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn location(response: &Response) -> &str {
    response
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
}

fn salted(site: Site) -> Site {
    let names = [
        "AUTH_KEY",
        "AUTH_SALT",
        "LOGGED_IN_KEY",
        "LOGGED_IN_SALT",
        "NONCE_KEY",
        "NONCE_SALT",
    ];
    site.with_salts(
        names
            .iter()
            .map(|name| (*name, Secret::new(*name)))
            .collect(),
    )
}

/// A visitor is sent to log in and back, as PHP sends them (here under
/// HyScribe's URLs).
#[tokio::test(flavor = "multi_thread")]
async fn visitors_are_sent_to_log_in() {
    let site = oracle_site().await;
    let login = "http://127.0.0.1:8081/hs-login?redirect_to=";
    for (uri, host, back) in [
        (
            "/hs-admin/",
            "127.0.0.1:8081",
            "http%3A%2F%2F127.0.0.1%3A8081%2Fhs-admin%2F",
        ),
        (
            "/hs-admin/edit?post_type=page&x=%22a%20b",
            "127.0.0.1:8081",
            "http%3A%2F%2F127.0.0.1%3A8081%2Fhs-admin%2Fedit%3Fpost_type%3Dpage%26x%3D%2522a%2520b",
        ),
        (
            "/hs-admin/",
            "other.test:9",
            "http%3A%2F%2Fother.test%3A9%2Fhs-admin%2F",
        ),
        (
            "/hs-admin",
            "127.0.0.1:8081",
            "http%3A%2F%2F127.0.0.1%3A8081%2Fhs-admin",
        ),
    ] {
        let response = get(&site, uri, host, "").await;
        assert_eq!(response.status(), StatusCode::FOUND, "{uri}");
        assert_eq!(location(&response), format!("{login}{back}&reauth=1"));
        let headers = response.headers();
        assert_eq!(headers.get("x-redirect-by").unwrap(), "WordPress");
        assert_eq!(
            headers.get(header::EXPIRES).unwrap(),
            "Wed, 11 Jan 1984 05:00:00 GMT"
        );
    }
}

/// A logged in user sees the ported screens, and a 501 for the others; an
/// https site asks for its secure cookie and sends visitors back over HTTPS.
#[tokio::test(flavor = "multi_thread")]
async fn logged_in_users_are_let_in() {
    let dir = tempfile::tempdir().unwrap();
    let site = Arc::try_unwrap(oracle_site().await).unwrap();
    let site =
        Arc::new(salted(site).with_sessions(hyscribe_auth::sessions::Sessions::new(dir.path())));
    let login = axum::http::Request::builder()
        .method("POST")
        .uri("/hs-login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("log=admin&pwd=reference-only-password"))
        .unwrap();
    let response = router(Arc::clone(&site)).oneshot(login).await.unwrap();
    let all: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|c| c.to_str().unwrap().split(';').next().unwrap().to_string())
        .collect();
    let auth = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .nth(1)
        .unwrap();
    let auth = auth
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let response = get(&site, "/hs-admin/edit?detached=1", "127.0.0.1:8081", &auth).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let response = get(
        &site,
        "/hs-admin/edit-tags?taxonomy=nav_menu",
        "127.0.0.1:8081",
        &auth,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    // A page past the last goes to the last.
    let response = get(&site, "/hs-admin/edit?paged=9", "127.0.0.1:8081", &auth).await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(location(&response), "/hs-admin/edit?paged=1");
    let response = get(&site, "/hs-admin/users", "127.0.0.1:8081", &auth).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let cookies = all.join("; ");
    let page = page_of(get(&site, "/hs-admin/edit", "127.0.0.1:8081", &cookies).await).await;
    let php = golden("administrator\tedit.php\t");
    assert_eq!(with_values(&php, &page), page);
    // A page of categories past the last goes to the last.
    let response = get(
        &site,
        "/hs-admin/edit-tags?taxonomy=category&paged=4",
        "127.0.0.1:8081",
        &auth,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        location(&response),
        "/hs-admin/edit-tags?taxonomy=category&paged=1"
    );
    // The comments, and their arguments not ported yet.
    let page =
        page_of(get(&site, "/hs-admin/edit-comments", "127.0.0.1:8081", &cookies).await).await;
    let list = include_str!("../../../hyscribe-admin/tests/golden/comments-list.tsv");
    let row = list
        .lines()
        .find(|l| l.starts_with("default\tadministrator\t"))
        .unwrap();
    let php = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
    let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap());
    let (php, page) = (
        hyscribe_testkit::settled_page(&php),
        hyscribe_testkit::settled_page(&page),
    );
    assert_eq!(with_values(&php, &page), page);
    let response = get(
        &site,
        "/hs-admin/edit-comments?s=x",
        "127.0.0.1:8081",
        &auth,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    // The categories.
    let path = "/hs-admin/edit-tags?taxonomy=category";
    let page = page_of(get(&site, path, "127.0.0.1:8081", &cookies).await).await;
    let list = include_str!("../../../hyscribe-admin/tests/golden/terms-list.tsv");
    let row = list
        .lines()
        .find(|l| l.starts_with("default?taxonomy=category\tadministrator\t"))
        .unwrap();
    let php = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
    let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap());
    assert_eq!(with_values(&php, &page), page);
    // The pages, as a tree.
    let path = "/hs-admin/edit?post_type=page";
    let page = page_of(get(&site, path, "127.0.0.1:8081", &cookies).await).await;
    let list = include_str!("../../../hyscribe-admin/tests/golden/posts-list.tsv");
    let row = list
        .lines()
        .find(|l| l.starts_with("default?post_type=page\tadministrator\t"))
        .unwrap();
    let php = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
    let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap());
    assert_eq!(with_values(&php, &page), page);
    // The settings cookie the admin's scripts keep chooses the view.
    let extended = format!("{cookies}; wp-settings-1=posts_list_mode%3Dexcerpt");
    let page = page_of(get(&site, "/hs-admin/edit", "127.0.0.1:8081", &extended).await).await;
    assert!(page.contains("table-view-excerpt"));
    // The dashboard, as PHP showed it to a user who had no auto-draft yet.
    for path in ["/hs-admin/", "/hs-admin/index"] {
        let page = page_of(get(&site, path, "127.0.0.1:8081", &cookies).await).await;
        let php = golden("administrator\tindex.php\t").replace("%auto-draft%", "");
        let php = php.replace("/hs-admin/index\"", &format!("{path}\""));
        assert_eq!(with_values(&php, &page), page, "{path}");
    }
    let response = get(
        &site,
        "/hs-admin/tools?message=1&x=2",
        "127.0.0.1:8081",
        &auth,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let headers = response.headers().clone();
    assert_eq!(headers.get(header::X_FRAME_OPTIONS).unwrap(), "SAMEORIGIN");
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    let page =
        page_of(get(&site, "/hs-admin/tools", "127.0.0.1:8081", &all.join("; ")).await).await;
    assert_eq!(
        with_values(&golden("administrator\ttools.php\t"), &page),
        page
    );
    // The page's nonces are the user's: its log out link logs them out.
    let at = page.find("action=logout&#038;_wpnonce=").unwrap() + 28;
    let nonce = page.get(at..at + 10).unwrap();
    let logout = format!("/hs-login?action=logout&_wpnonce={nonce}");
    let out = get(&site, &logout, "127.0.0.1:8081", &all.join("; ")).await;
    assert_eq!(out.status(), StatusCode::FOUND);
    assert!(body.contains("href=\"http://127.0.0.1:8081/hs-admin/tools?x=2\""));
    assert!(body.contains("<h2 class=\"title\">Categories and Tags Converter</h2>"));
    assert!(body.contains("/hs-login?action=logout&#038;_wpnonce="));
    let db = hyscribe_db::Db::connect(&hyscribe_testkit::fixture_database())
        .await
        .unwrap();
    let url = "https://127.0.0.1:8081";
    let secure = Arc::new(
        Site::load(db, Some(url.into()), Some(url.into()))
            .await
            .unwrap(),
    );
    let response = get(&secure, "/hs-admin/", "127.0.0.1:8081", &auth).await;
    assert_eq!(
        location(&response),
        "https://127.0.0.1:8081/hs-login?redirect_to=https%3A%2F%2F127.0.0.1%3A8081%2Fhs-admin%2F&reauth=1"
    );
    // A user whose `use_ssl` is on goes over HTTPS.
    let teapot = || crate::login::response(StatusCode::IM_A_TEAPOT, Vec::new(), Vec::new());
    let response = admitted(false, true, b"example.test/hs-admin/", teapot);
    assert_eq!(location(&response), "https://example.test/hs-admin/");
    assert_eq!(
        admitted(true, true, b"x", teapot).status(),
        StatusCode::IM_A_TEAPOT
    );
    assert_eq!(
        admitted(false, false, b"x", teapot).status(),
        StatusCode::IM_A_TEAPOT
    );
}

/// A page of `reference/admin-page/golden.sh`, by its row's start, under
/// hs-admin's URLs.
fn golden(row: &str) -> String {
    let golden = include_str!("../../../hyscribe-admin/tests/golden/pages.tsv");
    let row = golden.lines().find(|l| l.starts_with(row)).unwrap();
    let page = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
    hyscribe_testkit::hs_admin_page(&String::from_utf8(page).unwrap())
}

async fn page_of(response: Response) -> String {
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

/// `golden` with the ten characters `page` has where it has a nonce or time
/// placeholder: equal to `page` when the rest is.
fn with_values(golden: &str, page: &str) -> String {
    let mut out = String::new();
    let (mut golden, mut page) = (golden, page);
    let next = |golden: &str| {
        [golden.find("%nonce:"), golden.find("%time%")]
            .into_iter()
            .flatten()
            .min()
    };
    while let Some(at) = next(golden) {
        out.push_str(golden.get(..at).unwrap());
        let rest = golden.get(at + 1..).unwrap();
        out.push_str(page.get(at..at + 10).unwrap_or_default());
        golden = rest.get(rest.find('%').unwrap() + 1..).unwrap();
        page = page.get(at + 10..).unwrap_or_default();
    }
    out.push_str(golden);
    out
}

/// A user without the screen's capability gets `wp_die()`'s refusal.
#[tokio::test(flavor = "multi_thread")]
async fn refusals() {
    let site = oracle_site().await;
    let handle = Handle::current();
    let edit = tokio::task::spawn_blocking({
        let (site, handle) = (Arc::clone(&site), handle.clone());
        move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            let url = "http://h/hs-admin/edit";
            screen(
                &admin,
                "/hs-admin/edit",
                "/hs-admin/edit",
                url,
                "http://h",
                &Browser::default(),
            )
        }
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(edit.status(), StatusCode::FORBIDDEN);
    let response = tokio::task::spawn_blocking({
        let (site, handle) = (Arc::clone(&site), handle.clone());
        move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            let browser = Browser::default();
            screen(
                &admin,
                "/hs-admin/tools",
                "/hs-admin/tools",
                "http://h/hs-admin/tools",
                "http://h",
                &browser,
            )
        }
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("Sorry, you are not allowed to access this page."));
    let comments = tokio::task::spawn_blocking({
        let (site, handle) = (Arc::clone(&site), handle.clone());
        move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            let path = "/hs-admin/edit-comments";
            screen(
                &admin,
                path,
                path,
                &format!("http://h{path}"),
                "http://h",
                &Browser::default(),
            )
        }
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(comments.status(), StatusCode::FORBIDDEN);
    let terms = tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, now());
        let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
        let path = "/hs-admin/edit-tags?taxonomy=category";
        screen(
            &admin,
            "/hs-admin/edit-tags",
            path,
            &format!("http://h{path}"),
            "http://h",
            &Browser::default(),
        )
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(terms.status(), StatusCode::FORBIDDEN);
    let body = axum::body::to_bytes(terms.into_body(), usize::MAX)
        .await
        .unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    assert!(body.contains("Sorry, you are not allowed to manage terms in this taxonomy."));
}

/// The terms list reads its taxonomy, page, search and order.
#[test]
fn the_terms_list_reads_its_taxonomy() {
    let taxonomy = |uri| term_args(uri).map(|args| args.taxonomy);
    let args =
        term_args("/hs-admin/edit-tags?taxonomy=category&paged=2&s=a+b&orderby=slug&order=desc")
            .unwrap();
    assert_eq!(
        (
            args.paged.as_deref(),
            args.search.as_deref(),
            args.orderby.as_deref(),
            args.order.as_deref()
        ),
        (Some("2"), Some("a b"), Some("slug"), Some("desc"))
    );
    assert_eq!(
        taxonomy("/hs-admin/edit-tags?taxonomy=post_tag"),
        Terms::named(b"post_tag")
    );
    for refused in [
        "/hs-admin/edit-tags",
        "/hs-admin/edit-tags?taxonomy=nav_menu",
        "/hs-admin/edit-tags?taxonomy=category&x=1",
        "/hs-admin/edit-tags?taxonomy=category&s[]=x",
        "/hs-admin/edit-tags?taxonomy[]=category",
        "/hs-admin/edit-tags?taxonomy=category&paged=%FF",
    ] {
        assert!(taxonomy(refused).is_none(), "{refused}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn database_errors_show_no_details() {
    let mut config = hyscribe_testkit::fixture_database();
    config.prefix = hyscribe_config::Prefix::new("missing_").unwrap();
    let mut site = Arc::try_unwrap(oracle_site().await).unwrap();
    site.db = hyscribe_db::Db::connect(&config).await.unwrap();
    let site = Arc::new(
        salted(site).with_sessions(hyscribe_auth::sessions::Sessions::new("/nonexistent")),
    );
    let cookie = format!(
        "hyscribe_{}=admin%7C1%7Cx%7Cy",
        hyscribe_phpser::md5(b"http://127.0.0.1:8081")
    );
    let response = get(&site, "/hs-admin/", "127.0.0.1:8081", &cookie).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[test]
fn screens_have_no_extension() {
    for (path, screen) in [
        ("/hs-admin", true),
        ("/hs-admin/", true),
        ("/hs-admin/edit", true),
        ("/hs-admin/css/missing.css", false),
        ("/hs-adminx", false),
        ("/hs-admin.d/x", false),
    ] {
        assert_eq!(is_admin(path), screen, "{path}");
    }
}

/// What the admin reads of a site and its user, over databases that hold it:
/// the terms fixture's stored editor preferences, and the Theme Test Data's
/// pending comments.
#[tokio::test(flavor = "multi_thread")]
async fn the_user_s_data() {
    let site = |name: &str| {
        let config = hyscribe_config::Database {
            name: name.into(),
            ..hyscribe_testkit::fixture_database()
        };
        async move {
            let db = hyscribe_db::Db::connect(&config).await.unwrap();
            Site::load(db, None, None).await.unwrap()
        }
    };
    let mut terms = site("wordpress_terms").await;
    terms.options.insert("_transient_x".into(), b"v".to_vec());
    terms
        .options
        .insert("can_compress_scripts".into(), b"1".to_vec());
    let ttd = site("wordpress_ttd").await;
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let admin = |site, reads, id| {
            let user = hyscribe_model::User {
                id,
                ..hyscribe_model::User::default()
            };
            Admin::new(reads, site, user)
        };
        let reads = LoginReads::new(&terms, handle.clone(), now());
        let first = admin(&terms, &reads, 1);
        assert_eq!(first.admin_color().as_deref(), Some(&b"modern"[..]));
        assert!(
            first
                .preferences()
                .unwrap()
                .starts_with(b"a:2:{s:4:\"core\"")
        );
        assert_eq!(first.transient("x").as_deref(), Some(&b"v"[..]));
        assert!(first.compression_tested());
        assert_eq!(admin(&terms, &reads, 7).user_id(), 7);
        let reads = LoginReads::new(&ttd, handle, now());
        assert_eq!(admin(&ttd, &reads, 1).moderated_comments(), 3);
    })
    .await
    .unwrap();
}

/// The dashboard's reads for an administrator and a subscriber, over the
/// terms fixture and the Theme Test Data.
#[tokio::test(flavor = "multi_thread")]
async fn dashboard_data() {
    let load = |name: &str| {
        let config = hyscribe_config::Database {
            name: name.into(),
            ..hyscribe_testkit::fixture_database()
        };
        async move {
            let db = hyscribe_db::Db::connect(&config).await.unwrap();
            Site::load(db, None, None).await.unwrap()
        }
    };
    let (terms, ttd) = (load("wordpress_terms").await, load("wordpress_ttd").await);
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let check = |site: &Site, (id, login): (u64, &[u8]), test: &dyn Fn(&Admin)| {
            let reads = LoginReads::new(site, handle.clone(), now());
            // As a session's check finds the user.
            LoginSite::user_by_login(&reads, login);
            let user = reads.reads.wait(site.db.user(id)).unwrap_or_default();
            test(&Admin::new(&reads, site, user));
        };
        check(&terms, (1, b"admin"), &|admin| {
            assert_eq!(admin.quick_draft(), Some(103));
            let comments = admin.recent_comments(admin.recent_posts("publish").last());
            let postless = comments.iter().find(|c| c.comment.id == 2).unwrap();
            assert!(postless.post.is_none() && postless.can_edit);
            assert_eq!(postless.user.as_ref().unwrap().id, 1);
            assert!(postless.link.ends_with("#comment-2"));
            assert_eq!(admin.theme_name(), "Twenty Twenty-Five");
            assert_eq!(admin.user_email(), b"admin@default-install.test");
            assert_eq!(admin.show_welcome_panel().as_deref(), Some(&b"1"[..]));
            assert_eq!(admin.published("page"), 1);
            assert_eq!(admin.moderated_comments(), 0);
            assert!(admin.comment_editable(None));
            // What capability checks of other objects read.
            assert_eq!(admin.comment_post(2), None);
            assert_eq!(admin.term_taxonomy(1).as_deref(), Some("category"));
            assert_eq!(admin.count_terms("category", None), 1);
            let (link, author) = admin.parent_comment(1).unwrap();
            assert_eq!(
                (link.ends_with("#comment-1"), &author[..]),
                (true, &b"A WordPress Commenter"[..])
            );
            assert!(admin.parent_comment(999).is_none());
            assert!(admin.has_comments("comment") && !admin.has_comments("pings"));
            assert_eq!(admin.count_terms("category", Some("zzz")), 0);
            assert!(admin.term_taxonomy(999_999).is_none());
            let term = admin.terms("category").remove(0);
            assert!(admin.can_term("edit_term", &term) && !admin.can_term("delete_term", &term));
            assert_eq!(
                admin.term_link(&term, "category"),
                "http://default-install.test/?cat=1"
            );
            assert!(admin.user_exists(1) && !admin.user_exists(999));
            assert_eq!(
                CapabilityData::user_meta(admin, 1, "wp_capabilities").as_deref(),
                Some(&b"a:1:{s:13:\"administrator\";b:1;}"[..])
            );
            assert_eq!(
                CapabilityData::option(admin, "blogname").as_deref(),
                Some(&b"HyScribe Default Install"[..])
            );
            assert!(admin.constant("DISALLOW_UNFILTERED_HTML"));
        });
        check(&ttd, (1, b"admin"), &|admin| {
            // The one post draft is by another user.
            assert!(admin.drafts().is_empty());
            assert_eq!(admin.recent_posts("future").len(), 1);
            let comments = admin.recent_comments(admin.recent_posts("publish").last());
            assert_eq!(comments.len(), 5);
            assert!(admin.can_post("edit_post", &comments[0].post.clone().unwrap()));
            let mut counts = admin.status_counts("post", None);
            counts.sort();
            let counts: Vec<(&str, u64)> = counts.iter().map(|(s, n)| (s.as_str(), *n)).collect();
            assert_eq!(counts, [("draft", 1), ("future", 1), ("publish", 57)]);
            assert_eq!(admin.status_counts("page", Some(1)).len(), 2);
            assert_eq!(admin.sticky_count("post", &[1241, 155]), 1);
            assert_eq!(admin.sticky_count("post", &[155]), 0);
            // The menus with items: five of six.
            assert_eq!(admin.popular_terms("nav_menu").len(), 5);
            assert_eq!(admin.months("post", false).first(), Some(&(2030, 1)));
            let page = admin.full_post(155).unwrap();
            assert_eq!(admin.pending_comments(&page), 1);
            assert_eq!(admin.display_name(2).as_deref(), Some(&b"Theme Buster"[..]));
            let authors: Vec<u64> = admin
                .authors("edit_pages")
                .iter()
                .map(|user| user.id)
                .collect();
            assert_eq!(authors, [1]);
            assert_eq!(PostsSite::post(admin, 155).map(|post| post.id), Some(155));
            let templates = admin.page_templates("page");
            assert_eq!(templates[0].0, "page-no-title");
            let popular = admin.popular_terms("category");
            assert_eq!((popular.len(), popular[0], popular[1]), (10, 16, 7));
            assert_eq!(admin.terms("category").len(), 68);
            let sticky = admin.full_post(1241).unwrap();
            assert!(
                admin
                    .post_class("iedit", &sticky)
                    .starts_with("iedit post-1241 type-post status-publish")
            );
            assert!(
                admin
                    .post_class("iedit", &sticky)
                    .contains(" status-sticky ")
            );
            let slugs: Vec<Vec<u8>> = admin
                .post_terms(&sticky, "category")
                .into_iter()
                .map(|t| t.slug)
                .collect();
            assert_eq!(slugs, [b"classic".to_vec(), b"uncategorized".to_vec()]);
            let about = admin.full_post(2).unwrap();
            assert_eq!(
                admin.meta(&about, "_wp_page_template").as_deref(),
                Some(&b"default"[..])
            );
            assert!(
                String::from_utf8_lossy(&admin.excerpt(&sticky))
                    .starts_with("This is a sticky post.")
            );
            assert_eq!(admin.post_formats().len(), 9);
            let drafts = PostQuery {
                post_types: vec!["post".into()],
                statuses: vec![hyscribe_model::StatusFilter {
                    status: "draft".into(),
                    author: None,
                }],
                limit: 5,
                ..PostQuery::default()
            };
            let (posts, found) = admin.list_posts(&drafts);
            assert_eq!((posts.len(), found), (1, 1));
            assert_eq!(admin.comment_counts().moderated, 3);
            assert_eq!(admin.published("post"), 57);
            assert_eq!(
                admin.post_meta(2, "_wp_page_template").as_deref(),
                Some(&b"default"[..])
            );
            assert!(admin.quick_draft().is_none());
        });
        check(&ttd, (2, b"themedemos"), &|author| {
            let drafts: Vec<u64> = author.drafts().iter().map(|post| post.id).collect();
            assert_eq!(drafts, [1164]);
            assert_eq!(author.own_posts("post"), 39);
        });
        check(&ttd, (3, b"themereviewteam"), &|subscriber| {
            assert!(subscriber.recent_posts("future").is_empty());
            assert_eq!(
                PostsSite::user_option(subscriber, "admin_color").as_deref(),
                Some(&b"modern"[..])
            );
            let comments = subscriber.recent_comments(None);
            assert_eq!(comments.len(), 5);
            assert!(
                comments
                    .iter()
                    .all(|c| c.status == "approved" && !c.can_edit)
            );
            assert!(comments.iter().all(|c| c.comment.post_id != 1168));
            assert!(!subscriber.can_post("edit_post", &comments[0].post.clone().unwrap()));
        });
    })
    .await
    .unwrap();
}

/// The posts list's arguments, and the filters the fixture's terms make.
#[tokio::test(flavor = "multi_thread")]
async fn the_posts_list_reads_its_arguments() {
    let site = oracle_site().await;
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, now());
        let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
        let args = |uri| list_args(&admin, uri).map(|args| (args.status, args.base));
        assert_eq!(args("/hs-admin/edit"), Some((String::new(), true)));
        assert_eq!(
            args("/hs-admin/edit?post_type=post"),
            Some((String::new(), true))
        );
        let pages = list_args(&admin, "/hs-admin/edit?post_type=page").unwrap();
        assert_eq!(
            (pages.post_type, pages.base),
            (PostType::named(b"page").unwrap(), true)
        );
        assert_eq!(
            args("/hs-admin/edit?post_status=draft&post_type=post&post_status=trash"),
            Some(("trash".into(), false))
        );
        for refused in [
            "/hs-admin/edit?post_status=%FF",
            "/hs-admin/edit?post_status[]=draft",
            "/hs-admin/edit?post_type=attachment",
            "/hs-admin/edit?post_status=draft&detached=1",
            "/hs-admin/edit?mode=%FF",
            "/hs-admin/edit?paged=%FF",
            "/hs-admin/edit?order=%FF",
            "/hs-admin/edit?author=%FF",
        ] {
            assert_eq!(args(refused), None, "{refused}");
        }
        let sorted = list_args(&admin, "/hs-admin/edit?orderby=title&order=desc&paged=2").unwrap();
        assert_eq!(
            (sorted.orderby, sorted.order, sorted.paged, sorted.base),
            (
                Some("title".into()),
                Some("desc".into()),
                Some("2".into()),
                false
            )
        );
        assert!(list_args(&admin, "/hs-admin/edit?paged=2").unwrap().base);
        // A view mode alone keeps the base request; with the post type, not.
        for (uri, base) in [
            ("/hs-admin/edit?mode=excerpt&paged=2", true),
            ("/hs-admin/edit?mode=", false),
            ("/hs-admin/edit?mode=list&post_type=post", false),
        ] {
            let args = list_args(&admin, uri).unwrap();
            assert_eq!(args.base, base, "{uri}");
        }
        let filtered = list_args(
            &admin,
            "/hs-admin/edit?author=2&all_posts=1&show_sticky=1&m=2026102708&post_format=aside",
        )
        .unwrap();
        assert_eq!(
            (
                filtered.author.as_deref(),
                filtered.all_posts.as_deref(),
                filtered.show_sticky
            ),
            (Some("2"), Some("1"), true)
        );
        assert_eq!(
            (filtered.m.as_str(), filtered.post_format.as_str()),
            ("2026102708", "aside")
        );
        assert_eq!(
            (
                filtered.filters.year,
                filtered.filters.month,
                filtered.filters.day
            ),
            (Some(2026), Some(10), Some(27))
        );
        assert_eq!(
            filtered.filters.time,
            [hyscribe_model::TimeCondition::Hour(8)]
        );
        assert_eq!(filtered.filters.terms.len(), 1);
        let searched = list_args(&admin, "/hs-admin/edit?s=hello+world&exact=1").unwrap();
        assert_eq!(searched.search.as_deref(), Some("hello world"));
        assert_eq!(searched.filters.search, ["hello", "world"]);
        assert_eq!(searched.filters.search_count, 2);
        assert_eq!(
            (searched.filters.exact, searched.filters.sentence.as_str()),
            (true, "hello world")
        );
        // `0` is `empty()`, which searches nothing; over 1600 bytes is none.
        let zero = list_args(&admin, "/hs-admin/edit?s=0").unwrap();
        assert!(zero.filters.search.is_empty());
        let long = list_args(&admin, &format!("/hs-admin/edit?s={}", "x".repeat(1601))).unwrap();
        assert!(long.filters.search.is_empty());
        // A category's slug chooses it in the filter, as `cat` does.
        let named = list_args(&admin, "/hs-admin/edit?category_name=uncategorized").unwrap();
        assert_eq!(named.cat, "1");
        let given = list_args(&admin, "/hs-admin/edit?cat=0&tag=nothing").unwrap();
        assert_eq!(given.cat, "0");
        // `WP_Query` keeps digits, commas and minus signs, then sorts.
        for (raw, cat) in [("00", "0"), ("abc", ""), ("3x,-1", "-1,3")] {
            let args = list_args(&admin, &format!("/hs-admin/edit?cat={raw}")).unwrap();
            assert_eq!(args.cat, cat, "{raw}");
        }
    })
    .await
    .unwrap();
}
