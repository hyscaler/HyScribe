// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::Key;

use super::*;
use crate::tests::{blocks, check, resolved};
use crate::theme_json::{ORIGINS, Origin, new};

const BLOCKS: Options = Options {
    blocks: true,
    scope: None,
};

const ALL: Types = Types {
    variables: true,
    styles: true,
    presets: true,
    custom_css: false,
};
const CUSTOM: Types = Types {
    variables: false,
    styles: false,
    presets: false,
    custom_css: true,
};

/// `stylesheet_of()` in cases.php: the stylesheet of one theme.json, its
/// custom CSS, and the CSS of each block node.
pub(crate) fn stylesheet_of(tree: &Tree<'_>) -> Value {
    let blocks = tree
        .block_nodes(false)
        .iter()
        .enumerate()
        .map(|(i, node)| {
            let name = node
                .name
                .clone()
                .unwrap_or_else(|| crate::block_name_from_path(&node.path));
            let pair = vec![
                (Key::Int(0), string(&name)),
                (Key::Int(1), string(&tree.styles_for_block(node))),
            ];
            (Key::Int(i as i64), Value::Array(pair))
        })
        .collect();
    Value::Array(vec![
        (
            php::key("sheet"),
            string(&tree.stylesheet(ALL, &ORIGINS, BLOCKS)),
        ),
        (
            php::key("custom"),
            string(&tree.stylesheet(CUSTOM, &ORIGINS, BLOCKS)),
        ),
        (php::key("blocks"), Value::Array(blocks)),
    ])
}

fn json(text: &str) -> Value {
    hyscribe_phpser::json_decode(text.as_bytes()).unwrap()
}

/// The CSS of one theme.json with a single made-up block registered.
fn block_css(block: &str, metadata: &str, theme_json: &str) -> Vec<String> {
    let blocks = Blocks::new(
        &[crate::BlockType {
            name: block.to_owned(),
            metadata: json(metadata),
        }],
        &[],
    );
    let data = new(json(theme_json), Origin::Theme, &blocks);
    let tree = Tree {
        data: &data,
        blocks: &blocks,
        global: &Value::Null,
    };
    tree.block_nodes(false)
        .iter()
        .map(|n| tree.styles_for_block(n))
        .collect()
}

#[test]
fn custom_css_selectors() {
    let theme = r#"{"version": 3, "styles": {"blocks": {"test/a": {"css": "color: red"}}}}"#;
    assert_eq!(
        block_css("test/a", r#"{"selectors": {"css": ".x"}}"#, theme),
        [":root :where(.x){color: red}"]
    );
    assert_eq!(
        block_css("test/a", r#"{"selectors": {"css": {"root": ".y"}}}"#, theme),
        [":root :where(.y){color: red}"]
    );
    assert_eq!(
        block_css("test/a", r#"{"selectors": {"css": {"x": ".y"}}}"#, theme),
        [":root :where(.wp-block-test-a){color: red}"]
    );
}

#[test]
fn paragraph_indent_replaces_its_selector() {
    // The renamed rule takes the place of one already under the new selector.
    let metadata = r#"{"selectors": {"root": "p", "color": ".wp-block-paragraph", "typography": {"textIndent": ".wp-block-paragraph + .wp-block-paragraph"}}}"#;
    let theme = r#"{"version": 3, "settings": {"typography": {"textIndent": "all"}}, "styles": {"blocks": {"core/paragraph": {"color": {"text": "red"}, "typography": {"textIndent": "1em"}}}}}"#;
    assert_eq!(
        block_css("core/paragraph", metadata, theme),
        [":root :where(.wp-block-paragraph){text-indent: 1em;}"]
    );
    let theme = r#"{"version": 3, "settings": {"typography": {"textIndent": "all"}}, "styles": {"blocks": {"core/paragraph": {"color": {"text": "red"}}}}}"#;
    assert_eq!(
        block_css("core/paragraph", metadata, theme),
        [":root :where(.wp-block-paragraph){color: red;}"]
    );
}

#[test]
fn duotone_rules_skip_breakpoints() {
    let theme = r#"{"version": 3, "styles": {"blocks": {"test/a": {"filter": {"duotone": "x"}, "@mobile": {"filter": {"duotone": "y"}}}}}}"#;
    let metadata = r#"{"supports": {"color": {"__experimentalDuotone": "img"}}}"#;
    assert_eq!(
        block_css("test/a", metadata, theme),
        [".wp-block-test-a img{filter: x;}", ""]
    );
    let theme =
        r#"{"version": 3, "styles": {"blocks": {"test/a": {"filter": {"duotone": "unset"}}}}}"#;
    assert_eq!(block_css("test/a", metadata, theme), [""]);
    // Pseudo-class nodes keep it; their breakpoint nodes do not.
    let theme = r#"{"version": 3, "styles": {"blocks": {"core/button": {":hover": {"filter": {"duotone": "x"}}, "@mobile": {":hover": {"filter": {"duotone": "y"}}}}}}}"#;
    assert_eq!(
        block_css("core/button", metadata, theme),
        ["", "", ".wp-block-button img{filter: x;}", ""]
    );
}

#[test]
fn empty_feature_roots_are_ignored() {
    let theme =
        r#"{"version": 3, "styles": {"blocks": {"test/a": {"typography": {"fontSize": "2px"}}}}}"#;
    let metadata = r#"{"selectors": {"root": ".a", "typography": {"root": ""}}}"#;
    assert_eq!(
        block_css("test/a", metadata, theme),
        [":root :where(.a){font-size: 2px;}"]
    );
}

#[test]
fn background_urls_stay_in_their_string() {
    let image = json(r#"{"url": "a').b"}"#);
    assert_eq!(background_image(Some(&image), None), None);
    let image = json(r#"{"url": "a.png"}"#);
    assert_eq!(
        background_image(Some(&image), None),
        Some(string("url('a.png')"))
    );
    assert_eq!(background_image(None, Some(&json("[1]"))), None);
}

#[test]
fn matches_wp_theme_json() {
    check("stylesheet", |a| {
        let data = new(a[0].clone(), Origin::Theme, blocks());
        stylesheet_of(&Tree {
            data: &data,
            blocks: blocks(),
            global: resolved().settings(),
        })
    });
}
