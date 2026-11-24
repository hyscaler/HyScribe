// SPDX-License-Identifier: AGPL-3.0-or-later

//! `site-editor.php` for a user who may edit theme options: its redirects
//! from older addresses, its refusal of an unknown post type, and its page.

use hyscribe_auth::caps::post_type_exists;
use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, add_query_arg, is_empty_string, remove_query_arg};
use hyscribe_render::post::POST_FORMATS;
use hyscribe_render::template_types::{DEFAULT_TYPES, default_type};

use crate::assets::{Assets, Urls};
use crate::editor::form::block_scripts;
use crate::editor::page::{INDEX_FIELDS, editor_assets};
use crate::editor::settings::{Editor, Settings, get_block_editor_settings};
use crate::media::{MEDIA_SCRIPTS, MEDIA_STYLES};
use crate::menu::Screen;
use crate::page::{
    Extras, Page, Request, admin_page_with, json as page_json, object, query_strings,
    text as page_text,
};
use crate::posts::{Answer, PostsSite};
use crate::text;

/// `site-editor.php` with the query `query`, requested as `uri`
/// (`$_SERVER['REQUEST_URI']`): a redirect or `wp_die()`; `None` for the
/// editor's page and arguments that are not strings.
pub fn site_editor(query: &str, uri: &str) -> Option<Answer> {
    let args = query_strings(query)?;
    let get = |name: &str| {
        args.iter()
            .find(|(key, _)| key == name)
            .map(|(_, v)| v.as_str())
    };
    if get("p").is_none() && !is_empty_string(query.as_bytes()) {
        return Some(Answer::Redirect(redirection(&get, uri)));
    }
    let unknown = get("postType").is_some_and(|post_type| !post_type_exists(post_type));
    if unknown && get("postId").is_none() {
        return Some(Answer::Died("Invalid post type."));
    }
    None
}

/// `_wp_get_site_editor_redirection_url()` once it redirects: the new
/// address of the old arguments, else the same with `p=/`.
fn redirection<'a>(get: &dyn Fn(&str) -> Option<&'a str>, uri: &str) -> String {
    let full = |name: &str| get(name).is_some_and(|v| !is_empty_string(v.as_bytes()));
    let id = get("postId").unwrap_or_default();
    let (to, removed): (String, &[&[u8]]) = match (get("postType"), get("path")) {
        (Some("wp_navigation"), _) if full("postId") => {
            (format!("/wp_navigation/{id}"), &[b"postType", b"postId"])
        }
        (Some("wp_navigation"), _) => ("/navigation".into(), &[b"postType"]),
        (_, Some("/wp_global_styles")) => ("/styles".into(), &[b"path"]),
        (Some(kind @ ("page" | "wp_template" | "wp_block")), _) => {
            if full("canvas") && full("postId") {
                (format!("/{kind}/{id}"), &[b"postType", b"postId"])
            } else {
                let list = match kind {
                    "page" => "/page",
                    "wp_template" => "/template",
                    _ => "/pattern",
                };
                (list.into(), &[b"postType"])
            }
        }
        (Some("wp_template_part"), _) if full("canvas") && full("postId") => {
            (format!("/wp_template_part/{id}"), &[b"postType", b"postId"])
        }
        (Some("wp_template_part"), _) => ("/pattern".into(), &[]),
        (_, Some("/wp_template_part/all")) => {
            let uri = remove_query_arg(&[b"path"], uri.as_bytes());
            let args: [(&[u8], Option<&[u8]>); 2] = [
                (b"p", Some(b"/pattern")),
                (b"postType", Some(b"wp_template_part")),
            ];
            return text(&add_query_arg(&args, &uri));
        }
        (_, Some(path @ ("/page" | "/wp_template" | "/patterns" | "/navigation"))) => {
            let to = match path {
                "/wp_template" => "/template",
                "/patterns" => "/pattern",
                _ => path,
            };
            (to.into(), &[b"path"])
        }
        _ => ("/".into(), &[]),
    };
    let uri = remove_query_arg(removed, uri.as_bytes());
    text(&add_query_arg(&[(b"p", Some(to.as_bytes()))], &uri))
}

/// `$indexed_template_types`: `get_default_block_template_types()` as a
/// list, each with its slug.
fn template_types() -> Value {
    let formats = POST_FORMATS
        .iter()
        .map(|(format, _)| format!("taxonomy-post_format-post-format-{format}"));
    let slugs = DEFAULT_TYPES.iter().map(|(slug, _, _)| slug.to_string());
    let types = slugs.chain(formats).map(|slug| {
        let (title, description) = default_type(slug.as_bytes()).unwrap_or_default();
        object(vec![
            ("title", Value::String(title)),
            ("description", Value::String(description)),
            ("slug", Value::String(slug.into_bytes())),
        ])
    });
    list(types)
}

fn list(items: impl Iterator<Item = Value>) -> Value {
    Value::Array((0..).map(Key::Int).zip(items).collect())
}

/// `$custom_settings` for the bundled block theme, with the site's URL and
/// its `posts_per_page`; `styles` keeps its place for the value
/// `get_block_editor_settings()` sets.
pub fn custom_settings(site_url: &str, posts_per_page: Vec<u8>) -> Vec<(&'static str, Value)> {
    vec![
        ("siteUrl", page_text(site_url)),
        ("postsPerPage", Value::String(posts_per_page)),
        ("styles", Value::Null),
        ("defaultTemplateTypes", template_types()),
        (
            "defaultTemplatePartAreas",
            crate::editor::form::part_areas(),
        ),
        ("supportsLayout", Value::Bool(true)),
        ("supportsTemplatePartsMode", Value::Bool(false)),
        // Patterns and categories registered after `init`: none.
        (
            "__experimentalAdditionalBlockPatterns",
            Value::Array(Vec::new()),
        ),
        (
            "__experimentalAdditionalBlockPatternCategories",
            Value::Array(Vec::new()),
        ),
    ]
}

/// The post the page's arguments name, `postId` else `p`'s page: `Some(None)`
/// for none or a missing one; `None` for arguments that are not strings
/// and a post neither a post nor a page, which are not ported.
pub fn context_post(site: &dyn PostsSite, query: &str) -> Option<Option<Post>> {
    let args = query_strings(query)?;
    let get = |name: &str| {
        args.iter()
            .find(|(key, _)| key == name)
            .map(|(_, v)| Value::String(v.clone().into_bytes()))
    };
    let id = match (get("postId"), get("p")) {
        (Some(id), _) if !id.is_empty() && id.is_numeric() => Some(id.to_int()),
        (_, Some(path)) => page_of(&path.to_php_string()),
        _ => None,
    };
    let post = id
        .and_then(|id| u64::try_from(id).ok())
        .and_then(|id| site.post(id));
    match post {
        Some(post) if !["post", "page"].contains(&post.post_type.as_str()) => None,
        post => Some(post),
    }
}

/// The ID `preg_match( '/^\/page\/(\d+)$/', $path )` takes from `path`,
/// whose `$` also matches before a last line break. No digits give 0, of
/// no post, as no match does.
fn page_of(path: &[u8]) -> Option<i64> {
    let path = path.strip_suffix(b"\n").unwrap_or(path);
    let digits = path.strip_prefix(b"/page/")?;
    let numeric = digits.iter().all(u8::is_ascii_digit);
    numeric.then(|| Value::String(digits.to_vec()).to_int())
}

/// `site-editor.php`'s `$preload_paths` with `post`, the post it names:
/// each method and path. `styles` is the theme's global styles post.
pub fn preload_paths(
    post: Option<&Post>,
    styles: u64,
    stylesheet: &str,
) -> Vec<(&'static str, String)> {
    let get = |path: &str| ("GET", path.to_string());
    let options = |path: &str| ("OPTIONS", path.to_string());
    let mut paths = vec![
        options("/wp/v2/media"),
        options("/wp/v2/pages"),
        get("/wp/v2/types?context=view"),
        get("/wp/v2/types/wp_template?context=edit"),
        get("/wp/v2/types/wp_template_part?context=edit"),
        get("/wp/v2/templates?context=edit&per_page=-1"),
        get("/wp/v2/template-parts?context=edit&per_page=-1"),
        get("/wp/v2/themes?context=edit&status=active"),
        get(&format!("/wp/v2/global-styles/{styles}?context=edit")),
        options(&format!("/wp/v2/global-styles/{styles}")),
        get(&format!(
            "/wp/v2/global-styles/themes/{stylesheet}?context=view"
        )),
        get(&format!(
            "/wp/v2/global-styles/themes/{stylesheet}/variations?context=view"
        )),
        options("/wp/v2/navigation"),
        get(
            "/wp/v2/navigation?context=edit&per_page=100&order=desc&orderby=date&status[0]=publish&status[1]=draft",
        ),
        get("/wp/v2/settings"),
        options("/wp/v2/settings"),
        get("/wp/v2/block-patterns/categories"),
        get(&format!("/?_fields={INDEX_FIELDS}")),
    ];
    match post {
        Some(post) if post.post_type == "page" => {
            paths.push(get(&format!("/wp/v2/pages/{}?context=edit", post.id)));
            let slug = match post.name.is_empty() {
                true => "page".into(),
                false => format!("page-{}", String::from_utf8_lossy(&post.name)),
            };
            paths.push(get(&format!("/wp/v2/templates/lookup?slug={slug}")));
        }
        Some(post) => paths.push(get(&format!("/wp/v2/posts/{}?context=edit", post.id))),
        None => {
            paths.push(get("/wp/v2/templates/lookup?slug=front-page"));
            paths.push(get("/wp/v2/templates/lookup?slug=home"));
        }
    }
    paths
}

/// The styles the page enqueues before `admin-header.php`.
const STYLES: [&str; 6] = [
    "wp-edit-site",
    "wp-format-library",
    MEDIA_STYLES[0],
    MEDIA_STYLES[1],
    "wp-block-directory",
    "global-styles-css-custom-properties",
];

/// The scripts the page enqueues before `admin-header.php`.
const SCRIPTS: [&str; 8] = [
    "wp-edit-site",
    "wp-format-library",
    MEDIA_SCRIPTS[0],
    MEDIA_SCRIPTS[1],
    MEDIA_SCRIPTS[2],
    MEDIA_SCRIPTS[3],
    "wp-block-styles",
    "wp-block-directory",
];

/// The page's markup.
const CONTENT: &str = "\n<div class=\"edit-site\" id=\"site-editor\">\n\t\t<div class=\"wrap hide-if-js site-editor-no-js\">\n\t\t<h1 class=\"wp-heading-inline\">Edit Site</h1>\n\t\t<div class=\"notice notice-error hide-if-js\"><p>The site editor requires JavaScript. Please enable JavaScript in your browser settings.</p></div>\t</div>\n</div>\n\n";

/// The site editor's page with `post`, the post its arguments name, and
/// `preloads`, the memo `preloaded()` gives; `None` for a user the menu
/// turns away.
pub fn site_editor_page(
    site: &dyn PostsSite,
    post: Option<&Post>,
    preloads: &Value,
    request: &Request,
) -> Option<String> {
    let site_url = Urls::site_url(site);
    let per_page = site.option("posts_per_page").unwrap_or_default();
    let mut custom = Settings::default();
    for (key, value) in custom_settings(&site_url, per_page) {
        custom.set(key, value);
    }
    let settings = get_block_editor_settings(site, custom, Editor::Site(post));
    let [categories, definitions, bindings] = block_scripts(site.active_theme());
    let edit_site = format!(
        "wp.domReady( function() {{\n\t\t\twp.editSite.initializeEditor( \"site-editor\", {} );\n\t\t}} );",
        page_json(&settings)
    );
    let (templates, media) = crate::media::enqueue_media(site, request.browser, None);
    let more = |assets: &mut Assets| {
        editor_assets(site, assets, preloads);
        for script in [definitions.as_str(), &bindings, &categories] {
            assets.add_inline_script("wp-blocks", "after", script);
        }
        assets.add_inline_script("wp-edit-site", "after", &edit_site);
    };
    let modules = ["@wordpress/latex-to-mathml/loader".to_string()];
    let extras = Extras {
        body_class: " is-fullscreen-mode",
        block_editor: true,
        modules: &modules,
        assets: Some(&more),
        footer: None,
    };
    let page = Page {
        screen: Screen {
            pagenow: "site-editor.php",
            typenow: "",
            taxonomy: "",
            parent_file: Some("themes.php"),
            submenu_file: None,
            plugin_page: None,
        },
        id: "site-editor",
        title: "Editor",
        help: &[],
        sidebar: "",
        screen_options: "",
        styles: &STYLES,
        scripts: &SCRIPTS,
        late_scripts: &[],
        script_data: &media,
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: &templates,
        footer_scripts: "",
        content: CONTENT,
        canonical: request.canonical,
        origin: request.origin,
        view: None,
    };
    admin_page_with(site, &page, request.browser, &extras)
}

#[cfg(test)]
mod tests;
