// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn twentytwentyfive_is_built_in() {
    let theme = twentytwentyfive();
    assert_eq!(theme.slug(), "twentytwentyfive");
    let candidates = |slugs: &[&str]| slugs.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let (slug, content) = theme
        .template(&candidates(&["front-page", "home", "index"]))
        .unwrap();
    assert_eq!(slug, "home");
    assert!(content.starts_with(b"<!-- wp:template-part {\"slug\":\"header\"} /-->"));
    assert_eq!(
        theme
            .template(&candidates(&["single-post-x", "single"]))
            .unwrap()
            .0,
        "single"
    );
    assert!(theme.template(&candidates(&["nothing"])).is_none());

    let (header, area) = theme.part(b"header").unwrap();
    assert!(header.starts_with(b"<!-- wp:pattern"));
    assert_eq!(area, b"header");
    assert_eq!(theme.part(b"footer").unwrap().1, b"footer");
    assert_eq!(theme.part(b"sidebar").unwrap().1, b"uncategorized");
    assert!(theme.part(b"missing").is_none());
    assert!(theme.part(b"\xff").is_none());

    let query_loop = theme
        .pattern(b"twentytwentyfive/template-query-loop")
        .unwrap();
    assert!(query_loop.starts_with(b"<!-- wp:query"));
    assert!(theme.pattern(b"twentytwentyfive/missing").is_none());
}

#[test]
fn malformed_theme_data_is_empty() {
    assert!(part_areas(b"not json").is_empty());
    assert!(
        part_areas(b"{\"templateParts\":[1,{\"name\":\"x\"},{\"area\":\"y\"},{\"name\":\"a\",\"area\":\"b\"}]}")
            .len()
            == 1
    );
    assert!(part_areas(b"{\"version\":3}").is_empty());
    assert!(patterns(b"[").is_empty());
    assert_eq!(
        patterns(b"{\"a\":{\"content\":\"x\"},\"b\":{\"title\":\"no content\"},\"c\":3,\"7\":{\"content\":\"y\"}}").len(),
        1
    );
}

#[test]
fn every_part_has_a_known_area() {
    let theme = twentytwentyfive();
    for (slug, _) in PARTS {
        let (_, area) = theme.part(slug.as_bytes()).unwrap();
        assert!(
            [&b"header"[..], b"footer", b"uncategorized"].contains(&area),
            "{slug}"
        );
    }
}

#[test]
fn malformed_theme_data_is_skipped() {
    assert!(registered_blocks(b"[[1, 2], [\"core/x\", \"no json\"]]").is_empty());
    assert!(registered_blocks(b"[[\"core/x\"]]").is_empty());
    assert_eq!(style_partials(b"{\"0\": \"x\", \"a.json\": 3}"), []);
    assert_eq!(
        block_styles("no tab\ncore/list\tcheckmark-list"),
        [("core/list".to_string(), "checkmark-list".to_string())]
    );
}

#[test]
fn style_partials_reach_the_global_styles() {
    // The theme's section styles register block style variations.
    let global = twentytwentyfive().global;
    assert!(
        global
            .block_style("core/columns", &["variations", "section-1"])
            .is_some()
    );
}

/// `get_file_data()` reads `Version` from `style.css`, case-insensitively
/// and after comment markers.
#[test]
fn reads_the_style_header() {
    assert_eq!(twentytwentyfive().version, "1.5");
    assert_eq!(
        header("/*\n * version:  2.0 \nText: x\n*/", "Version"),
        "2.0"
    );
    assert_eq!(header("Theme Name: x", "Version"), "");
}

/// The theme's files as exported, and none of the port's own data.
#[test]
fn twentytwentyfive_files_are_built_in() {
    assert_eq!(
        twentytwentyfive_file("style.css"),
        Some((
            "text/css",
            &include_bytes!("../../../../themes/twentytwentyfive/style.css")[..]
        ))
    );
    assert_eq!(
        twentytwentyfive_file("assets/fonts/manrope/Manrope-VariableFont_wght.woff2")
            .map(|(content_type, bytes)| (content_type, bytes.len())),
        Some(("font/woff2", 53600))
    );
    for path in [
        "patterns.json",
        "styles.json",
        "block-styles.tsv",
        "functions.php",
        "nope.css",
    ] {
        assert_eq!(twentytwentyfive_file(path), None, "{path}");
    }
}
