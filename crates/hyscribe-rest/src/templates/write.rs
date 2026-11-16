// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Templates_Controller`'s writes (MVP 4.3): `create_item()`,
//! `update_item()`, which customises a theme file into a post or reverts
//! (`source` `theme`) to it by deleting that post, and `delete_item()`.
//! Templates and parts are `wp_template` and `wp_template_part` posts with
//! their `wp_theme` term, and a part its `wp_template_part_area`.

use hyscribe_phpser::{Value, is_empty_string, lookup};

use super::content::{hooked, resolve_patterns};
use super::query::{self, Kind, Query, Template, names_an_entity, stylesheet};
use super::{base, prepare, template_id};
use crate::Context;
use crate::posts::write::{Outcome, Plan, Save, Write, Writing, as_stored};
use crate::request::{Arg, Params, Sanitize, arg};
use crate::response::{Response, RestError, object, text};

/// The areas `get_allowed_block_template_part_areas()` gives.
const AREAS: [&[u8]; 4] = [
    b"uncategorized",
    b"header",
    b"footer",
    b"navigation-overlay",
];

/// The arguments of a create, with `required` and defaults, or an update.
pub(super) fn save_args(kind: Kind, create: bool) -> Vec<Arg> {
    let value = |name, schema: &str| arg(name, schema, true, Sanitize::Value);
    let (required, empty, publish) = match create {
        true => (
            r#","required":true"#,
            r#","default":"""#,
            r#","default":"publish""#,
        ),
        false => ("", "", ""),
    };
    let text_or =
        |raw: &str| format!(r#"{{"type":["object","string"],"properties":{{{raw}}}{empty}}}"#);
    let mut args = vec![
        value(
            "slug",
            &format!(
                r#"{{"type":"string","minLength":1,"pattern":"[a-zA-Z0-9_\\%-]+"{required}}}"#
            ),
        ),
        value("theme", r#"{"type":"string"}"#),
        value("type", r#"{"type":"string"}"#),
        value(
            "content",
            &text_or(
                r#""raw":{"type":"string"},"block_version":{"type":"integer","readonly":true}"#,
            ),
        ),
        value(
            "title",
            &text_or(r#""raw":{"type":"string"},"rendered":{"type":"string","readonly":true}"#),
        ),
        value("description", &format!(r#"{{"type":"string"{empty}}}"#)),
        value(
            "status",
            &format!(
                r#"{{"type":"string","enum":["publish","future","draft","pending","private"]{publish}}}"#
            ),
        ),
        value("author", r#"{"type":"integer"}"#),
    ];
    if kind == Kind::Part {
        args.push(value("area", r#"{"type":"string"}"#));
    }
    args
}

/// The kind of the route `regex`, and the rest of it after its base.
pub(super) fn route(regex: &str) -> Option<(Kind, &str)> {
    [Kind::Template, Kind::Part].into_iter().find_map(|kind| {
        let rest = regex.strip_prefix(&format!("/wp/v2/{}", base(kind)))?;
        Some((kind, rest))
    })
}

fn kind_of(params: &Params) -> Kind {
    // Only the templates' routes plan these writes.
    route(params.route).map_or(Kind::Template, |(kind, _)| kind)
}

/// The ported write of the route `regex`'s endpoint `index`, if one of
/// these: a collection's second endpoint creates, an item's second
/// updates and its third deletes.
pub(crate) fn writing(regex: &str, index: usize) -> Option<Writing> {
    let (_, rest) = route(regex)?;
    let unchanged = |_: &Context, _: &Params, args: Vec<Arg>| args;
    // Only write endpoints, which have no callback, ask.
    Some(match (rest, index) {
        ("", _) => Writing {
            unported: |_| false,
            args: unchanged,
            plan: plan_create,
            finish: finish_create,
        },
        (_, 1) => Writing {
            unported: |_| false,
            args: unchanged,
            plan: plan_update,
            finish: finish_update,
        },
        _ => Writing {
            unported: |_| false,
            args: unchanged,
            plan: plan_delete,
            finish: finish_delete,
        },
    })
}

/// `_filter_block_template_part_area()`.
fn area(area: &[u8]) -> Vec<u8> {
    let allowed = AREAS.contains(&area);
    if allowed { area } else { b"uncategorized" }.to_vec()
}

/// `$request[ $name ]` of a string, or of an array's `raw`, for the title
/// only when not empty (`! empty()`).
fn raw(params: &Params, name: &str) -> Option<Vec<u8>> {
    match params.get(name)? {
        Value::Array(parts) => {
            let raw = lookup(parts, "raw").filter(|raw| **raw != Value::Null)?;
            let empty = name == "title" && raw.is_empty();
            (!empty).then(|| raw.to_php_string())
        }
        // Validated as an object or a string.
        given => Some(given.to_php_string()),
    }
}

/// `prepare_item_for_database()` of `template`, found by the request's ID,
/// or of a new one.
fn changes(
    context: &Context,
    params: &Params,
    kind: Kind,
    template: Option<&Template>,
) -> Result<Save, RestError> {
    let file = template.filter(|template| template.source != "custom");
    let mut save = Save {
        post_type: kind.name().into(),
        status: Some("publish".into()),
        ..Save::default()
    };
    match template {
        None => {
            let theme = params.get("theme").map(Value::to_php_string);
            save.tax_input
                .push(("wp_theme", theme.unwrap_or_else(|| stylesheet(context))));
        }
        Some(template) => {
            save.slug = Some(template.slug.clone());
            if let Some(file) = file {
                save.tax_input.push(("wp_theme", file.theme.clone()));
                save.meta_input.push(("origin", text(file.source)));
            } else {
                save.id = template.wp_id;
            }
        }
    }
    let from_file = |field: fn(&Template) -> &Vec<u8>| file.map(|file| field(file).clone());
    save.content = match params.get("content") {
        Some(_) => raw(params, "content"),
        None => from_file(|file| &file.content),
    };
    save.title = match params.get("title") {
        Some(_) => raw(params, "title"),
        None => from_file(|file| &file.title),
    };
    save.excerpt = match params.get("description") {
        Some(description) => Some(description.to_php_string()),
        None => from_file(|file| &file.description),
    };
    let suggestion = params
        .get("is_wp_suggestion")
        .filter(|_| kind == Kind::Template);
    if let Some(suggestion) = suggestion {
        save.meta_input = vec![("is_wp_suggestion", suggestion.clone())];
    }
    if kind == Kind::Part {
        let stored = template.and_then(|template| template.area.clone());
        let given = match params.get("area") {
            Some(given) => Some(area(&given.to_php_string())),
            None if file.is_some() && stored.as_deref().is_some_and(|a| !is_empty_string(a)) => {
                stored.as_deref().map(area)
            }
            None if stored.as_deref().is_none_or(is_empty_string) => {
                Some(b"uncategorized".to_vec())
            }
            None => None,
        };
        save.tax_input
            .extend(given.map(|given| ("wp_template_part_area", given)));
    }
    let author = params.get("author").map_or(0, Value::to_int);
    if author != 0 {
        let known = u64::try_from(author)
            .ok()
            .filter(|&author| author == context.user || context.site.user(author).is_some());
        let invalid = || RestError::status("rest_invalid_author", "Invalid author ID.", 400);
        save.author = Some(known.ok_or_else(invalid)?);
    }
    let hook = format!("rest_pre_insert_{}", kind.name());
    Ok(crate::hooks::apply_filters(&hook, save, &[params]))
}

/// The answer after the save reads the template as stored, which must be
/// ported: its content's patterns resolve, and a template is not titled by
/// the post or term its slug names.
fn answerable(
    context: &Context,
    save: &Save,
    kind: Kind,
    template: Option<&Template>,
) -> Result<(), RestError> {
    let stored = |field: fn(&Template) -> &Vec<u8>| template.map(|t| field(t).clone());
    let content = save.content.clone().or_else(|| stored(|t| &t.content));
    let title = save.title.clone().or_else(|| stored(|t| &t.title));
    let description = save.excerpt.clone().or_else(|| stored(|t| &t.description));
    let unfiltered = context.can("unfiltered_html");
    let (content, title) = as_stored(
        context,
        unfiltered,
        content.unwrap_or_default(),
        &title.unwrap_or_default(),
    );
    resolve_patterns(&hooked(&content, kind == Kind::Part, &stylesheet(context)))?;
    let slug = save.slug.clone().unwrap_or_default();
    let untitled = is_empty_string(&title) || title == slug;
    let described = description.is_some_and(|description| !is_empty_string(&description));
    if kind == Kind::Template && !described && untitled && names_an_entity(&slug) {
        return Err(RestError::not_ported());
    }
    Ok(())
}

/// A plan of `write`, by a user who passed `manage`.
fn plan(context: &Context, write: Write, template: Option<Template>) -> Plan {
    Plan {
        user: context.user,
        can_publish: true,
        unfiltered_html: context.can("unfiltered_html"),
        write,
        previous: None,
        template: template.map(Box::new),
    }
}

/// `create_item()` up to its write.
fn plan_create(context: &Context, params: &Params) -> Result<Plan, RestError> {
    let kind = kind_of(params);
    let mut save = changes(context, params, kind, None)?;
    save.slug = Some(params.text("slug"));
    answerable(context, &save, kind, None)?;
    Ok(plan(context, Write::Save(Box::new(save)), None))
}

/// `get_block_template( $request['id'] )`, or `rest_template_not_found`.
fn found(context: &Context, params: &Params, kind: Kind) -> Result<Template, RestError> {
    let template = query::template(context, &template_id(params), kind)?;
    let message = "No templates exist with that id.";
    template.ok_or_else(|| RestError::status("rest_template_not_found", message, 404))
}

/// `update_item()` up to its write: a theme file's customised post
/// inserted, a customised one updated, or one deleted to revert to its
/// theme file. Reverting what has no customised post, or no theme file to
/// fall back to, is not ported: PHP deletes nothing, or fails.
fn plan_update(context: &Context, params: &Params) -> Result<Plan, RestError> {
    let kind = kind_of(params);
    let template = found(context, params, kind)?;
    if params.get("source") == Some(&text("theme")) {
        if template.wp_id == 0 || !template.has_theme_file {
            return Err(RestError::not_ported());
        }
        return Ok(plan(context, Write::Delete(template.wp_id), None));
    }
    let save = changes(context, params, kind, Some(&template))?;
    answerable(context, &save, kind, Some(&template))?;
    Ok(plan(context, Write::Save(Box::new(save)), None))
}

/// `delete_item()` up to its write: a customised post trashed, or deleted
/// with `force`.
fn plan_delete(context: &Context, params: &Params) -> Result<Plan, RestError> {
    let kind = kind_of(params);
    let mut template = found(context, params, kind)?;
    if template.source != "custom" {
        let message = "Templates based on theme files can't be removed.";
        return Err(RestError::status("rest_invalid_template", message, 400));
    }
    let id = template.wp_id;
    if params.get("force").is_some_and(|force| !force.is_empty()) {
        let mut edit = params.clone();
        edit.set("context", text("edit"));
        let previous = prepare(context, &edit, &template, kind)?.data;
        let mut plan = plan(context, Write::Delete(id), None);
        plan.previous = Some((hyscribe_model::Post::default(), previous));
        return Ok(plan);
    }
    if template.status == "trash" {
        let message = "The template has already been deleted.";
        return Err(RestError::status(
            "rest_template_already_trashed",
            message,
            410,
        ));
    }
    template.status = "trash".into();
    Ok(plan(context, Write::Trash(id), Some(template)))
}

/// `empty_content`, as `wp_insert_post()` refuses a template with none.
fn refused(outcome: Outcome) -> Result<u64, RestError> {
    match outcome {
        Outcome::Written(id) => Ok(id),
        Outcome::Refused("empty_content") => {
            let message = "Content, title, and excerpt are empty.";
            Err(RestError::status("empty_content", message, 400))
        }
        Outcome::Refused(_) => {
            let message = "The template cannot be deleted.";
            Err(RestError::status("rest_cannot_delete", message, 500))
        }
    }
}

/// `rest_after_insert_{$post_type}` of the post `id`.
fn after_insert(context: &Context, params: &Params, kind: Kind, (id, created): (u64, bool)) {
    let post = context.site.post(id).unwrap_or_default();
    let hook = format!("rest_after_insert_{}", kind.name());
    crate::hooks::do_action(&hook, &[&post, params, &created]);
}

/// The new template, found by its post's ID, with `201` and its `Location`.
fn finish_create(
    context: &Context,
    params: &mut Params,
    _: Plan,
    outcome: Outcome,
) -> Result<Response, RestError> {
    let kind = kind_of(params);
    let id = refused(outcome)?;
    let query = Query {
        wp_id: Some(Value::Int(id.cast_signed())),
        ..Query::default()
    };
    let posts = query::templates(context, &query, kind)?;
    let Some(template) = posts.into_iter().next() else {
        let message = "No templates exist with that id.";
        return Err(RestError::status(
            "rest_template_insert_error",
            message,
            400,
        ));
    };
    let template = query::template(context, &template.id, kind)?.unwrap_or(template);
    after_insert(context, params, kind, (id, true));
    let location = format!(
        "/wp/v2/{}/{}",
        base(kind),
        String::from_utf8_lossy(&template.id)
    );
    prepare(context, params, &template, kind).map(|mut response| {
        response.status = 201;
        response.header("Location", &context.rest_url(&location));
        response
    })
}

/// The template as the update or revert left it, in the `edit` context.
fn finish_update(
    context: &Context,
    params: &mut Params,
    plan: Plan,
    outcome: Outcome,
) -> Result<Response, RestError> {
    let kind = kind_of(params);
    let id = refused(outcome)?;
    params.set("context", text("edit"));
    let template = found(context, params, kind)?;
    if let Write::Save(_) = plan.write {
        after_insert(context, params, kind, (id, false));
    }
    prepare(context, params, &template, kind)
}

/// The deleted template's answer as it was, or the trashed one's.
fn finish_delete(
    context: &Context,
    params: &mut Params,
    plan: Plan,
    outcome: Outcome,
) -> Result<Response, RestError> {
    let kind = kind_of(params);
    refused(outcome)?;
    params.set("context", text("edit"));
    match (plan.template, plan.previous) {
        (Some(template), _) => prepare(context, params, &template, kind),
        (None, previous) => {
            let previous = previous.map(|(_, previous)| previous).unwrap_or_default();
            let deleted = object([("deleted", Value::Bool(true)), ("previous", previous)]);
            Ok(Response::new(deleted, 200))
        }
    }
}
