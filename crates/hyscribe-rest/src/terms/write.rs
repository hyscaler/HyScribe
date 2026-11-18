// SPDX-License-Identifier: AGPL-3.0-or-later

//! The categories' and tags' writes (MVP 4.3): `create_item()`,
//! `update_item()` and `delete_item()` of `WP_REST_Terms_Controller`. A
//! request whose checks pass gives the [`Plan`] `hyscribe-http` writes with
//! `wp_insert_term()`, `wp_update_term()` or `wp_delete_term()`; `meta` is
//! not ported.

use hyscribe_model::{Term, TermQuery};
use hyscribe_phpser::Value;

use super::{get_term, hierarchical, items_route_of, prepare, taxonomy};
use crate::Context;
use crate::posts::write::{Outcome, Plan, Write, Writing, invalid};
use crate::request::{Arg, Params, Sanitize, arg, force_arg, slug, typed};
use crate::response::{Response, RestError, int, object, text};

/// What a create or update saves, unslashed: `wp_insert_term()`'s or
/// `wp_update_term()`'s arguments where the request gives them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TermSave {
    /// `category` or `post_tag`.
    pub taxonomy: &'static str,
    /// The term to update, 0 to create one.
    pub id: u64,
    /// `name`.
    pub name: Option<Vec<u8>>,
    /// `slug`.
    pub slug: Option<Vec<u8>>,
    /// `description`.
    pub description: Option<Vec<u8>>,
    /// `parent`.
    pub parent: Option<u64>,
}

/// The taxonomies whose writes are ported.
const WRITTEN: [&str; 2] = ["category", "post_tag"];

/// The ported write of the route `regex`'s endpoint `index`, if any.
pub(crate) fn writing(regex: &str, index: usize) -> Option<Writing> {
    let items = ["/wp/v2/categories", "/wp/v2/tags"];
    let single = [
        "/wp/v2/categories/(?P<id>[\\d]+)",
        "/wp/v2/tags/(?P<id>[\\d]+)",
    ];
    let plan = match index {
        1 if items.contains(&regex) || single.contains(&regex) => plan_save,
        2 if single.contains(&regex) => plan_delete,
        _ => return None,
    };
    Some(Writing::plain(plan, finish))
}

/// `get_endpoint_args_for_item_schema()` of a create or update, for
/// categories and tags only; `meta` is not ported.
pub(super) fn save_args(taxonomy: &str, create: bool) -> Vec<Arg> {
    if !WRITTEN.contains(&taxonomy) {
        return create
            .then(|| crate::request::required("name"))
            .into_iter()
            .collect();
    }
    let string = r#"{"type":"string"}"#;
    let name = if create {
        r#"{"type":"string","required":true}"#
    } else {
        string
    };
    let mut args = vec![
        arg("description", string, true, Sanitize::Value),
        arg("name", name, true, Sanitize::TextField),
        arg("slug", string, true, Sanitize::Site(slug)),
    ];
    if hierarchical(taxonomy) {
        args.push(arg(
            "parent",
            r#"{"type":"integer"}"#,
            true,
            Sanitize::Value,
        ));
    }
    args.push(arg(
        "meta",
        r#"{"type":"object"}"#,
        false,
        Sanitize::NotPorted,
    ));
    args
}

/// A term's delete: `force`, false unless given.
pub(super) fn delete_args() -> Vec<Arg> {
    vec![typed("id", r#"{"type":"integer"}"#), force_arg()]
}

/// `get_term( $id, $taxonomy )` of an ID the request gives.
fn find(context: &Context, taxonomy: &str, id: i64) -> Option<Term> {
    let query = TermQuery {
        taxonomy: taxonomy.into(),
        include: vec![u64::try_from(id).ok()?],
        ..TermQuery::default()
    };
    context.site.terms(&query).into_iter().next()
}

/// `create_item()` and `update_item()` up to their writes, with
/// `prepare_item_for_database()`.
fn plan_save(context: &Context, params: &Params) -> Result<Plan, RestError> {
    let taxonomy = taxonomy(params);
    // An update's permission check found its term.
    let id = match params.route.ends_with(')') {
        true => get_term(context, params).unwrap_or_default().id,
        false => 0,
    };
    let parent = params.get("parent").map(Value::to_int);
    if let Some(parent) = parent {
        if !hierarchical(taxonomy) {
            let message = "Cannot set parent term, taxonomy is not hierarchical.";
            return Err(invalid("rest_taxonomy_not_hierarchical", message));
        }
        // `get_term( 0 )` is an error, which the check takes for a term.
        if parent != 0 && find(context, taxonomy, parent).is_none() {
            return Err(invalid("rest_term_invalid", "Parent term does not exist."));
        }
    }
    let given = |name| params.get(name).map(Value::to_php_string);
    let save = TermSave {
        taxonomy,
        id,
        name: given("name"),
        slug: given("slug"),
        description: given("description"),
        parent: parent.map(|parent| u64::try_from(parent).unwrap_or_default()),
    };
    Ok(Plan::new(context, Write::Term(Box::new(save)), Value::Null))
}

/// `delete_item()` up to its write: terms are deleted only with `force`.
fn plan_delete(context: &Context, params: &Params) -> Result<Plan, RestError> {
    // The permission check found it.
    let term = get_term(context, params).unwrap_or_default();
    if params.get("force").is_none_or(Value::is_empty) {
        let message = "Terms do not support trashing. Set 'force=true' to delete.";
        return Err(RestError::status("rest_trash_not_supported", message, 501));
    }
    let mut view = params.clone();
    view.set("context", text("view"));
    let previous = prepare(context, &view, &term).data;
    let write = Write::DeleteTerm(taxonomy(params), term.id);
    Ok(Plan::new(context, write, previous))
}

/// The answer once written: the term, a new one with `201`; a deleted
/// term's answer as it was; or a term function's error.
fn finish(
    context: &Context,
    params: &mut Params,
    plan: Plan,
    outcome: Outcome,
) -> Result<Response, RestError> {
    let id = match outcome {
        Outcome::Written(id) => id,
        Outcome::Exists(id) => return Ok(exists(params, id)),
        Outcome::Duplicate(slug) => {
            let slug = String::from_utf8_lossy(&slug);
            let message =
                format!("The slug &#8220;{slug}&#8221; is already in use by another term.");
            return Err(failed("duplicate_term_slug", message));
        }
        Outcome::Refused(code @ "empty_term_name") => {
            return Err(failed(code, "A name is required for this term.".into()));
        }
        Outcome::Refused(code @ "invalid_term_name") => {
            return Err(failed(code, "Invalid term name.".into()));
        }
        Outcome::Refused(code @ "missing_parent") => {
            return Err(failed(code, "Parent term does not exist.".into()));
        }
        Outcome::Refused(code @ "invalid_term") => return Err(failed(code, "Empty Term.".into())),
        // `wp_delete_term()` deleted nothing.
        _ => {
            let message = "The term cannot be deleted.";
            return Err(RestError::status("rest_cannot_delete", message, 500));
        }
    };
    if let Write::DeleteTerm(..) = plan.write {
        return Ok(plan.deleted());
    }
    params.set("context", text("edit"));
    let id_param = i64::try_from(id).unwrap_or_default();
    let term = find(context, taxonomy(params), id_param).unwrap_or_default();
    let mut response = prepare(context, params, &term);
    if matches!(plan.write, Write::Term(save) if save.id == 0) {
        response.status = 201;
        let route = items_route_of(taxonomy(params));
        response.header("Location", &context.rest_url(&format!("{route}/{id}")));
    }
    Ok(response)
}

/// A `WP_Error` a term function gave, with no data: sent with 500.
fn failed(code: &'static str, message: String) -> RestError {
    RestError {
        code,
        message,
        data: Value::Null,
    }
}

/// `term_exists`, as `create_item()` adds the term's ID to its data twice
/// before its status: the earlier data kept as `additional_data`.
fn exists(params: &Params, id: u64) -> Response {
    let message = if hierarchical(taxonomy(params)) {
        "A term with the name provided already exists with this parent."
    } else {
        "A term with the name provided already exists in this taxonomy."
    };
    let data = object([
        ("code", text("term_exists")),
        ("message", text(message)),
        (
            "data",
            object([("status", Value::Int(400)), ("term_id", int(id))]),
        ),
        (
            "additional_data",
            crate::response::list(vec![int(id), int(id)]),
        ),
    ]);
    Response::new(data, 400)
}
