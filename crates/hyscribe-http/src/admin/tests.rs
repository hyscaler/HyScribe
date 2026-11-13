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
    let response = get(&site, "/hs-admin/themes", "127.0.0.1:8081", &auth).await;
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
    // A view and a post's comments, as PHP lists them.
    for query in ["comment_status=moderated", "p=1"] {
        let path = format!("/hs-admin/edit-comments?{query}");
        let page = page_of(get(&site, &path, "127.0.0.1:8081", &cookies).await).await;
        let row = list
            .lines()
            .find(|l| l.starts_with(&format!("default?{query}\tadministrator\t")))
            .unwrap();
        let php = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
        let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap());
        let (php, page) = (
            hyscribe_testkit::settled_page(&php),
            hyscribe_testkit::settled_page(&page),
        );
        assert_eq!(with_values(&php, &page), page, "{query}");
    }
    let path = "/hs-admin/edit-comments?comment_status=trash&user_id=1&paged=1";
    let page = page_of(get(&site, path, "127.0.0.1:8081", &cookies).await).await;
    assert!(page.contains("No comments found in Trash."));
    // A page past the last goes to the last.
    let path = "/hs-admin/edit-comments?paged=5";
    let response = get(&site, path, "127.0.0.1:8081", &cookies).await;
    assert_eq!(response.status(), StatusCode::FOUND);
    assert_eq!(
        response.headers()[header::LOCATION],
        "/hs-admin/edit-comments?paged=1"
    );
    // Searched, filtered by type and sorted in the extended view.
    let path = "/hs-admin/edit-comments?s=WordPress&comment_type=comment&orderby=comment_author&order=asc&mode=excerpt";
    let page = page_of(get(&site, path, "127.0.0.1:8081", &cookies).await).await;
    assert!(page.contains("Search results for: <strong>WordPress</strong>"));
    assert!(page.contains("id='comment-1'") && page.contains("row-actions visible"));
    for path in [
        "/hs-admin/edit-comments?approved=1",
        "/hs-admin/edit-comments?p=3",
        "/hs-admin/edit-comments?orderby=meta_value",
        "/hs-admin/edit-comments?comment_status[]=x",
        "/hs-admin/edit-comments?comment_status=%FF",
    ] {
        let response = get(&site, path, "127.0.0.1:8081", &auth).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{path}");
    }
    // The users.
    let page = page_of(get(&site, "/hs-admin/users", "127.0.0.1:8081", &cookies).await).await;
    let list = include_str!("../../../hyscribe-admin/tests/golden/users-list.tsv");
    let row = list
        .lines()
        .find(|l| l.starts_with("default\tadministrator\t"))
        .unwrap();
    let php = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
    let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap());
    assert_eq!(with_values(&php, &page), page);
    let path = "/hs-admin/users?role=editor&s=admin&orderby=post_count&order=desc";
    let page = page_of(get(&site, path, "127.0.0.1:8081", &cookies).await).await;
    assert!(page.contains("No users found.") && page.contains("<strong>admin</strong>"));
    let path = "/hs-admin/users?role=none";
    let page = page_of(get(&site, path, "127.0.0.1:8081", &cookies).await).await;
    assert!(page.contains("id='user-1'"));
    // Only a role filter names its meta clauses.
    let path = "/hs-admin/users?role=none&orderby=mt1";
    let response = get(&site, path, "127.0.0.1:8081", &cookies).await;
    assert_eq!(response.status(), StatusCode::OK);
    let response = get(&site, "/hs-admin/users?paged=5", "127.0.0.1:8081", &cookies).await;
    assert_eq!(location(&response), "/hs-admin/users?paged=1");
    for path in [
        "/hs-admin/users?orderby=meta_value",
        "/hs-admin/users?role=editor&orderby=mt1",
        "/hs-admin/users?s=%FF",
        "/hs-admin/users?role[]=editor",
        "/hs-admin/users?update=add",
    ] {
        let response = get(&site, path, "127.0.0.1:8081", &auth).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{path}");
    }
    // The profile, with the session this request has.
    let page = page_of(get(&site, "/hs-admin/profile", "127.0.0.1:8081", &cookies).await).await;
    let list = include_str!("../../../hyscribe-admin/tests/golden/profile.tsv");
    let row = list
        .lines()
        .find(|l| l.starts_with("sessions1\tadministrator\t"))
        .unwrap();
    let php = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
    let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap());
    let page = masked(&page);
    assert_eq!(with_values(&php, &page), page);
    // A second session offers to end the others.
    let origin = hyscribe_auth::sessions::Origin {
        ip: b"",
        ua: b"",
        login: now(),
    };
    let sessions = site.sessions.as_ref().unwrap();
    sessions.create(1, now() + 3600, &origin).unwrap();
    let page = page_of(get(&site, "/hs-admin/profile", "127.0.0.1:8081", &cookies).await).await;
    assert!(page.contains("id=\"destroy-sessions\">Log Out Everywhere Else"));
    let response = get(
        &site,
        "/hs-admin/profile?updated=1",
        "127.0.0.1:8081",
        &auth,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    // The Discussion, Media, Permalinks and Privacy settings, without
    // their arguments and actions.
    let list = include_str!("../../../hyscribe-admin/tests/golden/settings-more.tsv");
    let today = |format| hyscribe_render::date::format_unix(format, now(), Zone::Utc);
    for file in [
        "options-discussion",
        "options-media",
        "options-permalink",
        "options-privacy",
    ] {
        let path = format!("/hs-admin/{file}");
        let page = page_of(get(&site, &path, "127.0.0.1:8081", &cookies).await).await;
        let row = list
            .lines()
            .find(|l| l.starts_with(&format!("default\t{file}.php\tadministrator\t")))
            .unwrap();
        let php = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
        let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap())
            .replace("%ymd%", &today("Y/m/d"))
            .replace("%ym%", &today("Y/m"));
        assert_eq!(with_values(&php, &page), page, "{file}");
        let response = get(
            &site,
            &format!("{path}?tab=policyguide"),
            "127.0.0.1:8081",
            &auth,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{file}");
        let form = axum::http::Request::builder()
            .method("POST")
            .uri(&path)
            .header(header::HOST, "127.0.0.1:8081")
            .header(header::COOKIE, &cookies)
            .body(Body::from("action=create-privacy-page"))
            .unwrap();
        let response = router(Arc::clone(&site)).oneshot(form).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{file}");
    }
    // Add User, as PHP showed it.
    let page = page_of(get(&site, "/hs-admin/user-new", "127.0.0.1:8081", &cookies).await).await;
    let php = edit_golden("u3\tuser-new.php\tadministrator\t");
    let page = masked(&page);
    assert_eq!(with_values(&php, &page), page);
    // The administrator's own profile through `user-edit.php`.
    let path = "/hs-admin/user-edit?user_id=1&wp_http_referer=%2Fhs-admin%2Fusers";
    let page = page_of(get(&site, path, "127.0.0.1:8081", &cookies).await).await;
    assert!(page.contains("name=\"wp_http_referer\" value=\"/hs-admin/users\""));
    assert!(page.contains("var userProfileL10n = {\"user_id\":\"1\""));
    assert!(page.contains("id='wp-admin-bar-view'"));
    let response = get(
        &site,
        "/hs-admin/user-edit?user_id=9",
        "127.0.0.1:8081",
        &auth,
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(page_of(response).await.contains("Invalid user ID."));
    // The Settings screens; Writing with its random strings masked, and
    // General, whose dates are now's, by its parts.
    let settings = include_str!("../../../hyscribe-admin/tests/golden/settings.tsv");
    for screen in ["writing", "reading"] {
        let path = format!("/hs-admin/options-{screen}");
        let page = page_of(get(&site, &path, "127.0.0.1:8081", &cookies).await).await;
        let start = format!("default\tadministrator\t{screen}\t");
        let row = settings.lines().find(|l| l.starts_with(&start)).unwrap();
        let php = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
        let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap());
        // The random strings are the eight characters of their `<kbd>`s.
        let parts: Vec<&str> = page.split("<kbd>").collect();
        let mut masked = parts[0].to_string();
        for part in &parts[1..] {
            let key = part.get(8..).filter(|rest| rest.starts_with("</kbd>"));
            masked.push_str("<kbd>");
            masked.push_str(&key.map_or(part.to_string(), |rest| format!("%password%{rest}")));
        }
        assert_eq!(with_values(&php, &masked), masked, "{screen}");
    }
    let page = page_of(
        get(
            &site,
            "/hs-admin/options-general",
            "127.0.0.1:8081",
            &cookies,
        )
        .await,
    )
    .await;
    assert!(page.contains("<h1>General Settings</h1>"));
    assert!(page.contains(
        "value=\"http://127.0.0.1:8081\" disabled='disabled' class=\"regular-text code disabled\""
    ));
    for path in [
        "/hs-admin/user-edit?user_id=1&updated=1",
        "/hs-admin/user-edit?user_id[]=1",
        "/hs-admin/user-edit?user_id=%FF",
        "/hs-admin/user-new?update=add",
        "/hs-admin/options-general?settings-updated=true",
        "/hs-admin/options-writing?x",
        "/hs-admin/options-reading?",
    ] {
        let response = get(&site, path, "127.0.0.1:8081", &auth).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{path}");
    }
    // No form is saved yet.
    let post = axum::http::Request::builder()
        .method("POST")
        .uri("/hs-admin/user-edit?user_id=1")
        .header(header::HOST, "127.0.0.1:8081")
        .header(header::COOKIE, &cookies)
        .body(Body::from("action=update"))
        .unwrap();
    let response = router(Arc::clone(&site)).oneshot(post).await.unwrap();
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

/// `database` seen from the oracle's address, with a login for `admin`:
/// the site and its cookies.
async fn logged_in(database: &str, dir: &std::path::Path) -> (Arc<Site>, String) {
    let config = hyscribe_config::Database {
        name: database.into(),
        ..hyscribe_testkit::fixture_database()
    };
    let db = hyscribe_db::Db::connect(&config).await.unwrap();
    let url = Some("http://127.0.0.1:8081".to_string());
    let site = Site::load(db, url.clone(), url).await.unwrap();
    let site = Arc::new(salted(site).with_sessions(hyscribe_auth::sessions::Sessions::new(dir)));
    let login = axum::http::Request::builder()
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
    (site, cookies.join("; "))
}

/// The administrator's Media screens of `reference/media/golden.sh` over
/// the default install and the Theme Test Data, as the database gives
/// them; a POST or an action answers 501.
#[tokio::test(flavor = "multi_thread")]
async fn media_screens_are_php_s() {
    let dir = tempfile::tempdir().unwrap();
    let mut pages = 0;
    for (database, golden) in [
        (
            "wordpress",
            include_str!("../../../hyscribe-admin/tests/golden/media.tsv"),
        ),
        (
            "wordpress_ttd",
            include_str!("../../../hyscribe-admin/tests/golden/media-ttd.tsv"),
        ),
    ] {
        let (site, cookies) = logged_in(database, dir.path()).await;
        for line in golden.lines() {
            let f: Vec<&str> = line.split('\t').collect();
            if f[0] != "default" || f[2] != "administrator" {
                continue;
            }
            let (file, query) = f[1].split_once('?').unwrap_or((f[1], ""));
            let path = format!("/hs-admin/{}?{query}", file.trim_end_matches(".php"));
            let path = path.trim_end_matches('?');
            let response = get(&site, path, "127.0.0.1:8081", &cookies).await;
            let page = match response.status() {
                StatusCode::FOUND => format!("302\t{}", location(&response)),
                _ => page_of(response).await,
            };
            pages += 1;
            // The fixture's pages are kept as hashes, every nonce masked;
            // the database breaks the ties of sorts by author or parent as
            // it goes, so their rows are left out.
            if let [_, _, _, _, hash] = f[..] {
                let page = masked_nonces(&page);
                let page = if query.contains("orderby=author") || query.contains("orderby=parent") {
                    let start = page.find("<tbody id=\"the-list\"").unwrap();
                    let end = page.find("\t</tbody>").unwrap();
                    format!("{}{}", page.get(..start).unwrap(), page.get(end..).unwrap())
                } else {
                    page
                };
                assert_eq!(sha256(&page), hash, "{database} {path}");
                continue;
            }
            let php = hyscribe_testkit::unhex(f[3]);
            let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap());
            assert_eq!(with_values(&php, &page), page, "{database} {path}");
        }
        for path in [
            "/hs-admin/upload?mode=list&action=delete&media=1",
            "/hs-admin/upload?mode=list&p=1",
            "/hs-admin/upload?mode=list&cat=1",
            "/hs-admin/upload?x[]=1",
            "/hs-admin/media-new?post_id[]=1",
            // An attachment's edit screen is not ported.
            "/hs-admin/post?post=611&action=edit",
        ] {
            let response = get(&site, path, "127.0.0.1:8081", &cookies).await;
            assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{path}");
        }
        let post = axum::http::Request::builder()
            .method("POST")
            .uri("/hs-admin/media-new")
            .header(header::HOST, "127.0.0.1:8081")
            .header(header::COOKIE, &cookies)
            .body(Body::from("html-upload=Upload"))
            .unwrap();
        let response = router(Arc::clone(&site)).oneshot(post).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    }
    assert_eq!(pages, 64);
}

/// `page` with its nonces, ten hexadecimal digits standing alone, and its
/// time masked, as the hashed goldens hold them.
fn masked_nonces(page: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    for c in page.chars().chain(['\0']) {
        if c.is_ascii_alphanumeric() || c == '_' {
            word.push(c);
            continue;
        }
        let nonce = word.len() == 10 && word.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f'));
        out.push_str(if nonce { "%nonce%" } else { &word });
        word.clear();
        out.push(c);
    }
    out.pop();
    let at = out.find("\"time\":\"").unwrap() + 8;
    let end = at + out.get(at..).unwrap().find('"').unwrap();
    format!(
        "{}%time%{}",
        out.get(..at).unwrap(),
        out.get(end..).unwrap()
    )
}

fn sha256(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// A page of `reference/admin-page/golden.sh`, by its row's start, under
/// hs-admin's URLs.
fn golden(row: &str) -> String {
    let golden = include_str!("../../../hyscribe-admin/tests/golden/pages.tsv");
    let row = golden.lines().find(|l| l.starts_with(row)).unwrap();
    let page = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
    hyscribe_testkit::hs_admin_page(&String::from_utf8(page).unwrap())
}

/// A page of `reference/user-edit/golden.sh`, by its row's start, under
/// hs-admin's URLs.
fn edit_golden(row: &str) -> String {
    let golden = include_str!("../../../hyscribe-admin/tests/golden/user-edit.tsv");
    let row = golden.lines().find(|l| l.starts_with(row)).unwrap();
    let page = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
    hyscribe_testkit::hs_admin_page(&String::from_utf8(page).unwrap())
}

/// `page` with its generated password, escaped and the attribute's whole
/// value, masked as the goldens mask it.
fn masked(page: &str) -> String {
    let at = page.find("data-pw=\"").unwrap() + 9;
    let end = at + page.get(at..).unwrap().find('"').unwrap();
    assert!(end - at >= 24);
    format!(
        "{}%password%{}",
        page.get(..at).unwrap(),
        page.get(end..).unwrap()
    )
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
    let users = tokio::task::spawn_blocking({
        let (site, handle) = (Arc::clone(&site), handle.clone());
        move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            let path = "/hs-admin/users";
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
    assert_eq!(users.status(), StatusCode::FORBIDDEN);
    // Over HTTPS the profile offers application passwords, not ported.
    let profile = tokio::task::spawn_blocking({
        let (site, handle) = (Arc::clone(&site), handle.clone());
        move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            let path = "/hs-admin/profile";
            screen(
                &admin,
                path,
                path,
                "https://h/hs-admin/profile",
                "https://h",
                &Browser::default(),
            )
        }
    })
    .await
    .unwrap();
    assert!(profile.is_none());
    let profile = tokio::task::spawn_blocking({
        let (site, handle) = (Arc::clone(&site), handle.clone());
        move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            let path = "/hs-admin/profile";
            screen(
                &admin,
                path,
                path,
                "http://h/hs-admin/profile",
                "http://h",
                &Browser::default(),
            )
        }
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(profile.status(), StatusCode::FORBIDDEN);
    let privacy = tokio::task::spawn_blocking({
        let (site, handle) = (Arc::clone(&site), handle.clone());
        move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            let path = "/hs-admin/options-privacy";
            screen(
                &admin,
                path,
                path,
                "http://h/hs-admin/options-privacy",
                "http://h",
                &Browser::default(),
            )
        }
    })
    .await
    .unwrap()
    .unwrap();
    assert_eq!(privacy.status(), StatusCode::FORBIDDEN);
    // Editing a user needs `edit_users`, adding one `create_users`; over
    // HTTPS the form offers application passwords.
    for (path, https, status) in [
        (
            "/hs-admin/user-edit?user_id=1",
            false,
            Some(StatusCode::INTERNAL_SERVER_ERROR),
        ),
        ("/hs-admin/user-edit?user_id=1", true, None),
        ("/hs-admin/user-new", false, Some(StatusCode::FORBIDDEN)),
    ] {
        let (site, handle) = (Arc::clone(&site), handle.clone());
        let response = tokio::task::spawn_blocking(move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            let origin = if https { "https://h" } else { "http://h" };
            let file = path.split_once('?').map_or(path, |(file, _)| file);
            let url = format!("{origin}{path}");
            screen(&admin, file, path, &url, origin, &Browser::default())
        })
        .await
        .unwrap();
        assert_eq!(response.map(|r| r.status()), status, "{path}");
    }
    let terms = tokio::task::spawn_blocking({
        let site = Arc::clone(&site);
        move || {
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
        }
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
    // The Settings screens refuse those who may not manage options.
    for path in [
        "/hs-admin/options-general",
        "/hs-admin/options-writing",
        "/hs-admin/options-reading",
    ] {
        let site = Arc::clone(&site);
        let handle = Handle::current();
        let response = tokio::task::spawn_blocking(move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            let url = format!("http://h{path}");
            let browser = Browser::default();
            screen(&admin, path, path, &url, "http://h", &browser)
        })
        .await
        .unwrap();
        assert_eq!(
            response.map(|r| r.status()),
            Some(StatusCode::FORBIDDEN),
            "{path}"
        );
    }
}

/// A Site Icon, and a static homepage with neither page chosen, which PHP
/// saves as the latest posts first, are not ported.
#[tokio::test(flavor = "multi_thread")]
async fn settings_not_ported() {
    let db = hyscribe_db::Db::connect(&hyscribe_testkit::fixture_database())
        .await
        .unwrap();
    let mut site = Site::load(db, None, None).await.unwrap();
    site.options.insert("site_icon".into(), b"5".to_vec());
    site.options
        .insert("show_on_front".into(), b"page".to_vec());
    let site = Arc::new(site);
    for path in ["/hs-admin/options-general", "/hs-admin/options-reading"] {
        let site = Arc::clone(&site);
        let handle = Handle::current();
        let response = tokio::task::spawn_blocking(move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            let url = format!("http://h{path}");
            let browser = Browser::default();
            screen(&admin, path, path, &url, "http://h", &browser)
        })
        .await
        .unwrap();
        assert!(response.is_none(), "{path}");
    }
    // Without `WP_HOME` and `WP_SITEURL` configured, neither is defined.
    let handle = Handle::current();
    let defined = tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, now());
        let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
        SettingsSite::defined(&admin, "WP_HOME") || SettingsSite::defined(&admin, "WP_SITEURL")
    })
    .await
    .unwrap();
    assert!(!defined);
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
        // Another user's, though they did not log in.
        assert!(first.user_can(1, "edit_users") && !first.user_can(99, "read"));
        let nickname = first.user_option_of(1, "nickname");
        assert_eq!(nickname.as_deref(), Some(&b"admin"[..]));
        assert_eq!(first.profile_user(1).unwrap().login, b"admin");
        assert_eq!(first.sessions(1), 0);
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
            assert!(admin.can_user("edit_user", 2));
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
            // The spam comment was approved before.
            assert!(admin.approved_before(3) && !admin.approved_before(2));
            assert!(admin.has_comments(&["", "comment"]) && !admin.has_comments(&["pingback"]));
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
            assert_eq!(admin.users_without_role("administrator"), [2, 3]);
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
            assert!(!subscriber.can_user("edit_user", 1));
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
