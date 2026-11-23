// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{Key, encode};

use super::*;
use crate::GlobalStyles;
use crate::tests::{arg, check, fixture, golden};

#[test]
fn layers_match_the_resolver() {
    let fixture = fixture();
    let sources = fixture.sources();
    let mut blocks = Blocks::new(sources.blocks, sources.block_styles);
    assert!(encode(&core_data(sources.core, &blocks)) == golden("raw-core.ser"));
    assert!(encode(&block_data(sources.blocks, &blocks)) == golden("raw-blocks.ser"));
    assert!(encode(&theme_data(&sources, &mut blocks)) == golden("raw-theme.ser"));
    assert!(encode(&user_data(None, &blocks)) == golden("raw-user.ser"));
    assert!(encode(&merged(&sources).0) == golden("raw-merged.ser"));
}

#[test]
fn matches_user_global_styles() {
    let fixture = fixture();
    check("user", |a| {
        let sources = Sources {
            user: Some(arg(a, 0).as_bytes()),
            ..fixture.sources()
        };
        let styles = GlobalStyles::resolve(&sources);
        let blocks = crate::stylesheet::tests::stylesheet_of(&styles.tree());
        let parts = [
            styles.settings().clone(),
            styles.styles().clone(),
            string(&styles.stylesheet()),
            string(&styles.inline_css(&|_| false)),
            php::get(&blocks, &["blocks"]).unwrap().clone(),
        ];
        Value::Array(
            parts
                .into_iter()
                .enumerate()
                .map(|(i, v)| (Key::Int(i as i64), v))
                .collect(),
        )
    });
}

#[test]
fn themes_without_theme_json() {
    let fixture = fixture();
    let sources = Sources {
        theme: None,
        theme_styles: &[],
        ..fixture.sources()
    };
    let styles = GlobalStyles::resolve(&sources);
    assert_eq!(
        styles.setting(&["color", "defaultPalette"]),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        styles.setting(&["shadow", "defaultPresets"]),
        Some(&Value::Bool(false))
    );
}

#[test]
fn block_style_partials() {
    let mut blocks = Blocks::new(&[], &[]);
    let files = [
        ("b/untitled.json".to_owned(), br#"{"version": 3, "blockTypes": ["core/x"], "styles": {"color": {"text": "a"}}}"#.to_vec()),
        ("a/titled.json".to_owned(), br#"{"version": 3, "blockTypes": ["core/x"], "title": "My Style", "styles": {"color": {"text": "b"}}}"#.to_vec()),
        ("c/none.json".to_owned(), br#"{"version": 3, "title": "Theme variation"}"#.to_vec()),
        ("d/empty.json".to_owned(), br#"{"version": 3, "blockTypes": [], "styles": {"color": {"text": "c"}}}"#.to_vec()),
        ("e/bad.json".to_owned(), b"{".to_vec()),
        ("f/unstyled.json".to_owned(), br#"{"version": 3, "blockTypes": ["core/x"], "title": "Unstyled"}"#.to_vec()),
    ];
    let theme = br#"{"version": 3, "styles": {"variations": {"my-style": {"color": {"background": "v"}}}, "blocks": {"core/x": {"variations": {"untitled": {"color": {"background": "w"}}}}}}}"#;
    let sources = Sources {
        core: b"",
        blocks: &[],
        block_styles: &[],
        theme: Some(theme),
        theme_styles: &files,
        parent_theme: None,
        user: None,
    };
    let names: Vec<String> = super::partials(&files, &blocks)
        .iter()
        .map(variation_name)
        .collect();
    assert_eq!(names, ["my-style", "untitled", "empty", "unstyled"]);
    let data = inject(decode(theme), &super::partials(&files, &blocks));
    let variations = php::get(&data, &["styles", "blocks", "core/x", "variations"]).unwrap();
    let expected = json_decode(
        br#"{"untitled": {"color": {"text": "a", "background": "w"}}, "my-style": {"color": {"text": "b", "background": "v"}}}"#,
    )
    .unwrap();
    assert_eq!(variations, &expected);
    // Unregistered blocks keep nothing.
    let raw = theme_data(&sources, &mut blocks);
    assert_eq!(php::get(&raw, &["styles"]), None);
}

/// A child theme's `theme.json` is merged over its parent's: what only the
/// parent has stays, a list the child sets replaces a preset list, and the
/// top level lists merge by position, as `array_replace_recursive()` does.
#[test]
fn a_child_theme_s_theme_json_is_merged_over_its_parent_s() {
    let fixture = fixture();
    let child = br##"{"version":3,"settings":{"color":{"palette":[{"slug":"c","color":"#111111","name":"C"}]}},"templateParts":[{"name":"a","area":"footer"}],"customTemplates":[{"name":"x","title":"X"}]}"##;
    let parent = br#"{"version":3,"settings":{"layout":{"contentSize":"500px"}},"templateParts":[{"name":"p","area":"header","title":"P"},{"name":"q","area":"header"}]}"#;
    let sources = Sources {
        theme: Some(child),
        parent_theme: Some(parent),
        theme_styles: &[],
        ..fixture.sources()
    };
    let styles = GlobalStyles::resolve(&sources);
    let size = styles.setting(&["layout", "contentSize"]).unwrap();
    assert_eq!(size.to_php_string(), b"500px");
    let palette = styles.setting(&["color", "palette", "theme"]).unwrap();
    assert_eq!(palette.as_array().unwrap().len(), 1);
    let parts = styles.list("templateParts");
    assert_eq!(parts.len(), 2);
    let first = &parts[0].1;
    let field = |name| first.get_str(name).unwrap().to_php_string();
    assert_eq!(field("name"), b"a");
    assert_eq!(field("area"), b"footer");
    assert_eq!(field("title"), b"P");
    assert_eq!(styles.list("customTemplates").len(), 1);
    assert!(styles.list("nothing").is_empty());
    // Without a parent's file, only the child's own is read.
    let alone = Sources {
        parent_theme: None,
        ..sources
    };
    assert_eq!(GlobalStyles::resolve(&alone).list("templateParts").len(), 1);
}
