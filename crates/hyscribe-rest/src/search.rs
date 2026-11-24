// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Search_Controller`: `/wp/v2/search` over its three handlers,
//! posts, terms and post formats. A term search over every taxonomy is
//! not ported.

use hyscribe_model::{OrderBy, Post, PostQuery, Term, TermOrder, TermQuery};
use hyscribe_phpser::Value;
use hyscribe_render::post::{POST_FORMATS, admin_title};

use crate::fields::{Properties, fields_for_response, item_response, links_wanted};
use crate::request::{Params, Sanitize, arg, collection_args, typed};
use crate::response::{Response, RestError, int, list, pages_linked, text};
use crate::schema::{Checked, Refusal, parse_list, parse_slug_list, sanitize_value_from_schema};
use crate::terms::ids;
use crate::{Context, Endpoint};

const ROUTE: &str = "/wp/v2/search";

const SHOWN: &[&str] = &["view", "embed"];

const PROPERTIES: &Properties = &[
    ("id", SHOWN),
    ("title", SHOWN),
    ("url", SHOWN),
    ("type", SHOWN),
    ("subtype", SHOWN),
];

/// The public post types shown in REST but attachments, and the public
/// taxonomies shown in REST: the post and term handlers' subtypes.
const POST_TYPES: [&str; 2] = ["post", "page"];
const TAXONOMIES: [&str; 2] = ["category", "post_tag"];

const SUBTYPE: &str = r#"{"default":"any","type":"array","items":{"enum":["post","page","category","post_tag","any"],"type":"string"}}"#;

/// The endpoints of the route `regex`, if ported.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    if regex != ROUTE {
        return None;
    }
    let mut args = collection_args();
    let context = arg(
        "context",
        r#"{"type":"string","enum":["view","embed"],"default":"view"}"#,
        true,
        Sanitize::Key,
    );
    args.splice(0..1, [context]);
    args.extend([
        typed(
            "type",
            r#"{"default":"post","type":"string","enum":["post","term","post-format"]}"#,
        ),
        arg("subtype", SUBTYPE, false, Sanitize::Params(subtypes)),
        typed(
            "exclude",
            r#"{"type":"array","items":{"type":"integer"},"default":[]}"#,
        ),
        typed(
            "include",
            r#"{"type":"array","items":{"type":"integer"},"default":[]}"#,
        ),
    ]);
    Some(vec![Endpoint {
        args,
        permission: |_, _| Ok(()),
        callback: Some(items),
    }])
}

/// `get_search_handler()`'s subtypes, by the request's type.
fn handler(params: &Params) -> Result<&'static [&'static str], RestError> {
    match params.get("type").and_then(Value::as_bytes) {
        Some(b"post") => Ok(&POST_TYPES),
        Some(b"term") => Ok(&TAXONOMIES),
        Some(b"post-format") => Ok(&[]),
        _ => Err(RestError::status(
            "rest_search_invalid_type",
            "Invalid type parameter.",
            400,
        )),
    }
}

/// `sanitize_subtypes()`: the slugs, checked by their schema, `any` alone
/// if asked for, else those of the type's handler.
fn subtypes(params: &Params, value: &Value) -> Checked<Value> {
    let slugs = list(parse_slug_list(value).into_iter().map(text).collect());
    let schema = hyscribe_phpser::json_decode(SUBTYPE.as_bytes()).unwrap_or_default();
    let slugs = crate::schema::validate_value_from_schema(&slugs, &schema, "subtype")
        .and_then(|()| sanitize_value_from_schema(&slugs, &schema, "subtype"))?;
    let any = text("any");
    if parse_list(&slugs).iter().any(|(_, slug)| *slug == any) {
        return Ok(list(vec![any]));
    }
    let known = handler(params).map_err(|error| Refusal(Some(error)))?;
    let kept = parse_list(&slugs).into_iter().filter(|(_, slug)| {
        known
            .iter()
            .any(|name| slug.as_bytes() == Some(name.as_bytes()))
    });
    Ok(Value::Array(kept.collect()))
}

/// The request's subtypes, `any` as all of the handler's.
fn asked(params: &Params, all: &[&str]) -> Vec<String> {
    let value = params.get("subtype").cloned().unwrap_or_default();
    let asked: Vec<String> = parse_list(&value)
        .iter()
        .map(|(_, slug)| String::from_utf8_lossy(&slug.to_php_string()).into_owned())
        .collect();
    if asked.iter().any(|slug| slug == "any") {
        return all.iter().map(|name| (*name).into()).collect();
    }
    asked
}

/// One result, as its handler prepares it.
enum Found {
    Post(Box<Post>),
    Term(String, Term),
    Format(&'static str, &'static str, String),
}

/// `get_items()`: the handler's results of the page, and the total.
fn items(context: &Context, params: &Params) -> Result<Response, RestError> {
    let int_param = |name| params.get(name).map_or(1, Value::to_int).unsigned_abs();
    let (page, per_page) = (int_param("page"), int_param("per_page"));
    let offset = crate::request::page_offset(page, per_page);
    let search = params.text("search");
    let search = Some(search).filter(|search| !Value::String(search.clone()).is_empty());
    let (found, total) = match params.text("type").as_slice() {
        b"post" => posts(context, params, search, per_page, offset),
        b"term" => terms(context, params, search, per_page, offset)?,
        _ => formats(context, search, per_page, offset),
    };
    let pages = total.div_ceil(per_page);
    if page > pages && total > 0 {
        let message = "The page number requested is larger than the number of pages available.";
        return Err(RestError::status(
            "rest_search_invalid_page_number",
            message,
            400,
        ));
    }
    let data = found
        .iter()
        .filter(|_| !context.head)
        .map(|found| prepare(context, params, found).into_data())
        .collect();
    let mut response = Response::new(list(data), 200);
    let collection = context.rest_url(ROUTE);
    let totals = (total, pages, page);
    pages_linked(
        &mut response,
        totals,
        page.saturating_sub(1),
        &params.get,
        &collection,
    );
    Ok(response)
}

/// The `include` and `exclude` lists: both queries ignore `exclude` when
/// `include` is not empty.
fn include_exclude(params: &Params) -> (Vec<u64>, Vec<u64>) {
    let include = ids(params, "include");
    let exclude = if include.is_empty() {
        ids(params, "exclude")
    } else {
        Vec::new()
    };
    (include, exclude)
}

/// `WP_REST_Post_Search_Handler::search_items()`: published posts, newest
/// first, by relevance first for a search; with no type left, posts. The
/// posts included leave none out.
fn posts(
    context: &Context,
    params: &Params,
    search: Option<Vec<u8>>,
    per_page: u64,
    offset: u64,
) -> (Vec<Found>, u64) {
    let mut post_types = asked(params, &POST_TYPES);
    if post_types.is_empty() {
        post_types.push("post".into());
    }
    let (include, exclude) = include_exclude(params);
    let mut query = PostQuery {
        post_types,
        include: (!include.is_empty()).then_some(include),
        exclude,
        order: vec![(OrderBy::Date, false)],
        logged_in: context.user != 0,
        limit: u32::try_from(per_page).unwrap_or(u32::MAX),
        offset: u32::try_from(offset).unwrap_or(u32::MAX),
        ..PostQuery::default()
    };
    if let Some(search) = search {
        crate::posts::searched(&mut query, &search);
        query.order.insert(0, (OrderBy::Relevance, false));
    }
    let reads = context.site.reads();
    let posts = reads.query(&query);
    // `found_posts` stays 0 for an empty page.
    let total = if posts.is_empty() {
        0
    } else {
        reads.count_posts(&query)
    };
    (
        posts
            .into_iter()
            .map(|post| Found::Post(Box::new(post)))
            .collect(),
        total,
    )
}

/// `WP_REST_Term_Search_Handler::search_items()`, by name. With no
/// taxonomy left, every taxonomy's terms are not ported.
fn terms(
    context: &Context,
    params: &Params,
    search: Option<Vec<u8>>,
    per_page: u64,
    offset: u64,
) -> Result<(Vec<Found>, u64), RestError> {
    let mut taxonomies = asked(params, &TAXONOMIES).into_iter();
    let taxonomy = taxonomies.next().ok_or_else(RestError::not_ported)?;
    let (include, exclude) = include_exclude(params);
    let query = TermQuery {
        taxonomy,
        taxonomies: taxonomies.collect(),
        include,
        exclude,
        search: search.map(|search| String::from_utf8_lossy(&search).into_owned()),
        order: Some((TermOrder::Name, false)),
        limit: Some(per_page),
        offset,
        ..TermQuery::default()
    };
    let ids: Vec<u64> = context.site.terms(&query).iter().map(|t| t.id).collect();
    // Each term with its taxonomy, as `get_term()` finds it, in order.
    let mut found = Vec::new();
    for taxonomy in std::iter::once(&query.taxonomy).chain(&query.taxonomies) {
        let within = TermQuery {
            taxonomy: taxonomy.clone(),
            include: ids.clone(),
            limit: Some(ids.len() as u64),
            ..TermQuery::default()
        };
        let terms = context.site.terms(&within).into_iter();
        found.extend(terms.map(|term| (taxonomy.clone(), term)));
    }
    found.sort_by_key(|(_, term)| ids.iter().position(|id| *id == term.id));
    let found = found
        .into_iter()
        .map(|(taxonomy, term)| Found::Term(taxonomy, term));
    Ok((found.collect(), context.site.count_terms(&query)))
}

/// `WP_REST_Post_Format_Search_Handler::search_items()`: the formats with
/// a term whose slug or name has the search in it, ignoring case.
fn formats(
    context: &Context,
    search: Option<Vec<u8>>,
    per_page: u64,
    offset: u64,
) -> (Vec<Found>, u64) {
    let terms = context.site.terms(&TermQuery {
        taxonomy: "post_format".into(),
        ..TermQuery::default()
    });
    // A format's name is its slug capitalised, so one match is both's.
    let found: Vec<Found> = POST_FORMATS
        .iter()
        .filter(|(slug, _)| {
            search.as_ref().is_none_or(|search| {
                let search = search.to_ascii_lowercase();
                slug.as_bytes()
                    .windows(search.len())
                    .any(|part| part == search)
            })
        })
        .filter_map(|(slug, name)| {
            let term = terms
                .iter()
                .find(|term| term.slug == format!("post-format-{slug}").into_bytes())?;
            Some(Found::Format(
                slug,
                name,
                context.site.term_link("post_format", term),
            ))
        })
        .collect();
    let total = found.len() as u64;
    let skip = usize::try_from(offset).unwrap_or(usize::MAX);
    let take = usize::try_from(per_page).unwrap_or(usize::MAX);
    (found.into_iter().skip(skip).take(take).collect(), total)
}

/// `prepare_item_for_response()`: the handler's fields and links.
fn prepare(context: &Context, params: &Params, found: &Found) -> Response {
    let wanted = fields_for_response(params, PROPERTIES);
    let mut response = match found {
        Found::Post(post) => {
            let fields: [(&str, &dyn Fn() -> Value); 5] = [
                ("id", &|| int(post.id)),
                ("title", &|| text(admin_title(post))),
                ("url", &|| {
                    text(hyscribe_render::permalinks::permalink(
                        context.site.reads(),
                        post,
                    ))
                }),
                ("type", &|| text("post")),
                ("subtype", &|| text(post.post_type.clone())),
            ];
            item_response(&fields, &wanted, PROPERTIES, &params.context())
        }
        Found::Term(taxonomy, term) => {
            let fields: [(&str, &dyn Fn() -> Value); 4] = [
                ("id", &|| int(term.id)),
                ("title", &|| text(term.name.clone())),
                ("url", &|| text(context.site.term_link(taxonomy, term))),
                ("type", &|| text(taxonomy.clone())),
            ];
            item_response(&fields, &wanted, PROPERTIES, &params.context())
        }
        Found::Format(slug, name, link) => {
            let fields: [(&str, &dyn Fn() -> Value); 4] = [
                ("id", &|| text(*slug)),
                ("title", &|| text(*name)),
                ("url", &|| text(link.clone())),
                ("type", &|| text("post-format")),
            ];
            item_response(&fields, &wanted, PROPERTIES, &params.context())
        }
    };
    if links_wanted(&wanted) {
        let mut linked = |own: String, about: String| {
            crate::self_link_with(context, &mut response, &own, true);
            response.add_link("about", context.rest_url(&about));
        };
        match found {
            Found::Post(post) => {
                let route = crate::types::items_route(&post.post_type).unwrap_or_default();
                let about = format!("/wp/v2/types/{}", post.post_type);
                linked(format!("{route}/{}", post.id), about);
            }
            Found::Term(taxonomy, term) => {
                let route = crate::terms::items_route_of(taxonomy);
                let about = format!("/wp/v2/taxonomies/{taxonomy}");
                linked(format!("{route}/{}", term.id), about);
            }
            Found::Format(..) => {}
        }
        response.add_link("collection", context.rest_url(ROUTE));
    }
    response
}
