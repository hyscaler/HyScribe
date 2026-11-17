// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Templates_Controller` for `wp_template` and `wp_template_part`:
//! `/wp/v2/templates` and `/wp/v2/template-parts`, their `/lookup` and
//! their templates by `theme//slug`, read from the active theme's files and
//! the site's template posts.

pub(crate) mod content;
mod query;
mod write;

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, is_empty_string};
use hyscribe_render::date::{Zone, format};
use hyscribe_render::post::admin_title;

use crate::fields::{fields_for_response, is_field_included, links_wanted};
use crate::request::{Arg, Params, Sanitize, string_arg, typed};
use crate::response::{Response, RestError, list, object, text};
use crate::types::{can_edit_any, context_arg};
use crate::{Context, Endpoint};

use content::resolve_patterns;
pub(crate) use query::Template;
use query::{Kind, Query, file_template, hierarchy, resolve, templates, theme};
pub(crate) use write::writing;

/// The template's ID: a theme folder, maybe in a subfolder, and a slug.
const ID: &str = "([^\\/:<>\\*\\?\"\\|]+(?:\\/[^\\/:<>\\*\\?\"\\|]+)?)[\\/\\w%-]+";

/// The schema's properties both post types have, and their contexts.
const PROPERTIES: [(&str, &[&str]); 17] = [
    ("id", &["embed", "view", "edit"]),
    ("slug", &["embed", "view", "edit"]),
    ("theme", &["embed", "view", "edit"]),
    ("type", &["embed", "view", "edit"]),
    ("source", &["embed", "view", "edit"]),
    ("origin", &["embed", "view", "edit"]),
    ("content", &["embed", "view", "edit"]),
    ("title", &["embed", "view", "edit"]),
    ("description", &["embed", "view", "edit"]),
    ("status", &["embed", "view", "edit"]),
    ("wp_id", &["embed", "view", "edit"]),
    ("has_theme_file", &["embed", "view", "edit"]),
    ("author", &["view", "edit", "embed"]),
    ("modified", &["view", "edit"]),
    ("author_text", &["view", "edit", "embed"]),
    ("original_source", &["view", "edit", "embed"]),
    ("date", &["view", "edit"]),
];

fn properties(kind: Kind) -> Vec<(&'static str, &'static [&'static str])> {
    let own: &[(&str, &[&str])] = match kind {
        Kind::Template => &[
            ("is_custom", &["embed", "view", "edit"]),
            ("plugin", &["view", "edit", "embed"]),
        ],
        Kind::Part => &[("area", &["embed", "view", "edit"])],
    };
    PROPERTIES.iter().chain(own).copied().collect()
}

fn base(kind: Kind) -> &'static str {
    match kind {
        Kind::Template => "templates",
        Kind::Part => "template-parts",
    }
}

/// The callback of the controller of `kind`.
fn per_kind(
    kind: Kind,
    template: crate::Callback,
    part: crate::Callback,
) -> Option<crate::Callback> {
    Some(match kind {
        Kind::Template => template,
        Kind::Part => part,
    })
}

/// A write endpoint, which [`writing`] plans: its permission check, which
/// `Allow` and links read, after its `args`.
fn write(args: Vec<Arg>) -> Endpoint {
    Endpoint {
        args,
        permission: manage,
        callback: None,
    }
}

pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    let (kind, rest) = write::route(regex)?;
    let id = Arg {
        name: "id",
        schema: object([("type", text("string"))]),
        validate: false,
        sanitize: Sanitize::Urldecode,
    };
    match rest {
        "" => Some(vec![
            Endpoint {
                args: vec![
                    context_arg(),
                    Arg {
                        name: "wp_id",
                        schema: object([("type", text("integer"))]),
                        validate: false,
                        sanitize: Sanitize::Schema,
                    },
                    string_arg("area"),
                    string_arg("post_type"),
                ],
                permission: read,
                callback: per_kind(
                    kind,
                    |context, params| items(context, params, Kind::Template),
                    |context, params| items(context, params, Kind::Part),
                ),
            },
            write(write::save_args(kind, true)),
        ]),
        "/lookup" => Some(vec![Endpoint {
            args: vec![
                Arg {
                    name: "slug",
                    schema: object([("type", text("string")), ("required", Value::Bool(true))]),
                    validate: false,
                    sanitize: Sanitize::Schema,
                },
                Arg {
                    name: "is_custom",
                    schema: object([("type", text("boolean"))]),
                    validate: false,
                    sanitize: Sanitize::Schema,
                },
                string_arg("template_prefix"),
            ],
            permission: read,
            callback: per_kind(
                kind,
                |context, params| lookup(context, params, Kind::Template),
                |context, params| lookup(context, params, Kind::Part),
            ),
        }]),
        _ if rest == format!("/(?P<id>{ID})") => Some(vec![
            Endpoint {
                args: vec![id.clone(), context_arg()],
                permission: read,
                callback: per_kind(
                    kind,
                    |context, params| item(context, params, Kind::Template),
                    |context, params| item(context, params, Kind::Part),
                ),
            },
            write([vec![id.clone()], write::save_args(kind, false)].concat()),
            write(vec![
                id,
                typed("force", r#"{"type":"boolean","default":false}"#),
            ]),
        ]),
        _ => None,
    }
}

fn cannot_manage(context: &Context) -> RestError {
    let message = "Sorry, you are not allowed to access the templates on this site.";
    context.forbidden("rest_cannot_manage_templates", message)
}

/// `get_items_permissions_check()` and `get_item_permissions_check()`.
fn read(context: &Context, _: &Params) -> Result<(), RestError> {
    if can_edit_any(context) {
        return Ok(());
    }
    Err(cannot_manage(context))
}

/// `permissions_check()`, which every write checks.
fn manage(context: &Context, _: &Params) -> Result<(), RestError> {
    if context.can("edit_theme_options") {
        return Ok(());
    }
    Err(cannot_manage(context))
}

/// `get_items()`.
fn items(context: &Context, params: &Params, kind: Kind) -> Result<Response, RestError> {
    if context.head {
        return Ok(Response::new(Value::Array(Vec::new()), 200));
    }
    let bytes = |name| params.get(name).map(Value::to_php_string);
    let query = Query {
        slug_in: Vec::new(),
        wp_id: params.get("wp_id").cloned(),
        area: bytes("area"),
        post_type: bytes("post_type"),
    };
    let found = templates(context, &query, kind);
    let data = found.and_then(|found| {
        let prepared = found
            .iter()
            .map(|template| prepare(context, params, template, kind));
        prepared
            .map(|response| response.map(Response::into_data))
            .collect()
    });
    data.map(|data| Response::new(list(data), 200))
}

/// `_sanitize_template_id()` after its `urldecode()`: the slug's slash
/// doubled when it is not.
fn template_id(params: &Params) -> Vec<u8> {
    let id = params.text("id");
    let Some(at) = id.iter().rposition(|&b| b == b'/') else {
        return id;
    };
    // `substr( $id, -1, 1 )` before a slash that starts the ID.
    let before = at.checked_sub(1).map_or(id.last(), |before| id.get(before));
    if before == Some(&b'/') {
        return id;
    }
    let (start, end) = id.split_at(at);
    [start, b"/", end].concat()
}

/// `get_item()`.
fn item(context: &Context, params: &Params, kind: Kind) -> Result<Response, RestError> {
    let id = template_id(params);
    let source = params.get("source").and_then(Value::as_bytes);
    let found = if matches!(source, Some(b"theme" | b"plugin")) {
        file_template(context, &id, kind)
    } else {
        query::template(context, &id, kind)
    };
    found.and_then(|found| {
        let message = "No templates exist with that id.";
        let found = found.ok_or_else(|| RestError::status("rest_template_not_found", message, 404));
        found.and_then(|found| prepare(context, params, &found, kind))
    })
}

/// `get_template_fallback()`: the first template of the slug's hierarchy
/// with content, or an empty object.
fn lookup(context: &Context, params: &Params, kind: Kind) -> Result<Response, RestError> {
    let slug = params.text("slug");
    let is_custom = params.get("is_custom") == Some(&Value::Bool(true));
    let prefix = params.get("template_prefix").map(Value::to_php_string);
    let hierarchy = hierarchy(&slug, is_custom, &prefix.unwrap_or_default());
    // Each time with the hierarchy's first slug off, until one has content.
    let mut found = Ok(None);
    for start in 0..hierarchy.len() {
        let tail = hierarchy.get(start..).unwrap_or_default();
        found = resolve(context, &slug, tail);
        let content = |found: &Option<Template>| {
            found
                .as_ref()
                .is_some_and(|found| !is_empty_string(&found.content))
        };
        if found.as_ref().map_or(true, content) {
            break;
        }
    }
    found.and_then(|found| match found {
        Some(found) => prepare(context, params, &found, kind),
        // `_fields` of an object fails in PHP.
        None if params.has("_fields") => Err(RestError::not_ported()),
        None => {
            let empty = Value::Object {
                class: b"stdClass".to_vec(),
                properties: Vec::new(),
            };
            Ok(Response::new(empty, 200))
        }
    })
}

/// `mysql_to_rfc3339()` of a stored date, `null` for none.
fn rfc3339(date: Option<&String>) -> Value {
    date.filter(|date| !date.is_empty())
        .map_or(Value::Null, |date| {
            text(format("Y-m-d\\TH:i:s", date, Zone::Utc))
        })
}

/// `get_wp_templates_original_source_field()` of a template no plugin
/// added.
fn original_source(template: &Template) -> &'static str {
    let origin = template.origin.as_deref();
    // Every template's source is `theme` or `custom`.
    let theme = origin.is_none_or(|origin| origin == b"theme");
    if template.has_theme_file && theme {
        return "theme";
    }
    if !template.has_theme_file && template.source == "custom" && template.author == 0 {
        return "site";
    }
    "user"
}

/// `get_wp_templates_author_text_field()`.
fn author_text(context: &Context, template: &Template) -> Vec<u8> {
    match original_source(template) {
        "theme" => theme().name().into_bytes(),
        "site" => context.option("blogname").unwrap_or_default(),
        _ => context
            .site
            .user(template.author)
            .map_or_else(|| b"Unknown author".to_vec(), |user| user.display_name),
    }
}

/// `prepare_item_for_response()` by the controller of `kind`, through
/// `rest_prepare_{$post_type}`.
fn prepare(
    context: &Context,
    params: &Params,
    template: &Template,
    kind: Kind,
) -> Result<Response, RestError> {
    let response = if context.head {
        Ok(Response::new(Value::Array(Vec::new()), 200))
    } else {
        resolve_patterns(&template.content)
            .and_then(|content| prepare_resolved(context, params, template, kind, &content))
    };
    let hook = format!("rest_prepare_{}", kind.name());
    response.map(|response| crate::hooks::apply_filters(&hook, response, &[template, params]))
}

/// [`prepare`] of a template whose patterns are resolved in `content`.
fn prepare_resolved(
    context: &Context,
    params: &Params,
    template: &Template,
    kind: Kind,
    content: &[u8],
) -> Result<Response, RestError> {
    let properties = properties(kind);
    let wanted = fields_for_response(params, &properties);
    let included = |field: &str| is_field_included(field, &wanted);
    let mut data: Vec<(&str, Value)> = Vec::new();
    let mut add = |field: &'static str, shown: bool, value: &dyn Fn() -> Value| {
        if shown && included(field) {
            data.push((field, value()));
        }
    };
    add("id", true, &|| text(template.id.clone()));
    add("theme", true, &|| text(template.theme.clone()));
    // The fields of an object that `_fields` asks for.
    let object = |name: &str, fields: Vec<(&str, &dyn Fn() -> Value)>| {
        let fields = fields
            .into_iter()
            .filter(|(field, _)| included(&format!("{name}.{field}")));
        let fields = fields.map(|(field, value)| (Key::String(field.as_bytes().to_vec()), value()));
        Value::Array(fields.collect())
    };
    add("content", true, &|| {
        let version = || Value::Int(i64::from(content.windows(8).any(|w| w == b"<!-- wp:")));
        object(
            "content",
            vec![("raw", &|| text(content)), ("block_version", &version)],
        )
    });
    add("slug", true, &|| text(template.slug.clone()));
    add("source", true, &|| text(template.source));
    add("origin", true, &|| {
        template.origin.clone().map_or(Value::Null, text)
    });
    add("type", true, &|| text(template.kind.name()));
    add("description", true, &|| text(template.description.clone()));
    add("title", true, &|| {
        let rendered = || {
            text(if template.wp_id == 0 {
                template.title.clone()
            } else {
                admin_title(&Post {
                    title: template.title.clone(),
                    ..Post::default()
                })
            })
        };
        object(
            "title",
            vec![
                ("raw", &|| text(template.title.clone())),
                ("rendered", &rendered),
            ],
        )
    });
    add("status", true, &|| text(template.status.clone()));
    let wp_id = i64::try_from(template.wp_id).unwrap_or_default();
    add("wp_id", true, &|| Value::Int(wp_id));
    add("has_theme_file", true, &|| {
        Value::Bool(template.has_theme_file)
    });
    add("is_custom", template.kind == Kind::Template, &|| {
        Value::Bool(template.is_custom)
    });
    let author = i64::try_from(template.author).unwrap_or_default();
    add("author", true, &|| Value::Int(author));
    add("area", template.kind == Kind::Part, &|| {
        template.area.clone().map_or(Value::Null, text)
    });
    add("modified", true, &|| rfc3339(template.modified.as_ref()));
    add("date", true, &|| rfc3339(template.date.as_ref()));
    add("author_text", true, &|| {
        text(author_text(context, template))
    });
    add("original_source", true, &|| text(original_source(template)));
    let shown = params.context();
    let shown = if Value::String(shown.clone()).is_empty() {
        b"view".to_vec()
    } else {
        shown
    };
    // The context's fields are kept; of the content, `raw` in the view and
    // edit contexts and `block_version` in the edit context only.
    let data = data.into_iter().map(|(name, mut value)| {
        if let ("content", Value::Array(parts)) = (name, &mut value) {
            parts.retain(|(part, _)| match part.to_php_string().as_slice() {
                b"raw" => shown == b"view" || shown == b"edit",
                _ => shown == b"edit",
            });
        }
        (Key::String(name.as_bytes().to_vec()), value)
    });
    let mut response = Response::new(Value::Array(data.collect()), 200);
    let linked = if links_wanted(&wanted) {
        links(context, &mut response, template, kind)
    } else {
        Ok(())
    };
    linked.map(|()| response)
}

/// `prepare_links()` and the actions the user may take.
fn links(
    context: &Context,
    response: &mut Response,
    template: &Template,
    kind: Kind,
) -> Result<(), RestError> {
    let id = String::from_utf8_lossy(&template.id).into_owned();
    let route = format!("/wp/v2/{}/{id}", base(kind));
    crate::self_link(context, response, &route);
    let collection = format!("/wp/v2/{}", base(kind));
    response.add_link("collection", context.rest_url(&collection));
    let about = format!("/wp/v2/types/{}", kind.name());
    response.add_link("about", context.rest_url(&about));
    let customized = query::template(context, &template.id, kind);
    customized.map(|customized| {
        let customized = customized.filter(|found| found.wp_id != 0);
        if let Some(found) = customized {
            let revisions = context.site.revisions(found.wp_id);
            let base = format!("{route}/revisions");
            let count = i64::try_from(revisions.len()).unwrap_or_default();
            let history = object([
                ("count", Value::Int(count)),
                ("href", text(context.rest_url(&base))),
            ]);
            response.add_link_attributes("version-history", history);
            if let Some(latest) = revisions.first() {
                let id = i64::try_from(*latest).unwrap_or_default();
                let predecessor = object([
                    ("id", Value::Int(id)),
                    ("href", text(context.rest_url(&format!("{base}/{latest}")))),
                ]);
                response.add_link_attributes("predecessor-version", predecessor);
            }
        }
        // Both template post types map `publish_posts` to `edit_theme_options`.
        let actions = [
            ("https://api.w.org/action-publish", "edit_theme_options"),
            (
                "https://api.w.org/action-unfiltered-html",
                "unfiltered_html",
            ),
        ];
        let allowed = actions.into_iter().filter(|(_, cap)| context.can(cap));
        allowed.for_each(|(rel, _)| response.add_link(rel, context.rest_url(&route)));
    })
}

#[cfg(test)]
mod tests;
