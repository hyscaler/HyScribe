// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Block_Template` objects from the active theme's files and from the
//! site's `wp_template` and `wp_template_part` posts, found as
//! `get_block_templates()`, `get_block_template()` and
//! `resolve_block_template()` find them.

use std::sync::OnceLock;

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, absint, is_empty_string};
use hyscribe_render::formatting::sanitize_title_for_query;
use hyscribe_render::post::POST_FORMATS;
use hyscribe_render::theme::{Theme, twentytwentyfive};

use super::content::hooked;
use crate::Context;
use crate::response::RestError;

/// The theme's templates and parts in the order PHP's directory iterator
/// lists them in the oracle's theme folders.
const TEMPLATE_FILES: [&str; 8] = [
    "archive",
    "404",
    "search",
    "index",
    "single",
    "page-no-title",
    "page",
    "home",
];
pub(super) const PART_FILES: [&str; 7] = [
    "sidebar",
    "header",
    "footer-newsletter",
    "footer-columns",
    "vertical-header",
    "footer",
    "header-large-title",
];

/// `get_allowed_block_template_part_areas()`.
const AREAS: [&str; 4] = ["uncategorized", "header", "footer", "navigation-overlay"];

/// `get_default_block_template_types()` but the post formats': each slug,
/// title and description.
const DEFAULT_TYPES: [(&str, &str, &str); 16] = [
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

/// The post types and taxonomies a default install registers, in order.
const POST_TYPES: [&str; 16] = [
    "post",
    "page",
    "attachment",
    "revision",
    "nav_menu_item",
    "custom_css",
    "customize_changeset",
    "oembed_cache",
    "user_request",
    "wp_block",
    "wp_template",
    "wp_template_part",
    "wp_global_styles",
    "wp_navigation",
    "wp_font_family",
    "wp_font_face",
];
const TAXONOMIES: [&str; 8] = [
    "category",
    "post_tag",
    "nav_menu",
    "link_category",
    "post_format",
    "wp_theme",
    "wp_template_part_area",
    "wp_pattern_category",
];

pub(crate) fn theme() -> &'static Theme {
    static THEME: OnceLock<Theme> = OnceLock::new();
    THEME.get_or_init(twentytwentyfive)
}

/// The template post types.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    Template,
    Part,
}

impl Kind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Kind::Template => "wp_template",
            Kind::Part => "wp_template_part",
        }
    }
}

/// A `WP_Block_Template`.
#[derive(Clone, Debug)]
pub(crate) struct Template {
    pub(crate) id: Vec<u8>,
    pub(crate) theme: Vec<u8>,
    pub(crate) content: Vec<u8>,
    pub(crate) slug: Vec<u8>,
    /// `theme` or `custom`.
    pub(crate) source: &'static str,
    pub(crate) origin: Option<Vec<u8>>,
    pub(crate) kind: Kind,
    pub(crate) description: Vec<u8>,
    pub(crate) title: Vec<u8>,
    pub(crate) status: String,
    pub(crate) wp_id: u64,
    pub(crate) has_theme_file: bool,
    pub(crate) is_custom: bool,
    pub(crate) author: u64,
    post_types: Option<Vec<Value>>,
    pub(crate) area: Option<Vec<u8>>,
    pub(crate) modified: Option<String>,
    pub(crate) date: Option<String>,
}

impl Template {
    /// A custom template for `post_type`: one without post types is for
    /// every type.
    pub(crate) fn custom_for(&self, post_type: &[u8]) -> bool {
        let listed = self.post_types.as_ref().is_none_or(|types| {
            types
                .iter()
                .any(|listed| listed.as_bytes() == Some(post_type))
        });
        self.is_custom && listed
    }
}

/// What `get_block_templates()` is asked for.
#[derive(Default)]
pub(crate) struct Query {
    pub(crate) slug_in: Vec<Vec<u8>>,
    pub(crate) wp_id: Option<Value>,
    pub(crate) area: Option<Vec<u8>>,
    pub(crate) post_type: Option<Vec<u8>>,
}

/// A theme file: its slug, its title and post types from `theme.json`'s
/// custom templates, or its part's area.
struct File {
    slug: &'static str,
    title: Vec<u8>,
    post_types: Option<Vec<Value>>,
    area: Option<Vec<u8>>,
}

fn string(value: Option<&Value>) -> Vec<u8> {
    value.map(Value::to_php_string).unwrap_or_default()
}

/// The theme's `theme.json` entry named `slug` in its list `list`.
fn theme_entry(list: &str, slug: &str) -> Option<Value> {
    let entries = theme().theme_json_list(list);
    entries
        .into_iter()
        .rfind(|entry| entry.get_str("name").map(Value::to_php_string) == Some(slug.into()))
}

/// The theme file of `slug`, as `_get_block_template_file()` finds it.
fn file(kind: Kind, slug: &[u8]) -> Option<File> {
    let names: &[&'static str] = match kind {
        Kind::Template => &TEMPLATE_FILES,
        Kind::Part => &PART_FILES,
    };
    let slug = *names.iter().find(|name| name.as_bytes() == slug)?;
    let field =
        |entry: &Option<Value>, name| string(entry.as_ref().and_then(|entry| entry.get_str(name)));
    Some(match kind {
        // `_add_block_template_info()`: a custom template is for pages
        // unless it names its post types.
        Kind::Template => {
            let custom = theme_entry("customTemplates", slug);
            let pages = Value::Array(vec![(Key::Int(0), Value::String(b"page".to_vec()))]);
            let post_types = custom.as_ref().map(|custom| {
                let types = custom.get_str("postTypes").unwrap_or(&pages);
                types
                    .as_array()
                    .unwrap_or_default()
                    .iter()
                    .map(|(_, t)| t.clone())
                    .collect()
            });
            File {
                slug,
                title: field(&custom, "title"),
                post_types,
                area: None,
            }
        }
        // `_add_block_template_part_area_info()`: an area not allowed, or
        // none, is `uncategorized`.
        Kind::Part => {
            let part = theme_entry("templateParts", slug);
            let area = field(&part, "area");
            let area = AREAS
                .into_iter()
                .find(|known| known.as_bytes() == area)
                .unwrap_or("uncategorized");
            File {
                slug,
                title: field(&part, "title"),
                post_types: None,
                area: Some(area.as_bytes().to_vec()),
            }
        }
    })
}

/// `get_default_block_template_types()`'s title and description of `slug`.
fn default_type(slug: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
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

fn stylesheet(context: &Context) -> Vec<u8> {
    context.option("stylesheet").unwrap_or_default()
}

/// `_build_block_template_result_from_file()`.
fn from_file(context: &Context, file: File, kind: Kind) -> Template {
    let content = match kind {
        Kind::Template => theme()
            .template(&[file.slug.into()])
            .map(|(_, content)| content),
        Kind::Part => theme()
            .part(file.slug.as_bytes())
            .map(|(content, _)| content),
    };
    let theme = stylesheet(context);
    let slug = file.slug.as_bytes().to_vec();
    let mut template = Template {
        id: [&theme[..], b"//", &slug].concat(),
        // Wrapping a part in a template part block, as PHP does, changes
        // none of the theme's (`tests::theme_parts_need_no_wrapper`).
        content: hooked(content.unwrap_or_default(), false, &theme),
        theme,
        title: if is_empty_string(&file.title) {
            slug.clone()
        } else {
            file.title
        },
        slug,
        source: "theme",
        origin: None,
        kind,
        description: Vec::new(),
        status: "publish".into(),
        wp_id: 0,
        has_theme_file: true,
        is_custom: true,
        author: 0,
        post_types: None,
        area: file.area,
        modified: None,
        date: None,
    };
    if kind == Kind::Template {
        if let Some((title, description)) = default_type(&template.slug) {
            template.title = title;
            template.description = description;
            template.is_custom = false;
        }
        template.post_types = file.post_types;
    }
    template
}

/// `preg_match( '/(author|page|single|tag|category|taxonomy)-(.+)/' )`.
fn names_an_entity(slug: &[u8]) -> bool {
    (0..slug.len()).any(|at| {
        let rest = slug.get(at..).unwrap_or_default();
        ["author", "page", "single", "tag", "category", "taxonomy"]
            .iter()
            .any(|word| {
                rest.strip_prefix(word.as_bytes())
                    .and_then(|rest| rest.strip_prefix(b"-"))
                    .and_then(|rest| rest.first())
                    .is_some_and(|first| *first != b'\n')
            })
    })
}

/// What `^word-(.+)$` captures after `word-`: one or more characters but a
/// new line, before an optional final one.
fn after<'a>(slug: &'a [u8], word: &str) -> Option<&'a [u8]> {
    let rest = slug.strip_prefix(word.as_bytes())?.strip_prefix(b"-")?;
    let rest = rest.strip_suffix(b"\n").unwrap_or(rest);
    (!rest.is_empty() && !rest.contains(&b'\n')).then_some(rest)
}

/// `_build_block_template_result_from_post()` of a post with a theme. A
/// template titled by its slug or not at all, and without a description,
/// would be titled after the post, term or author its slug names, and one
/// a plugin added is the plugin's: neither is ported.
fn from_post(context: &Context, post: &Post) -> Result<Template, RestError> {
    let site = context.site;
    let themes = site.post_terms(post.id, "wp_theme");
    let theme = themes.into_iter().next().unwrap_or_default();
    let kind = if post.post_type == "wp_template" {
        Kind::Template
    } else {
        Kind::Part
    };
    let file = (theme.name == stylesheet(context))
        .then(|| file(kind, &post.name))
        .flatten();
    let meta = |key| {
        site.post_meta(post.id, key)
            .filter(|value| !is_empty_string(value))
    };
    let mut template = Template {
        id: [&theme.name[..], b"//", &post.name].concat(),
        content: hooked(&post.content, kind == Kind::Part, &stylesheet(context)),
        theme: theme.name,
        slug: post.name.clone(),
        source: "custom",
        origin: meta("origin"),
        kind,
        description: post.excerpt.clone(),
        title: post.title.clone(),
        status: post.status.clone(),
        wp_id: post.id,
        has_theme_file: file.is_some(),
        is_custom: meta("is_wp_suggestion").is_none(),
        author: post.author,
        post_types: None,
        area: None,
        modified: Some(post.modified.clone()),
        date: Some(post.date.clone()),
    };
    match kind {
        Kind::Template => {
            template.post_types = file.and_then(|file| file.post_types);
            if default_type(&template.slug).is_some() {
                template.is_custom = false;
            }
            let untitled = is_empty_string(&template.title) || template.title == template.slug;
            let titled_by_slug = is_empty_string(&template.description)
                && untitled
                && names_an_entity(&template.slug);
            if titled_by_slug || template.origin.as_deref() == Some(b"plugin") {
                return Err(RestError::not_ported());
            }
        }
        Kind::Part => {
            let areas = site.post_terms(post.id, "wp_template_part_area");
            template.area = areas.into_iter().next().map(|area| area.name);
        }
    }
    Ok(template)
}

/// Whether `post` has a term of `taxonomy` named `name`, compared as the
/// database collation compares names.
fn has_term(context: &Context, post: &Post, taxonomy: &str, name: &[u8]) -> bool {
    let terms = context.site.post_terms(post.id, taxonomy);
    terms
        .iter()
        .any(|term| term.name.eq_ignore_ascii_case(name))
}

/// The posts of `kind` with the theme term `theme`, newest first.
fn theme_posts<'a>(
    context: &'a Context,
    kind: Kind,
    theme: &'a [u8],
) -> impl Iterator<Item = Post> + 'a {
    let posts = context.site.posts_of_type(kind.name()).into_iter();
    posts.filter(move |post| has_term(context, post, "wp_theme", theme))
}

/// `get_block_templates( $query, $template_type )`.
pub(crate) fn templates(
    context: &Context,
    query: &Query,
    kind: Kind,
) -> Result<Vec<Template>, RestError> {
    let names: Vec<Vec<u8>> = query
        .slug_in
        .iter()
        .map(|slug| sanitize_title_for_query(slug))
        .collect();
    let mut unique = names.clone();
    unique.sort();
    unique.dedup();
    let limit = if names.is_empty() {
        usize::MAX
    } else {
        unique.len()
    };
    let id = query.wp_id.as_ref().map(absint);
    let statuses: &[&str] = if id.is_some() {
        &["auto-draft", "draft", "publish"]
    } else {
        &["publish"]
    };
    let theme = stylesheet(context);
    let posts = theme_posts(context, kind, &theme)
        .filter(|post| statuses.contains(&post.status.as_str()))
        .filter(|post| names.is_empty() || names.contains(&post.name))
        .filter(|post| id.is_none_or(|id| id == 0 || post.id == id))
        .filter(|post| match (&query.area, kind) {
            (Some(area), Kind::Part) => has_term(context, post, "wp_template_part_area", area),
            _ => true,
        })
        .take(limit);
    let post_type = query
        .post_type
        .clone()
        .filter(|post_type| !is_empty_string(post_type));
    let built: Result<Vec<Template>, RestError> =
        posts.map(|post| from_post(context, &post)).collect();
    built.map(|built| files_after(context, query, kind, built, post_type.as_ref()))
}

/// The templates of posts `built` for a post type if asked, and the theme
/// files they do not replace unless one post was asked for.
fn files_after(
    context: &Context,
    query: &Query,
    kind: Kind,
    built: Vec<Template>,
    post_type: Option<&Vec<u8>>,
) -> Vec<Template> {
    let for_type =
        |template: &Template| post_type.is_none_or(|post_type| template.custom_for(post_type));
    let mut found: Vec<Template> = built.into_iter().filter(for_type).collect();
    if query.wp_id.is_some() {
        return found;
    }
    let names: &[&'static str] = match kind {
        Kind::Template => &TEMPLATE_FILES,
        Kind::Part => &PART_FILES,
    };
    let skipped: Vec<Vec<u8>> = found.iter().map(|template| template.slug.clone()).collect();
    let files = names
        .iter()
        .filter(|name| {
            query.slug_in.is_empty() || query.slug_in.iter().any(|slug| slug == name.as_bytes())
        })
        .filter(|name| !skipped.iter().any(|slug| slug == name.as_bytes()))
        .filter_map(|name| file(kind, name.as_bytes()))
        .filter(|file| query.area.is_none() || kind == Kind::Template || file.area == query.area);
    for file in files {
        let listed = match (&query.post_type, &file.post_types) {
            (None, _) => true,
            (Some(post_type), Some(types)) => types
                .iter()
                .any(|listed| listed.as_bytes() == Some(post_type.as_slice())),
            (Some(_), None) => default_type(file.slug.as_bytes()).is_none(),
        };
        if listed {
            found.push(from_file(context, file, kind));
        }
    }
    found
}

/// `get_block_template( $id, $template_type )`.
pub(crate) fn template(
    context: &Context,
    id: &[u8],
    kind: Kind,
) -> Result<Option<Template>, RestError> {
    let Some((theme, slug)) = split_id(id) else {
        return Ok(None);
    };
    let name = sanitize_title_for_query(slug);
    let statuses = ["auto-draft", "draft", "publish", "trash"];
    let post = theme_posts(context, kind, theme)
        .find(|post| post.name == name && statuses.contains(&post.status.as_str()));
    if let Some(post) = post {
        return from_post(context, &post).map(Some);
    }
    file_template(context, id, kind)
}

/// `explode( '//', $id, 2 )`, when it gives two parts.
fn split_id(id: &[u8]) -> Option<(&[u8], &[u8])> {
    let at = id.windows(2).position(|pair| pair == b"//")?;
    let (theme, slug) = id.split_at(at);
    Some((theme, slug.get(2..).unwrap_or_default()))
}

/// `get_block_file_template( $id, $template_type )`. A slug that is not a
/// plain file name is resolved by `realpath()`, which is not ported.
pub(crate) fn file_template(
    context: &Context,
    id: &[u8],
    kind: Kind,
) -> Result<Option<Template>, RestError> {
    let Some((theme, slug)) = split_id(id) else {
        return Ok(None);
    };
    if theme != stylesheet(context) {
        return Ok(None);
    }
    if slug.iter().any(|b| matches!(b, b'/' | b'.' | b'\\' | 0)) {
        return Err(RestError::not_ported());
    }
    Ok(file(kind, slug).map(|file| from_file(context, file, kind)))
}

/// `get_template_hierarchy( $slug, $is_custom, $template_prefix )`.
pub(crate) fn hierarchy(slug: &[u8], is_custom: bool, prefix: &[u8]) -> Vec<Vec<u8>> {
    let owned = |items: &[&[u8]]| items.iter().map(|item| item.to_vec()).collect();
    if slug == b"index" {
        return owned(&[b"index"]);
    }
    if is_custom {
        return owned(&[b"page", b"singular", b"index"]);
    }
    if slug == b"front-page" {
        return owned(&[b"front-page", b"home", b"index"]);
    }
    let first = |text: &[u8]| {
        text.split(|&b| b == b'-')
            .next()
            .unwrap_or_default()
            .to_vec()
    };
    let mut hierarchy = vec![slug.to_vec()];
    let prefixed = |word: &str| after(slug, word);
    if !is_empty_string(prefix) {
        let kind = first(prefix);
        if prefix != slug && prefix != kind.as_slice() {
            hierarchy.push(prefix.to_vec());
        }
        if slug != kind.as_slice() {
            hierarchy.push(kind);
        }
    } else if let Some(word) = ["author", "category", "archive", "tag", "page"]
        .into_iter()
        .find(|word| prefixed(word).is_some())
    {
        hierarchy.push(word.as_bytes().to_vec());
    } else if let Some((kind, rest)) = ["taxonomy", "single"]
        .into_iter()
        .find_map(|word| prefixed(word).map(|rest| (word, rest)))
    {
        let items: &[&str] = if kind == "single" {
            &POST_TYPES
        } else {
            &TAXONOMIES
        };
        for item in items {
            if !rest.starts_with(item.as_bytes()) {
                continue;
            }
            if rest == item.as_bytes() {
                hierarchy.push(kind.as_bytes().to_vec());
                break;
            }
            if rest.len() > item.len() + 1 {
                hierarchy.push(format!("{kind}-{item}").into_bytes());
                hierarchy.push(kind.as_bytes().to_vec());
                break;
            }
        }
    }
    let starts = |word: &str| slug.starts_with(word.as_bytes());
    if ["author", "taxonomy", "category", "tag"]
        .into_iter()
        .any(starts)
        || slug == b"date"
    {
        hierarchy.push(b"archive".to_vec());
    }
    if slug == b"attachment" {
        hierarchy.push(b"single".to_vec());
    }
    if starts("single") || starts("page") || slug == b"attachment" {
        hierarchy.push(b"singular".to_vec());
    }
    hierarchy.push(b"index".to_vec());
    hierarchy
}

/// `resolve_block_template( $slug, $hierarchy, '' )`.
pub(crate) fn resolve(
    context: &Context,
    slug: &[u8],
    hierarchy: &[Vec<u8>],
) -> Result<Option<Template>, RestError> {
    if Value::String(slug.to_vec()).is_empty() {
        return Ok(None);
    }
    // `_strip_template_file_suffix()`.
    let slugs: Vec<Vec<u8>> = hierarchy
        .iter()
        .map(|slug| {
            let stripped = slug
                .strip_suffix(b".php")
                .or_else(|| slug.strip_suffix(b".html"));
            stripped.unwrap_or(slug).to_vec()
        })
        .collect();
    let query = Query {
        slug_in: slugs.clone(),
        ..Query::default()
    };
    let found = templates(context, &query, Kind::Template);
    // `array_flip()` keeps a slug's last place; a slug not in it sorts as 0.
    let priority = |template: &Template| {
        slugs
            .iter()
            .rposition(|slug| *slug == template.slug)
            .unwrap_or(0)
    };
    found.map(|mut found| {
        found.sort_by_key(priority);
        found.into_iter().next()
    })
}
