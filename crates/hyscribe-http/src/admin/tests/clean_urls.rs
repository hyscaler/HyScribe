// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin prints its screens' URLs without `.php` (ADR 0008's 2026-11-17
//! amendment), whatever the screen.

use super::*;

/// The links in `page` that name an hs-admin screen with `.php`: after the
/// folder, plain, JSON-escaped or URL-encoded, or as a relative link.
fn php_links(page: &str) -> Vec<String> {
    let before = [
        "/hs-admin/",
        "\\/hs-admin\\/",
        "%2Fhs-admin%2F",
        "href=\"",
        "href='",
        "action=\"",
        "action='",
    ];
    let mut found = Vec::new();
    let named = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    for (at, _) in page.match_indices(".php") {
        let head = page.get(..at).unwrap();
        let next = page.get(at + 4..).unwrap().chars().next();
        let ends = next.is_none_or(|c| !(named(c) || "./".contains(c)));
        let linked = before.iter().any(|b| {
            head.rfind(b)
                .and_then(|i| head.get(i + b.len()..))
                .is_some_and(|name| !name.is_empty() && name.chars().all(named))
        });
        if ends && linked {
            let start = page.floor_char_boundary(at.saturating_sub(40));
            found.push(page.get(start..at + 4).unwrap().to_string());
        }
    }
    found
}

#[test]
fn php_links_are_found() {
    let page =
        "<a href=\"edit.php?x\"> %2Fhs-admin%2Fusers.php%3F /hs-admin/a.php.b /hs-admin/css/a.php";
    assert_eq!(php_links(page).len(), 2);
}

/// Redirects name screens without `.php`, whatever referer they go back to.
#[test]
fn redirects_name_clean_screens() {
    let response = redirect(nocache(), b"http://h/hs-admin/edit.php?post_type=page");
    assert_eq!(location(&response), "http://h/hs-admin/edit?post_type=page");
}

/// A sweep over full screens of the default install: no link to a screen
/// keeps `.php`, and the `.php` form of each redirects to it.
#[tokio::test(flavor = "multi_thread")]
async fn screens_print_no_php_links() {
    let dir = tempfile::tempdir().unwrap();
    let (site, cookies) = logged_in("wordpress", dir.path()).await;
    let host = "127.0.0.1:8081";
    let mut failures = Vec::new();
    for (screen, old) in [
        ("", "index.php"),
        ("edit", "edit.php"),
        ("edit?post_type=page", "edit.php?post_type=page"),
        ("upload", "upload.php"),
        ("media-new", "media-new.php"),
        ("edit-comments", "edit-comments.php"),
        (
            "comment?action=editcomment&c=1",
            "comment.php?action=editcomment&c=1",
        ),
        (
            "edit-tags?taxonomy=category",
            "edit-tags.php?taxonomy=category",
        ),
        (
            "term?taxonomy=category&tag_ID=1",
            "term.php?taxonomy=category&tag_ID=1",
        ),
        ("themes", "themes.php"),
        ("users", "users.php"),
        ("user-new", "user-new.php"),
        ("profile", "profile.php"),
        ("tools", "tools.php"),
        ("import", "import.php"),
        ("export", "export.php"),
        ("options-general", "options-general.php"),
        ("options-writing", "options-writing.php"),
        ("options-reading", "options-reading.php"),
        ("options-discussion", "options-discussion.php"),
        ("options-media", "options-media.php"),
        ("options-permalink", "options-permalink.php"),
        ("options-privacy", "options-privacy.php"),
    ] {
        let response = get(&site, &format!("/hs-admin/{screen}"), host, &cookies).await;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let page = String::from_utf8(body.to_vec()).unwrap();
        if status != StatusCode::OK || !page.contains("</html>") {
            failures.push(format!("{screen}: {status}"));
        }
        failures.extend(
            php_links(&page)
                .into_iter()
                .map(|link| format!("{screen}: {link}")),
        );
        let response = get(&site, &format!("/hs-admin/{old}"), host, &cookies).await;
        let moved = response.status() == StatusCode::MOVED_PERMANENTLY;
        if !moved || location(&response) != format!("http://{host}/hs-admin/{screen}") {
            failures.push(format!("{old}: {}", response.status()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
