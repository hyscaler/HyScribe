// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

/// The administrator's redirects and refusals of
/// `reference/site-editor/golden.sh`, as PHP answered them; the HTTP route
/// sanitizes the one with a quote and a space.
#[test]
fn redirects_and_refusals_are_php_s() {
    let golden = include_str!("../../../hyscribe-http/tests/golden/site-editor.tsv");
    let rows = golden
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>());
    let mut cases = 0;
    for f in rows.filter(|f| f[1] == "administrator" && !f[0].contains("%27")) {
        let query = f[0].strip_prefix('?').unwrap();
        let uri = format!("/wp-admin/site-editor.php{}", f[0]);
        let expected = match f[2] {
            "302" => Answer::Redirect(f[3].into()),
            _ => Answer::Died("Invalid post type."),
        };
        assert_eq!(site_editor(query, &uri), Some(expected), "{}", f[0]);
        cases += 1;
    }
    assert_eq!(cases, 31);
}

/// The editor's page, and arguments that are not strings, are not ported.
#[test]
fn the_page_is_not_ported() {
    for query in [
        "",
        "p=/",
        "p=/&postType=post",
        "p=/&postType=nope&postId=",
        "p[]=/",
    ] {
        assert_eq!(
            site_editor(query, "/wp-admin/site-editor.php"),
            None,
            "{query}"
        );
    }
}

/// The settings `site-editor.php` adds are PHP's, in its order.
#[test]
fn custom_settings_are_php_s() {
    let golden = include_str!("../../tests/golden/site-editor.tsv");
    let settings = custom_settings("http://127.0.0.1:8081", b"10".to_vec());
    for (key, value) in settings.iter().filter(|(key, _)| *key != "styles") {
        let prefix = format!("se-first\tsettings.{key}\t");
        let php = golden.lines().find_map(|line| line.strip_prefix(&prefix));
        let ours = crate::page::json(value);
        let ours = if ours.len() > 2000 {
            format!(
                "sha256:{} {}",
                crate::media::tests::sha256(&ours),
                ours.len()
            )
        } else {
            ours
        };
        assert_eq!(Some(ours.as_str()), php, "{key}");
    }
    let keys: Vec<&str> = settings.iter().map(|(key, _)| *key).collect();
    let order: Vec<&str> = golden
        .lines()
        .filter_map(|line| line.strip_prefix("se-first\tsettings."))
        .filter_map(|line| line.split('\t').next())
        .skip_while(|key| *key != "siteUrl")
        .take(keys.len())
        .collect();
    assert_eq!(keys, order);
}
