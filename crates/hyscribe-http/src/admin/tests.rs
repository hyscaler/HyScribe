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
    let response = get(&site, "/hs-admin/", "127.0.0.1:8081", &auth).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
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
    let page = tools(&site, &all.join("; ")).await;
    assert_eq!(placeholders(&page), administrator_tools());
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

/// The administrator's Tools screen as `reference/admin-page/golden.sh`
/// recorded it, under hs-admin's URLs.
fn administrator_tools() -> String {
    let golden = include_str!("../../../hyscribe-admin/tests/golden/pages.tsv");
    let row = golden
        .lines()
        .find(|l| l.starts_with("administrator\ttools.php\t"))
        .unwrap();
    let page = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
    hyscribe_testkit::hs_admin_page(&String::from_utf8(page).unwrap())
}

/// The Tools screen at its plain URL.
async fn tools(site: &Arc<Site>, auth: &str) -> String {
    let response = get(site, "/hs-admin/tools", "127.0.0.1:8081", auth).await;
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(body.to_vec()).unwrap()
}

/// A page with its nonces and time as the golden's placeholders.
fn placeholders(page: &str) -> String {
    let mut page = page.to_string();
    for (marker, action) in [
        ("_wpnonce=", "log-out"),
        ("heartbeatSettings = {\"nonce\":\"", "heartbeat-nonce"),
        ("createNonceMiddleware( \"", "wp_rest"),
        ("compressionNonce = \"", "update_can_compress_scripts"),
    ] {
        let at = page.find(marker).unwrap() + marker.len();
        page.replace_range(at..at + 10, &format!("%nonce:{action}%"));
    }
    let at = page.find("\"time\":\"").unwrap() + 8;
    let end = at + page.get(at..).unwrap().find('"').unwrap();
    page.replace_range(at..end, "%time%");
    page
}

/// A user without the screen's capability gets `wp_die()`'s refusal.
#[tokio::test(flavor = "multi_thread")]
async fn refusals() {
    let site = oracle_site().await;
    let handle = Handle::current();
    let response = tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, now());
        let caps = Capabilities::load(&reads, site.db.prefix(), 0);
        let admin = Admin {
            reads: &reads,
            site: &site,
            caps,
            user: hyscribe_model::User::default(),
        };
        let browser = Browser::default();
        screen(
            &admin,
            "/hs-admin/tools",
            "http://h/hs-admin/tools",
            "http://h",
            &browser,
        )
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
        let admin = |site, reads, id| Admin {
            reads,
            site,
            caps: Capabilities::load(reads, "wp_", id),
            user: hyscribe_model::User {
                id,
                ..hyscribe_model::User::default()
            },
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
