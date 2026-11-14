// SPDX-License-Identifier: AGPL-3.0-or-later

//! These tests read the default install fixture from a live MariaDB:
//! start it with `reference/default-install/db.sh up`.

use axum::body::Body;
use axum::http::StatusCode;
use hyscribe_auth::cookie::parse_auth_cookie;
use hyscribe_auth::sessions::Sessions;
use hyscribe_config::Secret;
use hyscribe_phpser::urldecode;
use hyscribe_testkit::unhex;
use tower::ServiceExt;

use super::*;
use crate::router;
use crate::tests::oracle_site;

/// The oracle's site, with its keys and salts and sessions in `dir`, under
/// `/%postname%/` permalinks when `pretty`.
async fn site(pretty: bool, dir: &std::path::Path) -> Arc<Site> {
    on(oracle_site().await, pretty, dir)
}

/// The oracle's site over the Theme Test Data fixture's database.
async fn ttd_site(pretty: bool, dir: &std::path::Path) -> Arc<Site> {
    let mut config = crate::tests::fixture_config();
    config.name = "wordpress_ttd".into();
    let db = crate::Db::connect(&config)
        .await
        .expect(hyscribe_testkit::NO_FIXTURE_DATABASE);
    let home = Some("http://127.0.0.1:8081".to_string());
    on(
        Arc::new(Site::load(db, home.clone(), home).await.unwrap()),
        pretty,
        dir,
    )
}

/// `site` with keys and salts, sessions in `dir`, pretty permalinks when
/// `pretty`.
fn on(site: Arc<Site>, pretty: bool, dir: &std::path::Path) -> Arc<Site> {
    let mut site = Arc::into_inner(site).unwrap();
    if pretty {
        site.options
            .insert("permalink_structure".into(), b"/%postname%/".to_vec());
        site.build_rewriter();
    }
    let salts = [
        "AUTH_KEY",
        "AUTH_SALT",
        "LOGGED_IN_KEY",
        "LOGGED_IN_SALT",
        "NONCE_KEY",
        "NONCE_SALT",
    ];
    let salts = salts
        .iter()
        .map(|name| (*name, Secret::new(*name)))
        .collect();
    Arc::new(site.with_salts(salts).with_sessions(Sessions::new(dir)))
}

async fn send(site: &Arc<Site>, request: axum::http::request::Builder, body: &str) -> Response {
    let request = request.body(Body::from(body.to_string())).unwrap();
    router(Arc::clone(site)).oneshot(request).await.unwrap()
}

async fn body(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8(bytes.to_vec()).unwrap()
}

/// The REST nonce of `user` with the session `token`.
async fn nonce(site: &Arc<Site>, user: u64, token: Vec<u8>) -> String {
    let (site, handle) = (Arc::clone(site), Handle::current());
    tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&site, handle, now());
        reads.act_as(user, token);
        reads.nonce("wp_rest")
    })
    .await
    .unwrap()
}

/// The administrator's logged in cookie, from logging in at `/hs-login`,
/// and their REST nonce.
async fn log_in(site: &Arc<Site>) -> (String, String) {
    let login = axum::http::Request::builder()
        .method("POST")
        .uri("/hs-login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    let response = send(site, login, "log=admin&pwd=reference-only-password").await;
    let cookie = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|cookie| {
            cookie
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_string()
        })
        .find(|cookie| cookie.starts_with("hyscribe_logged_in_"))
        .unwrap();
    let value = urldecode(cookie.split_once('=').unwrap().1.as_bytes());
    let token = parse_auth_cookie(&value).unwrap().token.to_vec();
    (cookie, nonce(site, 1, token).await)
}

/// The headers the golden keeps, sorted.
fn kept(headers: &HeaderMap, nonce: &str) -> Vec<String> {
    let kept = |name: &str| {
        [
            "content-type",
            "x-robots-tag",
            "link",
            "allow",
            "vary",
            "expires",
            "cache-control",
        ]
        .contains(&name)
            || name.starts_with("access-control-")
            || name.starts_with("x-wp-")
    };
    let mut headers: Vec<String> = headers
        .iter()
        .filter(|(name, _)| kept(name.as_str()))
        .map(|(name, value)| {
            format!("{name}: {}", value.to_str().unwrap()).replace(nonce, "%nonce%")
        })
        .collect();
    headers.sort();
    headers
}

/// The theme footer's credit, which names HyScribe, as JSON.
const CREDIT: [&str; 2] = [
    "Designed with <a href=\\\"https:\\/\\/wordpress.org\\\" rel=\\\"nofollow\\\">WordPress<\\/a>",
    "Designed with <a href=\\\"https:\\/\\/github.com\\/hyscaler\\/HyScribe\\\" rel=\\\"nofollow\\\">HyScribe<\\/a>",
];

/// The PHP oracle's answers to a visitor and the administrator, served
/// over HTTP; routes not ported answer 501.
#[tokio::test(flavor = "multi_thread")]
async fn answers_match_php() {
    let dir = tempfile::tempdir().unwrap();
    let sites = (site(false, dir.path()).await, site(true, dir.path()).await);
    let golden = include_str!("../../../hyscribe-rest/tests/golden/rest.tsv");
    assert_eq!(replay(&sites, golden, false).await, (2 * 64, 2 * 9));
    let golden = include_str!("../../../hyscribe-rest/tests/golden/content.tsv");
    assert_eq!(replay(&sites, golden, false).await, (2 * 89, 2 * 3));
    let golden = include_str!("../../../hyscribe-rest/tests/golden/templates.tsv");
    assert_eq!(replay(&sites, golden, false).await, (2 * 85, 0));
}

/// The content routes over the Theme Test Data fixture, by hash.
#[tokio::test(flavor = "multi_thread")]
async fn content_matches_php() {
    let dir = tempfile::tempdir().unwrap();
    let sites = (
        ttd_site(false, dir.path()).await,
        ttd_site(true, dir.path()).await,
    );
    let golden = include_str!("../../../hyscribe-rest/tests/golden/content-ttd.tsv");
    assert_eq!(replay(&sites, golden, true).await, (2 * 92, 2 * 4));
}

/// `golden`'s requests of a visitor and the administrator served on the
/// plain and pretty `sites`, bodies compared by SHA-256 when `hashed`: how
/// many matched and how many answered 501.
async fn replay(sites: &(Arc<Site>, Arc<Site>), golden: &str, hashed: bool) -> (usize, usize) {
    use sha2::Digest;
    let (plain, pretty) = sites;
    let (cookie, nonce) = log_in(plain).await;
    let visitor = self::nonce(plain, 0, Vec::new()).await;
    let (mut served, mut unported) = (0, 0);
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let text = |i: usize| String::from_utf8(unhex(fields[i])).unwrap();
        let (who, method, target, flags) = (fields[0], fields[1], text(2), fields[3]);
        if who != "visitor" && who != "administrator" {
            continue;
        }
        let flag = |name: &str| flags.split(',').any(|flag| flag == name);
        let site = if flag("pretty") { pretty } else { plain };
        let nonce = if who == "visitor" { &visitor } else { &nonce };
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri(target.replace("%nonce%", nonce).replace("[]", "%5B%5D"))
            .header(header::HOST, "127.0.0.1:8081");
        if who == "administrator" {
            request = request.header(header::COOKIE, &cookie);
            if !flag("nononce") {
                request = request.header("x-wp-nonce", nonce);
            }
        }
        for (name, value, set) in [
            ("origin", "http://example.test", flag("origin")),
            ("origin", "null", flag("originnull")),
            ("x-http-method-override", "DELETE", flag("override")),
        ] {
            if set {
                request = request.header(name, value);
            }
        }
        let response = send(site, request, "").await;
        let what = format!("{who} {method} {target} {flags}");
        if response.status() == StatusCode::NOT_IMPLEMENTED {
            assert!(
                !String::from_utf8(unhex(fields[6]))
                    .unwrap()
                    .contains("rest_no_route"),
                "{what}"
            );
            unported += 1;
            continue;
        }
        assert_eq!(response.status().as_u16().to_string(), fields[4], "{what}");
        let mut php: Vec<String> = text(5).lines().map(String::from).collect();
        php.sort();
        assert_eq!(kept(response.headers(), nonce), php, "{what}");
        let mut ours = body(response).await.replace(nonce, "%nonce%");
        if hashed {
            let digest = sha2::Sha256::digest(ours.as_bytes());
            ours = digest.iter().map(|b| format!("{b:02x}")).collect();
        }
        assert_eq!(ours, text(6).replace(CREDIT[0], CREDIT[1]), "{what}");
        served += 1;
    }
    (served, unported)
}

/// What WordPress serves as a page stays a page, and a `rest_route` that is
/// not a string is not ported.
#[tokio::test(flavor = "multi_thread")]
async fn only_rest_routes_reach_the_api() {
    let dir = tempfile::tempdir().unwrap();
    let plain = site(false, dir.path()).await;
    let get = |uri: &str| axum::http::Request::builder().uri(uri.to_string());
    let response = send(&plain, get("/?rest_route%5B%5D=/x"), "").await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    // Plain permalinks have no `/wp-json/` rule.
    for page in ["/?rest_route=", "/?rest_route=0", "/wp-json/"] {
        let response = send(&plain, get(page), "").await;
        assert_eq!(response.status(), StatusCode::OK, "{page}");
        assert!(
            body(response).await.starts_with("<!DOCTYPE html>"),
            "{page}"
        );
    }
    let put = axum::http::Request::builder().method("PUT").uri("/");
    assert_eq!(
        send(&plain, put, "").await.status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    // A form's nonce is checked before the query's.
    let visitor = nonce(&plain, 0, Vec::new()).await;
    let post = |uri: String| {
        axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
    };
    let uri = format!("/?rest_route=/wp/v2/types/post&_wpnonce={visitor}");
    let response = send(&plain, post(uri.clone()), "").await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let response = send(&plain, post(uri), "_wpnonce=abc").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // A cookie that does not validate is a visitor's.
    let name = Names::new(&plain).logged_in();
    let stranger = get("/?rest_route=/wp/v2/types&context=edit")
        .header(header::COOKIE, format!("{name}=admin%7C1%7Ct%7Ch"))
        .header("x-wp-nonce", "x");
    assert_eq!(
        send(&plain, stranger, "").await.status(),
        StatusCode::FORBIDDEN
    );
}

/// A database failure reading the cookie's user shows the error page.
#[tokio::test(flavor = "multi_thread")]
async fn database_errors_show_the_error_page() {
    let dir = tempfile::tempdir().unwrap();
    let plain = site(false, dir.path()).await;
    let (cookie, _) = log_in(&plain).await;
    let mut broken = Arc::into_inner(plain).unwrap();
    let mut config = crate::tests::fixture_config();
    config.prefix = hyscribe_config::Prefix::new("missing_").unwrap();
    broken.db = hyscribe_db::Db::connect(&config).await.unwrap();
    let request = axum::http::Request::builder()
        .uri("/?rest_route=/wp/v2/types")
        .header(header::COOKIE, cookie);
    let response = send(&Arc::new(broken), request, "").await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

/// The reads a site's own templates and global styles posts need, which
/// the default install has none of, over `reference/default-install/terms.sql`.
#[tokio::test(flavor = "multi_thread")]
async fn posts_are_read_for_the_routes() {
    let mut config = crate::tests::fixture_config();
    config.name = "wordpress_terms".into();
    let db = crate::Db::connect(&config)
        .await
        .expect(hyscribe_testkit::NO_FIXTURE_DATABASE);
    let home = Some("http://127.0.0.1:8081".to_string());
    let plain = Site::load(db, home.clone(), home).await.unwrap();
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&plain, handle, now());
        reads.act_as(1, Vec::new());
        let rest = Rest {
            reads,
            site: &plain,
            user: 1,
            caps: OnceCell::new(),
        };
        let site: &dyn hyscribe_rest::Site = &rest;
        assert_eq!(site.revisions(1), [105, 104]);
        let blocks = site.posts_of_type("wp_block");
        assert_eq!(blocks.iter().map(|post| post.id).collect::<Vec<_>>(), [102]);
        assert_eq!(site.post_terms(1, "category")[0].name, b"Uncategorized");
        assert_eq!(site.post_meta(2, "_wp_page_template").unwrap(), b"default");
        assert_eq!(site.user(1).unwrap().display_name, b"admin");
        assert!(rest.reads.reads.finish().is_ok());
    })
    .await
    .unwrap();
}
