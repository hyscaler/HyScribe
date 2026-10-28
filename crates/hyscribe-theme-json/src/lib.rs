// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's theme.json engine (`WP_Theme_JSON`, `WP_Theme_JSON_Resolver`
//! and `wp_get_global_*()`), ported byte for byte with PHP WordPress 7.1 as
//! the oracle.
//!
//! [`GlobalStyles::resolve`] merges the core `theme.json`, the styles blocks
//! declare, the theme (its `theme.json` and block style variation
//! partials) and the user's global styles, in that order. The result gives
//! the global settings and styles as PHP arrays, the lookups block supports
//! need, and the `global-styles-inline-css` stylesheet.
//!
//! All input is data: JSON is decoded with PHP's semantics and sanitized
//! against the theme.json schema, and every value that reaches CSS through
//! the layout rules passes the `safecss_filter_attr()` check.

mod blocks;
mod css;
mod kebab;
mod php;
mod resolver;
mod sanitize;
mod schema;
mod selector;
mod spacing;
mod stylesheet;
mod theme_json;
mod typography;
mod viewport;

use hyscribe_phpser::Value;

pub use blocks::BlockType;
pub use kebab::kebab_case;

use stylesheet::{Tree, Types};

/// Everything the engine reads, for one site and theme.
#[derive(Clone, Copy, Debug)]
pub struct Sources<'a> {
    /// `wp-includes/theme.json`.
    pub core: &'a [u8],
    /// The registered block types, in registration order.
    pub blocks: &'a [BlockType],
    /// Styles registered with `register_block_style()` by PHP (plugins,
    /// the theme's `functions.php`): `(block name, style name)`, in order.
    pub block_styles: &'a [(String, String)],
    /// The theme's `theme.json`, `None` when it has none.
    pub theme: Option<&'a [u8]>,
    /// The theme's `styles/` JSON files: `(path below styles/, contents)`.
    /// Those with `blockTypes` are block style variation partials.
    pub theme_styles: &'a [(String, Vec<u8>)],
    /// The content of the theme's `wp_global_styles` post, if any.
    pub user: Option<&'a [u8]>,
}

/// The merged global settings and styles of a site.
#[derive(Clone, Debug, PartialEq)]
pub struct GlobalStyles {
    data: Value,
    blocks: blocks::Blocks,
}

/// The origins whose presets and styles the stylesheet prints.
const STYLESHEET_ORIGINS: [&str; 3] = ["default", "theme", "custom"];

impl GlobalStyles {
    /// Resolves and merges every layer (`WP_Theme_JSON_Resolver::get_merged_data()`).
    pub fn resolve(sources: &Sources<'_>) -> GlobalStyles {
        let (data, blocks) = resolver::merged(sources);
        GlobalStyles { data, blocks }
    }

    fn tree(&self) -> Tree<'_> {
        Tree {
            data: &self.data,
            blocks: &self.blocks,
            global: self.settings(),
        }
    }

    /// `wp_get_global_settings()`.
    pub fn settings(&self) -> &Value {
        php::get(&self.data, &["settings"]).unwrap_or(&Value::Null)
    }

    /// `wp_get_global_styles()`.
    pub fn styles(&self) -> &Value {
        php::get(&self.data, &["styles"]).unwrap_or(&Value::Null)
    }

    /// A global setting, e.g. `["layout", "contentSize"]`; `None` where it
    /// is missing (PHP would return every setting).
    pub fn setting(&self, path: &[&str]) -> Option<&Value> {
        php::get(self.settings(), path)
    }

    /// A setting of one block (`settings.blocks.<block>`).
    pub fn block_setting(&self, block: &str, path: &[&str]) -> Option<&Value> {
        php::get(self.settings(), &["blocks", block]).and_then(|s| php::get(s, path))
    }

    /// A global style, e.g. `["spacing", "blockGap"]`.
    pub fn style(&self, path: &[&str]) -> Option<&Value> {
        php::get(self.styles(), path)
    }

    /// A style of one block (`styles.blocks.<block>`).
    pub fn block_style(&self, block: &str, path: &[&str]) -> Option<&Value> {
        php::get(self.styles(), &["blocks", block]).and_then(|s| php::get(s, path))
    }

    /// `settings.useRootPaddingAwareAlignments`, as block supports test it.
    pub fn use_root_padding_aware_alignments(&self) -> bool {
        !php::empty(self.setting(&["useRootPaddingAwareAlignments"]))
    }

    /// `settings.layout.contentSize`.
    pub fn content_size(&self) -> Option<&str> {
        self.setting(&["layout", "contentSize"])
            .and_then(php::as_str)
    }

    /// `settings.layout.wideSize`.
    pub fn wide_size(&self) -> Option<&str> {
        self.setting(&["layout", "wideSize"]).and_then(php::as_str)
    }

    /// Whether the theme supports block gap: `settings.spacing.blockGap`
    /// is set and not `null`.
    pub fn has_block_gap_support(&self) -> bool {
        php::isset(self.setting(&["spacing", "blockGap"]))
    }

    /// The presets under `path` (such as `["typography", "fontSizes"]`,
    /// `["spacing", "spacingSizes"]` or `["color", "palette"]`), as
    /// `(origin, presets)` in origin order.
    pub fn presets(&self, path: &[&str]) -> Vec<(&'static str, &[(hyscribe_phpser::Key, Value)])> {
        let by_origin = self.setting(path).unwrap_or(&Value::Null);
        theme_json::ORIGINS
            .iter()
            .filter_map(|origin| Some((*origin, php::get(by_origin, &[origin])?.as_array()?)))
            .collect()
    }

    /// The responsive style states and their media queries, from
    /// `settings.viewport` (`@mobile`, `@tablet`).
    pub fn viewport_media_queries(&self) -> Vec<(String, String)> {
        viewport::media_queries(self.setting(&["viewport"]))
    }

    /// `wp_get_global_stylesheet()`: variables, styles (block styles
    /// included) and preset classes.
    pub fn stylesheet(&self) -> String {
        self.main_stylesheet(true)
    }

    fn main_stylesheet(&self, blocks: bool) -> String {
        let tree = self.tree();
        let variables = Types {
            variables: true,
            styles: false,
            presets: false,
            custom_css: false,
        };
        let rest = Types {
            variables: false,
            styles: true,
            presets: true,
            custom_css: false,
        };
        let mut css = tree.stylesheet(variables, &STYLESHEET_ORIGINS, blocks);
        css.push_str(&tree.stylesheet(rest, &STYLESHEET_ORIGINS, blocks));
        css
    }

    /// The CSS of every block style node (`get_styles_block_nodes()` and
    /// `get_styles_for_block()`): `(block name, css)`, possibly empty.
    pub fn block_css(&self) -> Vec<(String, String)> {
        let tree = self.tree();
        tree.block_nodes()
            .iter()
            .map(|node| {
                let name = node
                    .name
                    .clone()
                    .unwrap_or_else(|| block_name_from_path(&node.path));
                (name, tree.styles_for_block(node))
            })
            .collect()
    }

    /// The content of `<style id='global-styles-inline-css'>` as
    /// `wp_enqueue_global_styles()` builds it for a block theme that loads
    /// block assets on demand: the stylesheet without block styles, the
    /// global custom CSS, then the styles of each block for which `on_page`
    /// is true (blocks outside `core/` always). Customizer CSS is not
    /// included.
    pub fn inline_css(&self, on_page: &dyn Fn(&str) -> bool) -> String {
        let mut stylesheet = self.main_stylesheet(false);
        let custom = Types {
            variables: false,
            styles: false,
            presets: false,
            custom_css: true,
        };
        stylesheet.push_str(&self.tree().stylesheet(custom, &STYLESHEET_ORIGINS, false));
        if stylesheet.is_empty() {
            return stylesheet;
        }
        let mut parts = vec![stylesheet];
        for (name, css) in self.block_css() {
            let wanted = !name.is_empty() && (!name.starts_with("core/") || on_page(&name));
            if wanted && !css.is_empty() && css != "0" {
                parts.push(css);
            }
        }
        parts.join("\n")
    }
}

/// `wp_get_block_name_from_theme_json_path()`.
fn block_name_from_path(path: &[String]) -> String {
    match path {
        [styles, blocks, name, ..]
            if styles == "styles" && blocks == "blocks" && name.contains('/') =>
        {
            name.clone()
        }
        _ => path
            .iter()
            .find(|p| p.contains("core/"))
            .cloned()
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests;
