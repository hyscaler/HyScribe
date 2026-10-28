// SPDX-License-Identifier: AGPL-3.0-or-later

//! Block themes as data (docs/EXTENSIONS.md): templates, template parts and
//! patterns. Twenty Twenty-Five, the default install's theme, is built in;
//! its port lives in `themes/twentytwentyfive/`.

use std::collections::HashMap;

use hyscribe_phpser::{Key, Value, json_decode};
use hyscribe_theme_json::{GlobalStyles, Sources};

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

const PATTERNS: &[u8] = include_bytes!("../../../themes/twentytwentyfive/patterns.json");
const THEME_JSON: &[u8] = include_bytes!("../../../themes/twentytwentyfive/theme.json");
const CORE_THEME_JSON: &[u8] = include_bytes!("../../../wordpress/wp-includes/theme.json");

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
}

/// Twenty Twenty-Five, as bundled with WordPress 7.1.3.
pub fn twentytwentyfive() -> Theme {
    Theme {
        slug: "twentytwentyfive",
        templates: TEMPLATES.into_iter().collect(),
        parts: PARTS.into_iter().collect(),
        areas: part_areas(THEME_JSON),
        patterns: patterns(PATTERNS),
        global: GlobalStyles::resolve(&Sources {
            core: CORE_THEME_JSON,
            blocks: &[],
            block_styles: &[],
            theme: Some(THEME_JSON),
            theme_styles: &[],
            user: None,
        }),
    }
}

impl Theme {
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
