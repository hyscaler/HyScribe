// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Terms_Controller` for categories and tags: `/wp/v2/categories`,
//! `/wp/v2/tags` and their items, read only.

use hyscribe_model::{Post, Term, TermOrder, TermQuery};
use hyscribe_phpser::{Key, Maybe, Value, add_query_arg, maybe_unserialize};
use hyscribe_render::formatting::{sanitize_title, sanitize_title_for_query};

use crate::fields::{Properties, fields_for_response, filter_by_context, is_field_included};
use crate::request::{Arg, Params, Sanitize, arg, collection_args};
use crate::response::{Response, RestError, list, paginate, text};
use crate::schema::parse_list;
use crate::taxonomies::{field, shown};
use crate::types::{context_arg, items_route};
use crate::{Context, Endpoint};

/// The schema's properties and their contexts; `parent` only for a
/// hierarchical taxonomy.
const PROPERTIES: &Properties = &[
    ("id", &["view", "embed", "edit"]),
    ("count", &["view", "edit"]),
    ("description", &["view", "edit"]),
    ("link", &["view", "embed", "edit"]),
    ("name", &["view", "embed", "edit"]),
    ("slug", &["view", "embed", "edit"]),
    ("taxonomy", &["view", "embed", "edit"]),
    ("parent", &["view", "edit"]),
    ("meta", &["view", "edit"]),
];

/// The taxonomy of the route.
fn taxonomy(params: &Params) -> &'static str {
    if params.route.starts_with("/wp/v2/categories") {
        "category"
    } else {
        "post_tag"
    }
}

/// The taxonomy's fields: both are shown in REST.
fn fields_of(taxonomy: &str) -> &'static Value {
    shown(taxonomy).unwrap_or(&Value::Null)
}

fn hierarchical(taxonomy: &str) -> bool {
    fields_of(taxonomy).get_str("hierarchical") == Some(&Value::Bool(true))
}

fn properties(taxonomy: &str) -> Vec<(&'static str, &'static [&'static str])> {
    PROPERTIES
        .iter()
        .filter(|(name, _)| *name != "parent" || hierarchical(taxonomy))
        .copied()
        .collect()
}

/// The arguments of the collection of `taxonomy`.
fn items_args(taxonomy: &'static str) -> Vec<Arg> {
    let mut args = collection_args();
    let ids = r#"{"type":"array","items":{"type":"integer"},"default":[]}"#;
    args.push(arg("exclude", ids, false, Sanitize::Schema));
    args.push(arg("include", ids, false, Sanitize::Schema));
    if !hierarchical(taxonomy) {
        args.push(arg(
            "offset",
            r#"{"type":"integer"}"#,
            false,
            Sanitize::Schema,
        ));
    }
    let order = r#"{"type":"string","default":"asc","enum":["asc","desc"]}"#;
    args.push(arg("order", order, false, Sanitize::Schema));
    let orderby = r#"{"type":"string","default":"name","enum":["id","include","name","slug","include_slugs","term_group","description","count"]}"#;
    args.push(arg("orderby", orderby, false, Sanitize::Schema));
    let hide = r#"{"type":"boolean","default":false}"#;
    args.push(arg("hide_empty", hide, false, Sanitize::Schema));
    if hierarchical(taxonomy) {
        args.push(arg(
            "parent",
            r#"{"type":"integer"}"#,
            false,
            Sanitize::Schema,
        ));
    }
    args.push(arg(
        "post",
        r#"{"type":"integer","default":null}"#,
        false,
        Sanitize::Schema,
    ));
    let slugs = r#"{"type":"array","items":{"type":"string"}}"#;
    args.push(arg("slug", slugs, false, Sanitize::Schema));
    args
}

/// The endpoints of the route `regex`, if ported.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    let (taxonomy, single) = match regex {
        "/wp/v2/categories" => ("category", false),
        "/wp/v2/tags" => ("post_tag", false),
        "/wp/v2/categories/(?P<id>[\\d]+)" => ("category", true),
        "/wp/v2/tags/(?P<id>[\\d]+)" => ("post_tag", true),
        _ => return None,
    };
    if !single {
        return Some(vec![
            Endpoint {
                args: items_args(taxonomy),
                permission: items_permission,
                callback: Some(items),
            },
            Endpoint {
                args: Vec::new(),
                permission: create_permission,
                callback: None,
            },
        ]);
    }
    let id = || arg("id", r#"{"type":"integer"}"#, false, Sanitize::Schema);
    let write = |permission| Endpoint {
        args: vec![id()],
        permission,
        callback: None,
    };
    Some(vec![
        Endpoint {
            args: vec![id(), context_arg()],
            permission: item_permission,
            callback: Some(item),
        },
        write(update_permission),
        write(delete_permission),
    ])
}

/// `get_items_permissions_check()`.
fn items_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let taxonomy = taxonomy(params);
    let edit_terms = field(fields_of(taxonomy), &["capabilities", "edit_terms"]);
    if params.context() == b"edit" && !context.can(&edit_terms) {
        let message = "Sorry, you are not allowed to edit terms in this taxonomy.";
        let status = context.authorization_required();
        return Err(RestError::status("rest_forbidden_context", message, status));
    }
    let Some(id) = post_param(params) else {
        return Ok(());
    };
    let Some(post) = context.site.post(id) else {
        return Err(RestError::status(
            "rest_post_invalid_id",
            "Invalid post ID.",
            400,
        ));
    };
    if !may_read_terms(context, taxonomy, &post) {
        let message = "Sorry, you are not allowed to view terms for this post.";
        let status = context.authorization_required();
        return Err(RestError::status("rest_forbidden_context", message, status));
    }
    Ok(())
}

/// `! empty( $request['post'] )`, as an ID.
fn post_param(params: &Params) -> Option<u64> {
    let post = params.get("post")?;
    (!post.is_empty()).then(|| u64::try_from(post.to_int()).unwrap_or_default())
}

/// `check_read_terms_permission_for_post()`.
fn may_read_terms(context: &Context, taxonomy: &str, post: &Post) -> bool {
    let types = fields_of(taxonomy)
        .get_str("types")
        .and_then(Value::as_array);
    let in_taxonomy = types
        .unwrap_or_default()
        .iter()
        .any(|(_, ty)| ty.to_php_string() == post.post_type.as_bytes());
    // `is_post_publicly_viewable()`: of the types here, published.
    in_taxonomy && (post.status == "publish" || context.can_for("read_post", post.id))
}

/// The ID asked for, `(int) $id`.
fn id_param(params: &Params) -> u64 {
    params
        .get("id")
        .map_or(0, |id| u64::try_from(id.to_int()).unwrap_or_default())
}

/// `get_term()` of the route's taxonomy.
fn get_term(context: &Context, params: &Params) -> Result<Term, RestError> {
    let id = id_param(params);
    let query = TermQuery {
        taxonomy: taxonomy(params).into(),
        include: vec![id],
        ..TermQuery::default()
    };
    // No term has the ID 0, which `(int) $id <= 0` refuses.
    let found = context.site.terms(&query).into_iter().next();
    found.ok_or_else(|| RestError::status("rest_term_invalid", "Term does not exist.", 404))
}

/// `create_item_permissions_check()`.
fn create_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let taxonomy = taxonomy(params);
    let cap = if hierarchical(taxonomy) {
        "edit_terms"
    } else {
        "assign_terms"
    };
    if context.can(&field(fields_of(taxonomy), &["capabilities", cap])) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to create terms in this taxonomy.";
    let status = context.authorization_required();
    Err(RestError::status("rest_cannot_create", message, status))
}

/// `update_item_permissions_check()`.
fn update_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let term = get_term(context, params)?;
    if context.can_for("edit_term", term.id) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to edit this term.";
    let status = context.authorization_required();
    Err(RestError::status("rest_cannot_update", message, status))
}

/// `delete_item_permissions_check()`.
fn delete_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let term = get_term(context, params)?;
    if context.can_for("delete_term", term.id) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to delete this term.";
    let status = context.authorization_required();
    Err(RestError::status("rest_cannot_delete", message, status))
}

/// `get_item_permissions_check()`.
fn item_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let term = get_term(context, params)?;
    if params.context() == b"edit" && !context.can_for("edit_term", term.id) {
        let message = "Sorry, you are not allowed to edit this term.";
        let status = context.authorization_required();
        return Err(RestError::status("rest_forbidden_context", message, status));
    }
    Ok(())
}

/// `get_item()`.
fn item(context: &Context, params: &Params) -> Result<Response, RestError> {
    get_term(context, params).map(|term| prepare(context, params, &term))
}

/// A parameter's integers, `wp_parse_id_list()`.
fn ids(params: &Params, name: &str) -> Vec<u64> {
    let mut ids: Vec<u64> = Vec::new();
    let value = params.get(name).cloned().unwrap_or_default();
    for (_, id) in parse_list(&value) {
        let id = id.to_int().unsigned_abs();
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

/// The query `get_items()` prepares, without its page.
fn prepared(params: &Params) -> TermQuery {
    let taxonomy = taxonomy(params);
    let text = |name| {
        params
            .get(name)
            .map(Value::to_php_string)
            .unwrap_or_default()
    };
    let slugs: Vec<Vec<u8>> = parse_list(&params.get("slug").cloned().unwrap_or_default())
        .iter()
        .map(|(_, slug)| slug.to_php_string())
        .collect();
    let include = ids(params, "include");
    let order = match text("orderby").as_slice() {
        b"id" => TermOrder::TermId,
        b"include" if !include.is_empty() => TermOrder::Include,
        b"include_slugs" if !slugs.is_empty() => TermOrder::Slugs,
        b"slug" => TermOrder::Slug,
        b"term_group" => TermOrder::TermGroup,
        b"description" => TermOrder::Description,
        b"count" => TermOrder::Count,
        _ => TermOrder::Name,
    };
    let objects: Vec<u64> = post_param(params).into_iter().collect();
    let search = text("search");
    let parent = params
        .get("parent")
        .filter(|_| hierarchical(taxonomy))
        .map(|parent| parent.to_int())
        .filter(|parent| *parent != 0 || params.get("parent") == Some(&Value::Int(0)));
    let descending = text("order") == b"desc";
    TermQuery {
        taxonomy: taxonomy.into(),
        exclude: if include.is_empty() {
            ids(params, "exclude")
        } else {
            Vec::new()
        },
        order: Some((order, descending)),
        search: (!Value::String(search.clone()).is_empty())
            .then(|| String::from_utf8_lossy(&search).into_owned()),
        nonempty: objects.is_empty() && params.get("hide_empty") == Some(&Value::Bool(true)),
        parent: parent.map(i64::unsigned_abs),
        slugs: slugs
            .iter()
            .map(|slug| {
                if order == TermOrder::Slugs {
                    sanitize_title_for_query(slug)
                } else {
                    sanitize_title(slug)
                }
            })
            .collect(),
        include,
        objects,
        ..TermQuery::default()
    }
}

/// `get_items()`.
fn items(context: &Context, params: &Params) -> Result<Response, RestError> {
    let int = |name| {
        params
            .get(name)
            .map_or(0, |value| value.to_int().unsigned_abs())
    };
    let per_page = int("per_page").max(1);
    let query = prepared(params);
    let hierarchical = hierarchical(&query.taxonomy);
    // Only a flat taxonomy's collection registers `offset`.
    let offset = match int("offset") {
        offset if offset > 0 && !hierarchical => offset,
        _ => int("page").saturating_sub(1) * per_page,
    };
    let tree = hierarchical && (query.nonempty || query.parent.is_some_and(|parent| parent > 0));
    // `_get_term_hierarchy()` writes the hierarchy when it is not stored.
    let hierarchy = match hierarchy(context, &query.taxonomy) {
        None if tree => return Err(RestError::not_ported()),
        hierarchy => hierarchy.unwrap_or_default(),
    };
    let childless = query
        .parent
        .is_some_and(|parent| parent > 0 && !hierarchy.iter().any(|(id, _)| *id == parent));
    let total = if childless {
        0
    } else {
        context.site.count_terms(&query)
    };
    let found = if childless {
        Vec::new()
    } else if hierarchical {
        // A hierarchical taxonomy's terms are queried whole, as PHP queries
        // them, then paged; with `hide_empty`, empty terms with terms under
        // them that are not are kept.
        let all = context.site.terms(&TermQuery {
            nonempty: false,
            ..query.clone()
        });
        let every = context.site.terms(&TermQuery {
            taxonomy: query.taxonomy.clone(),
            ..TermQuery::default()
        });
        let kept = all.into_iter().filter(|term| {
            !query.nonempty || term.count > 0 || counted_under(&every, &hierarchy, term.id)
        });
        let skip = usize::try_from(offset).unwrap_or(usize::MAX);
        let take = usize::try_from(per_page).unwrap_or(usize::MAX);
        kept.skip(skip).take(take).collect()
    } else {
        context.site.terms(&TermQuery {
            limit: Some(per_page),
            offset,
            ..query.clone()
        })
    };
    let mut response = if context.head {
        Response::new(Value::Array(Vec::new()), 200)
    } else {
        let edit = params.context() == b"edit";
        let data = found
            .iter()
            .filter(|term| !edit || context.can_for("edit_term", term.id))
            .map(|term| prepare(context, params, term).into_data())
            .collect();
        Response::new(list(data), 200)
    };
    let pages = total.div_ceil(per_page);
    let page = offset / per_page + 1 + u64::from(offset % per_page != 0);
    let route = items_route_of(&query.taxonomy);
    paginate(
        &mut response,
        (total, pages, page),
        &params.get,
        &context.rest_url(&route),
    );
    Ok(response)
}

/// `_get_term_hierarchy()` of a hierarchical taxonomy: the stored
/// `{$taxonomy}_children`, each parent and its children, if an array.
fn hierarchy(context: &Context, taxonomy: &str) -> Option<Vec<(u64, Vec<u64>)>> {
    let stored = context.site.option(&format!("{taxonomy}_children"))?;
    let Maybe::Value(Value::Array(parents)) = maybe_unserialize(&stored) else {
        return None;
    };
    let ids = |children: &Value| {
        let children = children.as_array().unwrap_or_default();
        children
            .iter()
            .map(|(_, id)| id.to_int().unsigned_abs())
            .collect()
    };
    let parents = parents.iter().map(|(parent, children)| {
        (
            Value::String(parent.to_php_string())
                .to_int()
                .unsigned_abs(),
            ids(children),
        )
    });
    Some(parents.collect())
}

/// Whether a term under `id`, of `terms`, has posts.
fn counted_under(terms: &[Term], hierarchy: &[(u64, Vec<u64>)], id: u64) -> bool {
    let counted = |id: u64| terms.iter().any(|term| term.id == id && term.count > 0);
    children(hierarchy, id).into_iter().any(counted)
}

/// `get_term_children()`: the terms under `id`, at any depth, each parent's
/// children before those under them.
fn children(hierarchy: &[(u64, Vec<u64>)], id: u64) -> Vec<u64> {
    let of = |id: u64| {
        hierarchy
            .iter()
            .find(|(parent, _)| *parent == id)
            .map(|(_, c)| c)
    };
    let Some(direct) = of(id) else {
        return Vec::new();
    };
    let mut all = direct.clone();
    for child in direct {
        if *child != id && of(*child).is_some() {
            all.extend(children(hierarchy, *child));
        }
    }
    all
}

/// `rest_get_route_for_taxonomy_items()`.
fn items_route_of(taxonomy: &str) -> String {
    let fields = fields_of(taxonomy);
    format!(
        "/{}/{}",
        field(fields, &["rest_namespace"]),
        field(fields, &["rest_base"])
    )
}

/// `prepare_item_for_response()`.
fn prepare(context: &Context, params: &Params, term: &Term) -> Response {
    if context.head {
        return Response::new(Value::Array(Vec::new()), 200);
    }
    let taxonomy = taxonomy(params);
    let properties = properties(taxonomy);
    let wanted = fields_for_response(params, &properties);
    let has = |name: &str| wanted.iter().any(|field| field == name.as_bytes());
    let int = |n: u64| Value::Int(i64::try_from(n).unwrap_or(i64::MAX));
    let fields = [
        ("id", int(term.id)),
        ("count", int(term.count)),
        ("description", text(term.description.clone())),
        ("link", text(context.site.term_link(taxonomy, term))),
        ("name", text(term.name.clone())),
        ("slug", text(term.slug.clone())),
        ("taxonomy", text(taxonomy)),
        ("parent", int(term.parent)),
        ("meta", Value::Array(Vec::new())),
    ];
    let data = fields
        .into_iter()
        .filter(|(name, _)| has(name))
        .map(|(name, value)| (Key::String(name.as_bytes().to_vec()), value))
        .collect();
    let data = filter_by_context(data, &properties, &params.context());
    let mut response = Response::new(Value::Array(data), 200);
    if is_field_included("_links", &wanted) || is_field_included("_embedded", &wanted) {
        let route = items_route_of(taxonomy);
        context.self_link(&mut response, &format!("{route}/{}", term.id));
        response.add_link("collection", context.rest_url(&route));
        let about = format!("/wp/v2/taxonomies/{taxonomy}");
        response.add_link("about", context.rest_url(&about));
        let parent = TermQuery {
            taxonomy: taxonomy.into(),
            include: vec![term.parent],
            ..TermQuery::default()
        };
        if !context.site.terms(&parent).is_empty() {
            let up = context.rest_url(&format!("{route}/{}", term.parent));
            response.add_link_with("up", up, vec![("embeddable", Value::Bool(true))]);
        }
        let base = field(fields_of(taxonomy), &["rest_base"]);
        let id = term.id.to_string();
        let types = fields_of(taxonomy)
            .get_str("types")
            .and_then(Value::as_array);
        let routes = types
            .unwrap_or_default()
            .iter()
            .filter_map(|(_, ty)| items_route(&String::from_utf8_lossy(&ty.to_php_string())));
        for path in routes {
            let url = context.rest_url(&path);
            let href = add_query_arg(&[(base.as_bytes(), Some(id.as_bytes()))], url.as_bytes());
            let href = String::from_utf8_lossy(&href).into_owned();
            response.add_link("https://api.w.org/post_type", href);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use hyscribe_phpser::Key;

    use super::*;

    fn params(route: &'static str, get: &[(&str, Value)]) -> Params {
        let get = get
            .iter()
            .map(|(name, value)| (Key::String(name.as_bytes().to_vec()), value.clone()));
        Params {
            route,
            get: get.collect(),
            ..Params::default()
        }
    }

    /// `get_items()`'s orders: `include` and `include_slugs` only with the
    /// list they order by, else by name; the slugs ordered by are
    /// `sanitize_title_for_query()`'s, as `parse_orderby()` lists them.
    #[test]
    fn orders_are_prepared_as_php_prepares_them() {
        let text = |t: &str| Value::String(t.as_bytes().to_vec());
        let tags = "/wp/v2/tags";
        let order = |get: &[(&str, Value)]| prepared(&params(tags, get)).order;
        for (name, expected) in [
            ("id", TermOrder::TermId),
            ("slug", TermOrder::Slug),
            ("term_group", TermOrder::TermGroup),
            ("description", TermOrder::Description),
            ("count", TermOrder::Count),
            ("include", TermOrder::Name),
            ("include_slugs", TermOrder::Name),
        ] {
            assert_eq!(
                order(&[("orderby", text(name))]),
                Some((expected, false)),
                "{name}"
            );
        }
        let slugs = [
            ("orderby", text("include_slugs")),
            ("slug", text("a\u{a0}b")),
        ];
        let query = prepared(&params(tags, &slugs));
        assert_eq!(query.order, Some((TermOrder::Slugs, false)));
        assert_eq!(
            query.slugs,
            [sanitize_title_for_query("a\u{a0}b".as_bytes())]
        );
        let query = prepared(&params(tags, &[("slug", text("a\u{a0}b"))]));
        assert_eq!(query.slugs, [sanitize_title("a\u{a0}b".as_bytes())]);
        assert_ne!(
            sanitize_title("a\u{a0}b".as_bytes()),
            sanitize_title_for_query("a\u{a0}b".as_bytes())
        );
    }

    /// `get_term_children()` walks the stored hierarchy: a child listed
    /// under itself is not walked again.
    #[test]
    fn children_are_found_at_any_depth() {
        let hierarchy = [(1, vec![2, 3]), (3, vec![4, 3]), (4, vec![5])];
        assert_eq!(children(&hierarchy, 1), [2, 3, 4, 3, 5]);
        assert_eq!(children(&hierarchy, 5), Vec::<u64>::new());
        let term = |id, count| Term {
            id,
            count,
            ..Term::default()
        };
        let terms = [term(2, 0), term(3, 0), term(4, 0), term(5, 1)];
        assert!(counted_under(&terms, &hierarchy, 1));
        assert!(counted_under(&terms, &hierarchy, 4));
        assert!(!counted_under(&terms[..3], &hierarchy, 1));
    }
}
