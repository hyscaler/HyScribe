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
mod insecure;
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
mod variations;
mod viewport;

use hyscribe_phpser::Value;

pub use blocks::BlockType;
pub use kebab::kebab_case;
pub use variations::instance_class;

use stylesheet::{Options, Tree, Types};

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

/// `WP_Duotone::get_slug_from_attribute( $duotone_attr )`: the slug of
/// `var:preset|duotone|<slug>` or `var(--wp--preset--duotone--<slug>)`,
/// whichever starts first, or none.
pub fn duotone_slug(attribute: &[u8]) -> String {
    let token = |from: usize| {
        attribute
            .get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| !matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
            .count()
    };
    let find = |needle: &[u8]| {
        attribute
            .windows(needle.len())
            .position(|window| window == needle)
            .map(|at| (at, at + needle.len()))
    };
    let colon = find(b"var:preset|duotone|").and_then(|(start, from)| {
        let length = token(from);
        (length > 0).then_some((start, from, from + length))
    });
    // `(\S+)\)`: the token up to its last parenthesis.
    let function = find(b"var(--wp--preset--duotone--").and_then(|(start, from)| {
        let run = attribute.get(from..from + token(from)).unwrap_or_default();
        let close = run.iter().rposition(|b| *b == b')').filter(|&i| i > 0)?;
        Some((start, from, from + close))
    });
    let found = match (colon, function) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    };
    let slug = found.map_or(&[][..], |(_, from, to)| {
        attribute.get(from..to).unwrap_or_default()
    });
    // `! empty( $matches[1] )`: "0" is empty.
    if slug == b"0" {
        return String::new();
    }
    String::from_utf8_lossy(slug).into_owned()
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

    /// `( new WP_Theme_JSON( $config ) )->get_raw_data()` of the theme's
    /// origin, as the theme's style variations are read, over the block
    /// types and styles registered once the theme's data is.
    pub fn theme_json(&self, config: Value) -> Value {
        theme_json::new(config, theme_json::Origin::Theme, &self.blocks)
    }

    /// `( new WP_Theme_JSON( $config, 'custom' ) )->get_raw_data()`, as
    /// the site's global styles post is read.
    pub fn user_theme_json(&self, config: Value) -> Value {
        theme_json::new(config, theme_json::Origin::Custom, &self.blocks)
    }

    /// `WP_Theme_JSON::remove_insecure_properties( $config, 'custom' )`
    /// for a user without `edit_css`.
    pub(crate) fn remove_insecure_properties(&self, config: Value) -> Value {
        insecure::remove_insecure_properties(config, &self.blocks, self.settings())
    }

    /// `wp_filter_global_styles_post( $data )` of unslashed content: a user
    /// theme.json without its insecure values, `None` for other content,
    /// which it leaves as it is.
    pub fn filter_global_styles_post(&self, content: &[u8]) -> Option<Vec<u8>> {
        let mut config = hyscribe_phpser::json_decode(content)?;
        let flag = php::get(&config, &["isGlobalStylesUserThemeJSON"]);
        if !matches!(config, Value::Array(_)) || php::empty(flag) {
            return None;
        }
        php::unset(&mut config, &["isGlobalStylesUserThemeJSON"]);
        let mut config = self.remove_insecure_properties(config);
        php::set(
            &mut config,
            &["isGlobalStylesUserThemeJSON"],
            Value::Bool(true),
        );
        // A failed wp_json_encode() leaves wp_slash( false ): nothing.
        Some(hyscribe_phpser::json_encode_hex_amp(&config).unwrap_or_default())
    }

    /// A global setting, e.g. `["layout", "contentSize"]`; `None` where it
    /// is missing (PHP would return every setting).
    pub fn setting(&self, path: &[&str]) -> Option<&Value> {
        php::get(self.settings(), path)
    }

    /// A global style, e.g. `["spacing", "blockGap"]`.
    pub fn style(&self, path: &[&str]) -> Option<&Value> {
        php::get(self.styles(), path)
    }

    /// A style of one block (`styles.blocks.<block>`).
    pub fn block_style(&self, block: &str, path: &[&str]) -> Option<&Value> {
        php::get(self.styles(), &["blocks", block]).and_then(|s| php::get(s, path))
    }

    /// `wp_get_block_css_selector( $block_type, array( 'filter', 'duotone' ),
    /// true )`: where a block's duotone filter applies; none for a block
    /// without one.
    pub fn duotone_selector(&self, block: &str) -> Option<&str> {
        self.blocks.get(block)?.duotone.as_deref()
    }

    /// `WP_Duotone::get_all_global_style_block_names()`: each block whose
    /// global styles set a duotone preset, with the preset's slug.
    pub fn duotone_blocks(&self) -> Vec<(String, String)> {
        self.blocks
            .iter()
            .filter(|meta| meta.duotone.as_deref().is_some_and(|s| !s.is_empty()))
            .filter_map(|meta| {
                let attribute = php::as_str(self.block_style(&meta.name, &["filter", "duotone"])?)?;
                let slug = duotone_slug(attribute.as_bytes());
                // `$slug && $slug !== $duotone_attr`: a slug is never the whole value.
                (!slug.is_empty()).then(|| (meta.name.clone(), slug))
            })
            .collect()
    }

    /// `settings.useRootPaddingAwareAlignments`, as block supports test it.
    pub fn use_root_padding_aware_alignments(&self) -> bool {
        !php::empty(self.setting(&["useRootPaddingAwareAlignments"]))
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

    /// `wp_get_typography_font_size_value( $preset )`: the preset's `size`,
    /// made fluid when the global settings ask for it; `None` without a
    /// size.
    pub fn font_size_value(&self, preset: &Value) -> Option<Value> {
        typography::font_size_value(preset, &php::array(), self.settings())
    }

    /// `wp_get_global_stylesheet()`: variables, styles (block styles
    /// included) and preset classes.
    pub fn stylesheet(&self) -> String {
        self.main_stylesheet(true)
    }

    /// `wp_get_global_stylesheet( array( $kind ) )` in the admin, block
    /// styles included: `variables`, `styles`, `presets` or `custom-css`.
    pub fn stylesheet_of(&self, kind: &str) -> String {
        let types = Types {
            variables: kind == "variables",
            styles: kind == "styles",
            presets: kind == "presets",
            custom_css: kind == "custom-css",
        };
        let options = Options {
            blocks: true,
            ..Options::default()
        };
        self.tree().stylesheet(types, &STYLESHEET_ORIGINS, options)
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
        let options = Options {
            blocks,
            ..Options::default()
        };
        let mut css = tree.stylesheet(variables, &STYLESHEET_ORIGINS, options);
        css.push_str(&tree.stylesheet(rest, &STYLESHEET_ORIGINS, options));
        css
    }

    /// The CSS of every block style node (`get_styles_block_nodes()` and
    /// `get_styles_for_block()`): `(block name, css)`, possibly empty.
    pub fn block_css(&self) -> Vec<(String, String)> {
        let tree = self.tree();
        tree.block_nodes(false)
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
        stylesheet.push_str(&self.tree().stylesheet(
            custom,
            &STYLESHEET_ORIGINS,
            Options::default(),
        ));
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
mod duotone_tests;
#[cfg(test)]
mod tests;
