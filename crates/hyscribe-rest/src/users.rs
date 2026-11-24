// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Users_Controller`: `/wp/v2/users`, `/wp/v2/users/me` and
//! `/wp/v2/users/<id>`, read only. The owner can turn the public users
//! endpoint off: a visitor then sees no user as having published
//! posts.

use hyscribe_model::{User, UserOrder, UserQuery};
use hyscribe_phpser::{Key, Value};
use hyscribe_render::date::{Zone, format, format_unix, strtotime};
use hyscribe_render::{avatar_url, user_name};

use crate::fields::{Properties, fields_for_response, item_response, links_wanted};
use crate::request::{Arg, Params, Sanitize, collection_args, typed};
use crate::response::{Response, RestError, int, list, object, paginate, text};
use crate::schema::{parse_list, prepare_value};
use crate::terms::ids;
use crate::types::{context_arg, shown_in_rest};
use crate::{Context, Endpoint};

pub(crate) mod write;

const ALL: &[&str] = &["embed", "view", "edit"];

/// The schema's properties and their contexts; `avatar_urls` only while
/// the site shows avatars.
const PROPERTIES: &Properties = &[
    ("id", ALL),
    ("username", &["edit"]),
    ("name", ALL),
    ("first_name", &["edit"]),
    ("last_name", &["edit"]),
    ("email", &["edit"]),
    ("url", ALL),
    ("description", ALL),
    ("link", ALL),
    ("locale", &["edit"]),
    ("nickname", &["edit"]),
    ("slug", ALL),
    ("registered_date", &["edit"]),
    ("roles", &["edit"]),
    ("password", &[]),
    ("capabilities", &["edit"]),
    ("extra_capabilities", &["edit"]),
    ("avatar_urls", ALL),
    ("meta", &["view", "edit"]),
];

const ROUTE: &str = "/wp/v2/users";
const ITEM: &str = "/wp/v2/users/(?P<id>[\\d]+)";
const ME: &str = "/wp/v2/users/me";

/// The persisted preferences' schema, `wp_register_persisted_preferences_meta()`'s.
const PREFERENCES: &str = r#"{"type":"object","context":["edit"],"properties":{"_modified":{"type":"string","format":"date-time","readonly":false}},"additionalProperties":true,"default":[]}"#;

/// Whether the current user is a visitor and the public users endpoint is
/// off, so no user counts as having published posts.
fn authors_hidden(context: &Context) -> bool {
    context.user == 0 && !context.site.public_users()
}

/// `mysql_to_rfc3339( $date )`.
pub(crate) fn rfc3339(date: &str) -> String {
    format("Y-m-d\\TH:i:s", date, Zone::Utc)
}

/// `rest_get_avatar_urls()`: each of `rest_get_avatar_sizes()` and the URL
/// of the avatar of `email` and `name`, or `false` for each where not `shown`.
pub(crate) fn avatar_urls(context: &Context, email: &[u8], name: &[u8], shown: bool) -> Value {
    let url = |size| {
        let option = |name: &str| context.option(name);
        let url = shown.then(|| text(avatar_url(&option, email, name, size)));
        (Key::Int(size), url.unwrap_or(Value::Bool(false)))
    };
    Value::Array([24, 48, 96].map(url).into())
}

fn properties(context: &Context) -> Vec<(&'static str, &'static [&'static str])> {
    let avatars = crate::comments::shows_avatars(context);
    PROPERTIES
        .iter()
        .filter(|(name, _)| *name != "avatar_urls" || avatars)
        .copied()
        .collect()
}

/// The names of the post types shown in REST.
fn rest_types() -> Vec<Value> {
    shown_in_rest()
        .map(|(name, _)| Value::String(name.to_php_string()))
        .collect()
}

/// The collection's arguments, `get_collection_params()`.
fn items_args() -> Vec<Arg> {
    let ids = r#"{"type":"array","items":{"type":"integer"},"default":[]}"#;
    let strings = r#"{"type":"array","items":{"type":"string"}}"#;
    let mut args = collection_args();
    args.extend([
        typed("exclude", ids),
        typed("include", ids),
        typed("offset", r#"{"type":"integer"}"#),
        typed("order", r#"{"default":"asc","enum":["asc","desc"],"type":"string"}"#),
        typed("orderby", r#"{"default":"name","enum":["id","include","name","registered_date","slug","include_slugs","email","url"],"type":"string"}"#),
        typed("slug", strings),
        typed("roles", strings),
        typed("capabilities", strings),
        typed("who", r#"{"type":"string","enum":["authors"]}"#),
    ]);
    let published = object([
        ("type", list(vec![text("boolean"), text("array")])),
        (
            "items",
            object([("type", text("string")), ("enum", list(rest_types()))]),
        ),
    ]);
    args.push(Arg {
        name: "has_published_posts",
        schema: published,
        validate: false,
        sanitize: Sanitize::Schema,
    });
    args.push(typed("search_columns", r#"{"default":[],"type":"array","items":{"enum":["email","name","id","username","slug"],"type":"string"}}"#));
    args
}

/// The endpoints of the route `regex`, if ported.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    let id = || typed("id", r#"{"type":"integer"}"#);
    let write = |args: Vec<Arg>, permission| Endpoint {
        args,
        permission,
        callback: None,
    };
    match regex {
        ROUTE => Some(vec![
            Endpoint {
                args: items_args(),
                permission: items_permission,
                callback: Some(items),
            },
            write(write::save_args(true), create_permission),
        ]),
        ITEM => Some(vec![
            Endpoint {
                args: vec![id(), context_arg()],
                permission: item_permission,
                callback: Some(item),
            },
            write(
                [vec![id()], write::save_args(false)].concat(),
                update_permission,
            ),
            write(write::delete_args(false), delete_permission),
        ]),
        ME => Some(vec![
            Endpoint {
                args: vec![context_arg()],
                permission: |_, _| Ok(()),
                callback: Some(me),
            },
            write(write::save_args(false), |context, params| {
                update_permission(context, &as_me(context, params))
            }),
            write(write::delete_args(true), |context, params| {
                delete_permission(context, &as_me(context, params))
            }),
        ]),
        _ => None,
    }
}

/// `$request['id'] = get_current_user_id()`, in every source that has it.
fn as_me(context: &Context, params: &Params) -> Params {
    let mut params = params.clone();
    params.set(
        "id",
        Value::Int(i64::try_from(context.user).unwrap_or_default()),
    );
    params
}

fn set(params: &Params, name: &str) -> bool {
    params.get(name).is_some_and(|value| !value.is_empty())
}

/// `get_items_permissions_check()`.
fn items_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let lists = context.can("list_users");
    for (name, what) in [("roles", "role"), ("capabilities", "capability")] {
        if set(params, name) && !lists {
            let message = format!("Sorry, you are not allowed to filter users by {what}.");
            return Err(context.forbidden("rest_user_cannot_view", &message));
        }
    }
    if params.context() == b"edit" && !lists {
        let message = "Sorry, you are not allowed to edit users.";
        return Err(context.forbidden("rest_forbidden_context", message));
    }
    let orderby = params.text("orderby");
    if (orderby == b"email" || orderby == b"registered_date") && !lists {
        let message = "Sorry, you are not allowed to order users by this parameter.";
        return Err(context.forbidden("rest_forbidden_orderby", message));
    }
    if params.text("who") == b"authors" {
        let edits = shown_in_rest().any(|(name, fields)| {
            let name = String::from_utf8_lossy(&name.to_php_string()).into_owned();
            let cap = fields
                .get_path(&["capabilities", "edit_posts"])
                .map(Value::to_php_string);
            crate::types::supports(&name, "author").is_some()
                && context.can(&String::from_utf8_lossy(&cap.unwrap_or_default()))
        });
        if !edits {
            let message = "Sorry, you are not allowed to query users by this parameter.";
            return Err(context.forbidden("rest_forbidden_who", message));
        }
    }
    Ok(())
}

/// The strings of a list parameter.
fn strings(params: &Params, name: &str) -> Vec<String> {
    let value = params.get(name).cloned().unwrap_or_default();
    parse_list(&value)
        .iter()
        .map(|(_, item)| String::from_utf8_lossy(&item.to_php_string()).into_owned())
        .collect()
}

/// `role__in` with the roles granting any of `capability__in`, and those.
fn roles(context: &Context, params: &Params) -> Vec<String> {
    let mut roles = strings(params, "roles");
    let caps = strings(params, "capabilities");
    if !caps.is_empty() {
        let prefix = context.site.table_prefix();
        let stored = context
            .option(&format!("{prefix}user_roles"))
            .unwrap_or_default();
        let defined = crate::unserialized(&stored);
        for (role, data) in defined.as_array().unwrap_or_default() {
            let granted = data.get_str("capabilities").and_then(Value::as_array);
            let grants = granted.unwrap_or_default().iter().any(|(cap, value)| {
                !value.is_empty() && caps.iter().any(|c| c.as_bytes() == cap.to_php_string())
            });
            if grants {
                roles.push(String::from_utf8_lossy(&role.to_php_string()).into_owned());
            }
        }
        roles.extend(caps);
    }
    let mut unique: Vec<String> = Vec::new();
    for role in roles {
        if !unique.contains(&role) {
            unique.push(role);
        }
    }
    unique
}

/// `WP_User_Query`'s search of `*text*`: the text without `*` either side,
/// in the columns asked for, else those guessed from it.
fn search(context: &Context, params: &Params) -> Option<(Vec<String>, String)> {
    let text = String::from_utf8_lossy(&params.text("search")).into_owned();
    if Value::String(text.clone().into_bytes()).is_empty() {
        return None;
    }
    let mut columns: Vec<&str> = if context.can("list_users") {
        vec![
            "ID",
            "user_login",
            "user_nicename",
            "user_email",
            "display_name",
        ]
    } else {
        vec!["ID", "user_login", "user_nicename", "display_name"]
    };
    let asked: Vec<&str> = strings(params, "search_columns")
        .iter()
        .map(|column| match column.as_str() {
            "id" => "ID",
            "username" => "user_login",
            "slug" => "user_nicename",
            "email" => "user_email",
            _ => "display_name",
        })
        .filter(|column| columns.contains(column))
        .collect();
    let preset = !context.can("list_users");
    if !asked.is_empty() {
        columns = asked;
    } else if !preset {
        columns.clear();
    }
    let text = text.trim_matches('*').to_string();
    if columns.is_empty() {
        columns = if text.contains('@') {
            vec!["user_email"]
        } else if Value::String(text.clone().into_bytes()).is_numeric() {
            vec!["user_login", "ID"]
        } else if text.starts_with("http://") || text.starts_with("https://") {
            vec!["user_url"]
        } else {
            vec![
                "user_login",
                "user_url",
                "user_email",
                "user_nicename",
                "display_name",
            ]
        };
    }
    Some((columns.into_iter().map(String::from).collect(), text))
}

/// The query `get_items()` prepares, without its page.
fn prepared(context: &Context, params: &Params) -> UserQuery {
    let prefix = context.site.table_prefix();
    let authors = params.text("who") == b"authors";
    let types = || {
        rest_types()
            .iter()
            .map(|ty| String::from_utf8_lossy(&ty.to_php_string()).into_owned())
            .collect()
    };
    let published = match params.get("has_published_posts") {
        Some(Value::Bool(true)) => Some(types()),
        Some(value) if !value.is_empty() => Some(strings(params, "has_published_posts")),
        _ => (!authors && !context.can("list_users")).then(types),
    };
    let include = if set(params, "include") {
        ids(params, "include")
    } else {
        Vec::new()
    };
    let nicenames: Vec<Vec<u8>> = strings(params, "slug")
        .into_iter()
        .map(String::into_bytes)
        .collect();
    let order = match params.text("orderby").as_slice() {
        b"id" => UserOrder::Id,
        b"include" if !include.is_empty() => UserOrder::Include,
        b"include" => UserOrder::Login,
        b"registered_date" => UserOrder::Registered,
        b"slug" => UserOrder::Nicename,
        b"include_slugs" => UserOrder::Nicenames,
        b"email" => UserOrder::Email,
        b"url" => UserOrder::Url,
        _ => UserOrder::DisplayName,
    };
    UserQuery {
        caps_key: format!("{prefix}capabilities"),
        // `who=authors` leaves the roles out, as it leaves the blog.
        roles: if authors {
            Vec::new()
        } else {
            roles(context, params)
        },
        level_key: authors.then(|| format!("{prefix}user_level")),
        published,
        nicenames,
        search: search(context, params),
        exclude: if include.is_empty() && set(params, "exclude") {
            ids(params, "exclude")
        } else {
            Vec::new()
        },
        include,
        // `FIELD()` of the slugs is written without a direction.
        order: (
            order,
            params.text("order") == b"desc" && order != UserOrder::Nicenames,
        ),
        ..UserQuery::default()
    }
}

/// `get_items()`.
fn items(context: &Context, params: &Params) -> Result<Response, RestError> {
    let int = |name| params.get(name).map_or(0, Value::to_int);
    let per_page = int("per_page").unsigned_abs().max(1);
    let offset = match params.get("offset") {
        Some(offset) if !offset.is_empty() => offset.to_int().unsigned_abs(),
        _ => crate::request::page_offset(int("page").unsigned_abs(), per_page),
    };
    let query = prepared(context, params);
    let hidden = query.published.is_some() && authors_hidden(context);
    let total = if hidden {
        0
    } else {
        context.site.count_users(&query)
    };
    let mut response = if context.head {
        Response::new(Value::Array(Vec::new()), 200)
    } else {
        let users = if hidden {
            Vec::new()
        } else {
            context.site.users(&UserQuery {
                limit: Some(per_page),
                offset,
                ..query
            })
        };
        let edit = params.context() == b"edit";
        let data = users
            .iter()
            .filter(|user| !edit || context.can_for("edit_user", user.id))
            .map(|user| prepare(context, params, user).into_data())
            .collect();
        Response::new(list(data), 200)
    };
    let page = offset / per_page + 1 + u64::from(offset % per_page != 0);
    paginate(
        &mut response,
        (total, total.div_ceil(per_page), page),
        &params.get,
        &context.rest_url(ROUTE),
    );
    Ok(response)
}

/// `get_user()`: the user of the route's ID.
fn get_user(context: &Context, params: &Params) -> Result<User, RestError> {
    // The route's ID is digits; no user has the ID 0.
    let id = params.get("id").map_or(0, Value::to_int);
    let found = context.site.user(id.unsigned_abs());
    found.ok_or_else(|| RestError::status("rest_user_invalid_id", "Invalid user ID.", 404))
}

/// `get_item_permissions_check()`.
fn item_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let user = get_user(context, params)?;
    if user.id == context.user {
        return Ok(());
    }
    let edits = context.can_for("edit_user", user.id);
    if params.context() == b"edit" && !edits {
        let message = "Sorry, you are not allowed to edit this user.";
        return Err(context.forbidden("rest_forbidden_context", message));
    }
    if !edits && !context.can("list_users") && !has_posts(context, user.id) {
        let message = "Sorry, you are not allowed to list users.";
        return Err(context.forbidden("rest_user_cannot_view", message));
    }
    Ok(())
}

/// `count_user_posts( $id, $types )`, of the post types shown in REST: a
/// type's private posts count where the current user may read them.
fn has_posts(context: &Context, author: u64) -> bool {
    if authors_hidden(context) {
        return false;
    }
    let types: Vec<(String, bool)> = shown_in_rest()
        .map(|(name, fields)| {
            let cap = fields.get_path(&["capabilities", "read_private_posts"]);
            let cap = String::from_utf8_lossy(&cap.map(Value::to_php_string).unwrap_or_default())
                .into_owned();
            let name = String::from_utf8_lossy(&name.to_php_string()).into_owned();
            (name, context.can(&cap) || author == context.user)
        })
        .collect();
    context.site.has_posts(author, &types)
}

/// `get_item()`.
fn item(context: &Context, params: &Params) -> Result<Response, RestError> {
    get_user(context, params).map(|user| prepare(context, params, &user))
}

/// `get_current_item()`.
fn me(context: &Context, params: &Params) -> Result<Response, RestError> {
    let user = (context.user != 0)
        .then(|| context.site.user(context.user))
        .flatten();
    let message = "You are not currently logged in.";
    let user = user.ok_or_else(|| RestError::status("rest_not_logged_in", message, 401))?;
    Ok(prepare(context, params, &user))
}

/// `create_item_permissions_check()`.
fn create_permission(context: &Context, _: &Params) -> Result<(), RestError> {
    if context.can("create_users") {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to create new users.";
    Err(context.forbidden("rest_cannot_create_user", message))
}

/// `update_item_permissions_check()`: a request with roles to set, beside
/// its context's default, asks for `edit_user` too.
fn update_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let user = get_user(context, params)?;
    if set(params, "roles") && !context.can_for("promote_user", user.id) {
        let message = "Sorry, you are not allowed to edit roles of this user.";
        return Err(context.forbidden("rest_cannot_edit_roles", message));
    }
    if context.can_for("edit_user", user.id) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to edit this user.";
    Err(context.forbidden("rest_cannot_edit", message))
}

/// `delete_item_permissions_check()`.
fn delete_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let user = get_user(context, params)?;
    if context.can_for("delete_user", user.id) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to delete this user.";
    Err(context.forbidden("rest_user_cannot_delete", message))
}

/// A user meta value, unserialized, `''` when not stored.
fn meta_value(context: &Context, user: u64, key: &str) -> Value {
    let stored = context.site.user_meta(user, key);
    stored
        .as_deref()
        .map_or_else(|| text(""), crate::unserialized)
}

/// An array as `(object)` casts it.
fn as_object(value: Value) -> Value {
    let properties = value.as_array().unwrap_or_default().to_vec();
    if properties.is_empty() {
        return Value::Object {
            class: b"stdClass".to_vec(),
            properties,
        };
    }
    Value::Array(properties)
}

/// `WP_REST_Meta_Fields::get_value()`: the persisted preferences, in the
/// edit context only.
fn meta(context: &Context, params: &Params, user: u64) -> Value {
    if params.context() != b"edit" {
        return Value::Array(Vec::new());
    }
    let key = format!("{}persisted_preferences", context.site.table_prefix());
    let stored = context.site.user_meta(user, &key);
    let value = stored
        .as_deref()
        .map_or_else(|| Value::Array(Vec::new()), crate::unserialized);
    let schema = hyscribe_phpser::json_decode(PREFERENCES.as_bytes()).unwrap_or_default();
    object([("persisted_preferences", prepare_value(&value, &schema, ""))])
}

/// `prepare_item_for_response()`.
fn prepare(context: &Context, params: &Params, user: &User) -> Response {
    if context.head {
        return Response::new(Value::Array(Vec::new()), 200);
    }
    let properties = properties(context);
    let wanted = fields_for_response(params, &properties);
    let prefix = context.site.table_prefix();
    let caps = || meta_value(context, user.id, &format!("{prefix}capabilities"));
    let roles = || {
        let stored = context
            .option(&format!("{prefix}user_roles"))
            .unwrap_or_default();
        let defined = crate::unserialized(&stored);
        let caps = caps();
        let names = caps
            .as_array()
            .unwrap_or_default()
            .iter()
            .filter(|(role, _)| {
                defined
                    .get(role)
                    .is_some_and(|role| role.get_str("name").is_some())
            });
        list(names.map(|(role, _)| text(role.to_php_string())).collect())
    };
    let locale = || {
        let locale = meta_value(context, user.id, "locale");
        if locale.is_empty() {
            let site = context
                .option("WPLANG")
                .filter(|locale| !Value::String(locale.clone()).is_empty());
            text(site.unwrap_or_else(|| b"en_US".to_vec()))
        } else {
            locale
        }
    };
    let registered = || {
        let date = context.site.user_registered(user.id);
        let unix = strtotime(&date).unwrap_or_default();
        text(format_unix("c", unix, Zone::Utc))
    };
    let avatars = || avatar_urls(context, &user.email, &user_name(user), true);
    let may_see_roles = || context.can("list_users") || context.can_for("edit_user", user.id);
    let fields: [(&str, &dyn Fn() -> Value); 18] = [
        ("id", &|| int(user.id)),
        ("username", &|| text(user.login.clone())),
        ("name", &|| text(user.display_name.clone())),
        ("first_name", &|| text(user.first_name.clone())),
        ("last_name", &|| text(user.last_name.clone())),
        ("email", &|| text(user.email.clone())),
        ("url", &|| text(user.url.clone())),
        ("description", &|| text(user.description.clone())),
        ("link", &|| text(context.site.author_link(user))),
        ("locale", &locale),
        ("nickname", &|| meta_value(context, user.id, "nickname")),
        ("slug", &|| text(user.nicename.clone())),
        ("roles", &roles),
        ("registered_date", &registered),
        ("capabilities", &|| {
            as_object(Value::Array(context.site.allcaps(user.id)))
        }),
        ("extra_capabilities", &|| as_object(caps())),
        ("avatar_urls", &avatars),
        ("meta", &|| meta(context, params, user.id)),
    ];
    let shown = |field: &&Vec<u8>| *field != b"roles" || may_see_roles();
    let kept: Vec<Vec<u8>> = wanted.iter().filter(shown).cloned().collect();
    let mut response = item_response(&fields, &kept, &properties, &params.context());
    if links_wanted(&wanted) {
        crate::self_link(context, &mut response, &format!("{ROUTE}/{}", user.id));
        response.add_link("collection", context.rest_url(ROUTE));
    }
    response
}
