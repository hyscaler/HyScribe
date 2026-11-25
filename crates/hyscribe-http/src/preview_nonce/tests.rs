// SPDX-License-Identifier: AGPL-3.0-or-later

//! These tests read the default install fixture from a live MariaDB:
//! start it with `reference/default-install/db.sh up`.

use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

use super::*;
use crate::admin::tests::salted;
use crate::router;
use crate::tests::oracle_site;
use hyscribe_admin::login::Site as _;

async fn status(site: &Arc<Site>, uri: &str, cookie: &str) -> StatusCode {
    let mut request = Request::builder().uri(uri);
    if !cookie.is_empty() {
        request = request.header(header::COOKIE, cookie);
    }
    let reply = router(Arc::clone(site))
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    reply.status()
}

/// `wp_create_nonce( 'post_preview_1' )` of the user a cookie names, else
/// of a visitor.
async fn nonce(site: &Arc<Site>, cookie: &str) -> String {
    let site = Arc::clone(site);
    let cookie = cookie.to_string();
    tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&site, Handle::current(), now());
        let jar = cookies([cookie.as_bytes()]);
        let user = reads.current_user(&jar, false);
        if let Some((user, token)) = user {
            reads.act_as(user, token);
        }
        reads.nonce("post_preview_1")
    })
    .await
    .unwrap()
}

/// A bad nonce dies with PHP's 403 whatever else the request asks (the page
/// itself is in `pages-default.tsv`), also an array for either var; a valid
/// one for a visitor or the logged in user gets past the check, and a nonce of
/// another user or post does not.
#[tokio::test(flavor = "multi_thread")]
async fn a_preview_needs_its_nonce() {
    let dir = tempfile::tempdir().unwrap();
    let site = Arc::try_unwrap(oracle_site().await).unwrap();
    let sessions = hyscribe_auth::sessions::Sessions::new(dir.path());
    let site = Arc::new(salted(site).with_sessions(sessions));
    let drafts = StatusCode::FORBIDDEN;
    for query in [
        "p=1&preview=true&preview_id=1&preview_nonce=x",
        "preview_id=1&preview_nonce=",
        "preview_id=1&preview_nonce[]=x",
        "preview_id[]=1&preview_nonce=x",
        "preview_id=2&preview_nonce=x&feed=rss2",
    ] {
        let uri = format!("/?{query}");
        assert_eq!(status(&site, &uri, "").await, drafts, "{query}");
    }
    // Only both vars ask for the check.
    for query in ["preview_id=1", "preview_nonce=x", "p=1&preview=true"] {
        let uri = format!("/?{query}");
        assert_ne!(status(&site, &uri, "").await, drafts, "{query}");
    }
    let visitor = nonce(&site, "").await;
    let valid = format!("/?p=1&preview=true&preview_id=1&preview_nonce={visitor}");
    assert_ne!(status(&site, &valid, "").await, drafts);
    let other = format!("/?p=1&preview=true&preview_id=2&preview_nonce={visitor}");
    assert_eq!(status(&site, &other, "").await, drafts);
    let login = Request::builder()
        .method("POST")
        .uri("/hs-login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("log=admin&pwd=reference-only-password"))
        .unwrap();
    let response = router(Arc::clone(&site)).oneshot(login).await.unwrap();
    let cookies: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|c| c.to_str().unwrap().split(';').next().unwrap().to_string())
        .collect();
    let cookies = cookies.join("; ");
    let own = nonce(&site, &cookies).await;
    assert_ne!(own, visitor);
    let mine = format!("/?p=1&preview=true&preview_id=1&preview_nonce={own}");
    assert_ne!(status(&site, &mine, &cookies).await, drafts);
    // A visitor's nonce is not the logged in user's, nor theirs a visitor's.
    assert_eq!(status(&site, &valid, &cookies).await, drafts);
    assert_eq!(status(&site, &mine, "").await, drafts);
}

/// The salted site with sessions, and the cookies of a login as the admin.
async fn session(dir: &std::path::Path) -> (Arc<Site>, Vec<String>) {
    let site = Arc::try_unwrap(oracle_site().await).unwrap();
    let sessions = hyscribe_auth::sessions::Sessions::new(dir);
    let site = Arc::new(salted(site).with_sessions(sessions));
    let login = Request::builder()
        .method("POST")
        .uri("/hs-login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("log=admin&pwd=reference-only-password"))
        .unwrap();
    let response = router(Arc::clone(&site)).oneshot(login).await.unwrap();
    let pairs = response.headers().get_all(header::SET_COOKIE).iter();
    let pairs = pairs.map(|c| c.to_str().unwrap().split(';').next().unwrap().to_string());
    (site, pairs.collect())
}

/// What the golden's PHP path is here: `wp-admin` is `hs-admin` without
/// `.php`, and `wp-login.php` is `hs-login`.
fn here(path: &str) -> String {
    let (file, query) = path.split_once('?').unwrap_or((path, ""));
    let file = match file.strip_prefix("/wp-admin/") {
        Some(screen) => format!("/hs-admin/{}", screen.trim_end_matches(".php")),
        None if file == "/wp-login.php" => "/hs-login".to_string(),
        None => file.to_string(),
    };
    if query.is_empty() {
        file
    } else {
        format!("{file}?{query}")
    }
}

/// A bad nonce at every entry point that loads WordPress dies as PHP's
/// `wp_die()` does there (`reference/preview-nonce/golden.sh`): the page, the
/// Ajax text, JSON or XML by the headers, nothing for `xmlrpc.php`; the
/// loaders do not load WordPress.
#[tokio::test(flavor = "multi_thread")]
async fn every_entry_point_refuses_as_php_does() {
    let dir = tempfile::tempdir().unwrap();
    let (site, _) = session(dir.path()).await;
    let golden = include_str!("../../tests/golden/preview-nonce.tsv");
    assert_eq!(golden.lines().count(), 64);
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (method, path, accept, kind, body) = (f[0], f[1], f[2], f[3], f[4]);
        let mut request = Request::builder().method(method).uri(here(path));
        for (name, value) in [(header::ACCEPT, accept), (header::CONTENT_TYPE, kind)] {
            if !value.is_empty() {
                request = request.header(name, value);
            }
        }
        let request = request.body(Body::from(body.to_string())).unwrap();
        let reply = router(Arc::clone(&site)).oneshot(request).await.unwrap();
        let what = format!("{method} {path} {accept} {kind}");
        assert_eq!(reply.status().as_u16().to_string(), f[5], "{what}");
        if f[5] == "405" {
            continue;
        }
        let kind = reply.headers().get(header::CONTENT_TYPE);
        assert_eq!(kind.unwrap().to_str().unwrap(), f[6], "{what}");
        // The loaders' caching headers are theirs.
        if f[7].starts_with("no-cache") {
            let cache = reply.headers().get(header::CACHE_CONTROL).unwrap();
            assert_eq!(cache.to_str().unwrap(), f[7], "{what}");
            let expires = reply.headers().get(header::EXPIRES).unwrap();
            assert_eq!(expires.to_str().unwrap(), f[8], "{what}");
        }
        let bytes = axum::body::to_bytes(reply.into_body(), usize::MAX).await;
        assert!(bytes.unwrap() == hyscribe_testkit::unhex(f[9]), "{what}");
    }
}

/// The entry points past a valid nonce, and without both vars, do their own
/// work: none answers with the refusal.
#[tokio::test(flavor = "multi_thread")]
async fn a_valid_nonce_passes_every_entry_point() {
    let dir = tempfile::tempdir().unwrap();
    let (site, _) = session(dir.path()).await;
    let good = nonce(&site, "").await;
    for (method, path) in [
        ("GET", "/hs-admin/index"),
        ("GET", "/hs-admin/admin-post"),
        ("POST", "/hs-admin/admin-ajax"),
        ("POST", "/hs-admin/admin-ajax?action=heartbeat"),
        ("GET", "/hs-login"),
        ("POST", "/wp-comments-post.php"),
        ("GET", "/wp-comments-post.php"),
        ("GET", "/wp-json/"),
        ("GET", "/wp-cron.php"),
        ("GET", "/xmlrpc.php"),
    ] {
        let sep = if path.contains('?') { '&' } else { '?' };
        for query in [
            format!("preview_id=1&preview_nonce={good}"),
            "preview_id=1".to_string(),
            "preview_nonce=x".to_string(),
        ] {
            let request = Request::builder()
                .method(method)
                .uri(format!("{path}{sep}{query}"))
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::empty())
                .unwrap();
            let reply = router(Arc::clone(&site)).oneshot(request).await.unwrap();
            let bytes = axum::body::to_bytes(reply.into_body(), usize::MAX).await;
            let text = String::from_utf8_lossy(&bytes.unwrap()).into_owned();
            assert!(!text.contains(MESSAGE), "{method} {path} {query}");
        }
    }
}

/// Whether `refuse` answers a GET with the preview vars of `nonce`.
async fn refusal(headers: &HeaderMap, entry: Entry, site: &Arc<Site>, nonce: &str) -> bool {
    let query = format!("preview_id=1&preview_nonce={nonce}");
    refuse(site, headers, Some(&query), false, entry)
        .await
        .is_some()
}

fn headers_of(pairs: &[(header::HeaderName, &str)]) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for (name, value) in pairs {
        headers.append(name.clone(), HeaderValue::from_str(value).unwrap());
    }
    headers
}

/// `init` sees the auth cookie's user, or the logged in cookie's, either of
/// which alone gives the user's nonce; `xmlrpc.php` discards the cookies.
#[tokio::test(flavor = "multi_thread")]
async fn init_finds_the_user_by_either_cookie() {
    let dir = tempfile::tempdir().unwrap();
    let (site, pairs) = session(dir.path()).await;
    let logged_in = pairs.iter().find(|p| p.contains("logged_in")).unwrap();
    let is_auth = |p: &&String| !p.contains("logged_in") && !p.contains("test_cookie");
    let auth = pairs.iter().find(is_auth).unwrap();
    let all = pairs.join("; ");
    let own = nonce(&site, &all).await;
    let visitor = nonce(&site, "").await;
    assert_ne!(own, visitor);
    for jar in [auth, logged_in] {
        let headers = headers_of(&[(header::COOKIE, jar)]);
        assert!(!refusal(&headers, Entry::Page, &site, &own).await, "{jar}");
        assert!(
            refusal(&headers, Entry::Page, &site, &visitor).await,
            "{jar}"
        );
    }
    let headers = headers_of(&[(header::COOKIE, &all)]);
    assert!(refusal(&headers, Entry::XmlRpc, &site, &own).await);
    assert!(!refusal(&headers, Entry::XmlRpc, &site, &visitor).await);
}

/// A POST or Ajax request gives an expired cookie an hour's grace.
#[test]
fn grace_covers_post_and_ajax() {
    assert!(grace(true, Entry::Page));
    assert!(grace(false, Entry::Ajax));
    assert!(!grace(false, Entry::Page));
    assert!(!grace(false, Entry::XmlRpc));
}

/// `wp_die()`'s handler: Ajax over JSON over XML-RPC over XML.
#[test]
fn the_handler_follows_wp_die() {
    use header::{ACCEPT, CONTENT_TYPE};
    let pick =
        |pairs: &[(header::HeaderName, &str)], entry| Handler::pick(&headers_of(pairs), entry);
    let json = [(ACCEPT, "application/json")];
    let xml = [(ACCEPT, "text/xml")];
    assert_eq!(pick(&[], Entry::Page), Handler::Default);
    assert_eq!(pick(&json, Entry::Ajax), Handler::Ajax);
    assert_eq!(pick(&xml, Entry::Ajax), Handler::Ajax);
    assert_eq!(pick(&json, Entry::Page), Handler::Json);
    assert_eq!(pick(&json, Entry::XmlRpc), Handler::Json);
    let typed = [(CONTENT_TYPE, "application/json")];
    assert_eq!(pick(&typed, Entry::Page), Handler::Json);
    assert_eq!(pick(&[], Entry::XmlRpc), Handler::XmlRpc);
    assert_eq!(pick(&xml, Entry::XmlRpc), Handler::XmlRpc);
    assert_eq!(pick(&xml, Entry::Page), Handler::Xml);
    let typed = [(CONTENT_TYPE, "text/xml")];
    assert_eq!(pick(&typed, Entry::Page), Handler::Xml);
    let both = [(ACCEPT, "text/xml"), (ACCEPT, "application/json")];
    assert_eq!(pick(&both, Entry::Page), Handler::Json);
    let split = [(ACCEPT, "text/html"), (ACCEPT, "text/xml")];
    assert_eq!(pick(&split, Entry::Page), Handler::Xml);
}

#[test]
fn json_media_types_match_the_regex() {
    for media in [
        "application/json",
        "APPLICATION/JSON; charset=utf-8",
        "text/html, application/json;q=0.9",
        "application/vnd.api+json",
        "application/ld+json",
        "application/json+oembed",
        "foo,application/json",
        "x\tapplication/json",
        "x\x0bapplication/json",
        "application/a+b+json",
        "application/json\t",
        "application/json,x",
        "application/_!#$.&^+json",
        "application/(+json",
    ] {
        assert!(is_json_media_type(media.as_bytes()), "{media}");
    }
    for media in [
        "",
        "application/",
        "application/jsonp",
        "application/json-patch",
        "text/json",
        "xapplication/json",
        "application/+json",
        "application/json+",
        "application/json+oembedx",
        "application/json+oembed+",
        "application/ json",
        "application/{+json",
    ] {
        assert!(!is_json_media_type(media.as_bytes()), "{media}");
    }
}

#[test]
fn xml_requests_match_by_accept_or_exact_content_type() {
    for accept in [
        "text/xml",
        "application/rss+xml",
        "application/atom+xml",
        "application/rdf+xml",
        "text/xml+oembed",
        "application/xml+oembed",
    ] {
        assert!(is_xml_request(accept.as_bytes(), b""), "{accept}");
        assert!(is_xml_request(b"", accept.as_bytes()), "{accept}");
    }
    assert!(is_xml_request(b"text/html, application/rss+xml", b""));
    assert!(!is_xml_request(b"application/xml", b""));
    assert!(!is_xml_request(b"", b"text/xml; charset=UTF-8"));
    assert!(!is_xml_request(b"", b"x text/xml"));
    assert!(!is_xml_request(b"", b""));
}

/// A check that panicked answers 500.
#[tokio::test]
async fn a_failed_check_answers_500() {
    let panicked = tokio::spawn(async { panic!("check bug") })
        .await
        .unwrap_err();
    let reply = settled(Err(panicked)).unwrap();
    assert_eq!(reply.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
