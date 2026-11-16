// SPDX-License-Identifier: AGPL-3.0-or-later

//! `_wp_get_iframed_editor_assets()`: the styles and scripts the editor's
//! canvas loads.

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Value, json_decode};

use super::EditorSite;
use crate::assets::Assets;
use crate::page::object;

const THEME_REGISTERED: &[u8] =
    include_bytes!("../../../../themes/twentytwentyfive/registered.json");

/// The inline styles of the block styles the theme registers, which
/// `enqueue_block_styles_assets()` adds to `wp-block-library`.
fn block_style_css() -> &'static [(Key, Value)] {
    static CSS: OnceLock<Value> = OnceLock::new();
    let css = CSS.get_or_init(|| {
        let registered = json_decode(THEME_REGISTERED).unwrap_or_default();
        let styles = registered.get_str("block_styles").and_then(Value::as_array);
        let inline = styles
            .unwrap_or_default()
            .iter()
            .filter_map(|(_, entry)| entry.get(&Key::Int(1))?.get_str("inline_style").cloned());
        Value::Array(inline.zip(0..).map(|(css, i)| (Key::Int(i), css)).collect())
    });
    css.as_array().unwrap_or_default()
}

/// `__unstableResolvedAssets`: `wp-edit-blocks` and what it needs, the
/// theme's fonts, and `wp-polyfill`.
pub(crate) fn resolved_assets(site: &dyn EditorSite) -> Value {
    let mut styles = Assets::new(false);
    // A theme with `theme.json` has its own reset.
    styles.style_done("wp-reset-editor-styles");
    styles.enqueue_style("wp-edit-blocks");
    styles.set_style_data(
        "wp-block-library",
        "after",
        Value::Array(block_style_css().to_vec()),
    );
    let mut printed = styles.print_admin_styles(site);
    printed.extend(site.font_faces(true));
    let mut scripts = Assets::new(true);
    scripts.enqueue_script("wp-polyfill");
    let mut script = scripts.print_head_scripts(site);
    script.extend(scripts.print_footer_scripts(site));
    object(vec![
        ("styles", Value::String(printed)),
        ("scripts", Value::String(script)),
    ])
}
