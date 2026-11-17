// SPDX-License-Identifier: AGPL-3.0-or-later

//! `site-editor.php` for a user who may edit theme options: its redirects
//! from older addresses and its refusal of an unknown post type; the
//! editor's page is not ported.

use hyscribe_auth::caps::post_type_exists;
use hyscribe_phpser::{Key, Value, add_query_arg, is_empty_string, remove_query_arg};
use hyscribe_render::post::POST_FORMATS;
use hyscribe_render::template_types::{DEFAULT_TYPES, PART_AREAS, default_type};

use crate::page::{object, query_strings, text as page_text};
use crate::posts::Answer;
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

/// `get_allowed_block_template_part_areas()` as PHP lists it.
fn part_areas() -> Value {
    list(PART_AREAS.iter().map(|area| {
        object(vec![
            ("area", page_text(area.area)),
            ("label", page_text(area.label)),
            ("description", page_text(area.description)),
            ("icon", page_text(area.icon)),
            ("area_tag", page_text(area.tag)),
        ])
    }))
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
        ("defaultTemplatePartAreas", part_areas()),
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

#[cfg(test)]
mod tests;
