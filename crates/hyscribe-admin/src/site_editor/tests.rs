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
