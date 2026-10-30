// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;

/// The query titles of term archives in `terms.tsv` and of a time's
/// archive in `pages.tsv`, from
/// `reference/head/golden.sh`, over the terms of
/// `reference/default-install/terms.sql`.
#[test]
fn term_archive_titles_match_php() {
    let golden = include_str!("../../tests/golden/terms.tsv");
    let term = |id, name: &str, slug: &str| Term {
        id,
        name: name.into(),
        slug: slug.into(),
        ..Term::default()
    };
    let cases = [
        (
            "/?tag=foo",
            Archive::Term(Taxonomy::Tag, Some(term(2, "Foo", "foo"))),
        ),
        (
            "/?post_format=aside",
            Archive::Term(
                Taxonomy::Format,
                Some(term(3, "Aside", "post-format-aside")),
            ),
        ),
        (
            "/?post_format=,post-format-aside",
            Archive::Term(Taxonomy::Format, None),
        ),
    ];
    // A time's archive, from the default install's pages.
    let pages = include_str!("../../tests/golden/pages.tsv");
    let time = ("/?hour=8", Archive::Date);
    for (path, archive) in cases.into_iter().chain([time]) {
        let key = format!("{}\t", hyscribe_phpser::bin2hex(path.as_bytes()));
        let line = golden
            .lines()
            .chain(pages.lines())
            .find(|line| line.starts_with(&key))
            .unwrap();
        let page = String::from_utf8(unhex(line.rsplit('\t').next().unwrap())).unwrap();
        let php = page
            .split("<h1 class=\"wp-block-query-title\">")
            .nth(1)
            .unwrap()
            .split("</h1>")
            .next()
            .unwrap();
        let ours = archive_title(&archive, "", Zone::Utc, b"", true);
        assert_eq!(String::from_utf8_lossy(&ours), php, "{path}");
    }
}
