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

mod golden;
mod url_details;
mod writes;
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
    log_in_as(site, "admin", 1).await
}

/// [`log_in`] as the user `id`, whose login is `login`.
async fn log_in_as(site: &Arc<Site>, login: &str, id: u64) -> (String, String) {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/hs-login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    let form = format!("log={login}&pwd=reference-only-password");
    let response = send(site, request, &form).await;
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
    (cookie, nonce(site, id, token).await)
}

/// The headers the golden keeps, sorted.
fn kept(headers: &HeaderMap, nonce: &str) -> Vec<String> {
    let kept = |name: &str| {
        [
            "content-type",
            "x-robots-tag",
            "link",
            "location",
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
    assert_eq!(replay(&sites, golden, false).await, (2 * 69, 2 * 4));
    let golden = include_str!("../../../hyscribe-rest/tests/golden/content.tsv");
    assert_eq!(replay(&sites, golden, false).await, (2 * 89 + 5, 1));
    let golden = include_str!("../../../hyscribe-rest/tests/golden/templates.tsv");
    assert_eq!(replay(&sites, golden, false).await, (2 * 85, 0));
    let golden = fixture_users(include_str!(
        "../../../hyscribe-rest/tests/golden/embed.tsv"
    ));
    assert_eq!(replay(&sites, &golden, false).await, (64, 0));
    let golden = include_str!("../../../hyscribe-rest/tests/golden/search.tsv");
    assert_eq!(replay(&sites, golden, false).await, (2 * 49, 2));
    let golden = include_str!("../../../hyscribe-rest/tests/golden/namespaces.tsv");
    assert_eq!(replay(&sites, golden, false).await, (2 * 33, 2));
    let golden = include_str!("../../../hyscribe-rest/tests/golden/widgets-default.tsv");
    assert_eq!(replay(&sites, golden, false).await, (2 * 2, 2 * 4));
}

/// The API signs with the site's `wp_hash()` and draws an embed's secret
/// as the embed page does.
#[tokio::test(flavor = "multi_thread")]
async fn the_api_signs_and_draws_as_the_site_does() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(false, dir.path()).await;
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let rest = Rest {
            reads: LoginReads::new(&site, handle, now()),
            site: &site,
            user: 0,
            caps: OnceCell::new(),
        };
        let signed = RenderSite::wp_hash(&rest.reads.reads, b"x");
        assert!(signed.is_some());
        assert_eq!(hyscribe_rest::Site::wp_hash(&rest, b"x"), signed);
        let secret = hyscribe_rest::Site::embed_secret(&rest);
        assert!(secret.len() == 10 && secret.bytes().all(|b| b.is_ascii_alphanumeric()));
    })
    .await
    .unwrap();
}

/// With `REST_PUBLIC_USERS=off`, a visitor sees no user as having
/// published posts.
#[tokio::test(flavor = "multi_thread")]
async fn public_users_off_match_php() {
    let dir = tempfile::tempdir().unwrap();
    let off = |site: Arc<Site>| Arc::new(Arc::into_inner(site).unwrap().with_public_users(false));
    let sites = (
        off(site(false, dir.path()).await),
        off(site(true, dir.path()).await),
    );
    let golden = fixture_users(include_str!(
        "../../../hyscribe-rest/tests/golden/people-off.tsv"
    ));
    assert_eq!(replay(&sites, &golden, false).await, (14, 0));
}

/// With `EMPTY_TRASH_DAYS=0`, deletes must be forced.
#[tokio::test(flavor = "multi_thread")]
async fn trash_off_matches_php() {
    let dir = tempfile::tempdir().unwrap();
    let constants = hyscribe_config::Constants {
        empty_trash_days: 0,
        ..hyscribe_config::Constants::default()
    };
    let off = |site: Arc<Site>| Arc::new(Arc::into_inner(site).unwrap().with_constants(constants));
    let sites = (
        off(site(false, dir.path()).await),
        off(site(true, dir.path()).await),
    );
    let golden = include_str!("../../../hyscribe-rest/tests/golden/trash-off.tsv");
    assert_eq!(replay(&sites, golden, false).await, (2 * 7, 0));
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
    assert_eq!(replay(&sites, golden, true).await, (2 * 93 + 5, 1));
    let golden = fixture_users(include_str!(
        "../../../hyscribe-rest/tests/golden/embed-ttd.tsv"
    ));
    assert_eq!(replay(&sites, &golden, true).await, (74, 0));
    let golden = include_str!("../../../hyscribe-rest/tests/golden/search-ttd.tsv");
    assert_eq!(replay(&sites, golden, true).await, (2 * 63, 2));
}

/// `golden` but for the requests of the users `reference/rest/users.sql`
/// adds, which the fixtures leave out.
fn fixture_users(golden: &str) -> String {
    let lines = golden.lines().filter(|line| {
        let target = String::from_utf8(unhex(line.split('\t').nth(2).unwrap())).unwrap();
        let users = target.contains("/wp/v2/users");
        let own = target.contains("/users/1&") || target.ends_with("/users/1");
        let fixture = ["include=1", "slug=admin", "exclude=701,702,703,704"];
        let listed = target.split('&').any(|arg| fixture.contains(&arg));
        !users || own || listed || target.contains("/users/me")
    });
    lines.collect::<Vec<_>>().join("\n")
}

/// The comments and users routes over both fixtures, but for the users
/// `reference/rest/users.sql` adds.
#[tokio::test(flavor = "multi_thread")]
async fn people_match_php() {
    let dir = tempfile::tempdir().unwrap();
    let sites = (site(false, dir.path()).await, site(true, dir.path()).await);
    let golden = fixture_users(include_str!(
        "../../../hyscribe-rest/tests/golden/people.tsv"
    ));
    assert_eq!(replay(&sites, &golden, false).await, (230, 0));
    let sites = (
        ttd_site(false, dir.path()).await,
        ttd_site(true, dir.path()).await,
    );
    let golden = fixture_users(include_str!(
        "../../../hyscribe-rest/tests/golden/people-ttd.tsv"
    ));
    assert_eq!(replay(&sites, &golden, true).await, (246, 0));
}

/// The media, revisions and autosaves routes over both fixtures, but for
/// what `reference/rest/media.sql` adds, which the fixtures leave out.
#[tokio::test(flavor = "multi_thread")]
async fn media_match_php() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = |golden: &str, kept: &[&str]| {
        let lines = golden.lines().filter(|line| {
            let target = String::from_utf8(unhex(line.split('\t').nth(2).unwrap())).unwrap();
            let route = target
                .split(['&', '?'])
                .nth(usize::from(target.starts_with("/?")));
            route.is_some_and(|route| kept.iter().any(|kept| route.ends_with(kept)))
        });
        lines.collect::<Vec<_>>().join("\n")
    };
    let sites = (site(false, dir.path()).await, site(true, dir.path()).await);
    let golden = include_str!("../../../hyscribe-rest/tests/golden/media.tsv");
    let kept = [
        "/media/1",
        "/media/99999",
        "/posts/2/revisions",
        "/posts/99999/revisions",
        "/posts/0/revisions/9111",
        "/posts/2/autosaves",
        "/posts/2/autosaves/1",
        "/pages/3/revisions",
        "/pages/3/autosaves",
        "/pages/3/autosaves/1",
        "/posts/0/autosaves",
    ];
    assert_eq!(
        replay(&sites, &fixture(golden, &kept), false).await,
        (22, 0)
    );
    let sites = (
        ttd_site(false, dir.path()).await,
        ttd_site(true, dir.path()).await,
    );
    let golden = include_str!("../../../hyscribe-rest/tests/golden/media-ttd.tsv");
    let kept = ["/611", "/821", "/1029", "/1690", "/1692", "/1023"];
    assert_eq!(replay(&sites, &fixture(golden, &kept), true).await, (16, 0));
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
        // PHP answers 501 too where a type does not support the Trash.
        if response.status() == StatusCode::NOT_IMPLEMENTED && fields[4] != "501" {
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
        if hashed || flag("hash") {
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

/// Whether a user has posts, which the users route asks of those who may
/// not list users, and users by slug are read from the database.
#[tokio::test(flavor = "multi_thread")]
async fn authors_posts_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(false, dir.path()).await;
    let handle = Handle::current();
    tokio::task::spawn_blocking(move || {
        let rest = Rest {
            reads: LoginReads::new(&site, handle, now()),
            site: &site,
            user: 0,
            caps: OnceCell::new(),
        };
        let types = [("post".to_string(), false)];
        assert!(hyscribe_rest::Site::has_posts(&rest, 1, &types));
        assert!(!hyscribe_rest::Site::has_posts(&rest, 2, &types));
        // `FIELD()` of no slugs, as PHP writes it, orders nothing.
        let slugs = UserQuery {
            order: (hyscribe_model::UserOrder::Nicenames, false),
            ..UserQuery::default()
        };
        let users = hyscribe_rest::Site::users(&rest, &slugs);
        assert_eq!(users.iter().map(|user| user.id).collect::<Vec<_>>(), [1]);
    })
    .await
    .unwrap();
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
        assert_eq!(site.nonce("wp_rest"), site.create_nonce());
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

/// A write's body is read as a form or JSON, and a multipart form, which
/// only PHP reads, leaves the request unanswered.
#[tokio::test(flavor = "multi_thread")]
async fn write_bodies_are_read() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(false, dir.path()).await;
    let post = |kind: &'static str| {
        axum::http::Request::builder()
            .method("POST")
            .uri("/?rest_route=/wp/v2/posts/1")
            .header(header::CONTENT_TYPE, kind)
    };
    let form = "application/x-www-form-urlencoded";
    let response = send(&site, post(form), "").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = send(&site, post(form), "_wpnonce=x").await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = send(&site, post(form), "title=x").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response = send(&site, post("application/json"), "{}").await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let multipart = "multipart/form-data; boundary=x";
    let response = send(&site, post(multipart), "--x--").await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}
