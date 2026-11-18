// SPDX-License-Identifier: AGPL-3.0-or-later

//! The link previews (MVP 4.4, ADR 0019) over HTTP: PHP's answers in
//! `url-details.tsv`, recorded against the test site, replayed as a visitor
//! and each role over `wordpress_saves` with `reference/rest/users.sql`'s
//! users and `url-details.sql`'s cache. `hyscribe-testkit` serves the test
//! site; a resolver answers a public address for `pages.test`, so the
//! client's address rules pass, and every connection goes to the test site.

use std::sync::atomic::{AtomicUsize, Ordering};

use hyscribe_config::{Database, Mode};
use hyscribe_db::Writer;
use hyscribe_fetch::{Client, Net, RootCertStore};
use hyscribe_phpser::md5;
use hyscribe_testkit::rest_cases;
use hyscribe_testkit::writes::load_default_install;
use sqlx::Connection as _;
use tokio::net::TcpStream;

use super::*;

/// `pages.test` at a public address, other names unknown; connections
/// counted, all to the test site.
#[derive(Default)]
struct Pages {
    dialed: AtomicUsize,
}

impl Net for Pages {
    fn resolve<'a>(
        &'a self,
        host: &'a str,
        port: u16,
    ) -> hyscribe_fetch::Boxed<'a, Vec<std::net::SocketAddr>> {
        let known = host == "pages.test";
        Box::pin(async move {
            match known {
                true => Ok(vec![std::net::SocketAddr::new(
                    [93, 184, 215, 14].into(),
                    port,
                )]),
                false => Err(std::io::Error::other("unknown host")),
            }
        })
    }

    fn dial(&self, _: std::net::SocketAddr) -> hyscribe_fetch::Boxed<'_, TcpStream> {
        self.dialed.fetch_add(1, Ordering::SeqCst);
        Box::pin(TcpStream::connect(
            hyscribe_testkit::pages::page_server().unwrap(),
        ))
    }
}

fn saves() -> Database {
    Database {
        name: "wordpress_saves".into(),
        ..hyscribe_testkit::fixture_database()
    }
}

/// Runs `sql` on `wordpress_saves`.
async fn run(sql: &str) {
    let db = saves();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut conn)
        .await
        .unwrap();
}

/// `wordpress_saves` as the oracle recorded over it: the default install,
/// the users and the cache.
async fn fresh() {
    load_default_install(&saves()).await.unwrap();
    run(include_str!("../../../../../reference/rest/users.sql")).await;
    let state = include_str!("../../../../../reference/rest/url-details.sql");
    run(state.split("-- cleanup").next().unwrap()).await;
}

/// The site over `wordpress_saves`, with sessions in `dir`, fetching
/// through `pages`, and with a writer when `writes`.
async fn previews(dir: &std::path::Path, pages: Arc<Pages>, writes: bool) -> Arc<Site> {
    let db = crate::Db::connect(&saves()).await.unwrap();
    let home = Some("http://127.0.0.1:8081".to_string());
    let site = Arc::new(Site::load(db, home.clone(), home).await.unwrap());
    let site = Arc::into_inner(on(site, false, dir)).unwrap();
    let writer = match writes {
        true => Writer::open(&saves(), Mode::Primary).await.unwrap(),
        false => None,
    };
    let client = Client::new(RootCertStore::empty(), pages, Arc::new(()));
    Arc::new(site.with_fetch(Some(client)).with_writer(writer))
}

/// The cached page of `/expired`, past its timeout in the fixture.
const EXPIRED: &str = "_site_transient_g_url_details_response_a752d9f881141719d6ebb4aa1fd73009";

/// The `option_id` of the option `name` in `wordpress_saves`.
async fn option_id(name: &str) -> u64 {
    let db = saves();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    let query = sqlx::query_scalar("SELECT option_id FROM wp_options WHERE option_name = ?");
    query.bind(name).fetch_one(&mut conn).await.unwrap()
}

/// The cached page of `url` and its timeout, as stored.
async fn cached(site: &Site, url: &str) -> (Option<Vec<u8>>, Option<Vec<u8>>) {
    let key = format!("g_url_details_response_{}", md5(url.as_bytes()));
    let (value, timeout) = (
        format!("_site_transient_{key}"),
        format!("_site_transient_timeout_{key}"),
    );
    let value = site.db.option(&value).await.unwrap();
    (value, site.db.option(&timeout).await.unwrap())
}

#[tokio::test(flavor = "multi_thread")]
async fn previews_match_php_and_are_cached() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh().await;
    let dir = tempfile::tempdir().unwrap();
    let pages = Arc::new(Pages::default());
    let site = previews(dir.path(), Arc::clone(&pages), true).await;
    let mut users = vec![(
        "visitor",
        (String::new(), nonce(&site, 0, Vec::new()).await),
    )];
    users.push(("administrator", log_in(&site).await));
    for (id, role) in (701..).zip(["editor", "author", "contributor", "subscriber"]) {
        users.push((role, log_in_as(&site, role, id).await));
    }
    let seeded = option_id(EXPIRED).await;
    let golden = include_str!("../../../../hyscribe-rest/tests/golden/url-details.tsv");
    let cases = rest_cases(golden);
    assert_eq!(cases.len(), 6 * 31 - 2);
    for case in &cases {
        let (_, (cookie, nonce)) = users.iter().find(|(who, _)| *who == case.who).unwrap();
        let mut request = axum::http::Request::builder()
            .method(case.method.as_str())
            .uri(case.target.replace("[]", "%5B%5D"))
            .header(header::HOST, "127.0.0.1:8081");
        if case.who != "visitor" {
            request = request
                .header(header::COOKIE, cookie)
                .header("x-wp-nonce", nonce);
        }
        let response = send(&site, request, "").await;
        let what = format!("{} {} {}", case.who, case.method, case.target);
        assert_eq!(response.status().as_u16(), case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(kept(response.headers(), nonce), php, "{what}");
        let body = body(response).await.replace(nonce.as_str(), "%nonce%");
        assert_eq!(body, String::from_utf8_lossy(&case.body), "{what}");
    }
    // Each page was fetched once, then read from the cache.
    let page = "http://pages.test:8080/page.html";
    let (value, timeout) = cached(&site, page).await;
    assert!(value.unwrap().starts_with(b"<!DOCTYPE html>"));
    let expires = String::from_utf8(timeout.unwrap())
        .unwrap()
        .parse::<i64>()
        .unwrap();
    assert!(
        (now() + 3500..=now() + 3600).contains(&expires),
        "{expires}"
    );
    let fetched = pages.dialed.load(Ordering::SeqCst);
    let again = format!("/?rest_route=/wp-block-editor/v1/url-details&url={page}");
    let (cookie, nonce) = &users[1].1;
    let request = axum::http::Request::builder()
        .uri(again)
        .header(header::COOKIE, cookie)
        .header("x-wp-nonce", nonce);
    assert_eq!(send(&site, request, "").await.status(), StatusCode::OK);
    assert_eq!(pages.dialed.load(Ordering::SeqCst), fetched);
    // The expired page was deleted and added again, as a new row.
    let (stale, _) = cached(&site, "http://pages.test:8080/expired").await;
    assert!(stale.unwrap().starts_with(b"<HTML>"));
    assert!(option_id(EXPIRED).await > seeded);
    fresh().await;
}

/// Shadow mode writes nothing: previews are fetched every time.
#[tokio::test(flavor = "multi_thread")]
async fn shadow_mode_fetches_without_caching() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh().await;
    let dir = tempfile::tempdir().unwrap();
    let pages = Arc::new(Pages::default());
    let writing = previews(dir.path(), Arc::new(Pages::default()), true).await;
    let (cookie, nonce) = log_in(&writing).await;
    let site = previews(dir.path(), Arc::clone(&pages), false).await;
    let page = "http://pages.test:8080/quotes.html";
    for _ in 0..2 {
        let request = axum::http::Request::builder()
            .uri(format!(
                "/?rest_route=/wp-block-editor/v1/url-details&url={page}"
            ))
            .header(header::COOKIE, &cookie)
            .header("x-wp-nonce", &nonce);
        let response = send(&site, request, "").await;
        assert_eq!(response.status(), StatusCode::OK);
    }
    assert_eq!(pages.dialed.load(Ordering::SeqCst), 2);
    assert_eq!(cached(&site, page).await, (None, None));
    // 150 KB are read: a description some kilobytes in is found.
    let request = axum::http::Request::builder()
        .uri("/?rest_route=/wp-block-editor/v1/url-details&url=http://pages.test:8080/mid.html")
        .header(header::COOKIE, &cookie)
        .header("x-wp-nonce", &nonce);
    let mid = body(send(&site, request, "").await).await;
    assert!(
        mid.contains("\"description\":\"within the limit\""),
        "{mid}"
    );
    // The expired page stays: shadow mode deletes nothing either.
    let expired = "/?rest_route=/wp-block-editor/v1/url-details&url=http://pages.test:8080/expired";
    let request = axum::http::Request::builder()
        .uri(expired)
        .header(header::COOKIE, &cookie)
        .header("x-wp-nonce", &nonce);
    assert_eq!(send(&site, request, "").await.status(), StatusCode::OK);
    let (stale, _) = cached(&site, "http://pages.test:8080/expired").await;
    assert_eq!(stale.as_deref(), Some(&b"<title>Stale</title>"[..]));
    fresh().await;
}

/// With outbound fetches off, the route answers 501 over HTTP too.
#[tokio::test(flavor = "multi_thread")]
async fn previews_are_not_ported_while_fetches_are_off() {
    let dir = tempfile::tempdir().unwrap();
    let site = site(false, dir.path()).await;
    let request = axum::http::Request::builder()
        .uri("/?rest_route=/wp-block-editor/v1/url-details&url=http://pages.test/");
    let response = send(&site, request, "").await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}

/// What rendering fetches and caches goes through the site's client and,
/// in primary mode only, its writer: an embed's meta and a feed's
/// transients, as `Reads` hands them to `hyscribe-write`.
#[tokio::test(flavor = "multi_thread")]
async fn rendering_fetches_and_caches_through_the_site() {
    use hyscribe_render::outbound::Get;
    use hyscribe_render::site::Site as _;
    let _saves = crate::admin::tests::SAVES.lock().await;
    fresh().await;
    let dir = tempfile::tempdir().unwrap();
    for writes in [true, false] {
        let site = previews(dir.path(), Arc::new(Pages::default()), writes).await;
        let site = Arc::new(
            Arc::into_inner(site)
                .unwrap()
                .with_feed_hosts(&["pages.test".to_string()]),
        );
        let handle = tokio::runtime::Handle::current();
        let checked = Arc::clone(&site);
        tokio::task::spawn_blocking(move || {
            let reads = crate::reads::Reads::new(&checked, handle);
            assert!(reads.feed_allowed(b"pages.test"));
            assert!(!reads.feed_allowed(b"other.test"));
            let get = Get::new(&reads);
            let page = reads.remote_get(b"http://pages.test:8080/page.html", &get);
            let page = page.unwrap().unwrap();
            assert_eq!(page.status, 200);
            assert!(page.headers.iter().any(|(name, _)| name == "content-type"));
            // The size limit reaches the client: the body is cut there.
            let small = Get {
                limit: Some(10),
                ..get.clone()
            };
            let cut = reads.remote_get(b"http://pages.test:8080/page.html", &small);
            assert_eq!(cut.unwrap().unwrap().body, page.body[..10]);
            let refused = reads.remote_get(b"http://nowhere.test/", &get).unwrap();
            assert_eq!(refused.unwrap_err(), b"A valid URL was not provided.");
            reads.update_post_meta(1, "_oembed_x", b"<b>embed</b>");
            assert_eq!(reads.post_meta(1, "_oembed_x").is_some(), writes);
            reads.set_site_transient("feed_x", b"a:1:{i:0;s:1:\"a\";}", 60);
            let stored = reads.option("_site_transient_feed_x");
            assert_eq!(stored.is_some(), writes);
            reads.delete_site_transient("feed_x");
            assert_eq!(reads.option("_site_transient_feed_x"), None);
            reads.finish().unwrap();
        })
        .await
        .unwrap();
        run("DELETE FROM wp_postmeta WHERE meta_key = '_oembed_x'").await;
    }
    let off = Site::load(
        crate::Db::connect(&saves()).await.unwrap(),
        Some("http://127.0.0.1:8081".into()),
        None,
    )
    .await
    .unwrap();
    let handle = tokio::runtime::Handle::current();
    tokio::task::spawn_blocking(move || {
        let reads = crate::reads::Reads::new(&off, handle);
        assert!(
            reads
                .remote_get(b"http://pages.test:8080/", &Get::new(&reads))
                .is_none()
        );
    })
    .await
    .unwrap();
}

/// A feed's 10 seconds, a discovery's size limit and the user agent reach
/// the client as WordPress's arguments.
#[test]
fn rendering_asks_with_its_own_arguments() {
    let get = hyscribe_render::outbound::Get {
        timeout: 10,
        limit: Some(153_600),
        user_agent: b"WordPress/7.1.3; http://a.test".to_vec(),
    };
    let args = crate::outbound::args(&get);
    assert_eq!(args.timeout, std::time::Duration::from_secs(10));
    assert_eq!(args.limit_response_size, Some(153_600));
    assert_eq!(args.user_agent, "WordPress/7.1.3; http://a.test");
    assert_eq!(args.redirection, 5);
}

#[test]
fn failures_read_as_wordpress_words_them() {
    use hyscribe_fetch::Kind;
    let messages: Vec<&[u8]> = [
        Kind::Invalid,
        Kind::Refused,
        Kind::Resolve,
        Kind::Connect,
        Kind::Tls,
        Kind::Timeout,
        Kind::Redirects,
        Kind::Protocol,
    ]
    .into_iter()
    .map(crate::outbound::message)
    .collect();
    assert_eq!(messages[..3], [&b"A valid URL was not provided."[..]; 3]);
    assert_eq!(messages[6], b"Too many redirects.");
    assert!(
        messages[3..]
            .iter()
            .all(|m| m.starts_with(b"cURL error") || m.starts_with(b"Too"))
    );
}
