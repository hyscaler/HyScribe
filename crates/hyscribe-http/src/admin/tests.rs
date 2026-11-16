// SPDX-License-Identifier: AGPL-3.0-or-later

//! These tests read the default install fixture from a live MariaDB:
//! start it with `reference/default-install/db.sh up`.

use axum::body::Body;
use hyscribe_config::Secret;
use tower::ServiceExt;

use super::saves::tests::{AFTER, ANSWERS, CASES, asked, case_row, hs};
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

pub(super) fn location(response: &Response) -> &str {
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
    let response = get(&site, "/hs-admin/site-editor", "127.0.0.1:8081", &auth).await;
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
        "/hs-admin/edit-comments?same=999",
        "/hs-admin/edit-comments?approved[]=1",
        "/hs-admin/edit-comments?x=1",
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
        "/hs-admin/users?update[]=add",
        "/hs-admin/users?x=1",
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
    // After a save; a profile's referer is not ported.
    let page = page_of(
        get(
            &site,
            "/hs-admin/profile?updated=1&error=x",
            "127.0.0.1:8081",
            &cookies,
        )
        .await,
    )
    .await;
    assert!(page.contains("<p><strong>Profile updated.</strong></p></div><div class=\"notice notice-error\"><p></p></div>"));
    let path = "/hs-admin/user-new?update=add&user_id=1";
    let page = page_of(get(&site, path, "127.0.0.1:8081", &cookies).await).await;
    assert!(page.contains("class=\"notice is-dismissible updated\"><p>User added.</p></div>"));
    assert!(page.contains("var userProfileL10n = {\"user_id\":\"1\""));
    // The profile of the user asked for, with the referer kept.
    let path = "/hs-admin/profile?user_id=1&wp_http_referer=%2Fhs-admin%2Fusers";
    let page = page_of(get(&site, path, "127.0.0.1:8081", &cookies).await).await;
    assert!(page.contains("name=\"wp_http_referer\" value=\"/hs-admin/users\""));
    assert!(page.contains("\t\t\t\t\tProfile\t\t\t</h1>"));
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
    // Themes and the Tools screens, without their arguments and actions;
    // Plugins, Site Health and the site editor are not ported.
    let list = include_str!("../../../hyscribe-admin/tests/golden/appearance.tsv");
    let cron = site.option("cron");
    let message = hyscribe_admin::appearance::auto_update_message(cron.as_bytes(), now());
    for file in [
        "themes",
        "import",
        "export",
        "export-personal-data",
        "erase-personal-data",
    ] {
        let path = format!("/hs-admin/{file}");
        let page = page_of(get(&site, &path, "127.0.0.1:8081", &cookies).await).await;
        let row = list
            .lines()
            .find(|l| l.starts_with(&format!("default\t{file}.php\tadministrator\t")))
            .unwrap();
        let php = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
        let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap())
            .replace("%auto-update%", &message);
        assert_eq!(with_values(&php, &page), page, "{file}");
        let response = get(&site, &format!("{path}?x=1"), "127.0.0.1:8081", &auth).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{file}");
    }
    // The request lists searched, which finds none here.
    for file in ["export-personal-data", "erase-personal-data"] {
        let path = format!(
            "/hs-admin/{file}?s=nobody&filter-status=request-pending&orderby=requester&order=asc&paged=1"
        );
        let page = page_of(get(&site, &path, "127.0.0.1:8081", &cookies).await).await;
        assert!(page.contains("name=\"s\" value=\"nobody\""), "{file}");
        let path = format!("/hs-admin/{file}?s[]=nobody");
        let response = get(&site, &path, "127.0.0.1:8081", &cookies).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{file}");
    }
    for path in [
        "/hs-admin/plugins",
        "/hs-admin/site-health",
        "/hs-admin/site-editor",
        "/hs-admin/site-editor?p=%2Fpage",
    ] {
        let response = get(&site, path, "127.0.0.1:8081", &auth).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{path}");
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
        "/hs-admin/user-edit?user_id=1&updated[]=1",
        "/hs-admin/user-edit?user_id=1&x=1",
        "/hs-admin/user-edit?user_id[]=1",
        "/hs-admin/user-edit?user_id=%FF",
        "/hs-admin/user-new?update=add&x=1",
        "/hs-admin/user-new?update[]=add",
        "/hs-admin/user-new?update=%FF",
        "/hs-admin/options-writing?x",
        "/hs-admin/options-reading?settings-updated=1&x",
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
    // The lists as an action sends users back to them, with its notices; a
    // request that makes PHP fail part way is not ported.
    let notices = include_str!("../../../hyscribe-admin/tests/golden/notices.tsv");
    for row in notices.lines() {
        let f: Vec<&str> = row.split('\t').collect();
        let file = f[1].split('?').next().unwrap();
        let lists = [
            "edit.php",
            "edit-tags.php",
            "edit-comments.php",
            "users.php",
        ];
        if f[0] != "default" || f[2] != "administrator" || !lists.contains(&file) {
            continue;
        }
        let path = hyscribe_testkit::hs_admin_page(&format!("/wp-admin/{}", f[1]));
        let response = get(&site, &path, "127.0.0.1:8081", &cookies).await;
        let php = String::from_utf8(hyscribe_testkit::unhex(f[3])).unwrap();
        if php.contains("There has been a critical error") {
            assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{path}");
            continue;
        }
        let page = hyscribe_testkit::settled_page(&page_of(response).await);
        let php = hyscribe_testkit::settled_page(&hyscribe_testkit::hs_admin_page(&php));
        assert_eq!(with_values(&php, &page), page, "{path}");
    }
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
pub(crate) async fn logged_in(database: &str, dir: &std::path::Path) -> (Arc<Site>, String) {
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

/// Searches of the Theme Test Data's posts in `reference/posts-list/golden.sh`,
/// which find its password protected post for a logged in user.
#[tokio::test(flavor = "multi_thread")]
async fn searches_find_protected_posts() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress_ttd", dir.path()).await;
    let golden = include_str!("../../../hyscribe-admin/tests/golden/posts-list-ttd.tsv");
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let path = format!("/hs-admin/edit{}", f[0].trim_start_matches("default"));
        let page = page_of(get(&site, &path, "127.0.0.1:8081", &cookies).await).await;
        assert!(page.contains("id=\"post-1168\""), "{path}");
        assert_eq!(sha256(&masked_nonces(&page)), f[3], "{path}");
    }
}

/// The attachment edit screen of `reference/attachment-edit/golden.sh`
/// where the database holds no edit lock: a missing post is PHP's `404`;
/// an attachment, whose view would write the user's lock, another post
/// and any other argument answer 501, with a referer sent or not.
#[tokio::test(flavor = "multi_thread")]
async fn attachment_edit_screen_is_php_s() {
    let dir = tempfile::tempdir().unwrap();
    let golden = include_str!("../../../hyscribe-admin/tests/golden/attachment-edit.tsv");
    let missing = golden.lines().next().unwrap().split('\t').nth(3).unwrap();
    let missing = String::from_utf8(hyscribe_testkit::unhex(missing)).unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let response = get(
        &site,
        "/hs-admin/post?post=9999&action=edit",
        "127.0.0.1:8081",
        &cookies,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(format!("404\t{}", page_of(response).await), missing);
    let (site, cookies) = logged_in("wordpress_ttd", dir.path()).await;
    for path in [
        "/hs-admin/post?post=611&action=edit",
        "/hs-admin/post?post=555&action=edit",
        "/hs-admin/post?post=0&action=edit",
        "/hs-admin/post?post=611",
        "/hs-admin/post?post=611&action=edit&get-post-lock=1",
        "/hs-admin/post?post[]=611&action=edit",
    ] {
        let response = get(&site, path, "127.0.0.1:8081", &cookies).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{path}");
    }
    let request = axum::http::Request::builder()
        .uri("/hs-admin/post?post=611&action=edit")
        .header(header::HOST, "127.0.0.1:8081")
        .header(header::COOKIE, &cookies)
        .header(header::REFERER, "http://127.0.0.1:8081/hs-admin/upload")
        .body(Body::empty())
        .unwrap();
    let response = router(Arc::clone(&site)).oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}

/// The block editor's screens of `reference/post-edit/golden.sh`: PHP's
/// refusals and the redirect of attachments; the editor's page, which
/// writes the auto-draft or the edit lock, answers 501.
#[tokio::test(flavor = "multi_thread")]
async fn the_block_editor_s_refusals() {
    let dir = tempfile::tempdir().unwrap();
    let golden = include_str!("../../../hyscribe-admin/tests/golden/post-edit.tsv");
    let invalid = golden
        .lines()
        .find(|line| line.starts_with("default\tpost-new.php?post_type=nope\tadministrator\t"))
        .unwrap();
    let invalid = hyscribe_testkit::unhex(invalid.rsplit('\t').next().unwrap());
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let response = get(
        &site,
        "/hs-admin/post-new?post_type=nope",
        "127.0.0.1:8081",
        &cookies,
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let page = page_of(response).await;
    assert_eq!(format!("500\t{page}").into_bytes(), invalid);
    let response = get(
        &site,
        "/hs-admin/post-new?post_type=attachment",
        "127.0.0.1:8081",
        &cookies,
    )
    .await;
    assert_eq!(response.status(), StatusCode::FOUND);
    let location = &response.headers()[header::LOCATION];
    assert_eq!(location, "http://127.0.0.1:8081/hs-admin/media-new");
    for path in [
        "/hs-admin/post-new",
        "/hs-admin/post-new?post_type=page",
        "/hs-admin/post?post=1&action=edit",
        "/hs-admin/post?post=2&action=edit",
    ] {
        let response = get(&site, path, "127.0.0.1:8081", &cookies).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{path}");
    }
}

/// `page` with its nonces, ten hexadecimal digits standing alone, and its
/// time masked, as the hashed goldens hold them.
pub(super) fn masked_nonces(page: &str) -> String {
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

pub(crate) async fn page_of(response: Response) -> String {
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

/// The site editor's refusals of `reference/site-editor/golden.sh`, whatever
/// the arguments.
#[tokio::test(flavor = "multi_thread")]
async fn the_site_editor_s_refusals() {
    let site = oracle_site().await;
    let handle = Handle::current();
    let golden = include_str!("../../tests/golden/site-editor.tsv");
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (site, handle, query) = (Arc::clone(&site), handle.clone(), f[0].to_string());
        let response = tokio::task::spawn_blocking(move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            let path = "/hs-admin/site-editor";
            let uri = format!("{path}{query}");
            let url = format!("http://h{uri}");
            screen(&admin, path, &uri, &url, "http://h", &Browser::default()).unwrap()
        })
        .await
        .unwrap();
        assert_eq!(response.status().as_str(), f[2], "{line}");
        let php = String::from_utf8(hyscribe_testkit::unhex(f[3])).unwrap();
        assert_eq!(page_of(response).await, php, "{}", f[0]);
    }
    assert_eq!(golden.lines().count(), 12);
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
    // No user may edit user 0's profile.
    assert_eq!(profile.status(), StatusCode::INTERNAL_SERVER_ERROR);
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
    // An item in the Trash is PHP's `409`.
    let conflict = tokio::task::spawn_blocking({
        let (site, handle) = (Arc::clone(&site), handle.clone());
        move || {
            let reads = LoginReads::new(&site, handle, now());
            let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
            served(&admin, Answer::Conflict("In the Trash.")).status()
        }
    })
    .await
    .unwrap();
    assert_eq!(conflict, StatusCode::CONFLICT);
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
        ("/hs-admin/user-new?update[]=add", false, None),
        ("/hs-admin/themes?broken[]=1", false, None),
        ("/hs-admin/themes", false, Some(StatusCode::FORBIDDEN)),
        ("/hs-admin/import", false, Some(StatusCode::FORBIDDEN)),
        ("/hs-admin/export", false, Some(StatusCode::FORBIDDEN)),
        ("/hs-admin/plugins", false, Some(StatusCode::FORBIDDEN)),
        ("/hs-admin/site-health", false, Some(StatusCode::FORBIDDEN)),
        (
            "/hs-admin/export-personal-data",
            false,
            Some(StatusCode::FORBIDDEN),
        ),
        (
            "/hs-admin/erase-personal-data",
            false,
            Some(StatusCode::FORBIDDEN),
        ),
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

/// The Themes screen with an active theme the site does not have, and with
/// `broken`, as `reference/appearance/golden.sh` recorded them, without the
/// write PHP makes.
#[tokio::test(flavor = "multi_thread")]
async fn a_missing_theme_is_php_s() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let host = "127.0.0.1:8081";
    let golden = include_str!("../../../hyscribe-admin/tests/golden/appearance.tsv");
    let cron = site.option("cron");
    let message = hyscribe_admin::appearance::auto_update_message(cron.as_bytes(), now());
    let php = |variant: &str| {
        let row = golden
            .lines()
            .find(|l| l.starts_with(&format!("{variant}\tthemes.php\tadministrator\t")))
            .unwrap();
        let php = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
        hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap())
            .replace("%auto-update%", &message)
    };
    let page = page_of(get(&site, "/hs-admin/themes?broken=1", host, &cookies).await).await;
    assert_eq!(with_values(&php("g9broken"), &page), page);
    let mut gone = Arc::try_unwrap(site).ok().unwrap();
    for name in ["stylesheet", "template"] {
        gone.options.insert(name.into(), b"gone".to_vec());
    }
    let gone = Arc::new(gone);
    let page = page_of(get(&gone, "/hs-admin/themes", host, &cookies).await).await;
    assert_eq!(with_values(&php("g9gone"), &page), page);
    let response = get(&gone, "/hs-admin/themes?broken=1&x=1", host, &cookies).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    // A missing child of the bundled theme, with its parent's fonts.
    let mut kid = Arc::try_unwrap(gone).ok().unwrap();
    kid.options.insert("stylesheet".into(), b"kid".to_vec());
    kid.options
        .insert("template".into(), b"twentytwentyfive".to_vec());
    let kid = Arc::new(kid);
    let page = page_of(get(&kid, "/hs-admin/themes", host, &cookies).await).await;
    assert_eq!(with_values(&php("j9child"), &page), page);
}

/// Paged comments link to their page, on the comments list and the
/// dashboard, as `reference/comments-list/golden.sh` and
/// `reference/dashboard/golden.sh` recorded them.
#[tokio::test(flavor = "multi_thread")]
async fn comment_links_name_their_page() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let mut paged = Arc::try_unwrap(site).ok().unwrap();
    for name in ["page_comments", "comments_per_page"] {
        paged.options.insert(name.into(), b"1".to_vec());
    }
    let paged = Arc::new(paged);
    let host = "127.0.0.1:8081";
    let page = page_of(get(&paged, "/hs-admin/edit-comments", host, &cookies).await).await;
    let list = include_str!("../../../hyscribe-admin/tests/golden/comments-list.tsv");
    let row = list
        .lines()
        .find(|l| l.starts_with("g9pages\tadministrator\t"))
        .unwrap();
    let php = hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap());
    let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap());
    let (php, page) = (
        hyscribe_testkit::settled_page(&php),
        hyscribe_testkit::settled_page(&page),
    );
    assert_eq!(with_values(&php, &page), page);
    let dashboard = page_of(get(&paged, "/hs-admin/", host, &cookies).await).await;
    assert!(dashboard.contains("href=\"http://127.0.0.1:8081/?p=1&#038;cpage=1#comment-1\""));
}

/// The dashboard's `welcome` saves for a user who may see the panel, so it
/// answers 501 only for them; the menu folds from the settings cookie.
#[tokio::test(flavor = "multi_thread")]
async fn dashboard_arguments_and_folding() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let host = "127.0.0.1:8081";
    let welcome = get(&site, "/hs-admin/?welcome=0", host, &cookies).await;
    assert_eq!(welcome.status(), StatusCode::NOT_IMPLEMENTED);
    let folded = format!("{cookies}; wp-settings-1=mfold%3Df");
    let edit = get(&site, "/hs-admin/?edit=dashboard_primary", host, &folded).await;
    assert_eq!(edit.status(), StatusCode::OK);
    assert!(
        page_of(edit)
            .await
            .contains(" index-php folded auto-fold admin-bar ")
    );
    let handle = Handle::current();
    let shown = tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, now());
        let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
        let (path, uri) = ("/hs-admin/", "/hs-admin/?welcome=0");
        let browser = Browser::default();
        screen(
            &admin,
            path,
            uri,
            "http://h/hs-admin/",
            "http://h",
            &browser,
        )
        .is_some()
    })
    .await
    .unwrap();
    assert!(shown);
}

/// A Site Icon and a static homepage with neither page chosen, which PHP
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

/// Over the Theme Test Data fixture's content: the Edit link to a restored
/// post of a type not ported (attachment 611), and another user's saves.
#[tokio::test(flavor = "multi_thread")]
async fn notices_over_more_content() {
    let mut config = hyscribe_testkit::fixture_database();
    config.name = "wordpress_ttd".into();
    let db = hyscribe_db::Db::connect(&config)
        .await
        .expect(hyscribe_testkit::NO_FIXTURE_DATABASE);
    let site = Arc::new(Site::load(db, None, None).await.unwrap());
    let handle = Handle::current();
    let (restored, saved) = tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, now());
        // Found as a login finds them, which loads their capabilities.
        let user = LoginSite::user_by_login(&reads, b"admin").unwrap();
        let user = reads.reads.wait(site.db.user(user.id)).unwrap();
        let admin = Admin::new(&reads, &site, user);
        let served = |path: &str, query: &str| {
            let uri = format!("{path}?{query}");
            let url = format!("http://h{uri}");
            screen(&admin, path, &uri, &url, "http://h", &Browser::default())
        };
        let restored = |id: u64| {
            let query = format!("untrashed=1&ids={id}");
            served("/hs-admin/edit", &query).map(|response| response.status())
        };
        let query = "user_id=2&updated=1&wp_http_referer=%2Fhs-admin%2Fusers%3Frole%3Deditor";
        (
            (restored(611), restored(1)),
            served("/hs-admin/user-edit", query).unwrap(),
        )
    })
    .await
    .unwrap();
    assert_eq!(restored, (None, Some(StatusCode::OK)));
    let page = page_of(saved).await;
    assert!(page.contains("<p><a href=\"/hs-admin/users?role=editor\">&larr; Go to Users</a></p>"));
}

/// A save sends the screen back with `settings-updated` (`updated` from
/// `options.php`'s email confirmation), which shows the notices it kept.
#[tokio::test(flavor = "multi_thread")]
async fn settings_after_a_save() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let mut site = Arc::try_unwrap(site).ok().unwrap();
    let kept = b"a:1:{i:0;a:4:{s:7:\"setting\";s:7:\"general\";s:4:\"code\";s:16:\"settings_updated\";s:7:\"message\";s:15:\"Settings saved.\";s:4:\"type\";s:7:\"success\";}}";
    site.options
        .insert("_transient_settings_errors".into(), kept.to_vec());
    let site = Arc::new(site);
    for (query, notice) in [
        ("?settings-updated=true", Some(true)),
        ("?settings-updated=0", Some(false)),
        ("?updated=true", Some(false)),
        ("", Some(false)),
        ("?settings-updated=true&x", None),
    ] {
        for file in ["options-writing", "options-media"] {
            let uri = format!("/hs-admin/{file}{query}");
            let response = send(&site, "GET", &uri, &cookies, None).await;
            let shown = match response.status() {
                StatusCode::OK => Some(page_of(response).await.contains("Settings saved.")),
                _ => None,
            };
            assert_eq!(shown, notice, "{file}{query}");
        }
    }
}

/// The terms list reads its taxonomy, page, search and order.
#[test]
fn the_terms_list_reads_its_taxonomy() {
    let term_args = |uri: &str| {
        let query = uri.split_once('?').map_or("", |(_, query)| query);
        term_args(query)
    };
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
    for uri in ["/hs-admin/", "/hs-admin/admin-ajax?action=x"] {
        let response = get(&site, uri, "127.0.0.1:8081", &cookie).await;
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
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
        assert_eq!(first.post_authors("post"), [1]);
        assert!(first.post_authors("attachment").is_empty());
        // Another post's slug, in any case, as the database compares it.
        assert!(first.slug_taken(b"Hello-World", 2) && !first.slug_taken(b"hello-world", 1));
        let named = first.named_status_counts("post", "hello-world");
        assert_eq!(named, [("publish".to_string(), 1)]);
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
    // An owner who lets administrators post unfiltered HTML.
    let open = hyscribe_config::Constants {
        disallow_unfiltered_html: false,
        allow_unfiltered_uploads: true,
        ..hyscribe_config::Constants::default()
    };
    let ttd = ttd.with_constants(open);
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
            assert!(admin.can_for("edit_user", 2));
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
            assert_eq!(admin.comment_post(2), Some(0));
            assert_eq!(admin.comment_post(99), None);
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
            assert!(admin.can_for("edit_term", term.id) && !admin.can_for("delete_term", term.id));
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
            assert_eq!(saves::Acting::constants(admin), open);
            assert!(!admin.constant("DISALLOW_UNFILTERED_HTML"));
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
            assert_eq!(admin.userdata(2).unwrap().display_name, b"Theme Buster");
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
            assert!(!subscriber.can_for("edit_user", 1));
            assert!(subscriber.recent_posts("future").is_empty());
            assert_eq!(
                PageSite::user_option(subscriber, "admin_color").as_deref(),
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
            "/hs-admin/edit?trashed=%FF",
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
        // `skipped` is accepted and has no notice.
        let acted = list_args(&admin, "/hs-admin/edit?ids=2&skipped=1&trashed=3").unwrap();
        let trashed = acted.counts.get(3).cloned().flatten();
        assert_eq!((acted.ids, trashed), (Some("2".into()), Some("3".into())));
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

/// `method` on `uri` with `cookie` and a body of `content_type`.
pub(crate) async fn send(
    site: &Arc<Site>,
    method: &str,
    uri: &str,
    cookie: &str,
    form: Option<(&str, &str)>,
) -> Response {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(uri)
        .header(header::HOST, "127.0.0.1:8081")
        .header(header::COOKIE, cookie);
    let body = match form {
        Some((kind, body)) => {
            request = request.header(header::CONTENT_TYPE, kind);
            body.to_string()
        }
        None => String::new(),
    };
    let request = request.body(Body::from(body)).unwrap();
    router(Arc::clone(site)).oneshot(request).await.unwrap()
}

/// The nonce of `action` for the session of the logged in cookie in `cookies`.
pub(crate) async fn session_nonce(site: &Arc<Site>, cookies: &str, action: &str) -> String {
    let auth = cookies
        .split("; ")
        .find(|cookie| cookie.starts_with("hyscribe_logged_in_"))
        .unwrap();
    let value = hyscribe_phpser::urldecode(auth.split_once('=').unwrap().1.as_bytes());
    let token = hyscribe_auth::cookie::parse_auth_cookie(&value)
        .unwrap()
        .token
        .to_vec();
    let (site, handle, action) = (Arc::clone(site), Handle::current(), action.to_string());
    tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&site, handle, now());
        reads.act_as(1, token);
        LoginSite::nonce(&reads, &action)
    })
    .await
    .unwrap()
}

/// An action is refused as PHP refuses it until its checks pass, then
/// answered as shadow mode answers a write; a POST whose body only PHP
/// reads is not answered.
#[tokio::test(flavor = "multi_thread")]
async fn actions_are_checked_first() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let trash = "/hs-admin/edit?action=trash&post[]=1";
    let response = send(&site, "GET", trash, &cookies, None).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let page = page_of(response).await;
    assert!(page.contains("<title>An error occurred.</title>"), "{page}");
    assert!(page.contains("The link you followed has expired."));
    let nonce = session_nonce(&site, &cookies, "bulk-posts").await;
    let valid = format!("{trash}&_wpnonce={nonce}");
    let response = send(&site, "GET", &valid, &cookies, None).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let form = "application/x-www-form-urlencoded";
    let body = format!("action=trash&post[]=1&_wpnonce={nonce}");
    let response = send(
        &site,
        "POST",
        "/hs-admin/edit",
        &cookies,
        Some((form, &body)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = send(
        &site,
        "POST",
        "/hs-admin/edit",
        &cookies,
        Some((form, "x=1")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let json = Some(("application/json", "{\"action\":\"trash\"}"));
    let response = send(&site, "POST", "/hs-admin/edit?action=trash", &cookies, json).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let empty = Some((form, ""));
    let response = send(&site, "POST", "/hs-admin/edit", &cookies, empty).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    // An empty body is an empty `$_POST`: the query's action is checked.
    let response = send(&site, "POST", trash, &cookies, empty).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// `admin-ajax.php` answers who is not logged in itself, and reads the
/// logged in cookie as well as the auth cookie.
#[tokio::test(flavor = "multi_thread")]
async fn ajax_actions_are_checked_first() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let ajax = "/hs-admin/admin-ajax";
    let form = "application/x-www-form-urlencoded";
    let response = send(&site, "POST", ajax, "", Some((form, "action=delete-tag"))).await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(page_of(response).await, "0");
    let response = send(&site, "POST", ajax, "", Some((form, "action=heartbeat"))).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let logged_in = cookies
        .split("; ")
        .find(|cookie| cookie.contains("logged_in"))
        .unwrap();
    let response = send(
        &site,
        "POST",
        ajax,
        logged_in,
        Some((form, "action=delete-tag")),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(page_of(response).await, "-1");
    let nonce = session_nonce(&site, &cookies, "save-color-scheme").await;
    let body = format!("action=save-user-color-scheme&color_scheme=x&nonce={nonce}");
    let response = send(&site, "POST", ajax, &cookies, Some((form, &body))).await;
    assert_eq!(response.status(), StatusCode::OK);
    let kind = response
        .headers()
        .get(header::CONTENT_TYPE)
        .unwrap()
        .clone();
    assert_eq!(kind, "application/json; charset=UTF-8");
    assert_eq!(page_of(response).await, "{\"success\":false}");
    // Without a writer, the read only site's `wp_die()` of an Ajax request.
    let body = format!("action=save-user-color-scheme&color_scheme=ocean&nonce={nonce}");
    let response = send(&site, "POST", ajax, &cookies, Some((form, &body))).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(page_of(response).await, READ_ONLY);
}

/// The checks read the referer, comments and roles from the site.
#[tokio::test(flavor = "multi_thread")]
async fn actions_read_the_site() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let request = axum::http::Request::builder()
        .uri("/hs-admin/edit?action=trash")
        .header(header::HOST, "127.0.0.1:8081")
        .header(header::COOKIE, &cookies)
        .header(
            header::REFERER,
            "http://127.0.0.1:8081/hs-admin/edit?updated=1",
        );
    let response = router(Arc::clone(&site))
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let page = page_of(response).await;
    assert!(
        page.contains("<a href=\"http://127.0.0.1:8081/hs-admin/edit\">Please try again.</a>"),
        "{page}"
    );
    let nonce = session_nonce(&site, &cookies, "bulk-users").await;
    let promote = format!("/hs-admin/users?changeit=1&new_role=king&users[]=1&_wpnonce={nonce}");
    let response = send(&site, "GET", &promote, &cookies, None).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    // Past its checks, in shadow mode: the site is read only.
    let promote = promote.replace("king", "editor");
    let response = send(&site, "GET", &promote, &cookies, None).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let page = page_of(response).await;
    let read_only = hyscribe_admin::die::page(true, "WordPress &rsaquo; Error", READ_ONLY);
    assert_eq!(page.into_bytes(), read_only);
    let form = Some((
        "application/x-www-form-urlencoded",
        "action=dim-comment&id=1",
    ));
    let response = send(&site, "POST", "/hs-admin/admin-ajax", &cookies, form).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let form = Some((
        "application/x-www-form-urlencoded",
        "action=dim-comment&id=99",
    ));
    let response = send(&site, "POST", "/hs-admin/admin-ajax", &cookies, form).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(page_of(response).await, READ_ONLY);
    // The Customizer is not ported, whatever the action.
    for body in [
        "action=customize_save&wp_customize=on",
        "action=delete-tag&tag_ID=1&wp_customize=on",
    ] {
        let form = Some(("application/x-www-form-urlencoded", body));
        let response = send(&site, "POST", "/hs-admin/admin-ajax", &cookies, form).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{body}");
        assert!(page_of(response).await.is_empty(), "{body}");
    }
    // An action past its checks with no handler.
    let ajax = "/hs-admin/admin-ajax?action=rest-nonce";
    let response = send(&site, "GET", ajax, &cookies, None).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let own = format!("/hs-admin/users?changeit=1&new_role=none&users[]=1&_wpnonce={nonce}");
    let response = send(&site, "GET", &own, &cookies, None).await;
    assert!(
        page_of(response)
            .await
            .contains("cannot remove your own role")
    );
    let response = send(&site, "GET", "/hs-admin/users", &cookies, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let wrong = "/hs-admin/edit?action=trash&post[]=1&_wpnonce=0123456789";
    let response = send(&site, "GET", wrong, &cookies, None).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let posts = session_nonce(&site, &cookies, "bulk-posts").await;
    let missing = format!("/hs-admin/edit?action=trash&post[]=999&_wpnonce={posts}");
    let response = send(&site, "GET", &missing, &cookies, None).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

/// A site icon from the database: in the admin bar and head, and on the
/// login screen.
#[tokio::test(flavor = "multi_thread")]
async fn the_site_icon_is_read_from_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress_ttd", dir.path()).await;
    let mut icon = Arc::try_unwrap(site).ok().unwrap();
    icon.options.insert("site_icon".into(), b"611".to_vec());
    let icon = Arc::new(icon);
    let uploads = "http://127.0.0.1:8081/wp-content/uploads/";
    let page = page_of(get(&icon, "/hs-admin/tools", "127.0.0.1:8081", &cookies).await).await;
    assert!(page.contains(&format!("<img class=\"site-icon\" src=\"{uploads}")));
    assert!(page.contains(&format!("<link rel=\"icon\" href=\"{uploads}")));
    let page = page_of(get(&icon, "/hs-login", "127.0.0.1:8081", "").await).await;
    assert!(page.contains(&format!("<link rel=\"apple-touch-icon\" href=\"{uploads}")));
}

/// A writer as the fixture's read only user, with `prefix`: its actions
/// run, and a write would fail, so a test never changes the fixture.
pub(crate) async fn unwritable(site: Arc<Site>, prefix: &str) -> Arc<Site> {
    let config = hyscribe_config::Database {
        user: "wordpress_read".into(),
        password: Secret::new("reference-only"),
        prefix: hyscribe_config::Prefix::new(prefix).unwrap(),
        ..hyscribe_testkit::fixture_database()
    };
    let writer = hyscribe_db::Writer::open(&config, hyscribe_config::Mode::Primary).await;
    let site = Arc::try_unwrap(site).ok().unwrap();
    Arc::new(site.with_writer(writer.unwrap()))
}

/// The users list's cases of `reference/admin-saves/golden.sh` whose rows
/// and answers the write oracle compares, in `saves`: they write, or need
/// rows the fixture lacks.
const ORACLE_CASES: [&str; 4] = [
    "users-promote",
    "users-dodelete",
    "users-dodelete-delete",
    "users-resetpassword",
];

/// The other cases that write nothing the read only user cannot.
const HTTP_CASES: [&str; 6] = [
    "user-new-bad-role",
    "destroy-sessions-self",
    "destroy-sessions-missing",
    "send-password-reset-missing",
    "options-adminhash-bad",
    "options-dismiss",
];

/// Past their checks, the actions of `reference/admin-saves/golden.sh`
/// that write nothing answer as PHP's; what PHP cannot go on with is not
/// ported, and a failed write is the database error page.
#[tokio::test(flavor = "multi_thread")]
async fn actions_answer_as_php_once_checked() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let site = unwritable(site, "wp_").await;
    let nonce = session_nonce(&site, &cookies, "bulk-users").await;
    // The cases that write nothing the read only user cannot.
    let rows = ANSWERS.lines().filter(|row| {
        let case = row.split('\t').next().unwrap_or_default();
        case.starts_with("users-") && !ORACLE_CASES.contains(&case)
            || case.starts_with("comments-")
            || HTTP_CASES.contains(&case)
    });
    for row in rows {
        let [case, status, php_location, body] = row.split('\t').collect::<Vec<_>>()[..] else {
            panic!("{row}");
        };
        let [_, method, target, data, action, field, ..] = case_row(CASES, case).unwrap()[..]
        else {
            panic!("{case}");
        };
        let nonce = session_nonce(&site, &cookies, action).await;
        let (data, uri) = (hs(data), format!("/hs-admin/{}", hs(target)));
        let response = if method == "POST" {
            let form = (
                "application/x-www-form-urlencoded",
                &*format!("{data}&{field}={nonce}"),
            );
            send(&site, "POST", &uri, &cookies, Some(form)).await
        } else {
            let uri = format!("{uri}&{field}={nonce}");
            send(&site, "GET", &uri, &cookies, None).await
        };
        assert_eq!(response.status().as_str(), status, "{case}");
        let ours = response.headers().get(header::LOCATION);
        let ours = ours.map_or("", |to| to.to_str().unwrap());
        assert_eq!(ours, hs(php_location), "{case}");
        let php = hyscribe_testkit::unhex(body);
        if php.starts_with(b"{") {
            let kind = response.headers().get(header::CONTENT_TYPE).unwrap();
            assert_eq!(kind, "application/json; charset=UTF-8", "{case}");
        }
        assert_eq!(page_of(response).await.into_bytes(), php, "{case}");
    }
    // The current session outlasts the others' end.
    let response = send(&site, "GET", "/hs-admin/profile", &cookies, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let promote = format!("/hs-admin/users?changeit=1&new_role=editor&_wpnonce={nonce}");
    for users in [
        "users[]=1099511627776",
        "users[]=-1",
        "users[]=1&wp_http_referer[]=x",
    ] {
        let response = send(&site, "GET", &format!("{promote}&{users}"), &cookies, None).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{users}");
    }
    // Arrays PHP cannot read, and `comment_footer_die()`'s page.
    let comments = session_nonce(&site, &cookies, "bulk-comments").await;
    let missing = session_nonce(&site, &cookies, "approve-comment_999").await;
    let comment = session_nonce(&site, &cookies, "delete-comment_1").await;
    for uri in [
        format!(
            "edit-comments?delete_all=1&comment_status[]=trash&pagegen_timestamp=1&_wpnonce={comments}"
        ),
        format!("edit-comments?action=approve&ids[]=1&_wpnonce={comments}"),
        format!("edit-comments?delete_all=1&pagegen_timestamp[]=1&_wpnonce={comments}"),
        format!("comment?action=approvecomment&c=999&_wpnonce={missing}"),
        format!("comment?action=spamcomment&c=1&_wp_original_http_referer[]=x&_wpnonce={comment}"),
    ] {
        let response = send(&site, "GET", &format!("/hs-admin/{uri}"), &cookies, None).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{uri}");
    }
    // An action whose write has no handler.
    let nonce = session_nonce(&site, &cookies, "add-user").await;
    let add = format!("action=adduser&email=x%40example.test&_wpnonce_add-user={nonce}");
    let form = ("application/x-www-form-urlencoded", add.as_str());
    let response = send(&site, "POST", "/hs-admin/user-new", &cookies, Some(form)).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    // A database whose `db_version` cannot be read takes no write.
    let site = unwritable(site, "missing_").await;
    let response = send(
        &site,
        "GET",
        &format!("{promote}&users[]=1"),
        &cookies,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let page = page_of(response).await;
    assert!(
        page.contains("Error establishing a database connection"),
        "{page}"
    );
}

/// Past its checks, a deletion asks for its confirmation as PHP does (the
/// page of `reference/users-list/golden.sh`), or goes back to the list
/// with no user; arguments not ported, a referer PHP cannot read, a user
/// missing and a POST are not answered.
#[tokio::test(flavor = "multi_thread")]
async fn deletions_are_confirmed_first() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let nonce = session_nonce(&site, &cookies, "bulk-users").await;
    let base = format!("/hs-admin/users?action=delete&user=1&_wpnonce={nonce}");
    let response = send(&site, "GET", &base, &cookies, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    let golden = include_str!("../../../hyscribe-admin/tests/golden/users-delete.tsv");
    let row = golden
        .lines()
        .find(|row| row.starts_with("users?action=delete&user=1&"));
    let php = hyscribe_testkit::unhex(row.unwrap().split('\t').nth(2).unwrap());
    let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap());
    let page = page_of(response).await;
    assert_eq!(with_values(&php, &page), page);
    let referer = format!("{base}&wp_http_referer=%2Fhs-admin%2Fusers%3Frole%3Dauthor");
    let page = page_of(send(&site, "GET", &referer, &cookies, None).await).await;
    assert!(page.contains("name=\"wp_http_referer\" value=\"/hs-admin/users?role=author\""));
    let back = format!("/hs-admin/users?action=delete&_wpnonce={nonce}");
    let response = send(&site, "GET", &back, &cookies, None).await;
    assert_eq!(location(&response), "users");
    for more in ["&bogus=1", "&wp_http_referer[]=x", "&users[]=999"] {
        let response = send(&site, "GET", &format!("{base}{more}"), &cookies, None).await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{more}");
    }
    let form = ("application/x-www-form-urlencoded", "x=1");
    let response = send(&site, "POST", &base, &cookies, Some(form)).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
}

/// Another user's sessions all end, and PHP's answer names them.
#[tokio::test(flavor = "multi_thread")]
async fn another_user_s_sessions_end() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress_ttd", dir.path()).await;
    let sessions = hyscribe_auth::sessions::Sessions::new(dir.path());
    let origin = hyscribe_auth::sessions::Origin {
        ip: b"",
        ua: b"",
        login: now(),
    };
    let token = sessions.create(2, now() + 60, &origin).unwrap();
    let nonce = session_nonce(&site, &cookies, "update-user_2").await;
    let body = format!("action=destroy-sessions&user_id=2&nonce={nonce}");
    let form = ("application/x-www-form-urlencoded", body.as_str());
    let response = send(&site, "POST", "/hs-admin/admin-ajax", &cookies, Some(form)).await;
    let php = hyscribe_testkit::unhex(case_row(ANSWERS, "destroy-sessions-other").unwrap()[3]);
    let php = String::from_utf8(php)
        .unwrap()
        .replace("other", "Theme Buster");
    assert_eq!(page_of(response).await, php);
    assert!(!sessions.verify(2, token.as_bytes(), now()));
    // The confirmation of their deletion asks what becomes of their posts.
    let nonce = session_nonce(&site, &cookies, "bulk-users").await;
    let delete = format!("/hs-admin/users?action=delete&users=2&_wpnonce={nonce}");
    let page = page_of(send(&site, "GET", &delete, &cookies, None).await).await;
    assert!(page.contains("<strong>themedemos</strong> (ID #2): What should be done"));
}

/// The fixture's administrator owns content, and no other user does.
#[tokio::test(flavor = "multi_thread")]
async fn content_has_its_owners() {
    let site = oracle_site().await;
    let handle = Handle::current();
    let owns = tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, now());
        let admin = Admin::new(&reads, &site, hyscribe_model::User::default());
        (admin.owns_content(1), admin.owns_content(2))
    });
    assert_eq!(owns.await.unwrap(), (true, false));
}

/// An answer sets the cookies given, a cookie no header can carry left out.
#[test]
fn answers_set_the_cookies_given() {
    let cookies = vec!["a=1; path=/".to_string(), "b=\n".to_string()];
    let response = with_cookies(not_ported(), cookies);
    let set: Vec<_> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .collect();
    assert_eq!(set, ["a=1; path=/"]);
}

/// Past their checks, the categories and tags actions of
/// `reference/admin-saves/golden.sh` that write nothing answer as PHP's, and
/// the screens after them are PHP's; the others are the write oracle's, in
/// `saves`.
#[tokio::test(flavor = "multi_thread")]
async fn term_actions_answer_as_php_once_checked() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let mut nonces = Vec::new();
    for action in ["add-tag", "bulk-tags", "update-tag_1", "taxinlineeditnonce"] {
        nonces.push((action, session_nonce(&site, &cookies, action).await));
    }
    let site = unwritable(site, "wp_").await;
    for case in [
        "terms-add-exists",
        "terms-add-empty",
        "terms-bulk-missing",
        "terms-edited-empty",
        "ajax-add-exists",
        "ajax-add-empty",
        "ajax-add-missing-parent",
        "ajax-add-invalid-name",
        "ajax-inline-empty",
    ] {
        let [_, _, target, data, action, field, ..] = case_row(CASES, case).unwrap()[..] else {
            panic!("{case}");
        };
        let nonce = &nonces.iter().find(|(a, _)| *a == action).unwrap().1;
        let form = (
            "application/x-www-form-urlencoded",
            &*format!("{}&{field}={nonce}", hs(data)),
        );
        let uri = format!("/hs-admin/{}", hs(target));
        let response = send(&site, "POST", &uri, &cookies, Some(form)).await;
        let php = case_row(ANSWERS, case).unwrap();
        assert_eq!(response.status().as_str(), php[1], "{case}");
        let to = response.headers().get(header::LOCATION);
        let to = to.map_or(String::new(), |to| to.to_str().unwrap().to_string());
        assert_eq!(to, hyscribe_testkit::hs_admin_page(php[2]), "{case}");
        let body = String::from_utf8(hyscribe_testkit::unhex(php[3])).unwrap();
        assert_eq!(page_of(response).await, body, "{case}");
        let Some(screen) = case_row(AFTER, case) else {
            continue;
        };
        let response = send(&site, "GET", &to, &cookies, None).await;
        assert_eq!(response.status().as_str(), screen[1], "{case}");
        let page = page_of(response).await;
        let php = String::from_utf8(hyscribe_testkit::unhex(screen[2])).unwrap();
        let php = hyscribe_testkit::hs_admin_page(&php);
        assert_eq!(page, with_values(&php, &page), "{case}");
    }
    // Not ported: the edit screen's other arguments, and the terms of a
    // taxonomy hyscribe-write does not write.
    let response = send(
        &site,
        "GET",
        "/hs-admin/term?taxonomy=category&x=1",
        &cookies,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    let add = &nonces[0].1;
    let menu = format!(
        "action=add-tag&taxonomy=nav_menu&screen=edit-nav_menu&tag-name=X&_wpnonce_add-tag={add}"
    );
    let form = ("application/x-www-form-urlencoded", &*menu);
    let response = send(&site, "POST", "/hs-admin/admin-ajax", &cookies, Some(form)).await;
    assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED);
    // A database whose `db_version` cannot be read takes no write.
    let site = unwritable(site, "missing_").await;
    let tag = format!(
        "action=add-tag&taxonomy=post_tag&screen=edit-post_tag&tag-name=X&_wpnonce_add-tag={add}"
    );
    let form = ("application/x-www-form-urlencoded", &*tag);
    let response = send(&site, "POST", "/hs-admin/admin-ajax", &cookies, Some(form)).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

/// Quick Edit's row is PHP's once committed, and the add form's answer is
/// the terms list's XML; in shadow mode the Ajax actions are read only.
#[tokio::test(flavor = "multi_thread")]
async fn term_ajax_answers_are_sent_once_committed() {
    let site = oracle_site().await;
    let php = hyscribe_testkit::unhex(case_row(ANSWERS, "ajax-inline-unchanged").unwrap()[3]);
    let php = hyscribe_testkit::hs_admin_page(&String::from_utf8(php).unwrap());
    let handle = Handle::current();
    let (row, added) = tokio::task::spawn_blocking(move || {
        let reads = LoginReads::new(&site, handle, now());
        let user = hyscribe_model::User {
            id: 1,
            ..hyscribe_model::User::default()
        };
        let admin = Admin::new(&reads, &site, user);
        let asked = Asked {
            home: &site.home,
            ..asked("/hs-admin/admin-ajax", &[], &[])
        };
        let category = Terms::named(b"category").unwrap();
        let term = admin.terms("category").into_iter().next().unwrap();
        let row = saves::ajax_served(&admin, &asked, saves::Ajax::TermRow(category, term.clone()));
        let added = hyscribe_admin::terms::Added {
            term,
            term_group: 0,
            term_taxonomy_id: 1,
        };
        let xml = hyscribe_admin::terms::term_added(&admin, category, &added, "");
        let answer = saves::ajax_served(&admin, &asked, saves::Ajax::TermAdded(category, added));
        (row, (answer, xml))
    })
    .await
    .unwrap();
    assert_eq!(page_of(row).await, php);
    let (answer, xml) = added;
    let kind = answer.headers().get(header::CONTENT_TYPE).unwrap();
    assert_eq!(kind, "text/xml; charset=UTF-8");
    assert_eq!(page_of(answer).await, xml);
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let nonce = session_nonce(&site, &cookies, "taxinlineeditnonce").await;
    let form =
        format!("action=inline-save-tax&taxonomy=category&tax_ID=1&name=X&_inline_edit={nonce}");
    let form = Some(("application/x-www-form-urlencoded", &*form));
    let response = send(&site, "POST", "/hs-admin/admin-ajax", &cookies, form).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(page_of(response).await, READ_ONLY);
}

/// A page view whose writes fail, before the page or after it, is the
/// database error page; one with nothing to write sets the cookies alone.
#[tokio::test(flavor = "multi_thread")]
async fn page_view_writes_that_fail() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let site = unwritable(site, "wp_").await;
    let host = "127.0.0.1:8081";
    let newer = format!("{cookies}; wp-settings-1=a%3D1; wp-settings-time-1=9999999999");
    for (uri, cookie) in [
        ("/hs-admin/", &cookies),
        ("/hs-admin/?welcome=0", &cookies),
        ("/hs-admin/tools", &newer),
    ] {
        let response = get(&site, uri, host, cookie).await;
        assert_eq!(
            response.status(),
            StatusCode::INTERNAL_SERVER_ERROR,
            "{uri}"
        );
        let page = page_of(response).await;
        assert!(
            page.contains("Error establishing a database connection"),
            "{uri}"
        );
    }
    // Only a list's mode and the dashboard's `welcome` save.
    for uri in [
        "/hs-admin/tools",
        "/hs-admin/tools?mode=excerpt",
        "/hs-admin/tools?welcome=0",
        "/hs-admin/edit?mode=",
    ] {
        let response = get(&site, uri, host, &cookies).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert!(set_cookies(&response).starts_with("wp-settings-1=deleted;"));
    }
    // The Quick Draft: a field PHP cannot trim and a post that is not an
    // auto-draft are not ported; the form after a bad nonce writes an auto-draft.
    let nonce = session_nonce(&site, &cookies, "add-post").await;
    let form = "application/x-www-form-urlencoded";
    for (body, status) in [
        ("post_ID=1&content[]=x", StatusCode::NOT_IMPLEMENTED),
        ("post_ID=1&content=x", StatusCode::NOT_IMPLEMENTED),
        (
            "post_ID=1&content=x&_wpnonce=bad",
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    ] {
        let body = format!("action=post-quickdraft-save&_wpnonce={nonce}&{body}");
        let response = send(
            &site,
            "POST",
            "/hs-admin/post",
            &cookies,
            Some((form, &body)),
        )
        .await;
        assert_eq!(response.status(), status, "{body}");
    }
    // A list's mode is not saved with a page that is not ported.
    let unported = get(
        &site,
        "/hs-admin/edit?mode=excerpt&unknown=1",
        host,
        &cookies,
    )
    .await;
    assert_eq!(unported.status(), StatusCode::NOT_IMPLEMENTED);
    // A writer that refuses the database's version begins no view, and is
    // not asked to where there is nothing to write.
    let site = unwritable(site, "missing_").await;
    let response = get(&site, "/hs-admin/", host, &cookies).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    for (uri, cookie, status) in [
        ("/hs-admin/tools", &cookies, StatusCode::OK),
        ("/hs-admin/edit?paged=9", &newer, StatusCode::FOUND),
    ] {
        assert_eq!(
            get(&site, uri, host, cookie).await.status(),
            status,
            "{uri}"
        );
    }
}

/// `text` with each run from `start` to the next `end` replaced by `with`.
fn replaced(text: &str, (start, end): (&str, &str), with: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some((at, to)) = rest
        .find(start)
        .and_then(|at| Some((at, at + rest.get(at..)?.find(end)? + end.len())))
    {
        out.push_str(rest.get(..at).unwrap());
        out.push_str(with);
        rest = rest.get(to..).unwrap();
    }
    out + rest
}

/// The Set-Cookie headers of `response` as `reference/admin-views/golden.sh`
/// masks them.
fn set_cookies(response: &Response) -> String {
    let cookies = response.headers().get_all(header::SET_COOKIE).iter();
    let cookies: Vec<String> = cookies
        .map(|cookie| {
            let cookie = cookie.to_str().unwrap();
            let cookie = replaced(
                cookie,
                ("wp-settings-time-1=", ";"),
                "wp-settings-time-1=%now%;",
            );
            let year = ("expires=", "; Max-Age=31536000");
            replaced(&cookie, year, "expires=%expires%; Max-Age=31536000")
        })
        .collect();
    cookies.join(" | ")
}

/// The requests of `reference/admin-views/golden.sh`, in order, in primary
/// mode over a fresh copy of the default install, really writing: each
/// answer, cookie and page is PHP's.
#[tokio::test(flavor = "multi_thread")]
async fn page_views_and_screen_state_write_as_php() {
    let _saves = SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..hyscribe_testkit::fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in(&db.name, dir.path()).await;
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary).await;
    let site = Arc::new(
        Arc::try_unwrap(site)
            .ok()
            .unwrap()
            .with_writer(writer.unwrap()),
    );
    let golden = include_str!("../../../hyscribe-admin/tests/golden/admin-views.tsv");
    let cases = include_str!("../../../../reference/admin-views/cases.list");
    let cases = cases.lines().filter(|line| !line.starts_with('#'));
    for (row, line) in golden.lines().zip(cases) {
        let [case, status, php_location, php_cookies, body] =
            row.split('\t').collect::<Vec<_>>()[..]
        else {
            panic!("{row}");
        };
        let [_, method, target, data, action, field, cookie] =
            line.split('\t').collect::<Vec<_>>()[..]
        else {
            panic!("{line}");
        };
        let uri = format!("/hs-admin/{}", target.replace(".php", ""));
        let jar = match cookie {
            "-" => cookies.clone(),
            cookie => format!("{cookies}; {cookie}"),
        };
        let response = if method == "POST" {
            let nonce = match action {
                "-" => "bad".to_string(),
                action => session_nonce(&site, &cookies, action).await,
            };
            let form = format!("{data}&{field}={nonce}");
            let form = ("application/x-www-form-urlencoded", form.as_str());
            send(&site, "POST", &uri, &jar, Some(form)).await
        } else {
            send(&site, "GET", &uri, &jar, None).await
        };
        assert_eq!(response.status().as_str(), status, "{case}");
        let ours = response.headers().get(header::LOCATION);
        let php_location = php_location
            .replace("/wp-admin/", "/hs-admin/")
            .replace(".php", "");
        assert_eq!(
            ours.map_or("", |l| l.to_str().unwrap()),
            php_location,
            "{case}"
        );
        assert_eq!(set_cookies(&response), php_cookies, "{case}");
        // `admin-ajax.php`'s answers are not pages: JSON, or text.
        if target == "admin-ajax.php" {
            let kind = response.headers().get(header::CONTENT_TYPE).unwrap();
            // The body is hex: `{` is `7b`.
            let json = body.starts_with("7b");
            assert_eq!(
                kind.to_str().unwrap().starts_with("application/json"),
                json,
                "{case}"
            );
            assert!(response.headers().contains_key("x-robots-tag"), "{case}");
        }
        let dates = ("<time datetime=\"", "</time>");
        let page = replaced(
            &page_of(response).await,
            dates,
            "<time datetime=\"%date%\">%date%</time>",
        );
        let page = hyscribe_testkit::settled_page(&page);
        let php = String::from_utf8(hyscribe_testkit::unhex(body)).unwrap();
        let php = hyscribe_testkit::settled_page(&hyscribe_testkit::hs_admin_page(&php));
        assert_eq!(with_values(&php, &page), page, "{case}");
    }
    assert_eq!(golden.lines().count(), 17);
}

/// One test at a time over `wordpress_saves`, which each reloads.
pub(crate) static SAVES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// In primary mode a Settings save's notice shows on the screen it sends
/// back, which deletes it, so a reload shows none; shadow mode only reads.
#[tokio::test(flavor = "multi_thread")]
async fn a_saved_notice_shows_once() {
    let _saves = SAVES.lock().await;
    let db = hyscribe_config::Database {
        name: "wordpress_saves".into(),
        ..hyscribe_testkit::fixture_database()
    };
    hyscribe_testkit::writes::load_default_install(&db)
        .await
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in(&db.name, dir.path()).await;
    let nonce = session_nonce(&site, &cookies, "writing-options").await;
    let writer = hyscribe_db::Writer::open(&db, hyscribe_config::Mode::Primary).await;
    let site = Arc::try_unwrap(site).ok().unwrap();
    let site = Arc::new(site.with_writer(writer.unwrap()));
    let form = format!(
        "option_page=writing&action=update&_wp_http_referer=%2Fhs-admin%2Foptions-writing&default_category=1&_wpnonce={nonce}"
    );
    let form = ("application/x-www-form-urlencoded", form.as_str());
    let saved = send(&site, "POST", "/hs-admin/options", &cookies, Some(form)).await;
    let back = location(&saved).to_string();
    assert_eq!(back, "/hs-admin/options-writing?settings-updated=true");
    let notice = "<p><strong>Settings saved.</strong></p>";
    // Other views leave the notice: a Settings screen not sent back, and a
    // page that writes for another reason.
    for other in ["/hs-admin/options-writing", "/hs-admin/edit?mode=excerpt"] {
        let viewed = send(&site, "GET", other, &cookies, None).await;
        assert_eq!(viewed.status(), StatusCode::OK, "{other}");
    }
    let first = page_of(send(&site, "GET", &back, &cookies, None).await).await;
    assert!(first.contains(notice));
    let again = page_of(send(&site, "GET", &back, &cookies, None).await).await;
    assert!(!again.contains(notice));
}

mod comment_saves;
