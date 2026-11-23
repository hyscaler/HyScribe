// SPDX-License-Identifier: AGPL-3.0-or-later

//! Block themes as data (docs/EXTENSIONS.md): templates, template parts and
//! patterns. The embedded themes, Twenty Twenty-Five (the default install's)
//! among them, are built in; each port lives in `themes/<slug>/`. A child
//! theme (`Template:` in its `style.css`) has only its own files: lookups go
//! to it first and then to its parent, as PHP's do.

use std::collections::HashMap;

use hyscribe_phpser::{Key, Value, json_decode};
pub use hyscribe_theme_json::GlobalStyles;
use hyscribe_theme_json::{BlockType as Metadata, Sources};

/// A theme port's data, embedded at build time from its folder in
/// `themes/` (build.rs).
#[derive(Debug)]
pub(crate) struct Data {
    slug: &'static str,
    /// Every file of the theme but its PHP, by path, with its content type.
    files: &'static [(&'static str, &'static str, &'static [u8])],
    /// The export's own files, which are not served.
    patterns: &'static [u8],
    registered: &'static [u8],
    styles: &'static [u8],
}

impl Data {
    /// One of the theme's files as the web server sends it from the
    /// theme's folder: its content type and bytes.
    fn file(&self, path: &str) -> Option<(&'static str, &'static [u8])> {
        self.files
            .iter()
            .find(|(name, _, _)| *name == path)
            .map(|(_, content_type, bytes)| (*content_type, *bytes))
    }

    /// The embedded theme this one is a child of: the one `Template` in its
    /// `style.css` names, when it names another.
    fn parent(&self) -> Option<&'static Data> {
        let template = header(self.style_css(), "Template");
        THEMES
            .iter()
            .find(|data| data.slug == template && data.slug != self.slug)
    }

    /// The text of the theme's `style.css`.
    fn style_css(&self) -> &'static str {
        let css = self.file("style.css").map_or(&[][..], |(_, bytes)| bytes);
        std::str::from_utf8(css).unwrap_or_default()
    }

    /// The names (without folder and extension) and contents of the theme's
    /// files in `folder` ending in `extension`.
    fn folder(
        &self,
        folder: &'static str,
        extension: &'static str,
    ) -> impl Iterator<Item = (&'static str, &'static [u8])> {
        self.files.iter().filter_map(move |(name, _, bytes)| {
            let rest = name.strip_prefix(folder)?.strip_prefix('/')?;
            Some((rest.strip_suffix(extension)?, *bytes))
        })
    }
}

include!(concat!(env!("OUT_DIR"), "/themes.rs"));

/// The theme the site falls back to, `WP_DEFAULT_THEME`.
pub const DEFAULT: &str = "twentytwentyfive";

/// The slugs of the embedded themes, in order.
pub fn slugs() -> impl Iterator<Item = &'static str> {
    THEMES.iter().map(|data| data.slug)
}

/// What the Themes screen lists of the embedded theme `slug` that its files
/// give, as `reference/rest/registered.php` exports it: `id`, `name`,
/// `description`, `author`, `authorAndUri`, `tags` and `version`.
pub fn screen_entry(slug: &str) -> Option<Value> {
    let data = THEMES.iter().find(|data| data.slug == slug)?;
    let registered = json_decode(data.registered).unwrap_or_default();
    registered.get_str("themes_screen").cloned()
}

/// The template of the embedded theme `slug`, `get_template()`: its
/// parent's slug when it is a child theme, else its own.
pub fn template_of(slug: &str) -> Option<&'static str> {
    let data = THEMES.iter().find(|data| data.slug == slug)?;
    Some(data.parent().map_or(data.slug, |parent| parent.slug))
}

/// The `Theme Name` of the parent of the embedded child theme `slug`, none
/// for a theme that is no child.
pub fn parent_name(slug: &str) -> Option<String> {
    let data = THEMES.iter().find(|data| data.slug == slug)?;
    let parent = data.parent()?;
    Some(header(parent.style_css(), "Theme Name"))
}

/// The embedded theme `slug` names, or else the default, as WordPress
/// falls back to its default theme.
pub fn resolve(slug: &str) -> &'static str {
    slugs().find(|known| *known == slug).unwrap_or(DEFAULT)
}

/// One of an embedded theme's files, as the web server sends it from the
/// theme's folder: its content type and bytes.
pub fn theme_file(slug: &str, path: &str) -> Option<(&'static str, &'static [u8])> {
    THEMES
        .iter()
        .find(|data| data.slug == slug)
        .and_then(|data| data.file(path))
}

/// The theme's style variations, `WP_Theme_JSON_Resolver::get_style_variations()`
/// in path order: its `styles/` files that are not for a block type, and
/// those of its parent's that a child has no file of the same name for. A
/// path starts with the theme's slug, as PHP's start with its folder.
pub(crate) fn style_variations(theme: &Theme) -> impl Iterator<Item = Value> {
    let files = |data: &'static Data| {
        data.files
            .iter()
            .filter(|(name, _, _)| name.starts_with("styles/") && name.ends_with(".json"))
            .map(move |(name, _, bytes)| {
                let base = name.rsplit('/').next().unwrap_or(name);
                (base, format!("{}/{name}", data.slug), *bytes)
            })
    };
    let mut found: Vec<(&str, String, &[u8])> = files(theme.data).collect();
    if let Some(parent) = theme.parent {
        let own: Vec<&str> = found.iter().map(|(base, _, _)| *base).collect();
        let inherited: Vec<_> = files(parent)
            .filter(|(base, _, _)| !own.contains(base))
            .collect();
        found.extend(inherited);
    }
    found.sort_by(|a, b| a.1.cmp(&b.1));
    found
        .into_iter()
        .filter_map(|(_, _, bytes)| json_decode(bytes))
        .filter(|variation| variation.get_str("blockTypes").is_none())
}

const CORE_THEME_JSON: &[u8] = include_bytes!("../../../wordpress/wp-includes/theme.json");
const REGISTERED_BLOCKS: &[u8] =
    include_bytes!("../../../wordpress/wp-includes/blocks/registered.json");

/// A stylesheet of the theme's own files that it enqueues.
#[derive(Debug)]
pub(crate) struct OwnStyle {
    /// The block whose rendering enqueues it, `None` for `wp_enqueue_scripts`.
    pub(crate) block: Option<String>,
    pub(crate) handle: String,
    /// Its path in the folder of the theme named by `theme`.
    pub(crate) src: String,
    /// The slug of the theme whose folder has it: a child theme enqueues
    /// its parent's stylesheet from the parent's URL.
    pub(crate) theme: &'static str,
    pub(crate) css: &'static str,
    /// `ver`; `None` for the WordPress version.
    pub(crate) version: Option<String>,
}

/// A block theme.
#[derive(Debug)]
pub struct Theme {
    /// The theme's stylesheet, a child theme's own slug.
    pub(crate) slug: &'static str,
    /// The theme's template, its parent's slug for a child theme.
    pub(crate) template: &'static str,
    pub(crate) data: &'static Data,
    /// The theme this one is a child of.
    pub(crate) parent: Option<&'static Data>,
    pub(crate) templates: HashMap<&'static str, &'static [u8]>,
    pub(crate) parts: HashMap<&'static str, &'static [u8]>,
    /// The area of each template part, from `theme.json`.
    pub(crate) areas: HashMap<Vec<u8>, Vec<u8>>,
    pub(crate) patterns: HashMap<Vec<u8>, Vec<u8>>,
    /// Core and theme `theme.json` merged, as `wp_get_global_settings()`
    /// and `wp_get_global_styles()` read them.
    pub(crate) global: GlobalStyles,
    /// The style handles each core block's `block.json` names, which
    /// rendering the block enqueues, then the theme's own for the block.
    pub(crate) style_handles: HashMap<String, Vec<String>>,
    /// The `inline_style` of each block style the theme registers, by
    /// block, which `enqueue_block_styles_assets()` adds to its stylesheet.
    pub(crate) inline_block_styles: Vec<(String, String)>,
    /// The stylesheets of its own files the theme enqueues.
    pub(crate) styles: Vec<OwnStyle>,
    pub(crate) post_formats: Vec<&'static str>,
    /// `registered.json` as decoded: what the theme registers.
    pub(crate) registered: Value,
    /// `patterns.json` as decoded: each pattern's title, content, and its
    /// description and categories where it has them.
    pub(crate) pattern_data: Value,
}

/// The post formats WordPress knows, which `add_theme_support( 'post-formats' )`
/// can list.
const POST_FORMATS: [&str; 10] = [
    "standard", "aside", "audio", "chat", "gallery", "image", "link", "quote", "status", "video",
];

/// The default theme, Twenty Twenty-Five, as bundled with WordPress 7.1.3.
pub fn twentytwentyfive() -> Theme {
    load(DEFAULT, None)
}

/// The theme `slug`, an embedded one or else the default as WordPress
/// falls back to it, with the site's global styles, `user` the content of
/// its `wp_global_styles` post if it has one.
pub fn load(slug: &str, user: Option<&[u8]>) -> Theme {
    with_user_styles(slug, user, &[])
}

/// The global styles of a classic theme whose `theme.json` is `theme_json`,
/// with `user` as the site's global styles (ADR 0023).
pub fn classic_global_styles(theme_json: &[u8], user: Option<&[u8]>) -> GlobalStyles {
    let blocks = registered_blocks(REGISTERED_BLOCKS);
    GlobalStyles::resolve(&Sources {
        core: CORE_THEME_JSON,
        blocks: &blocks,
        block_styles: &[],
        theme: Some(theme_json),
        parent_theme: None,
        theme_styles: &[],
        user,
    })
}

/// The default theme with `user` as the site's global styles and `styles`
/// registered after its own block styles.
#[cfg(test)]
pub(crate) fn twentytwentyfive_with(user: &[u8], styles: &[(String, String)]) -> Theme {
    with_user_styles(DEFAULT, Some(user), styles)
}

fn with_user_styles(slug: &str, user: Option<&[u8]>, styles: &[(String, String)]) -> Theme {
    let slug = resolve(slug);
    let data = THEMES
        .iter()
        .find(|data| data.slug == slug)
        .unwrap_or(&THEMES[0]);
    let parent = data.parent();
    // `WP_Theme::get_file_path( 'theme.json' )`: the child's own file, else
    // its parent's, which a child's own is merged over.
    let own_json = data.file("theme.json").map(|(_, bytes)| bytes);
    let parent_json = parent.and_then(|parent| parent.file("theme.json").map(|(_, bytes)| bytes));
    let blocks = registered_blocks(REGISTERED_BLOCKS);
    let registered = block_styles(data.registered);
    let mut block_styles: Vec<(String, String)> = registered
        .iter()
        .map(|(block, name, _)| (block.clone(), name.clone()))
        .collect();
    block_styles.extend_from_slice(styles);
    let inline_block_styles = registered
        .into_iter()
        .filter(|(_, _, css)| !css.is_empty())
        .map(|(block, _, css)| (block, css))
        .collect();
    let own = own_styles(data, parent);
    let theme_styles = block_styles_support(data.registered);
    let mut style_handles: HashMap<String, Vec<String>> = blocks
        .iter()
        .map(|block| {
            (
                block.name.clone(),
                style_handles(&block.name, &block.metadata, theme_styles),
            )
        })
        .collect();
    for style in &own {
        if let Some(block) = &style.block {
            style_handles
                .entry(block.clone())
                .or_default()
                .push(style.handle.clone());
        }
    }
    // The parent's files first, which the child's of the same name replace.
    let inherited = |folder| {
        parent
            .into_iter()
            .flat_map(move |p| p.folder(folder, ".html"))
    };
    let global = GlobalStyles::resolve(&Sources {
        core: CORE_THEME_JSON,
        blocks: &blocks,
        block_styles: &block_styles,
        theme: Some(own_json.or(parent_json).unwrap_or_default()),
        parent_theme: own_json.and(parent_json),
        theme_styles: &style_partials(data.styles),
        user,
    });
    Theme {
        slug: data.slug,
        template: parent.map_or(data.slug, |parent| parent.slug),
        data,
        parent,
        templates: inherited("templates")
            .chain(data.folder("templates", ".html"))
            .collect(),
        parts: inherited("parts")
            .chain(data.folder("parts", ".html"))
            .collect(),
        areas: part_areas(global.list("templateParts")),
        patterns: patterns(data.patterns),
        global,
        style_handles,
        inline_block_styles,
        styles: own,
        post_formats: post_formats(data.registered),
        registered: json_decode(data.registered).unwrap_or_default(),
        pattern_data: json_decode(data.patterns).unwrap_or_default(),
    }
}

/// `registered.json`'s `styles`: the stylesheets of the theme's files it
/// enqueues, with their contents; those the export marks `parent` are files
/// of the child's parent.
fn own_styles(data: &'static Data, parent: Option<&'static Data>) -> Vec<OwnStyle> {
    let root = json_decode(data.registered).unwrap_or_default();
    let text = |value: Option<&Value>| {
        let bytes = value.and_then(Value::as_bytes)?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    };
    root.get_str("styles")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|(_, style)| {
            let src = text(style.get_str("src"))?;
            let from = match style.get_str("parent") {
                Some(Value::Bool(true)) => parent.unwrap_or(data),
                _ => data,
            };
            let css = from.file(&src).map_or(&[][..], |(_, bytes)| bytes);
            Some(OwnStyle {
                block: text(style.get_str("block")),
                handle: text(style.get_str("handle"))?,
                css: std::str::from_utf8(css).unwrap_or_default(),
                version: text(style.get_str("ver")),
                theme: from.slug,
                src,
            })
        })
        .collect()
}

/// `registered.json`'s `block_styles_support`: whether `functions.php` adds
/// theme support for `wp-block-styles`.
fn block_styles_support(registered: &[u8]) -> bool {
    let root = json_decode(registered).unwrap_or_default();
    matches!(
        root.get_str("block_styles_support"),
        Some(Value::Bool(true))
    )
}

/// `registered.json`'s `post_formats`, the formats `functions.php` adds
/// theme support for.
fn post_formats(registered: &[u8]) -> Vec<&'static str> {
    let root = json_decode(registered).unwrap_or_default();
    let listed = root
        .get_str("post_formats")
        .and_then(Value::as_array)
        .unwrap_or_default();
    POST_FORMATS
        .into_iter()
        .filter(|format| {
            listed
                .iter()
                .any(|(_, listed)| listed.as_bytes() == Some(format.as_bytes()))
        })
        .collect()
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
/// whatever `block.json` names there, then the other handles it lists. The
/// query block's own is left out: `render_block_core_query()` removes it
/// unless the query is interactive, which `Renderer::query` enqueues. A theme
/// with `wp-block-styles` support adds the block's `-theme` handle last.
fn style_handles(name: &str, metadata: &Value, theme_styles: bool) -> Vec<String> {
    let text = |handle: &Value| String::from_utf8_lossy(&handle.to_php_string()).into_owned();
    let mut handles = Vec::new();
    // A block registered without a block.json (the legacy post comments)
    // has no handle of its own, as its metadata names the handles of others.
    let from_file = metadata.get_str("apiVersion").is_some();
    if name != "core/query" && from_file {
        handles.push(format!("wp-block-{}", name.trim_start_matches("core/")));
    }
    if let Some(Value::Array(listed)) = metadata.get_str("style") {
        handles.extend(listed.iter().skip(1).map(|(_, handle)| text(handle)));
    }
    if theme_styles && from_file {
        handles.push(format!(
            "wp-block-{}-theme",
            name.trim_start_matches("core/")
        ));
    }
    handles
}

impl Theme {
    /// The theme's global settings and styles, without the site's own:
    /// `WP_Theme_JSON_Resolver::get_merged_data( 'theme' )`.
    pub fn global_styles(&self) -> &GlobalStyles {
        &self.global
    }

    /// `WP_Theme_JSON_Resolver::get_style_variations()`: the theme's style
    /// variations as raw data, each titled.
    pub fn style_variations(&self) -> Vec<Value> {
        style_variations(self)
            .map(|variation| self.global.theme_json(variation))
            .collect()
    }

    /// The list `name` of what the theme registers, as
    /// `reference/rest/registered.php` exports it: `block_styles`,
    /// `partial_styles`, `pattern_categories`, `patterns`, `rest`,
    /// `template_files` and `part_files`.
    pub fn registered(&self, name: &str) -> &[(Key, Value)] {
        let list = self.registered.get_str(name);
        list.and_then(Value::as_array).unwrap_or_default()
    }

    /// A theme pattern as exported: its title, content, and its description
    /// and categories where it has them.
    pub fn pattern_entry(&self, slug: &[u8]) -> Option<&Value> {
        self.pattern_data.get(&Key::String(slug.to_vec()))
    }

    /// The stylesheets of the theme's own files that `wp_enqueue_block_style()`
    /// enqueues in the block editor: each one's handle, its path under the
    /// site's address, and its `ver` (empty for the WordPress version).
    pub fn editor_styles(&self) -> Vec<(&str, String, &str)> {
        let own = self.styles.iter().filter(|style| style.block.is_some());
        own.map(|style| {
            let src = format!("/wp-content/themes/{}/{}", style.theme, style.src);
            (
                style.handle.as_str(),
                src,
                style.version.as_deref().unwrap_or_default(),
            )
        })
        .collect()
    }

    /// Whether the theme adds an editor stylesheet, `add_editor_style()`: its
    /// theme support `editor-style`.
    pub fn has_editor_style(&self) -> bool {
        let mut features = self.registered("theme_features").iter();
        features.any(|(_, feature)| feature.as_bytes() == Some(b"editor-style"))
    }

    /// The post formats `functions.php` adds theme support for.
    pub fn post_formats(&self) -> &[&'static str] {
        &self.post_formats
    }

    /// `get_page_templates( null, $post_type )`: the slug and title of each
    /// of `theme.json`'s custom templates for `post_type`, by title.
    pub fn page_templates(&self, post_type: &str) -> Vec<(String, String)> {
        custom_templates(self.global.list("customTemplates"), post_type)
    }

    /// The list `name` of the theme's `theme.json` (a child's merged over
    /// its parent's), such as `customTemplates` or `templateParts`.
    pub fn theme_json_list(&self, name: &str) -> Vec<Value> {
        let list = self.global.list(name);
        list.iter().map(|(_, item)| item.clone()).collect()
    }

    /// The theme's `Theme Name` header.
    pub fn name(&self) -> String {
        header(self.style_css(), "Theme Name")
    }

    /// The text of the theme's `style.css`.
    fn style_css(&self) -> &'static str {
        self.data.style_css()
    }

    /// The theme's slug, its `stylesheet`.
    pub fn slug(&self) -> &str {
        self.slug
    }

    /// The theme's `template`, `get_template()`: its parent's slug when it
    /// is a child theme, else its own.
    pub fn template_slug(&self) -> &str {
        self.template
    }

    /// The URL of a file of the theme under the site at `siteurl`,
    /// `get_theme_file_uri()`: the child's file if it has one, else the
    /// template's.
    pub fn file_uri(&self, siteurl: &str, path: &str) -> String {
        let folder = match self.data.file(path) {
            Some(_) => self.slug,
            None => self.template,
        };
        format!("{siteurl}/wp-content/themes/{folder}/{path}")
    }

    /// The theme's `template_files` or `part_files`: the slugs of the
    /// child's own files in the order PHP lists them, then its parent's
    /// that the child has no file of the same name for.
    pub fn file_slugs(&self, list: &str) -> Vec<String> {
        let names = |registered: &Value| {
            let files = registered.get_str(list).and_then(Value::as_array);
            let files = files.unwrap_or_default().iter();
            let text = |(_, name): &(Key, Value)| {
                String::from_utf8_lossy(&name.to_php_string()).into_owned()
            };
            files.map(text).collect::<Vec<String>>()
        };
        let mut slugs = names(&self.registered);
        if let Some(parent) = self.parent {
            let inherited = names(&json_decode(parent.registered).unwrap_or_default());
            let new: Vec<String> = inherited
                .into_iter()
                .filter(|slug| !slugs.contains(slug))
                .collect();
            slugs.extend(new);
        }
        slugs
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

    /// A registered pattern's block markup, as exported: its theme file URLs
    /// are those of the export's site (see [`under_site`]).
    pub fn pattern(&self, slug: &[u8]) -> Option<&[u8]> {
        self.patterns.get(slug).map(Vec::as_slice)
    }
}

/// The content URL the theme export ran under, which PHP printed the URLs of
/// the theme's files with.
const EXPORTED_CONTENT_URL: &[u8] = b"http://127.0.0.1:8081/wp-content";

/// An exported pattern's markup with its theme file URLs under the content
/// URL of the site at `siteurl`, as `get_theme_file_uri()` prints them.
pub fn under_site(content: &[u8], siteurl: &[u8]) -> Vec<u8> {
    let own = [siteurl, b"/wp-content"].concat();
    crate::template::replace(content, EXPORTED_CONTENT_URL, &own)
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

/// `registered.json`'s `block_styles`: the block styles registered with
/// the theme, by block and style name, with their `inline_style`.
fn block_styles(json: &[u8]) -> Vec<(String, String, String)> {
    let root = json_decode(json).unwrap_or_default();
    let text = |value: Option<&Value>| {
        let bytes = value.and_then(Value::as_bytes)?;
        Some(String::from_utf8_lossy(bytes).into_owned())
    };
    root.get_str("block_styles")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|(_, entry)| {
            let block = text(entry.get(&Key::Int(0)))?;
            let style = entry.get(&Key::Int(1))?;
            let css = text(style.get_str("inline_style")).unwrap_or_default();
            Some((block, text(style.get_str("name"))?, css))
        })
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

/// The custom templates of `theme.json`'s `customTemplates` list for
/// `post_type`, by title.
fn custom_templates(custom: &[(Key, Value)], post_type: &str) -> Vec<(String, String)> {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    let mut templates: Vec<(String, String)> = custom
        .iter()
        .filter(|(_, template)| match template.get_str("postTypes") {
            Some(Value::Array(types)) => types
                .iter()
                .any(|(_, name)| name.as_bytes() == Some(post_type.as_bytes())),
            _ => post_type == "page",
        })
        .filter_map(|(_, template)| {
            let name = string(template, "name")?;
            Some((text(name), text(string(template, "title").unwrap_or(name))))
        })
        .collect();
    // `array_flip()` keeps the last template of a title; `ksort()` compares
    // numeric titles as numbers.
    let mut kept: Vec<(String, String)> = Vec::new();
    for template in templates.drain(..).rev() {
        if !kept.iter().any(|(_, title)| *title == template.1) {
            kept.push(template);
        }
    }
    let number = |title: &str| {
        let value = Value::String(title.as_bytes().to_vec());
        value.is_numeric().then(|| value.to_float())
    };
    kept.sort_by(|a, b| match (number(&a.1), number(&b.1)) {
        (Some(x), Some(y)) => x.total_cmp(&y),
        _ => a.1.cmp(&b.1),
    });
    kept
}

/// `templateParts` in `theme.json`: part name to area.
fn part_areas(parts: &[(Key, Value)]) -> HashMap<Vec<u8>, Vec<u8>> {
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
