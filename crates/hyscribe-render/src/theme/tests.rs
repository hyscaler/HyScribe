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
    for slug in theme.parts.keys() {
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
    let theme = twentytwentyfive();
    assert_eq!(header(theme.style_css(), "Version"), "1.5");
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
        theme_file("twentytwentyfive", "style.css"),
        Some((
            "text/css",
            &include_bytes!("../../../../themes/twentytwentyfive/style.css")[..]
        ))
    );
    assert_eq!(
        theme_file(
            "twentytwentyfive",
            "assets/fonts/manrope/Manrope-VariableFont_wght.woff2",
        )
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
        assert_eq!(theme_file("twentytwentyfive", path), None, "{path}");
    }
}

#[test]
fn the_theme_is_named_by_its_header() {
    assert_eq!(twentytwentyfive().name(), "Twenty Twenty-Five");
}

/// `functions.php`'s post formats.
#[test]
fn post_formats_are_the_theme_s() {
    let theme = twentytwentyfive();
    let formats = theme.post_formats();
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

/// Every embedded theme is a theme of the folders of `themes/`, and an
/// unknown stylesheet falls back to the default as WordPress does.
#[test]
fn themes_are_chosen_by_slug() {
    let slugs: Vec<&str> = slugs().collect();
    assert_eq!(
        slugs,
        [
            "twentytwentyfive",
            "twentytwentyfour",
            "twentytwentythree",
            "twentytwentytwo"
        ]
    );
    for slug in &slugs {
        assert_eq!(resolve(slug), *slug);
        let theme = load(slug, None);
        assert_eq!(theme.slug(), *slug);
        assert!(theme.template(&["index".to_string()]).is_some(), "{slug}");
        assert!(theme.part(b"header").is_some(), "{slug}");
    }
    assert_eq!(resolve("nope"), DEFAULT);
    assert_eq!(load("nope", None).slug(), DEFAULT);
    assert_eq!(load("twentytwentyfour", None).name(), "Twenty Twenty-Four");
    assert_eq!(
        load("twentytwentythree", None).name(),
        "Twenty Twenty-Three"
    );
    assert_eq!(load("twentytwentytwo", None).name(), "Twenty Twenty-Two");
    assert_eq!(theme_file("nope", "style.css"), None);
    assert!(theme_file("twentytwentyfour", "style.css").is_some());
    assert_eq!(theme_file("twentytwentyfour", "patterns.json"), None);
}

/// The stylesheets a theme enqueues come from its export: Twenty
/// Twenty-Five's own on every page, Twenty Twenty-Four's for the button
/// block, Twenty Twenty-Three's none.
#[test]
fn the_themes_enqueued_stylesheets_are_read() {
    let five = twentytwentyfive();
    assert_eq!(five.styles.len(), 1);
    assert_eq!(five.styles[0].block, None);
    assert_eq!(five.styles[0].handle, "twentytwentyfive-style");
    assert_eq!(five.styles[0].src, "style.min.css");
    assert_eq!(five.styles[0].version.as_deref(), Some("1.5"));
    assert_eq!(
        five.styles[0].css.len(),
        include_str!("../../../../themes/twentytwentyfive/style.min.css").len()
    );

    let four = load("twentytwentyfour", None);
    assert_eq!(four.styles.len(), 1);
    assert_eq!(four.styles[0].block.as_deref(), Some("core/button"));
    assert_eq!(four.styles[0].src, "assets/css/button-outline.css");
    assert!(four.styles[0].css.contains(".is-style-outline"));
    assert_eq!(
        four.style_handles["core/button"],
        ["wp-block-button", "twentytwentyfour-button-style-outline"]
    );
    assert_eq!(five.style_handles["core/button"], ["wp-block-button"]);
    // The query block's handle is enqueued by an interactive query only.
    assert!(five.style_handles["core/query"].is_empty());
    assert!(load("twentytwentythree", None).styles.is_empty());
    // Theme support for wp-block-styles adds every core block's -theme handle.
    assert_eq!(
        load("twentytwentytwo", None).style_handles["core/group"],
        ["wp-block-group", "wp-block-group-theme"]
    );
    assert!(block_styles_support(br#"{"block_styles_support":true}"#));
    assert!(!block_styles_support(br#"{"block_styles_support":false}"#));
    assert!(!block_styles_support(b"{}"));
    // A pinned theme, with the stylesheet its functions.php enqueues.
    let two = load("twentytwentytwo", None);
    assert_eq!(two.styles.len(), 1);
    assert_eq!(two.styles[0].block, None);
    assert_eq!(two.styles[0].handle, "twentytwentytwo-style");
    assert_eq!(two.styles[0].src, "style.min.css");
    assert_eq!(two.styles[0].version.as_deref(), Some("2.2"));
}

/// Post formats come from the export, only the known ones, in WordPress's
/// order.
#[test]
fn post_formats_are_exported() {
    assert!(load("twentytwentyfour", None).post_formats().is_empty());
    let listed = br#"{"post_formats":["video","bogus","aside",7]}"#;
    assert_eq!(post_formats(listed), ["aside", "video"]);
    assert!(post_formats(b"{}").is_empty());
}

/// An export with a style missing its handle or source skips it, and one
/// whose file is not in the theme has no CSS.
#[test]
fn malformed_enqueued_styles_are_skipped() {
    let data = Data {
        slug: "t",
        files: &[],
        patterns: b"",
        styles: b"",
        registered: br#"{"styles":[{"src":"a.css"},{"handle":"h"},{"handle":"x","src":"gone.css","block":null,"ver":false}]}"#,
    };
    let styles = own_styles(&data);
    assert_eq!(styles.len(), 1);
    assert_eq!((styles[0].handle.as_str(), styles[0].css), ("x", ""));
    assert_eq!((&styles[0].block, &styles[0].version), (&None, &None));
}

/// A theme folder lists the files of its folder with the extension, and
/// nothing named alike.
#[test]
fn a_folder_lists_its_own_files() {
    let data = Data {
        slug: "t",
        files: &[
            ("templates/a.html", "text/html", b"a"),
            ("templates/b.txt", "text/plain", b"b"),
            ("templatesx/c.html", "text/html", b"c"),
            ("templates", "", b"d"),
        ],
        patterns: b"",
        registered: b"",
        styles: b"",
    };
    let found: Vec<_> = data.folder("templates", ".html").collect();
    assert_eq!(found, [("a", &b"a"[..])]);
}

/// A theme's exported patterns: its own by slug, none for another's.
#[test]
fn patterns_are_looked_up_by_slug() {
    let four = load("twentytwentyfour", None);
    let entry = four.pattern_entry(b"twentytwentyfour/banner-hero").unwrap();
    let title = entry.get_str("title").map(Value::to_php_string);
    assert_eq!(title.unwrap(), b"Hero");
    assert!(four.pattern_entry(b"twentytwentyfive/header").is_none());
}

/// The Themes screen's entry of an embedded theme, and none for another.
#[test]
fn the_themes_screen_lists_embedded_themes() {
    let entry = screen_entry("twentytwentyfour").unwrap();
    let name = entry.get_str("name").map(Value::to_php_string);
    assert_eq!(name.unwrap(), b"Twenty Twenty-Four");
    assert!(screen_entry("gone").is_none());
}

/// The block styles a theme's own stylesheets enqueue in the editor, and
/// whether it adds an editor stylesheet, as its registered data says.
#[test]
fn the_editor_s_own_styles_follow_the_theme() {
    let four = load("twentytwentyfour", None);
    let styles = four.editor_styles();
    let (handle, src, version) = styles.first().unwrap();
    assert_eq!(*handle, "twentytwentyfour-button-style-outline");
    assert_eq!(
        src,
        "/wp-content/themes/twentytwentyfour/assets/css/button-outline.css"
    );
    assert_eq!(*version, "1.6");
    assert!(!four.has_editor_style());
    assert!(load("twentytwentyfive", None).has_editor_style());
    assert!(load("twentytwentythree", None).editor_styles().is_empty());
}
