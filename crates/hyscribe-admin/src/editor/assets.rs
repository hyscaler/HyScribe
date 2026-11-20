// SPDX-License-Identifier: AGPL-3.0-or-later

//! `_wp_get_iframed_editor_assets()`: the styles and scripts the editor's
//! canvas loads.

use hyscribe_phpser::{Key, Value};
use hyscribe_render::theme::Theme;

use super::EditorSite;
use crate::assets::Assets;
use crate::page::object;

/// The inline styles of the block styles the theme registers, which
/// `enqueue_block_styles_assets()` adds to `wp-block-library`.
pub(crate) fn block_style_css(theme: &Theme) -> Vec<(Key, Value)> {
    let inline = theme
        .registered("block_styles")
        .iter()
        .filter_map(|(_, entry)| {
            let style = entry.get(&Key::Int(1));
            style
                .and_then(|style| style.get_str("inline_style"))
                .cloned()
        });
    inline.zip(0..).map(|(css, i)| (Key::Int(i), css)).collect()
}

/// `__unstableResolvedAssets`: `wp-edit-blocks` and what it needs, the
/// theme's fonts, and `wp-polyfill`.
pub(crate) fn resolved_assets(site: &dyn EditorSite) -> Value {
    let mut styles = Assets::new(false);
    // A theme with `theme.json` has its own reset.
    styles.style_done("wp-reset-editor-styles");
    styles.enqueue_style("wp-edit-blocks");
    for (handle, src, ver) in site.theme().editor_styles() {
        styles.register_style(handle, &src, ver);
        styles.enqueue_style(handle);
    }
    // Nothing is added to the stylesheet for a theme without inline block styles.
    let inline = block_style_css(site.theme());
    if !inline.is_empty() {
        styles.set_style_data("wp-block-library", "after", Value::Array(inline));
    }
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
