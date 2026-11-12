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

/// A logged in user is let in (to no screen yet); an https site asks for
/// its secure cookie and sends visitors back over HTTPS.
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
    let response = admitted(false, true, b"example.test/hs-admin/");
    assert_eq!(location(&response), "https://example.test/hs-admin/");
    assert_eq!(
        admitted(true, true, b"x").status(),
        StatusCode::NOT_IMPLEMENTED
    );
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
