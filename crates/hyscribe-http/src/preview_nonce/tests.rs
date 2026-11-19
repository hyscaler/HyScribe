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
        let names = Names::new(&site);
        let user = reads.logged_in_user(self::cookie(&jar, &names.logged_in()), false);
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

/// A check that panicked answers 500.
#[tokio::test]
async fn a_failed_check_answers_500() {
    let panicked = tokio::spawn(async { panic!("check bug") })
        .await
        .unwrap_err();
    let reply = settled(Err(panicked)).unwrap();
    assert_eq!(reply.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
