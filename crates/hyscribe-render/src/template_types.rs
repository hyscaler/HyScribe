// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's default block template types and template part areas.

use crate::post::POST_FORMATS;

/// `get_default_block_template_types()` but the post formats': each slug,
/// title and description.
pub const DEFAULT_TYPES: [(&str, &str, &str); 16] = [
    (
        "index",
        "Index",
        "Used as a fallback template for all pages when a more specific template is not defined.",
    ),
    (
        "home",
        "Blog Home",
        "Displays the latest posts as either the site homepage or as the \"Posts page\" as defined under reading settings. If it exists, the Front Page template overrides this template when posts are shown on the homepage.",
    ),
    (
        "front-page",
        "Front Page",
        "Displays your site's homepage, whether it is set to display latest posts or a static page. The Front Page template takes precedence over all templates.",
    ),
    (
        "singular",
        "Single Entries",
        "Displays any single entry, such as a post or a page. This template will serve as a fallback when a more specific template (e.g. Single Post, Page, or Attachment) cannot be found.",
    ),
    (
        "single",
        "Single Posts",
        "Displays a single post on your website unless a custom template has been applied to that post or a dedicated template exists.",
    ),
    (
        "page",
        "Pages",
        "Displays a static page unless a custom template has been applied to that page or a dedicated template exists.",
    ),
    (
        "archive",
        "All Archives",
        "Displays any archive, including posts by a single author, category, tag, taxonomy, custom post type, and date. This template will serve as a fallback when more specific templates (e.g. Category or Tag) cannot be found.",
    ),
    (
        "author",
        "Author Archives",
        "Displays a single author's post archive. This template will serve as a fallback when a more specific template (e.g. Author: Admin) cannot be found.",
    ),
    (
        "category",
        "Category Archives",
        "Displays a post category archive. This template will serve as a fallback when a more specific template (e.g. Category: Recipes) cannot be found.",
    ),
    (
        "taxonomy",
        "Taxonomy",
        "Displays a custom taxonomy archive. Like categories and tags, taxonomies have terms which you use to classify things. For example: a taxonomy named \"Art\" can have multiple terms, such as \"Modern\" and \"18th Century.\" This template will serve as a fallback when a more specific template (e.g. Taxonomy: Art) cannot be found.",
    ),
    (
        "date",
        "Date Archives",
        "Displays a post archive when a specific date is visited (e.g., example.com/2023/).",
    ),
    (
        "tag",
        "Tag Archives",
        "Displays a post tag archive. This template will serve as a fallback when a more specific template (e.g. Tag: Pizza) cannot be found.",
    ),
    (
        "attachment",
        "Attachment Pages",
        "Displays when a visitor views the dedicated page that exists for any media attachment.",
    ),
    (
        "search",
        "Search Results",
        "Displays when a visitor performs a search on your website.",
    ),
    (
        "privacy-policy",
        "Privacy Policy",
        "Displays your site's Privacy Policy page.",
    ),
    (
        "404",
        "Page: 404",
        "Displays when a visitor views a non-existent page, such as a dead link or a mistyped URL.",
    ),
];

/// `get_default_block_template_types()`'s title and description of `slug`.
pub fn default_type(slug: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    let found = DEFAULT_TYPES
        .iter()
        .find(|(name, _, _)| name.as_bytes() == slug)
        .map(|(_, title, description)| {
            (title.as_bytes().to_vec(), description.as_bytes().to_vec())
        });
    found.or_else(|| {
        let format = slug.strip_prefix(b"taxonomy-post_format-post-format-")?;
        let (_, name) = POST_FORMATS
            .iter()
            .find(|(format_slug, _)| format_slug.as_bytes() == format)?;
        Some((
            format!("Post Format: {name}").into_bytes(),
            format!("Displays the {name} post format archive.").into_bytes(),
        ))
    })
}

/// A template part area of `get_allowed_block_template_part_areas()`.
pub struct Area {
    /// Its slug.
    pub area: &'static str,
    /// Its name.
    pub label: &'static str,
    /// What it holds.
    pub description: &'static str,
    /// The name of its icon in the editor.
    pub icon: &'static str,
    /// The element a part of it is wrapped in.
    pub tag: &'static str,
}

/// `get_allowed_block_template_part_areas()`.
pub const PART_AREAS: [Area; 4] = [
    Area {
        area: "uncategorized",
        label: "General",
        description: "General templates often perform a specific role like displaying post content, and are not tied to any particular area.",
        icon: "layout",
        tag: "div",
    },
    Area {
        area: "header",
        label: "Header",
        description: "The Header template defines a page area that typically contains a title, logo, and main navigation.",
        icon: "header",
        tag: "header",
    },
    Area {
        area: "footer",
        label: "Footer",
        description: "The Footer template defines a page area that typically contains site credits, social links, or any other combination of blocks.",
        icon: "footer",
        tag: "footer",
    },
    Area {
        area: "navigation-overlay",
        label: "Navigation Overlay",
        description: "The Navigation Overlay template defines an overlay area that typically contains navigation links and can be toggled open and closed.",
        icon: "navigation-overlay",
        tag: "div",
    },
];

#[cfg(test)]
mod tests;
