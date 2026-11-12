// SPDX-License-Identifier: AGPL-3.0-or-later

//! Block themes as data (docs/EXTENSIONS.md): templates, template parts and
//! patterns. Twenty Twenty-Five, the default install's theme, is built in;
//! its port lives in `themes/twentytwentyfive/`.

use std::collections::HashMap;

use hyscribe_phpser::{Key, Value, json_decode};
use hyscribe_theme_json::{BlockType as Metadata, GlobalStyles, Sources};

const TEMPLATES: [(&str, &[u8]); 8] = [
    (
        "404",
        include_bytes!("../../../themes/twentytwentyfive/templates/404.html"),
    ),
    (
        "archive",
        include_bytes!("../../../themes/twentytwentyfive/templates/archive.html"),
    ),
    (
        "home",
        include_bytes!("../../../themes/twentytwentyfive/templates/home.html"),
    ),
    (
        "index",
        include_bytes!("../../../themes/twentytwentyfive/templates/index.html"),
    ),
    (
        "page",
        include_bytes!("../../../themes/twentytwentyfive/templates/page.html"),
    ),
    (
        "page-no-title",
        include_bytes!("../../../themes/twentytwentyfive/templates/page-no-title.html"),
    ),
    (
        "search",
        include_bytes!("../../../themes/twentytwentyfive/templates/search.html"),
    ),
    (
        "single",
        include_bytes!("../../../themes/twentytwentyfive/templates/single.html"),
    ),
];

const PARTS: [(&str, &[u8]); 7] = [
    (
        "footer",
        include_bytes!("../../../themes/twentytwentyfive/parts/footer.html"),
    ),
    (
        "footer-columns",
        include_bytes!("../../../themes/twentytwentyfive/parts/footer-columns.html"),
    ),
    (
        "footer-newsletter",
        include_bytes!("../../../themes/twentytwentyfive/parts/footer-newsletter.html"),
    ),
    (
        "header",
        include_bytes!("../../../themes/twentytwentyfive/parts/header.html"),
    ),
    (
        "header-large-title",
        include_bytes!("../../../themes/twentytwentyfive/parts/header-large-title.html"),
    ),
    (
        "sidebar",
        include_bytes!("../../../themes/twentytwentyfive/parts/sidebar.html"),
    ),
    (
        "vertical-header",
        include_bytes!("../../../themes/twentytwentyfive/parts/vertical-header.html"),
    ),
];

/// Twenty Twenty-Five's own files, every one but its PHP, by path, with
/// their content types.
const FILES: &[(&str, &str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/theme_files.rs"));

/// One of Twenty Twenty-Five's files, as the web server sends it from the
/// theme's folder: its content type and bytes.
pub fn twentytwentyfive_file(path: &str) -> Option<(&'static str, &'static [u8])> {
    FILES
        .iter()
        .find(|(name, _, _)| *name == path)
        .map(|(_, content_type, bytes)| (*content_type, *bytes))
}

/// The theme's style variations, `WP_Theme_JSON_Resolver::get_style_variations()`
/// in path order: its `styles/` files that are not for a block type.
pub(crate) fn style_variations() -> impl Iterator<Item = Value> {
    FILES
        .iter()
        .filter(|(name, _, _)| name.starts_with("styles/") && name.ends_with(".json"))
        .filter_map(|(_, _, bytes)| json_decode(bytes))
        .filter(|variation| variation.get_str("blockTypes").is_none())
}

const PATTERNS: &[u8] = include_bytes!("../../../themes/twentytwentyfive/patterns.json");
const THEME_JSON: &[u8] = include_bytes!("../../../themes/twentytwentyfive/theme.json");
const CORE_THEME_JSON: &[u8] = include_bytes!("../../../wordpress/wp-includes/theme.json");
const REGISTERED_BLOCKS: &[u8] =
    include_bytes!("../../../wordpress/wp-includes/blocks/registered.json");
const BLOCK_STYLES: &str = include_str!("../../../themes/twentytwentyfive/block-styles.tsv");
const STYLE_PARTIALS: &[u8] = include_bytes!("../../../themes/twentytwentyfive/styles.json");
const STYLE_CSS: &str = include_str!("../../../themes/twentytwentyfive/style.css");
const STYLE_MIN_CSS: &str = include_str!("../../../themes/twentytwentyfive/style.min.css");

/// A block theme.
#[derive(Debug)]
pub struct Theme {
    pub(crate) slug: &'static str,
    pub(crate) templates: HashMap<&'static str, &'static [u8]>,
    pub(crate) parts: HashMap<&'static str, &'static [u8]>,
    /// The area of each template part, from `theme.json`.
    pub(crate) areas: HashMap<Vec<u8>, Vec<u8>>,
    pub(crate) patterns: HashMap<Vec<u8>, Vec<u8>>,
    /// Core and theme `theme.json` merged, as `wp_get_global_settings()`
    /// and `wp_get_global_styles()` read them.
    pub(crate) global: GlobalStyles,
    /// The style handles each core block's `block.json` names, which
    /// rendering the block enqueues.
    pub(crate) style_handles: HashMap<String, Vec<String>>,
    /// `style.min.css`, which the theme enqueues.
    pub(crate) stylesheet: &'static str,
    /// The `Version` header of `style.css`.
    pub(crate) version: String,
}

/// Twenty Twenty-Five, as bundled with WordPress 7.1.3.
pub fn twentytwentyfive() -> Theme {
    with_user_styles(None, &[])
}

/// Twenty Twenty-Five with `user` as the site's global styles and `styles`
/// registered after its own block styles.
#[cfg(test)]
pub(crate) fn twentytwentyfive_with(user: &[u8], styles: &[(String, String)]) -> Theme {
    with_user_styles(Some(user), styles)
}

fn with_user_styles(user: Option<&[u8]>, styles: &[(String, String)]) -> Theme {
    let blocks = registered_blocks(REGISTERED_BLOCKS);
    let mut block_styles = block_styles(BLOCK_STYLES);
    block_styles.extend_from_slice(styles);
    Theme {
        slug: "twentytwentyfive",
        templates: TEMPLATES.into_iter().collect(),
        parts: PARTS.into_iter().collect(),
        areas: part_areas(THEME_JSON),
        patterns: patterns(PATTERNS),
        global: GlobalStyles::resolve(&Sources {
            core: CORE_THEME_JSON,
            blocks: &blocks,
            block_styles: &block_styles,
            theme: Some(THEME_JSON),
            theme_styles: &style_partials(STYLE_PARTIALS),
            user,
        }),
        style_handles: blocks
            .iter()
            .map(|block| {
                (
                    block.name.clone(),
                    style_handles(&block.name, &block.metadata),
                )
            })
            .collect(),
        stylesheet: STYLE_MIN_CSS,
        version: header(STYLE_CSS, "Version"),
    }
}

/// A `style.css` header field, as `get_file_data()` reads it.
fn header(css: &str, name: &str) -> String {
    css.lines()
        .find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.trim_start_matches([' ', '\t', '*', '/', '#', '@'])
                .eq_ignore_ascii_case(name)
                .then(|| value.trim().to_owned())
        })
        .unwrap_or_default()
}

/// The style handles a core block registers, as
/// `register_block_type_from_metadata()` does: `wp-block-<name>` first,
/// whatever `block.json` names there, then the other handles it lists.
fn style_handles(name: &str, metadata: &Value) -> Vec<String> {
    let text = |handle: &Value| String::from_utf8_lossy(&handle.to_php_string()).into_owned();
    let mut handles = vec![format!("wp-block-{}", name.trim_start_matches("core/"))];
    if let Some(Value::Array(listed)) = metadata.get_str("style") {
        handles.extend(listed.iter().skip(1).map(|(_, handle)| text(handle)));
    }
    handles
}

impl Theme {
    /// The theme's `Theme Name` header.
    pub fn name(&self) -> String {
        header(STYLE_CSS, "Theme Name")
    }

    /// The theme's slug, its `stylesheet`.
    pub fn slug(&self) -> &str {
        self.slug
    }

    /// The first of `candidates` (template slugs in hierarchy order) the
    /// theme has, with its content.
    pub fn template(&self, candidates: &[String]) -> Option<(String, &[u8])> {
        candidates.iter().find_map(|slug| {
            self.templates
                .get(slug.as_str())
                .map(|content| (slug.clone(), *content))
        })
    }

    /// A template part's content and area (`uncategorized` when
    /// `theme.json` names none).
    pub fn part(&self, slug: &[u8]) -> Option<(&[u8], &[u8])> {
        let content = std::str::from_utf8(slug)
            .ok()
            .and_then(|slug| self.parts.get(slug))?;
        let area = self
            .areas
            .get(slug)
            .map_or(&b"uncategorized"[..], Vec::as_slice);
        Some((content, area))
    }

    /// A registered pattern's block markup.
    pub fn pattern(&self, slug: &[u8]) -> Option<&[u8]> {
        self.patterns.get(slug).map(Vec::as_slice)
    }
}

/// `registered.json`: each registered core block's `block.json`, in
/// registration order.
fn registered_blocks(json: &[u8]) -> Vec<Metadata> {
    let root = json_decode(json).unwrap_or_default();
    root.as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(|(_, entry)| {
            let name = entry.get(&Key::Int(0)).and_then(Value::as_bytes)?;
            let metadata = json_decode(entry.get(&Key::Int(1)).and_then(Value::as_bytes)?)?;
            Some(Metadata {
                name: String::from_utf8_lossy(name).into_owned(),
                metadata,
            })
        })
        .collect()
}

/// `block-styles.tsv`: the block styles registered with the theme, block
/// and style name per line.
fn block_styles(tsv: &str) -> Vec<(String, String)> {
    tsv.lines()
        .filter_map(|line| line.split_once('\t'))
        .map(|(block, style)| (block.to_string(), style.to_string()))
        .collect()
}

/// `styles.json`: the theme's style partials that name block types, file
/// name to contents.
fn style_partials(json: &[u8]) -> Vec<(String, Vec<u8>)> {
    let root = json_decode(json).unwrap_or_default();
    root.as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(|(key, contents)| match key {
            Key::String(name) => Some((
                String::from_utf8_lossy(name).into_owned(),
                contents.as_bytes()?.to_vec(),
            )),
            Key::Int(_) => None,
        })
        .collect()
}

/// The string at `key` of a decoded JSON object.
fn string<'v>(value: &'v Value, key: &str) -> Option<&'v [u8]> {
    value.get_str(key).and_then(Value::as_bytes)
}

/// `templateParts` in `theme.json`: part name to area.
fn part_areas(theme_json: &[u8]) -> HashMap<Vec<u8>, Vec<u8>> {
    let root = json_decode(theme_json).unwrap_or(Value::Null);
    let parts = root
        .get_str("templateParts")
        .and_then(Value::as_array)
        .unwrap_or_default();
    parts
        .iter()
        .filter_map(|(_, part)| {
            Some((
                string(part, "name")?.to_vec(),
                string(part, "area")?.to_vec(),
            ))
        })
        .collect()
}

/// `patterns.json` from the theme export: slug to content.
fn patterns(json: &[u8]) -> HashMap<Vec<u8>, Vec<u8>> {
    let root = json_decode(json).unwrap_or(Value::Null);
    root.as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(|(key, pattern)| match key {
            Key::String(slug) => Some((slug.clone(), string(pattern, "content")?.to_vec())),
            Key::Int(_) => None,
        })
        .collect()
}

#[cfg(test)]
mod tests;
