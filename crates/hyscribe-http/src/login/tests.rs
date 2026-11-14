// SPDX-License-Identifier: AGPL-3.0-or-later

//! These tests read the default install fixture from a live MariaDB:
//! start it with `reference/default-install/db.sh up`.

use axum::body::Body;
use hyscribe_config::Secret;
use hyscribe_testkit::unhex;
use tower::ServiceExt;

use super::*;
use crate::router;
use crate::tests::oracle_site;

const SALTS: [&str; 8] = [
    "AUTH_KEY",
    "SECURE_AUTH_KEY",
    "LOGGED_IN_KEY",
    "NONCE_KEY",
    "AUTH_SALT",
    "SECURE_AUTH_SALT",
    "LOGGED_IN_SALT",
    "NONCE_SALT",
];

/// The oracle's site with salts, in primary mode with sessions in `dir`,
/// or in shadow mode without.
async fn login_site(dir: Option<&std::path::Path>) -> Arc<Site> {
    let site = Arc::try_unwrap(oracle_site().await).unwrap();
    let site = site.with_salts(
        SALTS
            .iter()
            .map(|name| (*name, Secret::new(format!("{name} value"))))
            .collect(),
    );
    Arc::new(match dir {
        Some(dir) => site.with_sessions(hyscribe_auth::sessions::Sessions::new(dir)),
        None => site,
    })
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

async fn send(site: &Arc<Site>, method: &str, uri: &str, cookie: &str, form: &str) -> Reply {
    let mut request = axum::http::Request::builder().method(method).uri(uri);
    if !cookie.is_empty() {
        request = request.header(header::COOKIE, cookie);
    }
    if method == "POST" {
        request = request.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    }
    let request = request
        .header(header::USER_AGENT, "Agent/1.0")
        .body(Body::from(form.to_string()))
        .unwrap();
    let response = router(Arc::clone(site)).oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    Reply {
        status,
        headers,
        body: String::from_utf8(bytes.to_vec()).unwrap(),
    }
}

fn all(headers: &HeaderMap, name: header::HeaderName) -> Vec<String> {
    headers
        .get_all(name)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect()
}

/// A page of `reference/login/golden.sh` under HyScribe's URLs, with the
/// request's nonce for its placeholder.
fn golden_page(method: &str, body: &str, state: &str, nonce: &str) -> String {
    let golden = include_str!("../../../hyscribe-admin/tests/golden/login.tsv");
    let line = golden
        .lines()
        .find(|line| line.starts_with(&format!("{method}\t\t{body}\t{state}\t")))
        .unwrap();
    let page = String::from_utf8(unhex(line.rsplit('\t').next().unwrap())).unwrap();
    page.replace("/wp-login.php", "/hs-login")
        .replace("/wp-admin/load-styles.php", "/hs-admin/load-styles")
        .replace("/wp-admin/load-scripts.php", "/hs-admin/load-scripts")
        .replace("/wp-admin/admin-ajax.php", "/hs-admin/admin-ajax")
        .replace("/wp-admin/", "/hs-admin/")
        .replace("%nonce:reset-password-for-user%", nonce)
}

fn nonce(page: &str) -> String {
    let at = page.find("\"nonce\":\"").unwrap() + 9;
    page.get(at..at + 10).unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_screen_is_php_s_under_hyscribe_s_urls() {
    let dir = tempfile::tempdir().unwrap();
    let site = login_site(Some(dir.path())).await;
    let reply = send(&site, "GET", "/hs-login", "", "").await;
    assert_eq!(reply.status, StatusCode::OK);
    assert_eq!(reply.body, golden_page("GET", "", "0", &nonce(&reply.body)));
    let header = |name| reply.headers.get(name).unwrap().to_str().unwrap();
    assert_eq!(header(header::EXPIRES), "Wed, 11 Jan 1984 05:00:00 GMT");
    assert_eq!(
        header(header::CACHE_CONTROL),
        "no-cache, must-revalidate, max-age=0, no-store, private"
    );
    assert_eq!(header(header::X_FRAME_OPTIONS), "SAMEORIGIN");
    assert_eq!(header(header::CONTENT_TYPE), "text/html; charset=UTF-8");
    assert_eq!(
        all(&reply.headers, header::SET_COOKIE),
        ["hyscribe_test_cookie=WP%20Cookie%20check; path=/; HttpOnly; SameSite=Lax"]
    );
    // A wrong password, with the test cookie back.
    let failed = send(
        &site,
        "POST",
        "/hs-login",
        "hyscribe_test_cookie=WP%20Cookie%20check",
        "log=admin&pwd=wrong&testcookie=1",
    )
    .await;
    let expected = golden_page(
        "POST",
        "log=admin&pwd=wrong&testcookie=1",
        "1",
        &nonce(&failed.body),
    );
    assert_eq!(failed.body, expected);
    // The nonce is the visitor's for the request's tick.
    let salt = site.salt("nonce", |_| None).unwrap();
    let tick = nonce_tick(now());
    let fresh = |tick| create_nonce(&salt, tick, "reset-password-for-0", 0, b"");
    assert!([fresh(tick), fresh(tick - 1)].contains(&nonce(&reply.body)));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_login_keeps_a_session_and_sets_its_cookies() {
    let dir = tempfile::tempdir().unwrap();
    let site = login_site(Some(dir.path())).await;
    let form = "log=admin&pwd=reference-only-password&rememberme=forever&testcookie=1";
    let reply = send(&site, "POST", "/hs-login", "hyscribe_test_cookie=1", form).await;
    assert_eq!(reply.status, StatusCode::FOUND);
    let header = |name| {
        reply
            .headers
            .get(name)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(header(header::LOCATION), "http://127.0.0.1:8081/hs-admin/");
    assert_eq!(
        header(header::HeaderName::from_static("x-redirect-by")),
        "WordPress"
    );
    let cookies = all(&reply.headers, header::SET_COOKIE);
    let hash = md5(b"http://127.0.0.1:8081");
    assert_eq!(cookies.len(), 3);
    assert!(cookies[1].starts_with(&format!("hyscribe_{hash}=admin%7C")));
    assert!(cookies[1].ends_with("; path=/hs-admin; HttpOnly; SameSite=Lax"));
    assert!(cookies[2].starts_with(&format!("hyscribe_logged_in_{hash}=admin%7C")));
    assert!(
        cookies[2].contains("; expires=")
            && cookies[2].ends_with("; path=/; HttpOnly; SameSite=Lax")
    );
    // Remembered for 14 days, the cookies for half a day more.
    assert!(cookies[2].contains("; Max-Age=1252800;"), "{}", cookies[2]);
    assert!(dir.path().join("1").exists());
    // The auth cookie comes back: its session logs in again.
    let auth = cookies[1].split(';').next().unwrap();
    let again = send(&site, "GET", "/hs-login?redirect_to=%2Fx", auth, "").await;
    assert_eq!(again.status, StatusCode::FOUND);
    assert_eq!(again.headers.get(header::LOCATION).unwrap(), "/x");
    // The reset password cookie fills in its login.
    let reset = format!("hyscribe-resetpass-{hash}=admin%3Akey");
    let screen = send(&site, "GET", "/hs-login", &reset, "").await;
    assert!(screen.body.contains("class=\"input ltr\" value=\"admin\""));
    // A forged one does not.
    let forged = auth.replace("admin%7C", "admin%7C1");
    let expired = send(&site, "GET", "/hs-login", &forged, "").await;
    assert!(expired.body.contains("<p>Please log in again.</p>"));
    // An email address logs in too.
    let reply = send(
        &site,
        "POST",
        "/hs-login",
        "",
        "log=admin%40default-install.test&pwd=reference-only-password",
    )
    .await;
    assert_eq!(reply.status, StatusCode::FOUND);
    // Without "Remember Me", session cookies.
    let reply = send(
        &site,
        "POST",
        "/hs-login",
        "",
        "log=admin&pwd=reference-only-password&redirect_to=",
    )
    .await;
    assert_eq!((reply.status, reply.body.as_str()), (StatusCode::OK, ""));
    let cookies = all(&reply.headers, header::SET_COOKIE);
    assert!(!cookies[2].contains("expires="));
}

#[tokio::test(flavor = "multi_thread")]
async fn shadow_mode_shows_the_screen_but_logs_nobody_in() {
    let site = login_site(None).await;
    let form = "log=admin&pwd=reference-only-password";
    let reply = send(&site, "POST", "/hs-login", "", form).await;
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.body.contains("This site is read only"));
    // With no sessions, no auth or logged in cookie is valid.
    let hash = md5(b"http://127.0.0.1:8081");
    let logged_in = format!("hyscribe_logged_in_{hash}=admin%7C9999999999%7Cx%7Cy");
    let asked = send(&site, "GET", "/hs-login?action=logout", &logged_in, "").await;
    let visitor = send(&site, "GET", "/hs-login?action=logout", "", "").await;
    assert_eq!(logout_nonce(&asked.body), logout_nonce(&visitor.body));
    // Logging out needs no session store.
    let uri = format!(
        "/hs-login?action=logout&_wpnonce={}",
        logout_nonce(&visitor.body)
    );
    assert_eq!(
        send(&site, "GET", &uri, "", "").await.status,
        StatusCode::FOUND
    );
    let cookie = format!(
        "hyscribe_{}=admin%7C1%7Cx%7Cy",
        md5(b"http://127.0.0.1:8081")
    );
    let reply = send(&site, "GET", "/hs-login", &cookie, "").await;
    assert!(reply.body.contains("<p>Please log in again.</p>"));
}

#[tokio::test(flavor = "multi_thread")]
async fn other_actions_are_not_ported_yet() {
    let site = login_site(None).await;
    for uri in [
        "/hs-login?action=register",
        "/hs-login?action=lostpassword",
        "/hs-login?key=x",
    ] {
        assert_eq!(
            send(&site, "GET", uri, "", "").await.status,
            StatusCode::NOT_IMPLEMENTED,
            "{uri}"
        );
    }
    assert_eq!(
        send(
            &site,
            "GET",
            "/hs-login?action=entered_recovery_mode",
            "",
            ""
        )
        .await
        .status,
        StatusCode::OK
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn failures_are_server_errors() {
    // Sessions cannot be written where a file stands.
    let file = tempfile::NamedTempFile::new().unwrap();
    let site = login_site(Some(file.path())).await;
    let reply = send(
        &site,
        "POST",
        "/hs-login",
        "",
        "log=admin&pwd=reference-only-password",
    )
    .await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    // A database that fails.
    let mut config = hyscribe_testkit::fixture_database();
    config.prefix = hyscribe_config::Prefix::new("missing_").unwrap();
    let mut broken = Arc::try_unwrap(login_site(None).await).unwrap();
    broken.db = hyscribe_db::Db::connect(&config).await.unwrap();
    let reply = send(
        &Arc::new(broken),
        "POST",
        "/hs-login",
        "",
        "log=admin&pwd=x",
    )
    .await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!reply.body.contains("missing_"));
    // A panic while answering.
    let panicked = tokio::spawn(async { panic!("login bug") })
        .await
        .unwrap_err();
    assert_eq!(
        joined(Err(panicked)).status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

/// A user's capability check.
type Can = dyn Fn(&str) -> bool;

#[tokio::test(flavor = "multi_thread")]
async fn logged_in_users_go_where_they_may() {
    let site = login_site(None).await;
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, 1_900_000_000);
        let admin = "http://127.0.0.1:8081/hs-admin/";
        let everything = |_: &str| true;
        let reader = |cap: &str| cap == "read";
        let nothing = |_: &str| false;
        let https = "https://127.0.0.1:8081/hs-admin/";
        let cases: [(&Can, bool, &str, &str); 11] = [
            // Past the admin email's lifespan, an administrator confirms it.
            (&everything, false, admin, "http://127.0.0.1:8081/hs-login?redirect_to=http%3A%2F%2F127.0.0.1%3A8081%2Fhs-admin%2F&action=confirm_admin_email&wp_lang=en_US"),
            (&everything, false, "", "http://127.0.0.1:8081/hs-login?action=confirm_admin_email&wp_lang=en_US"),
            (&everything, true, "0", "https://127.0.0.1:8081/hs-login?action=confirm_admin_email&wp_lang=en_US"),
            (&reader, false, admin, "http://127.0.0.1:8081/hs-admin/profile"),
            (&reader, false, "wp-admin/", "http://127.0.0.1:8081/hs-admin/profile"),
            (&reader, true, https, "https://127.0.0.1:8081/hs-admin/profile"),
            (&nothing, false, "", "http://127.0.0.1:8081/"),
            (&nothing, false, "0", "http://127.0.0.1:8081/"),
            (&nothing, false, "http://example.test/x", admin),
            (&nothing, true, "http://example.test/x", https),
            (&nothing, false, "/hs-admin/edit?post_type=page", "/hs-admin/edit?post_type=page"),
        ];
        for (can, secure, to, expected) in cases {
            assert_eq!(location(&reads, can, (1, secure), to.as_bytes()), expected, "{to}");
        }
        // The capability data the login and the REST API read, and what they
        // never ask.
        assert!(reads.user_exists(1) && !reads.user_exists(999));
        assert_eq!(reads.post(1).map(|post| post.post_type).as_deref(), Some("post"));
        assert!(reads.post(999).is_none() && reads.post_meta(1, "x").is_none());
        assert_eq!(reads.comment_post(1), Some(1));
        assert!(reads.comment_post(999).is_none());
        assert_eq!(reads.term_taxonomy(1).as_deref(), Some("category"));
        assert!(reads.constant("DISALLOW_UNFILTERED_HTML") && !reads.constant("MULTISITE"));
        // `get_user_option()` reads the site's prefixed meta first.
        assert_eq!(reads.user_option(1, "use_ssl").as_deref(), Some(&b"0"[..]));
        let caps = reads.user_option(1, "capabilities");
        assert_eq!(caps.as_deref(), Some(&b"a:1:{s:13:\"administrator\";b:1;}"[..]));
        assert_eq!(reads.admin_url("edit.php?post_type=page"), "http://127.0.0.1:8081/hs-admin/edit?post_type=page");
        let filled = reads.fill("%rest_url% \"%admin_path% %user_id% %time% %site_url%");
        assert_eq!(filled, "http://127.0.0.1:8081/index.php?rest_route=/ \"/hs-admin/ 0 1900000000 http://127.0.0.1:8081");
        assert_eq!(reads.fill("%nonce:wp_rest%").len(), 10);
        // At the lifespan's last second, no confirmation yet.
        let lifespan = reads.reads.option("admin_email_lifespan").unwrap();
        let edge = LoginReads::new(&site, Handle::current(), Value::String(lifespan).to_int());
        assert_eq!(location(&edge, &everything, (1, false), b""), "");
    })
    .await
    .unwrap();
    // Under pretty permalinks the REST root is `wp-json`.
    let mut pretty = Arc::try_unwrap(login_site(None).await).unwrap();
    pretty
        .options
        .insert("permalink_structure".into(), b"/%postname%/".to_vec());
    pretty.options.insert("WPLANG".into(), b"de_DE".to_vec());
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&pretty, handle, 1_900_000_000);
        assert_eq!(reads.fill("%rest_url%"), "http://127.0.0.1:8081/wp-json/");
        // A user without a locale of their own confirms in the site's.
        let confirm = location(&reads, &|_: &str| true, (1, false), b"");
        assert!(confirm.ends_with("&wp_lang=de_DE"), "{confirm}");
    })
    .await
    .unwrap();
}

#[test]
fn cookies_are_set_as_php_sets_them_and_same_site() {
    let found = cookies([&b"a=1; b=%7C; a=2"[..], b"=x;c"]);
    assert_eq!(cookie(&found, "a"), Some(&b"1"[..]));
    assert_eq!(cookie(&found, "b"), Some(&b"|"[..]));
    assert_eq!(cookie(&found, "c"), Some(&b""[..]));
    assert_eq!(cookie(&found, "d"), None);
    assert_eq!(cookie(&found, ""), None);
    assert_eq!(
        set_cookie("n", b"v w", (5, 2), "/p", true),
        "n=v%20w; expires=Thu, 01 Jan 1970 00:00:05 GMT; Max-Age=3; path=/p; secure; HttpOnly; SameSite=Lax"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_site_in_a_subfolder_sets_cookies_on_both_paths() {
    let db = hyscribe_db::Db::connect(&hyscribe_testkit::fixture_database())
        .await
        .unwrap();
    let site = Site::load(
        db,
        Some("http://127.0.0.1:8081".into()),
        Some("http://127.0.0.1:8081/wp".into()),
    )
    .await
    .unwrap();
    let names = Names::new(&site);
    assert_eq!(names.site_paths(), ["/", "/wp/"]);
    let site = Arc::new(site);
    let reply = send(&site, "GET", "/hs-login", "", "").await;
    assert_eq!(all(&reply.headers, header::SET_COOKIE).len(), 2);
}

/// A primary site without salts cannot sign cookies: no auth cookie is
/// valid and a login is a server error.
#[tokio::test(flavor = "multi_thread")]
async fn without_salts_nobody_logs_in() {
    let dir = tempfile::tempdir().unwrap();
    let bare = Arc::try_unwrap(oracle_site().await).unwrap();
    let site = Arc::new(bare.with_sessions(hyscribe_auth::sessions::Sessions::new(dir.path())));
    let form = "log=admin&pwd=reference-only-password";
    let reply = send(&site, "POST", "/hs-login", "", form).await;
    assert_eq!(reply.status, StatusCode::INTERNAL_SERVER_ERROR);
    let hash = md5(b"http://127.0.0.1:8081");
    for user in ["admin", "nobody"] {
        let cookie = format!("hyscribe_{hash}={user}%7C9999999999%7Cx%7Cy");
        let reply = send(&site, "GET", "/hs-login", &cookie, "").await;
        assert!(reply.body.contains("<p>Please log in again.</p>"), "{user}");
    }
    // Nor is a logged in cookie, without the salt to check it by.
    let cookie = format!("hyscribe_logged_in_{hash}=admin%7C9999999999%7Cx%7Cy");
    let asked = send(&site, "GET", "/hs-login?action=logout", &cookie, "").await;
    assert_eq!(asked.status, StatusCode::FORBIDDEN);
    // A logged in cookie stands in for the test cookie.
    let cookie = format!("hyscribe_logged_in_{hash}=x");
    let reply = send(
        &site,
        "POST",
        "/hs-login",
        &cookie,
        "log=admin&pwd=wrong&testcookie=1",
    )
    .await;
    assert!(!reply.body.contains("Cookies are blocked"));
    assert!(reply.body.contains("is incorrect"));
}

/// A user with `use_ssl` gets the secure auth cookie.
#[tokio::test(flavor = "multi_thread")]
async fn use_ssl_users_get_the_secure_cookie() {
    let dir = tempfile::tempdir().unwrap();
    let site = login_site(Some(dir.path())).await;
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, now());
        reads.credentials(UserField::Login, b"admin").unwrap();
        let names = Names::new(&site);
        let (cookies, _) =
            logged_in(&reads, &names, &HeaderMap::new(), (1, false, true), b"/").unwrap();
        let hash = md5(b"http://127.0.0.1:8081");
        let expiration = reads.now + 2 * 86_400;
        assert!(cookies[0].starts_with(&format!("hyscribe_sec_{hash}=admin%7C{expiration}%7C")));
        assert!(cookies[0].contains("; secure;"));
        // The home URL is http: the logged in cookie is not secure.
        assert!(!cookies[1].contains("secure"));
    })
    .await
    .unwrap();
}

/// A site whose URL is https is behind a TLS proxy: its cookies are secure,
/// the auth cookie the secure one, and that is the one checked.
#[tokio::test(flavor = "multi_thread")]
async fn an_https_site_keeps_its_cookies_on_https() {
    let dir = tempfile::tempdir().unwrap();
    let db = hyscribe_db::Db::connect(&hyscribe_testkit::fixture_database())
        .await
        .unwrap();
    let url = "https://127.0.0.1:8081";
    let site = Site::load(db, Some(url.into()), Some(url.into()))
        .await
        .unwrap();
    let salts = SALTS
        .iter()
        .map(|name| (*name, Secret::new(*name)))
        .collect();
    let site = Arc::new(
        site.with_salts(salts)
            .with_sessions(hyscribe_auth::sessions::Sessions::new(dir.path())),
    );
    let reply = send(
        &site,
        "POST",
        "/hs-login",
        "",
        "log=admin&pwd=reference-only-password",
    )
    .await;
    assert_eq!(
        reply.headers.get(header::LOCATION).unwrap(),
        "https://127.0.0.1:8081/hs-admin/"
    );
    let cookies = all(&reply.headers, header::SET_COOKIE);
    let hash = md5(url.as_bytes());
    assert!(cookies[0].starts_with("hyscribe_test_cookie=") && cookies[0].contains("; secure;"));
    assert!(
        cookies[1].starts_with(&format!("hyscribe_sec_{hash}="))
            && cookies[1].contains("; secure;")
    );
    assert!(cookies[2].contains("; secure;"));
    let auth = cookies[1].split(';').next().unwrap();
    let again = send(&site, "GET", "/hs-login", auth, "").await;
    assert_eq!(again.status, StatusCode::FOUND);
    // An interim login, for the admin's session expiry dialog, is not ported.
    let form = "log=admin&pwd=reference-only-password&interim-login=1";
    assert_eq!(
        send(&site, "POST", "/hs-login", "", form).await.status,
        StatusCode::NOT_IMPLEMENTED
    );
}

/// The logout link's nonce on a page.
fn logout_nonce(page: &str) -> String {
    let at = page.find("_wpnonce=").unwrap() + 9;
    page.get(at..at + 10).unwrap().to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn logging_out_asks_without_a_nonce_and_ends_the_session_with_one() {
    let dir = tempfile::tempdir().unwrap();
    let site = login_site(Some(dir.path())).await;
    // Without a nonce, PHP's page asking first, under HyScribe's URLs.
    let asked = send(
        &site,
        "GET",
        "/hs-login?action=logout&redirect_to=%2Fx%22",
        "",
        "",
    )
    .await;
    assert_eq!(asked.status, StatusCode::FORBIDDEN);
    let golden = include_str!("../../../hyscribe-admin/tests/golden/login.tsv");
    let line = golden
        .lines()
        .find(|l| l.starts_with("GET\taction=logout&redirect_to=%2Fx%22\t"))
        .unwrap();
    let php = String::from_utf8(unhex(line.rsplit('\t').next().unwrap())).unwrap();
    let expected = php
        .replace("/wp-login.php", "/hs-login")
        .replace("%nonce:log-out%", &logout_nonce(&asked.body));
    assert_eq!(asked.body, expected);
    // A visitor with their own nonce logs out, which ends no session.
    let nonce = logout_nonce(&asked.body);
    let out = send(
        &site,
        "GET",
        &format!("/hs-login?action=logout&_wpnonce={nonce}"),
        "",
        "",
    )
    .await;
    assert_eq!(out.status, StatusCode::FOUND);
    assert_eq!(
        out.headers.get(header::LOCATION).unwrap(),
        "http://127.0.0.1:8081/hs-login?loggedout=true&wp_lang=en_US"
    );
    let hash = md5(b"http://127.0.0.1:8081");
    let cookies = all(&out.headers, header::SET_COOKIE);
    assert_eq!(cookies.len(), 4);
    assert!(cookies[1].starts_with(&format!("hyscribe_{hash}=%20; expires=")));
    assert!(cookies[1].ends_with("; Max-Age=0; path=/hs-admin; HttpOnly; SameSite=Lax"));
    // They expired a year ago.
    let date = cookies[1]
        .split("expires=")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let expired = httpdate::parse_http_date(date).unwrap();
    let ago = std::time::SystemTime::now()
        .duration_since(expired)
        .unwrap()
        .as_secs();
    assert!(
        (365 * 86_400 - 60..=365 * 86_400 + 60).contains(&ago),
        "{ago}"
    );
    assert!(cookies[2].starts_with(&format!("hyscribe_sec_{hash}=%20;")));
    assert!(
        cookies[3].starts_with(&format!("hyscribe_logged_in_{hash}=%20;"))
            && cookies[3].contains("; path=/;")
    );
    // A logged in user's link carries their own nonce, and ends their session.
    let form = "log=admin&pwd=reference-only-password";
    let login = send(&site, "POST", "/hs-login", "", form).await;
    let set = all(&login.headers, header::SET_COOKIE);
    let auth = set[1].split(';').next().unwrap().to_string();
    let logged_in = set[2].split(';').next().unwrap().to_string();
    let asked = send(&site, "GET", "/hs-login?action=logout", &logged_in, "").await;
    let theirs = logout_nonce(&asked.body);
    // A logged in cookie for nobody is a visitor's.
    let nobody = logged_in.replacen("admin%7C", "nobody%7C", 1);
    let visitor = send(&site, "GET", "/hs-login?action=logout", &nobody, "").await;
    assert_eq!(logout_nonce(&visitor.body), nonce);
    assert_ne!(theirs, nonce);
    let uri = format!("/hs-login?action=logout&_wpnonce={theirs}&redirect_to=%2Fy");
    let out = send(&site, "GET", &uri, &logged_in, "").await;
    assert_eq!(out.headers.get(header::LOCATION).unwrap(), "/y");
    let after = send(&site, "GET", "/hs-login", &auth, "").await;
    assert!(after.body.contains("<p>Please log in again.</p>"));
    // Its logged in cookie is a visitor's now.
    let asked = send(&site, "GET", "/hs-login?action=logout", &logged_in, "").await;
    assert_eq!(logout_nonce(&asked.body), nonce);
    // A wrong nonce asks again.
    let wrong = send(
        &site,
        "GET",
        "/hs-login?action=logout&_wpnonce=0123456789",
        "",
        "",
    )
    .await;
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    // A redirect off the site goes to the admin instead; the form counts.
    let form = format!("_wpnonce={nonce}&redirect_to=http%3A%2F%2Fexample.test%2F");
    let out = send(&site, "POST", "/hs-login?action=logout", "", &form).await;
    assert_eq!(
        out.headers.get(header::LOCATION).unwrap(),
        "http://127.0.0.1:8081/hs-admin/"
    );
}

/// The request's `user_id` goes to user-profile's data; a site that asks not
/// to be indexed says so to robots; logging out of a site whose login is on
/// another host than its home goes to the admin, as `wp_safe_redirect()`
/// keeps it.
#[tokio::test(flavor = "multi_thread")]
async fn the_request_and_the_site_shape_the_screens() {
    let site = login_site(None).await;
    let page = send(&site, "GET", "/hs-login?user_id=7x", "", "").await;
    assert!(page.body.contains("\"user_id\":\"7\""));
    let mut private = Arc::try_unwrap(login_site(None).await).unwrap();
    private.options.insert("blog_public".into(), b"0".to_vec());
    let private = Arc::new(private);
    let page = send(&private, "GET", "/hs-login", "", "").await;
    assert!(
        page.body
            .contains("<meta name='robots' content='noindex, nofollow, noarchive' />")
    );
    let asked = send(&private, "GET", "/hs-login?action=logout", "", "").await;
    assert!(
        asked
            .body
            .contains("<meta name='robots' content='noindex, nofollow' />")
    );
    let db = hyscribe_db::Db::connect(&hyscribe_testkit::fixture_database())
        .await
        .unwrap();
    let split = Site::load(
        db,
        Some("http://home.test".into()),
        Some("http://login.test".into()),
    )
    .await
    .unwrap();
    let salts = SALTS
        .iter()
        .map(|name| (*name, Secret::new(*name)))
        .collect();
    let split = Arc::new(split.with_salts(salts));
    let asked = send(&split, "GET", "/hs-login?action=logout", "", "").await;
    let uri = format!(
        "/hs-login?action=logout&_wpnonce={}",
        logout_nonce(&asked.body)
    );
    let out = send(&split, "GET", &uri, "", "").await;
    assert_eq!(
        out.headers.get(header::LOCATION).unwrap(),
        "http://login.test/hs-admin/"
    );
}
