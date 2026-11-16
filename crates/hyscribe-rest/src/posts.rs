// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Posts_Controller` for posts and pages: `/wp/v2/posts`,
//! `/wp/v2/pages` and their single items, read only, and for the types
//! whose controllers extend it. Writes answer 501 but their permission
//! checks give `Allow` and `targetHints`.

pub(crate) mod args;
mod menu_items;
pub(crate) mod prepare;

use hyscribe_model::{FormatFilter, OrderBy, Post, PostQuery, StatusFilter, TermFilter};
use hyscribe_phpser::{Value, decode};
use hyscribe_render::formatting::sanitize_title_for_query;
use hyscribe_render::rest::Rendering;
use hyscribe_render::site::Request;
use hyscribe_render::{order_by, search_count, search_terms, sticky_posts};

use crate::request::Params;
use crate::response::{Response, RestError, paginate};
use crate::statuses::STATUSES;
use crate::{Context, Endpoint};

/// A post type the controller serves.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Post,
    Page,
    Block,
    Navigation,
    MenuItem,
    Attachment,
}

const KINDS: [Kind; 6] = [
    Kind::Post,
    Kind::Page,
    Kind::Block,
    Kind::Navigation,
    Kind::MenuItem,
    Kind::Attachment,
];

impl Kind {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Kind::Post => "post",
            Kind::Page => "page",
            Kind::Block => "wp_block",
            Kind::Navigation => "wp_navigation",
            Kind::MenuItem => "nav_menu_item",
            Kind::Attachment => "attachment",
        }
    }

    pub(crate) fn base(self) -> &'static str {
        match self {
            Kind::Post => "posts",
            Kind::Page => "pages",
            Kind::Block => "blocks",
            Kind::Navigation => "navigation",
            Kind::MenuItem => "menu-items",
            Kind::Attachment => "media",
        }
    }

    /// The type of a route under `/wp/v2/<base>`.
    pub(crate) fn of(route: &str) -> Kind {
        let base = route.trim_start_matches("/wp/v2/").split('/').next();
        let found = KINDS.into_iter().find(|kind| base == Some(kind.base()));
        found.unwrap_or(Kind::Post)
    }

    fn field(self, path: &[&str]) -> Option<&'static Value> {
        crate::types::post_type(self.name()).and_then(|fields| fields.get_path(path))
    }

    /// `$post_type->cap->$cap`.
    pub(crate) fn cap(self, cap: &str) -> String {
        let mapped = self.field(&["capabilities", cap]);
        String::from_utf8_lossy(&mapped.map(Value::to_php_string).unwrap_or_default()).into_owned()
    }

    /// `post_type_supports()`.
    pub(crate) fn supports(self, feature: &str) -> bool {
        self.field(&["supports", feature]) == Some(&Value::Bool(true))
    }

    /// The REST taxonomies of the type: their names and REST bases.
    pub(crate) fn taxonomies(self) -> &'static [(&'static str, &'static str)] {
        match self {
            Kind::Post => &[("category", "categories"), ("post_tag", "tags")],
            Kind::Block => &[("wp_pattern_category", "wp_pattern_category")],
            Kind::MenuItem => &[("nav_menu", "menus")],
            Kind::Page | Kind::Navigation | Kind::Attachment => &[],
        }
    }
}

/// The endpoints of the route `regex`, if one of these.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    let (kind, single) = match regex {
        "/wp/v2/posts" => (Kind::Post, false),
        "/wp/v2/pages" => (Kind::Page, false),
        "/wp/v2/posts/(?P<id>[\\d]+)" => (Kind::Post, true),
        "/wp/v2/pages/(?P<id>[\\d]+)" => (Kind::Page, true),
        _ => return None,
    };
    Some(type_endpoints(kind, single))
}

/// The endpoints of a type's collection, or of its single items; each
/// finds its type from the route.
pub(crate) fn type_endpoints(kind: Kind, single: bool) -> Vec<Endpoint> {
    if !single {
        return vec![
            Endpoint {
                args: args::collection(kind),
                permission: |c, p| items_permission(c, p, Kind::of(p.route)),
                callback: Some(|c, p| items(c, p, Kind::of(p.route))),
            },
            Endpoint {
                args: Vec::new(),
                permission: |c, p| create_permission(c, p, Kind::of(p.route)),
                callback: None,
            },
        ];
    }
    let write = |permission| Endpoint {
        args: vec![args::id()],
        permission,
        callback: None,
    };
    vec![
        Endpoint {
            args: args::item(),
            permission: |c, p| item_permission(c, p, Kind::of(p.route)),
            callback: Some(|c, p| item(c, p, Kind::of(p.route))),
        },
        write(|c, p| update_permission(c, p, Kind::of(p.route))),
        write(|c, p| delete_permission(c, p, Kind::of(p.route))),
    ]
}

/// `get_items_permissions_check()`.
fn items_permission(context: &Context, params: &Params, kind: Kind) -> Result<(), RestError> {
    if args::not_ported().iter().any(|name| params.has(name)) {
        return Err(RestError::not_ported());
    }
    if params.context() == b"edit" && !context.can(&kind.cap("edit_posts")) {
        let message = "Sorry, you are not allowed to edit posts in this post type.";
        return Err(context.forbidden("rest_forbidden_context", message));
    }
    menu_item_access(context, kind)
}

/// The menu items' `check_has_read_only_access()`.
fn menu_item_access(context: &Context, kind: Kind) -> Result<(), RestError> {
    if kind != Kind::MenuItem || reads_menus(context) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to view menu items.";
    Err(context.forbidden("rest_cannot_view", message))
}

/// The menus controllers' `check_has_read_only_access()`: whoever can
/// edit the theme's options or posts of any type shown in REST.
pub(crate) fn reads_menus(context: &Context) -> bool {
    context.can("edit_theme_options") || crate::types::can_edit_any(context)
}

/// `! empty( $request[ $name ] )`.
fn set(params: &Params, name: &str) -> bool {
    params.get(name).is_some_and(|value| !value.is_empty())
}

/// `get_current_user_id() !== $request['author']`, of an `author` set:
/// only an integer can be the user's ID.
fn other_author(context: &Context, params: &Params) -> bool {
    set(params, "author") && params.get("author") != Some(&Value::Int(context.user.cast_signed()))
}

/// `check_assign_terms_permission()`: each term of the type's taxonomies
/// the request names, that exists, can be assigned. An empty value, of a
/// list or of a term query's fields, is `get_term()`'s error, checked as 0.
fn can_assign_terms(context: &Context, params: &Params, kind: Kind) -> bool {
    kind.taxonomies().iter().all(|(taxonomy, base)| {
        let values = match params.get(base) {
            Some(Value::Array(values)) => values.iter().map(|(_, v)| v.clone()).collect(),
            Some(value) => vec![value.clone()],
            None => Vec::new(),
        };
        values.into_iter().all(|value| {
            let id = value.to_int().unsigned_abs();
            let missing = context.site.reads().term(id, taxonomy).is_none();
            !value.is_empty() && missing || context.can_for("assign_term", id)
        })
    })
}

/// `create_item_permissions_check()`.
fn create_permission(context: &Context, params: &Params, kind: Kind) -> Result<(), RestError> {
    if set(params, "id") {
        return Err(RestError::status(
            "rest_post_exists",
            "Cannot create existing post.",
            400,
        ));
    }
    let others = context.can(&kind.cap("edit_others_posts"));
    if other_author(context, params) && !others {
        let message = "Sorry, you are not allowed to create posts as this user.";
        return Err(context.forbidden("rest_cannot_edit_others", message));
    }
    if set(params, "sticky") && (!others || !context.can(&kind.cap("publish_posts"))) {
        let message = "Sorry, you are not allowed to make posts sticky.";
        return Err(context.forbidden("rest_cannot_assign_sticky", message));
    }
    if !context.can(&kind.cap("create_posts")) {
        let message = "Sorry, you are not allowed to create posts as this user.";
        return Err(context.forbidden("rest_cannot_create", message));
    }
    if !can_assign_terms(context, params, kind) {
        let message = "Sorry, you are not allowed to assign the provided terms.";
        return Err(context.forbidden("rest_cannot_assign_term", message));
    }
    if kind == Kind::Attachment {
        crate::media::create_permission(context, params)?;
    }
    Ok(())
}

/// `get_post()`: the post of the type with the request's `id`.
fn get_post(context: &Context, params: &Params, kind: Kind) -> Result<Post, RestError> {
    // The route takes digits only, and no post has ID 0.
    let id = params.get("id").map_or(0, Value::to_int).unsigned_abs();
    let post = context.site.post(id);
    let post = post.filter(|post| post.post_type == kind.name());
    post.ok_or_else(|| RestError::status("rest_post_invalid_id", "Invalid post ID.", 404))
}

/// `check_update_permission()`.
fn can_update(context: &Context, post: &Post) -> bool {
    context.can_for("edit_post", post.id)
}

/// `check_read_permission()`: a post of a type shown in REST, published,
/// the only public status core registers, or readable by the user, or
/// inheriting a readable parent's status, or published without one; a
/// pattern only by those who may read it.
pub(crate) fn can_read(context: &Context, post: &Post) -> bool {
    let mut seen = Vec::new();
    let mut post = post.clone();
    loop {
        let readable = || context.can_for("read_post", post.id);
        let shown = crate::types::post_type(&post.post_type).is_some();
        if !shown || post.post_type == "wp_block" && !readable() {
            return false;
        }
        if post.status == "publish" || readable() {
            return true;
        }
        if post.status != "inherit" {
            return false;
        }
        seen.push(post.id);
        let parent = (!seen.contains(&post.parent))
            .then(|| context.site.post(post.parent))
            .flatten();
        match parent {
            Some(parent) => post = parent,
            None => return true,
        }
    }
}

/// `get_item_permissions_check()`.
fn item_permission(context: &Context, params: &Params, kind: Kind) -> Result<(), RestError> {
    let post = get_post(context, params, kind)?;
    if params.context() == b"edit" && !can_update(context, &post) {
        let message = "Sorry, you are not allowed to edit this post.";
        return Err(context.forbidden("rest_forbidden_context", message));
    }
    let password =
        hyscribe_phpser::lookup(&params.get, "password").filter(|password| !password.is_empty());
    if password.is_some_and(|password| password.to_php_string() != post.password) {
        return Err(RestError::status(
            "rest_post_incorrect_password",
            "Incorrect post password.",
            403,
        ));
    }
    if !can_read(context, &post) {
        let message = "Sorry, you are not allowed to do that.";
        return Err(context.forbidden("rest_forbidden", message));
    }
    menu_item_access(context, kind)
}

/// `update_item_permissions_check()`.
pub(crate) fn update_permission(
    context: &Context,
    params: &Params,
    kind: Kind,
) -> Result<(), RestError> {
    let post = get_post(context, params, kind)?;
    if !can_update(context, &post) {
        let message = "Sorry, you are not allowed to edit this post.";
        return Err(context.forbidden("rest_cannot_edit", message));
    }
    let others = context.can(&kind.cap("edit_others_posts"));
    if other_author(context, params) && !others {
        let message = "Sorry, you are not allowed to update posts as this user.";
        return Err(context.forbidden("rest_cannot_edit_others", message));
    }
    let sticky = params.get("sticky").map(|sticky| !sticky.is_empty());
    let changes = sticky.is_some_and(|sticky| sticky != is_sticky(context, post.id));
    if changes && (!others || !context.can(&kind.cap("publish_posts"))) {
        let message = "Sorry, you are not allowed to make posts sticky.";
        return Err(context.forbidden("rest_cannot_assign_sticky", message));
    }
    if !can_assign_terms(context, params, kind) {
        let message = "Sorry, you are not allowed to assign the provided terms.";
        return Err(context.forbidden("rest_cannot_assign_term", message));
    }
    Ok(())
}

/// `delete_item_permissions_check()`.
fn delete_permission(context: &Context, params: &Params, kind: Kind) -> Result<(), RestError> {
    let post = get_post(context, params, kind)?;
    if context.can_for("delete_post", post.id) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to delete this post.";
    Err(context.forbidden("rest_cannot_delete", message))
}

/// The `sticky_posts` option.
fn sticky(context: &Context) -> Vec<u64> {
    sticky_posts(&context.site.option("sticky_posts").unwrap_or_default())
}

/// `is_sticky( $id )`.
pub(crate) fn is_sticky(context: &Context, id: u64) -> bool {
    sticky(context).contains(&id)
}

/// `get_permalink()`: an attachment's link, or the post's.
pub(crate) fn link(context: &Context, post: &Post) -> Vec<u8> {
    if post.post_type == "attachment" {
        return crate::media::attachment_link(context, post, false);
    }
    hyscribe_render::permalinks::permalink(context.site.reads(), post)
}

/// `get_item()`.
fn item(context: &Context, params: &Params, kind: Kind) -> Result<Response, RestError> {
    // The permission check found the post.
    let post = get_post(context, params, kind).unwrap_or_default();
    let request = render_request(context);
    let mut rendering = Rendering::new(context.site.theme(), context.site.reads(), &request);
    let mut response = prepare::post(context, params, kind, &post, &mut rendering)?;
    if crate::types::viewable(kind.name()) {
        let link = link(context, &post);
        let link = String::from_utf8_lossy(&link);
        response.header(
            "Link",
            &format!("<{link}>; rel=\"alternate\"; type=text/html"),
        );
    }
    Ok(response)
}

pub(crate) fn ids(params: &Params, name: &str) -> Vec<u64> {
    let list = params
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default();
    list.iter()
        .map(|(_, id)| id.to_int().unsigned_abs())
        .collect()
}

fn strings(params: &Params, name: &str) -> Vec<Vec<u8>> {
    let list = params
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default();
    list.iter().map(|(_, item)| item.to_php_string()).collect()
}

/// `WP_Query`'s `post_status` of the request's statuses: `any` is every
/// status not excluded from searches, those not internal for the posts and
/// pages any status of theirs is.
fn statuses(params: &Params) -> Vec<StatusFilter> {
    let asked = strings(params, "status");
    let status = |name: &str| StatusFilter {
        status: name.into(),
        author: None,
    };
    if asked.iter().any(|name| name == b"any") {
        return STATUSES
            .iter()
            .filter(|known| !known.internal || asked.contains(&known.name.as_bytes().to_vec()))
            .map(|known| status(known.name))
            .collect();
    }
    STATUSES
        .iter()
        .filter(|known| asked.contains(&known.name.as_bytes().to_vec()))
        .map(|known| status(known.name))
        .collect()
}

/// The `WP_Query` of `get_items()` and `prepare_items_query()`, and the
/// page it asks for.
fn query(
    context: &Context,
    params: &Params,
    kind: Kind,
) -> Result<(PostQuery, u64, u32), RestError> {
    order_errors(params)?;
    let orderby = params.text("orderby");
    let search = params
        .get("search")
        .map(Value::to_php_string)
        .filter(|s| !s.is_empty());
    let mut include = ids(params, "include");
    // Only the type's own arguments are read.
    let registered = args::collection(kind);
    let param = |name: &str| {
        let known = registered.iter().any(|arg| arg.name == name);
        params.get(name).filter(|_| known)
    };
    let listed = |name: &str| param(name).map_or_else(Vec::new, |_| ids(params, name));
    let mut exclude = ids(params, "exclude");
    if let Some(wanted) = param("sticky") {
        let sticky = sticky(context);
        if !wanted.is_empty() {
            include = if include.is_empty() {
                sticky
            } else {
                sticky
                    .into_iter()
                    .filter(|id| include.contains(id))
                    .collect()
            };
            if include.is_empty() {
                include = vec![0];
            }
        } else {
            exclude.extend(sticky);
        }
    }
    let per_page = params.get("per_page").map_or(10, Value::to_int).max(1);
    let page = params
        .get("page")
        .map_or(1, Value::to_int)
        .max(0)
        .unsigned_abs();
    let offset = params.get("offset").map_or(0, Value::to_int).unsigned_abs();
    let ascending = params
        .get("order")
        .is_some_and(|order| order.to_php_string() == b"asc");
    let names: Vec<String> = strings(params, "slug")
        .iter()
        .map(|slug| String::from_utf8_lossy(&sanitize_title_for_query(slug)).into_owned())
        .collect();
    let mut query = PostQuery {
        post_types: vec![kind.name().into()],
        statuses: statuses(params),
        include: (!include.is_empty()).then_some(include),
        exclude,
        authors: listed("author"),
        excluded_authors: listed("author_exclude"),
        parents: listed("parent"),
        excluded_parents: listed("parent_exclude"),
        names,
        menu_order: param("menu_order").map(Value::to_int),
        search_columns: strings(params, "search_columns")
            .into_iter()
            .map(|column| String::from_utf8_lossy(&column).into_owned())
            .collect(),
        formats: param("format").map(|_| {
            let formats = strings(params, "format");
            FormatFilter {
                standard: formats.iter().any(|format| format == b"standard"),
                slugs: formats
                    .iter()
                    .filter(|format| *format != b"standard")
                    .map(|format| format!("post-format-{}", String::from_utf8_lossy(format)))
                    .collect(),
            }
        }),
        exact: params.get("search_semantics").is_some(),
        logged_in: context.user != 0,
        mime_types: crate::media::mime_types(params, kind),
        filenames: kind == Kind::Attachment,
        limit: u32::try_from(per_page).unwrap_or(u32::MAX),
        offset: u32::try_from(if offset > 0 {
            offset
        } else {
            page.saturating_sub(1)
                .saturating_mul(per_page.unsigned_abs())
        })
        .unwrap_or(u32::MAX),
        ..PostQuery::default()
    };
    let by = match orderby.as_slice() {
        b"id" => &b"ID"[..],
        b"include" => b"post__in",
        b"slug" => b"post_name",
        b"include_slugs" => b"post_name__in",
        by => by,
    };
    query.order = order_by(&Value::String(by.to_vec()), ascending, &query);
    query.terms = tax_query(params, kind)?;
    if let Some(search) = search {
        searched(&mut query, &search);
        if orderby == b"relevance" {
            query.order.insert(0, (OrderBy::Relevance, false));
        }
    }
    // A post list is a home list, which puts sticky posts first on its
    // first page, unless asked not to or limited to some posts.
    let ignore = params
        .get("ignore_sticky")
        .is_none_or(|ignore| !ignore.is_empty());
    let home = query.search.is_empty() && query.include.is_none();
    if kind == Kind::Post && !ignore && home && page <= 1 {
        query.sticky = sticky(context);
    }
    Ok((query, page, u32::try_from(per_page).unwrap_or(u32::MAX)))
}

/// `prepare_tax_query()`: each taxonomy's terms to include and leave out.
/// Children of terms, every term required and either of several clauses
/// are not ported.
fn tax_query(params: &Params, kind: Kind) -> Result<Vec<TermFilter>, RestError> {
    let mut filters = Vec::new();
    for (taxonomy, base) in kind.taxonomies() {
        for (name, exclude) in [(*base, false), (args::exclude_name(base), true)] {
            let Some(limit) = params.get(name).filter(|limit| !limit.is_empty()) else {
                continue;
            };
            let field = |key: &str| limit.get_str(key).filter(|value| !value.is_empty());
            let terms = match crate::schema::is_list(limit) {
                true => Some(limit),
                false => field("terms"),
            };
            let all = !exclude && field("operator").is_some_and(|op| op.as_bytes() == Some(b"AND"));
            if field("include_children").is_some() || all {
                return Err(RestError::not_ported());
            }
            let terms: Vec<u64> = terms
                .and_then(Value::as_array)
                .unwrap_or_default()
                .iter()
                .map(|(_, id)| id.to_int().unsigned_abs())
                .collect();
            if !terms.is_empty() {
                filters.push(TermFilter {
                    taxonomy: (*taxonomy).into(),
                    terms,
                    exclude,
                });
            }
        }
    }
    if filters.len() > 1 && params.text("tax_relation") == b"OR" {
        return Err(RestError::not_ported());
    }
    Ok(filters)
}

/// `WP_Query`'s search for `s`, not empty.
pub(crate) fn searched(query: &mut PostQuery, search: &[u8]) {
    let text = String::from_utf8_lossy(search).into_owned();
    query.search = search_terms(&text);
    query.sentence = text.replace(['\r', '\n'], "");
    query.search_count = search_count(&text);
}

/// The ordering errors `get_items()` checks: relevance without a search,
/// and the order of `include` without one.
pub(crate) fn order_errors(params: &Params) -> Result<(), RestError> {
    let orderby = params.text("orderby");
    if orderby == b"relevance" && !params.get("search").is_some_and(|s| !s.is_empty()) {
        let message = "You need to define a search term to order by relevance.";
        return Err(RestError::status(
            "rest_no_search_term_defined",
            message,
            400,
        ));
    }
    if orderby == b"include" && ids(params, "include").is_empty() {
        let message = "You need to define an include parameter to order by include.";
        return Err(RestError::status(
            "rest_orderby_include_missing_include",
            message,
            400,
        ));
    }
    Ok(())
}

/// `get_items()`.
fn items(context: &Context, params: &Params, kind: Kind) -> Result<Response, RestError> {
    let (query, page, per_page) = query(context, params, kind)?;
    let reads = context.site.reads();
    let posts = reads.query(&query);
    // `found_posts` stays 0 for an empty page; past the first, it is
    // counted again from the offset alone, which on the first finds
    // nothing either.
    let total = if posts.is_empty() {
        let offset = params.get("offset").map_or(0, Value::to_int).unsigned_abs();
        let recount = PostQuery {
            offset: u32::try_from(offset).unwrap_or(u32::MAX),
            ..query.clone()
        };
        let empty = reads.query(&recount).is_empty();
        if empty {
            0
        } else {
            reads.count_posts(&recount)
        }
    } else {
        reads.count_posts(&query)
    };
    let pages = total.div_ceil(u64::from(per_page));
    if page > pages && total > 0 {
        let message = "The page number requested is larger than the number of pages available.";
        return Err(RestError::status(
            "rest_post_invalid_page_number",
            message,
            400,
        ));
    }
    let edit = params.context() == b"edit";
    let shown = posts.iter().filter(|_| !context.head).filter(|post| {
        if edit {
            can_update(context, post)
        } else {
            can_read(context, post)
        }
    });
    let request = render_request(context);
    let mut rendering = Rendering::new(context.site.theme(), reads, &request);
    let mut prepared = |post| prepare::post(context, params, kind, post, &mut rendering);
    let data: Result<Vec<Value>, RestError> =
        shown.map(|post| Ok(prepared(post)?.into_data())).collect();
    let data = data?;
    let mut response = Response::new(crate::response::list(data), 200);
    let collection = context.rest_url(&format!("/wp/v2/{}", kind.base()));
    paginate(
        &mut response,
        (total, pages, page),
        &params.get,
        &collection,
    );
    Ok(response)
}

/// The request as rendering reads it: its URI, which links keep, and no
/// main query, which `rest_api_loaded()` leaves unrun.
pub(crate) fn render_request(context: &Context) -> Request {
    Request {
        uri: context.uri.to_vec(),
        ..Request::default()
    }
}

/// The revisions of a post: how many, and the newest.
pub(crate) fn revisions(context: &Context, post: &Post) -> (u64, Option<u64>) {
    let revisions = context.site.revisions(post.id);
    (revisions.len() as u64, revisions.first().copied())
}

/// `get_post_meta( $id, $key, true )` unserialized.
pub(crate) fn meta(context: &Context, id: u64, key: &str) -> Option<Value> {
    let stored = context.site.post_meta(id, key)?;
    Some(decode(&stored).unwrap_or(Value::String(stored)))
}
