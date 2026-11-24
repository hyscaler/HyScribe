// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Menu_Items_Controller::prepare_item_for_response()`: a classic
//! menu's item as `wp_setup_nav_menu_item()` sets it up.

use hyscribe_kses::strip_tags;
use hyscribe_model::{Post, TermQuery};
use hyscribe_phpser::{Key, Value, absint};
use hyscribe_render::formatting::{CORE_SHORTCODE_TAGS, wptexturize};
use hyscribe_render::post::{admin_title, sanitize_html_class};
use hyscribe_render::wp_trim_words;

use super::prepare::{actions, links, properties};
use super::{Kind, meta};
use crate::Context;
use crate::fields::{Properties, fields_for_response, item_response, links_wanted};
use crate::request::Params;
use crate::response::{Response, RestError, int, list, text};

const ALL: &[&str] = &["view", "edit", "embed"];

/// The schema's properties and their contexts.
pub(super) const PROPERTIES: &Properties = &[
    ("title", ALL),
    ("id", ALL),
    ("type_label", ALL),
    ("type", ALL),
    ("status", ALL),
    ("parent", ALL),
    ("attr_title", ALL),
    ("classes", ALL),
    ("description", ALL),
    ("menu_order", ALL),
    ("object", ALL),
    ("object_id", ALL),
    ("target", ALL),
    ("url", ALL),
    ("xfn", ALL),
    ("invalid", ALL),
    ("menus", &["view", "edit"]),
    ("meta", &["view", "edit"]),
];

const TITLE: &Properties = &[("raw", &["edit"]), ("rendered", ALL)];

/// A menu item set up: what it links to and how it shows.
#[derive(Default)]
struct Item {
    kind: Vec<u8>,
    object: Vec<u8>,
    object_id: Vec<u8>,
    type_label: Vec<u8>,
    title: Vec<u8>,
    url: Vec<u8>,
    invalid: bool,
}

/// The `the_title` filters.
fn the_title(title: &[u8]) -> Vec<u8> {
    admin_title(&Post {
        title: title.to_vec(),
        ..Post::default()
    })
}

/// A label of a registered type's or taxonomy's.
fn label(fields: &Value) -> Vec<u8> {
    let found = fields.get_path(&["labels", "singular_name"]);
    found.map(Value::to_php_string).unwrap_or_default()
}

/// A post meta value as text, `''` when not stored.
fn stored(context: &Context, post: &Post, key: &str) -> Vec<u8> {
    context.site.post_meta(post.id, key).unwrap_or_default()
}

/// `wp_setup_nav_menu_item()` of a `nav_menu_item` post, without the post
/// states only the admin loads. An archive's item, or one of a type or
/// taxonomy registered but not shown in REST, is not ported.
fn setup(context: &Context, post: &Post) -> Result<Item, RestError> {
    let mut item = Item {
        kind: stored(context, post, "_menu_item_type"),
        object: stored(context, post, "_menu_item_object"),
        object_id: stored(context, post, "_menu_item_object_id"),
        ..Item::default()
    };
    let object = String::from_utf8_lossy(&item.object).into_owned();
    let id = absint(&Value::String(item.object_id.clone()));
    let no_title = format!(
        "#{} (no title)",
        Value::String(item.object_id.clone()).to_int()
    );
    let original = match item.kind.as_slice() {
        b"post_type" => {
            match crate::types::post_type(&object) {
                Some(fields) => item.type_label = label(fields),
                None if crate::types::registered(&object) => return Err(RestError::not_ported()),
                None => (item.type_label, item.invalid) = (item.object.clone(), true),
            }
            let found = context.site.post(id);
            item.invalid |= found.as_ref().is_none_or(|found| found.status == "trash");
            found.map(|found| {
                item.url = crate::posts::link(context, &found);
                the_title(&found.title)
            })
        }
        b"taxonomy" => {
            match crate::taxonomies::shown(&object) {
                Some(fields) => item.type_label = label(fields),
                None if crate::taxonomies::registered(&object) => {
                    return Err(RestError::not_ported());
                }
                None => (item.type_label, item.invalid) = (item.object.clone(), true),
            }
            let query = TermQuery {
                taxonomy: object.clone(),
                include: vec![id],
                ..TermQuery::default()
            };
            let found = context.site.terms(&query).into_iter().next();
            item.invalid |= found.is_none();
            found.map(|term| {
                item.url = context.site.term_link(&object, &term).into_bytes();
                term.name
            })
        }
        b"post_type_archive" => return Err(RestError::not_ported()),
        _ => {
            item.type_label = b"Custom Link".to_vec();
            item.url = stored(context, post, "_menu_item_url");
            Some(post.title.clone())
        }
    };
    let original = original.filter(|title| !title.is_empty());
    item.title = if post.title.is_empty() {
        original.unwrap_or_else(|| no_title.into_bytes())
    } else {
        post.title.clone()
    };
    Ok(item)
}

/// `prepare_item_for_response()`.
pub(super) fn prepare(
    context: &Context,
    params: &Params,
    post: &Post,
) -> Result<Response, RestError> {
    let item = setup(context, post)?;
    // `get_the_terms()` is false for an item in no menu, which leaves the
    // field out.
    let menus = context.site.post_terms(post.id, "nav_menu");
    let mut wanted = fields_for_response(params, properties(Kind::MenuItem));
    wanted.retain(|field| !menus.is_empty() || field != b"menus");
    let shown = params.context();
    let title = || {
        let parts = [
            ("raw", text(item.title.clone())),
            ("rendered", text(the_title(&item.title))),
        ];
        let parts = parts.into_iter().filter(|(name, _)| {
            let field = format!("title.{name}");
            crate::fields::is_field_included(&field, &wanted)
        });
        let parts = parts.map(|(name, value)| (Key::String(name.as_bytes().to_vec()), value));
        let parts = crate::fields::filter_by_context(parts.collect(), TITLE, &shown);
        Value::Array(parts)
    };
    let filtered = |text: &[u8]| strip_tags(&wptexturize(text, &CORE_SHORTCODE_TAGS));
    let classes = || match meta(context, post.id, "_menu_item_classes") {
        Some(Value::Array(classes)) => Value::Array(classes),
        found => list(vec![found.unwrap_or_else(|| text(""))]),
    };
    let xfn = || {
        let xfn = stored(context, post, "_menu_item_xfn");
        let classes = xfn.split(|&b| b == b' ');
        list(
            classes
                .map(|class| text(sanitize_html_class(class, b"")))
                .collect(),
        )
    };
    let menu = || menus.first().map_or(Value::Int(0), |menu| int(menu.id));
    let parent = || Value::String(stored(context, post, "_menu_item_menu_item_parent")).to_int();
    let fields: [(&str, &dyn Fn() -> Value); 18] = [
        ("id", &|| int(post.id)),
        ("title", &title),
        ("status", &|| text(post.status.clone())),
        ("url", &|| text(item.url.clone())),
        ("attr_title", &|| text(filtered(&post.excerpt))),
        ("description", &|| {
            text(filtered(&wp_trim_words(&post.content, 200, b"&hellip;")))
        }),
        ("type", &|| text(item.kind.clone())),
        ("type_label", &|| text(item.type_label.clone())),
        ("object", &|| text(item.object.clone())),
        ("object_id", &|| {
            int(absint(&Value::String(item.object_id.clone())))
        }),
        ("parent", &|| Value::Int(parent())),
        ("menu_order", &|| Value::Int(post.menu_order)),
        ("target", &|| {
            text(stored(context, post, "_menu_item_target"))
        }),
        ("classes", &classes),
        ("xfn", &xfn),
        ("invalid", &|| Value::Bool(item.invalid)),
        ("meta", &|| Value::Array(Vec::new())),
        ("menus", &menu),
    ];
    let mut response = item_response(&fields, &wanted, PROPERTIES, &shown);
    if links_wanted(&wanted) {
        links(context, Kind::MenuItem, post, &mut response);
        object_link(context, &item, &mut response);
        actions(context, params, Kind::MenuItem, post, &mut response);
    }
    Ok(response)
}

/// The `https://api.w.org/menu-item-object` link to the post or term the
/// item links to, if shown in REST.
fn object_link(context: &Context, item: &Item, response: &mut Response) {
    let id = absint(&Value::String(item.object_id.clone()));
    let route = match item.kind.as_slice() {
        b"post_type" => context.site.post(id).and_then(|post| {
            let route = crate::types::items_route(&post.post_type)?;
            Some((route, post.post_type))
        }),
        // `get_term()` of the ID alone, in whichever taxonomy has it.
        b"taxonomy" => {
            let names = crate::taxonomies::taxonomies().iter();
            let mut names =
                names.map(|(name, _)| String::from_utf8_lossy(&name.to_php_string()).into_owned());
            let has = |name: &String| {
                let query = TermQuery {
                    taxonomy: name.clone(),
                    include: vec![id],
                    ..TermQuery::default()
                };
                !context.site.terms(&query).is_empty()
            };
            names
                .find(has)
                .filter(|name| crate::taxonomies::shown(name).is_some())
                .map(|name| (crate::terms::items_route_of(&name), name))
        }
        _ => None,
    };
    let Some((route, ty)) = route.filter(|_| id != 0) else {
        return;
    };
    let kind = String::from_utf8_lossy(&item.kind).into_owned();
    let attributes = vec![(kind.as_str(), text(ty)), ("embeddable", Value::Bool(true))];
    let href = context.rest_url(&format!("{route}/{id}"));
    response.add_link_with("https://api.w.org/menu-item-object", href, attributes);
}
