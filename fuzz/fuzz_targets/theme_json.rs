// SPDX-License-Identifier: AGPL-3.0-or-later

//! An arbitrary theme.json, as core, theme, block style partial and user
//! global styles at once, resolves and prints its stylesheet, and the
//! stylesheet of the style variation the input as a class name picks, and
//! as a global styles post loses its insecure values, without panicking or
//! hanging.

#![no_main]

use std::sync::OnceLock;

use hyscribe_theme_json::{BlockType, GlobalStyles, Sources};
use libfuzzer_sys::fuzz_target;

/// A few blocks with the metadata the engine reads: selectors, feature
/// selectors, duotone, states, style variations, layout.
fn blocks() -> &'static [BlockType] {
    static BLOCKS: OnceLock<Vec<BlockType>> = OnceLock::new();
    BLOCKS.get_or_init(|| {
        [
            (
                "core/button",
                r#"{"selectors": {"root": ".wp-block-button .wp-block-button__link", "dimensions": {"root": ".wp-block-button", "width": ".wp-block-button"}}, "styles": [{"name": "outline"}]}"#,
            ),
            (
                "core/paragraph",
                r#"{"selectors": {"root": "p", "typography": {"textIndent": ".wp-block-paragraph + .wp-block-paragraph"}}}"#,
            ),
            (
                "core/navigation-link",
                r#"{"selectors": {"states": {"-current": ".current-menu-item"}}}"#,
            ),
            (
                "core/image",
                r#"{"selectors": {"filter": {"duotone": ".wp-block-image img"}}}"#,
            ),
            (
                "core/group",
                r#"{"supports": {"layout": true, "spacing": {"blockGap": {"__experimentalDefault": "1em"}}}}"#,
            ),
        ]
        .iter()
        .map(|(name, json)| BlockType {
            name: (*name).to_owned(),
            metadata: hyscribe_phpser::json_decode(json.as_bytes())
                .unwrap_or(hyscribe_phpser::Value::Null),
        })
        .collect()
    })
}

fuzz_target!(|data: &[u8]| {
    let partials = [("blocks/fuzz.json".to_owned(), data.to_vec())];
    let styles = GlobalStyles::resolve(&Sources {
        core: data,
        blocks: blocks(),
        block_styles: &[],
        theme: Some(data),
        theme_styles: &partials,
        user: Some(data),
    });
    let _ = hyscribe_phpser::encode(styles.settings());
    let _ = styles.stylesheet();
    let _ = styles.inline_css(&|_| true);
    let _ = styles.filter_global_styles_post(data);
    for block in blocks() {
        if let Some(variation) = styles.style_variation(&block.name, data) {
            let _ = styles.style_variation_css(&block.name, &variation, &format!("{variation}--1"));
        }
    }
});
