// SPDX-License-Identifier: AGPL-3.0-or-later

//! A site in a folder (`home` and `siteurl` of `http://host/blog`): the
//! admin's absolute URLs and redirects keep the folder, as PHP's, which
//! build them from `$_SERVER['REQUEST_URI']` and the site's URL. The PHP
//! oracle serves from its root, so each expected value here is derived from
//! the function named with it in WordPress 7.1.3, not recorded.

use super::*;
use crate::tests::site_at;

const HOME: &str = "http://127.0.0.1:8180/blog";
const HOST: &str = "127.0.0.1:8180";

async fn folder_site(dir: &std::path::Path) -> Arc<Site> {
    let site = Arc::try_unwrap(site_at(Some(HOME), Some(HOME)).await).unwrap();
    Arc::new(salted(site).with_sessions(hyscribe_auth::sessions::Sessions::new(dir)))
}

/// A login posted to the folder's `/hs-login`, with the cookies it set.
async fn login(site: &Arc<Site>, redirect_to: &str) -> (Response, String) {
    let form = format!(
        "log=admin&pwd=reference-only-password&testcookie=1&redirect_to={}",
        encoded(redirect_to)
    );
    posted(site, "/blog/hs-login", form).await
}

fn encoded(url: &str) -> String {
    url.replace(':', "%3A").replace('/', "%2F")
}

async fn posted(site: &Arc<Site>, uri: &str, form: String) -> (Response, String) {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri(uri)
        .header(header::HOST, HOST)
        .header(header::COOKIE, "hyscribe_test_cookie=WP%20Cookie%20check")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from(form))
        .unwrap();
    let response = router(Arc::clone(site)).oneshot(request).await.unwrap();
    let cookies: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|c| c.to_str().unwrap().split(';').next().unwrap().to_string())
        .collect();
    (response, cookies.join("; "))
}

/// `auth_redirect()`: a visitor goes to `wp_login_url( $redirect, true )`
/// with `set_url_scheme( 'http://' . HTTP_HOST . REQUEST_URI )`, the folder
/// included, as `reauth=1` and `redirect_to`.
#[tokio::test(flavor = "multi_thread")]
async fn a_visitor_is_sent_back_to_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let site = folder_site(dir.path()).await;
    let login = format!("{HOME}/hs-login?redirect_to=");
    for (uri, back) in [
        (
            "/blog/hs-admin/edit",
            "http%3A%2F%2F127.0.0.1%3A8180%2Fblog%2Fhs-admin%2Fedit",
        ),
        (
            "/blog/hs-admin/edit?x=1",
            "http%3A%2F%2F127.0.0.1%3A8180%2Fblog%2Fhs-admin%2Fedit%3Fx%3D1",
        ),
        (
            "/blog/hs-admin/",
            "http%3A%2F%2F127.0.0.1%3A8180%2Fblog%2Fhs-admin%2F",
        ),
    ] {
        let response = get(&site, uri, HOST, "").await;
        assert_eq!(response.status(), StatusCode::FOUND, "{uri}");
        assert_eq!(location(&response), format!("{login}{back}&reauth=1"));
    }
}

/// `wp-login.php` sends a login to `wp_safe_redirect( $redirect_to )`,
/// which keeps any path on the site's host: the folder's admin, and a path
/// outside the folder, which `wp_validate_redirect()` does not look at.
#[tokio::test(flavor = "multi_thread")]
async fn a_login_goes_back_to_where_it_came_from() {
    let dir = tempfile::tempdir().unwrap();
    let site = folder_site(dir.path()).await;
    for back in [
        format!("{HOME}/hs-admin/edit"),
        "http://127.0.0.1:8180/hs-admin/edit".to_string(),
    ] {
        let (response, cookies) = login(&site, &back).await;
        assert_eq!(response.status(), StatusCode::FOUND, "{back}");
        assert_eq!(location(&response), back);
        assert!(!cookies.is_empty());
    }
    // The form posted to a URL that names `redirect_to` in its query
    // (`$_REQUEST`) and not in its body goes back as well. With `reauth`
    // in the request `wp-login.php` does not redirect (`! $reauth`): the
    // 200 a verifier saw posting to the URL a visitor was sent to is PHP's;
    // the form's own action has no query, so a browser is redirected.
    let back = "http://127.0.0.1:8180/blog/hs-admin/edit";
    let form = "log=admin&pwd=reference-only-password&testcookie=1".to_string();
    let uri = format!("/blog/hs-login?redirect_to={}", encoded(back));
    let (response, _) = posted(&site, &uri, form.clone()).await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(location(&response), back);
    let (response, _) = posted(&site, &format!("{uri}&reauth=1"), form).await;
    assert_eq!(response.status(), StatusCode::OK);
}

/// `wp_admin_canonical_url()`, `wp_referer_field()` and the redirect of a
/// page past the last, as `add_query_arg()` of `$_SERVER['REQUEST_URI']`:
/// all of them carry the folder.
#[tokio::test(flavor = "multi_thread")]
async fn admin_pages_keep_the_folder() {
    let dir = tempfile::tempdir().unwrap();
    let site = folder_site(dir.path()).await;
    let (_, cookies) = login(&site, &format!("{HOME}/hs-admin/")).await;
    let dashboard = page_of(get(&site, "/blog/hs-admin/", HOST, &cookies).await).await;
    assert!(
        dashboard.contains(&format!("rel=\"canonical\" href=\"{HOME}/hs-admin/\"")),
        "{dashboard}"
    );
    let path = "/blog/hs-admin/edit-tags?taxonomy=category";
    let tags = page_of(get(&site, path, HOST, &cookies).await).await;
    assert!(
        tags.contains(&format!("name=\"_wp_http_referer\" value=\"{path}\"")),
        "{tags}"
    );
    let response = get(&site, "/blog/hs-admin/edit?paged=9", HOST, &cookies).await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(location(&response), "/blog/hs-admin/edit?paged=1");
}
