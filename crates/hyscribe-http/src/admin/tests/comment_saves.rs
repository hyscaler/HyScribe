// SPDX-License-Identifier: AGPL-3.0-or-later

//! Replies, Quick Edit and the comment edit screen against
//! `reference/admin-views/golden.sh comments`, really writing.

use super::*;

/// The address the requests come from.
const IP: &str = "192.0.2.1";

/// `page` with the comments' submission times as the golden masks them.
fn dates_masked(page: &str) -> String {
    let mut out = String::new();
    let mut rest = page;
    while let Some(at) = rest.find(" at ") {
        // `YYYY/MM/DD at h:mm am`: ten characters before, up to eight after.
        let date = rest.get(at.saturating_sub(10)..at).unwrap_or_default();
        let time_end = rest
            .get(at + 4..)
            .and_then(|t| t.find('m'))
            .map(|n| at + 4 + n + 1);
        let shaped = date.len() == 10
            && date.bytes().enumerate().all(|(n, b)| {
                if n == 4 || n == 7 {
                    b == b'/'
                } else {
                    b.is_ascii_digit()
                }
            });
        match time_end.filter(|end| shaped && end - at <= 13) {
            Some(end) => {
                out.push_str(rest.get(..at - 10).unwrap());
                out.push_str("%date%");
                rest = rest.get(end..).unwrap();
            }
            None => {
                out.push_str(rest.get(..at + 4).unwrap());
                rest = rest.get(at + 4..).unwrap();
            }
        }
    }
    out + rest
}

/// The requests of `reference/admin-views/comments.list`, in order, over a
/// fresh copy of the default install in primary mode: each answer and page
/// is PHP's, each request reading the site as it is now.
#[tokio::test(flavor = "multi_thread")]
async fn comment_saves_are_php_s() {
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
    let golden = include_str!("../../../../hyscribe-admin/tests/golden/admin-views-comments.tsv");
    let cases = include_str!("../../../../../reference/admin-views/comments.list");
    let cases = cases.lines().filter(|line| !line.starts_with('#'));
    for (row, line) in golden.lines().zip(cases) {
        let [case, status, php_location, _, body] = row.split('\t').collect::<Vec<_>>()[..] else {
            panic!("{row}");
        };
        let [_, method, target, data, action, field, _] = line.split('\t').collect::<Vec<_>>()[..]
        else {
            panic!("{line}");
        };
        // A second apart, as the golden's requests, which the list sorts by.
        if case.starts_with("reply") {
            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        }
        let uri = format!("/hs-admin/{}", target.replace(".php", ""));
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri(&uri)
            .header(header::HOST, "127.0.0.1:8081")
            .header(header::COOKIE, &cookies)
            .extension(crate::Peer(IP.parse().unwrap()));
        let form = if method == "POST" {
            let nonce = match action {
                "-" => "bad".to_string(),
                action => session_nonce(&site, &cookies, action).await,
            };
            let data = data
                .replace("%2Fwp-admin%2F", "%2Fhs-admin%2F")
                .replace(".php", "");
            request = request.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
            format!("{data}&{field}={nonce}")
        } else {
            String::new()
        };
        let request = request.body(Body::from(form)).unwrap();
        let response = crate::live(Arc::clone(&site))
            .oneshot(request)
            .await
            .unwrap();
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
        let page = dates_masked(&page_of(response).await).replace(IP, "%ip%");
        // The edit screen's copy of the time now.
        let now = |page: &str| replaced(page, ("<input type=\"hidden\" id=\"cur_", " />"), "%now%");
        let page = now(&hyscribe_testkit::settled_page(&page));
        let php = String::from_utf8(hyscribe_testkit::unhex(body)).unwrap();
        let php = now(&hyscribe_testkit::settled_page(
            &hyscribe_testkit::hs_admin_page(&php),
        ));
        let php = with_values(&php, &page);
        let at = php
            .bytes()
            .zip(page.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let around = |text: &str| {
            let bytes = text.as_bytes();
            let part = bytes.get(at.saturating_sub(150)..(at + 150).min(bytes.len()));
            format!("{:?}", String::from_utf8_lossy(part.unwrap_or_default()))
        };
        assert!(
            php == page,
            "{case}:\n php: {}\nours: {}",
            around(&php),
            around(&page)
        );
    }
    assert_eq!(golden.lines().count(), 34);
    // A phone's browser is not sent to the author's name, as `wp_is_mobile()`.
    let phone = axum::http::Request::builder()
        .uri("/hs-admin/comment?action=editcomment&c=1")
        .header(header::HOST, "127.0.0.1:8081")
        .header(header::COOKIE, &cookies)
        .header(header::USER_AGENT, "Mozilla/5.0 (iPhone) Mobile")
        .header(
            header::REFERER,
            "http://127.0.0.1:8081/hs-admin/edit-comments",
        );
    let phone = crate::live(Arc::clone(&site))
        .oneshot(phone.body(Body::empty()).unwrap())
        .await;
    let page = page_of(phone.unwrap()).await;
    assert!(!page.contains("document.post.name.focus()"));
    // Its Update goes back where the browser came from.
    let back = "id=\"referredby\" value=\"http://127.0.0.1:8081/hs-admin/edit-comments\"";
    assert!(page.contains(back), "{page}");
    // What PHP cannot go on with, or answers with `comment_footer_die()`'s
    // page or a screen not ported, is not ported.
    let reply = session_nonce(&site, &cookies, "replyto-comment").await;
    let update = session_nonce(&site, &cookies, "update-comment_1").await;
    let trash = session_nonce(&site, &cookies, "delete-comment_4").await;
    let ajax = |data: &str| format!("{data}&_ajax_nonce-replyto-comment={reply}");
    let trashed = format!("action=delete-comment&id=4&trash=1&_ajax_nonce={trash}");
    let response = send(
        &site,
        "POST",
        "/hs-admin/admin-ajax",
        &cookies,
        Some(("application/x-www-form-urlencoded", &trashed)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    for (uri, data) in [
        (
            "admin-ajax",
            ajax("action=edit-comment&comment_ID=1&comment_post_ID=1&content=x&checkbox=0"),
        ),
        (
            "admin-ajax",
            ajax("action=edit-comment&comment_ID=1&content=x&checkbox=1&status[]=1"),
        ),
        (
            "admin-ajax",
            ajax("action=replyto-comment&comment_ID=1&comment_post_ID=1&content=x&mode=single"),
        ),
        (
            "admin-ajax",
            ajax("action=replyto-comment&comment_ID=1&comment_post_ID=1&content[]=x"),
        ),
        (
            "comment",
            format!(
                "action=editedcomment&comment_ID=1&comment_post_ID=1&referredby[]=x&_wpnonce={update}"
            ),
        ),
    ] {
        let form = ("application/x-www-form-urlencoded", data.as_str());
        let response = send(
            &site,
            "POST",
            &format!("/hs-admin/{uri}"),
            &cookies,
            Some(form),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{data}");
    }
    for query in [
        "action=editcomment&c[]=1",
        "action=editcomment&c=999",
        "action=editcomment&c=4",
        "action=editcomment",
        "action=editcomment&c=1&x=1",
        "action=approve&c=1",
    ] {
        let response = send(
            &site,
            "GET",
            &format!("/hs-admin/comment?{query}"),
            &cookies,
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_IMPLEMENTED, "{query}");
    }
}
