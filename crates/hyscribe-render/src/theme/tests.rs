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
fn pattern_urls_move_to_the_site() {
    let exported =
        br#"<img src="http://127.0.0.1:8081/wp-content/themes/t/a.webp"> http://127.0.0.1:8081/o"#;
    let moved = under_site(exported, b"https://wp.test/sub");
    let expected =
        br#"<img src="https://wp.test/sub/wp-content/themes/t/a.webp"> http://127.0.0.1:8081/o"#;
    assert_eq!(moved, expected);
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
        block_styles(br#"{"block_styles": [1, ["core/x"], ["core/x", {}], ["core/list", {"name": "checkmark-list"}], ["core/quote", {"name": "q", "inline_style": "q{}"}]]}"#),
        [
            ("core/list".into(), "checkmark-list".into(), String::new()),
            ("core/quote".into(), "q".into(), "q{}".into())
        ]
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

/// The theme's style variations, sanitized, and its own global styles.
#[test]
fn style_variations_are_read() {
    let theme = twentytwentyfive();
    let titles: Vec<Vec<u8>> = theme
        .style_variations()
        .iter()
        .map(|variation| variation.get_str("title").unwrap().to_php_string())
        .collect();
    assert_eq!(titles.len(), 23);
    assert_eq!(titles[0], b"Evening");
    assert!(std::ptr::eq(theme.global_styles(), &theme.global));
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
        "registered.json",
        "functions.php",
        "nope.css",
    ] {
        assert_eq!(twentytwentyfive_file(path), None, "{path}");
    }
}

#[test]
fn the_theme_is_named_by_its_header() {
    assert_eq!(twentytwentyfive().name(), "Twenty Twenty-Five");
}

/// `functions.php`'s post formats.
#[test]
fn post_formats_are_the_theme_s() {
    let formats = twentytwentyfive().post_formats();
    assert_eq!(formats.first(), Some(&"aside"));
    assert_eq!(formats.len(), 9);
}

/// The lists of `theme.json`, as the templates' REST routes read them.
#[test]
fn theme_json_lists_are_read() {
    let theme = twentytwentyfive();
    let custom = theme.theme_json_list("customTemplates");
    let name = Value::String(b"page-no-title".to_vec());
    assert_eq!(custom.len(), 1);
    assert_eq!(custom[0].get_str("name"), Some(&name));
    assert_eq!(theme.theme_json_list("templateParts").len(), 7);
    assert!(theme.theme_json_list("nope").is_empty());
}

#[test]
fn page_templates_are_the_custom_ones_for_the_type() {
    let theme = twentytwentyfive();
    let no_title = ("page-no-title".to_string(), "Page No Title".to_string());
    assert_eq!(theme.page_templates("page"), vec![no_title]);
    assert!(theme.page_templates("post").is_empty());
    // Templates naming no post types are for pages and need a name, titled by
    // it if they have no title; the last of a title is kept, numbers sorted as numbers.
    let json = br#"{"customTemplates":[{"name":"z","title":"Zed"},{"name":"a","title":"Ay","postTypes":["post","page"]},{"name":"x"},{"title":"No name"},{"name":"t10","title":"10"},{"name":"old","title":"9"},{"name":"t9","title":"9"}]}"#;
    let pairs = |list: &[(&str, &str)]| -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| ((*a).into(), (*b).into()))
            .collect()
    };
    assert_eq!(
        custom_templates(json, "page"),
        pairs(&[
            ("t9", "9"),
            ("t10", "10"),
            ("a", "Ay"),
            ("z", "Zed"),
            ("x", "x")
        ])
    );
    assert_eq!(custom_templates(json, "post"), pairs(&[("a", "Ay")]));
    assert!(custom_templates(b"[]", "page").is_empty());
}
