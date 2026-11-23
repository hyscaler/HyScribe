// SPDX-License-Identifier: AGPL-3.0-or-later

//! The navigation post `WP_Navigation_Fallback::create_classic_menu_fallback()`
//! makes: the classic menu it picks (the one at the `primary` location, the
//! one slugged `primary`, else the newest) and its items as
//! `WP_Classic_To_Block_Menu_Converter` converts them. The menu items are
//! set up as `wp_get_nav_menu_items()` does (`item`) and given the classes
//! of the page asked for (`context`).

use hyscribe_model::{OrderBy, PostQuery, Term, TermFilter};
use hyscribe_phpser::{Key, Maybe, Value, maybe_unserialize, stripslashes};

use crate::blocks::{Block, MAX_BLOCK_DEPTH, serialize_blocks};
use crate::query_loop::query_limit;
use crate::site::{NavMenuItem, Request, Site};

mod context;
mod item;
mod nav_menu;

pub use nav_menu::{NavMenu, NavMenuArgs, Printed, wp_nav_menu};

use item::Skip;

/// What `wp_insert_post()` is given: the menu's name and slug and its
/// blocks, as it unslashes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Classic {
    /// `post_title`.
    pub title: Vec<u8>,
    /// `post_name`.
    pub name: Vec<u8>,
    /// `post_content`.
    pub content: Vec<u8>,
}

/// What `WP_Navigation_Fallback::create_classic_menu_fallback()` makes.
pub(super) enum Conversion {
    /// The menu's blocks, to be written as a post.
    Made(Classic),
    /// The `WP_Error` it gives: no menu, an empty one, or one that does
    /// not convert, after which the default fallback is made.
    Error,
    /// A menu not ported, which makes no post at all: one of an item kind
    /// not ported, or nested deeper than blocks go.
    NotPorted,
}

/// A menu that is not ported.
pub(crate) struct NotPorted;

/// `get_fallback_classic_menu()` and the conversion of the menu it finds.
pub(super) fn classic_menu(site: &dyn Site, request: &Request) -> Conversion {
    let Some(menu) = fallback_menu(site) else {
        return Conversion::Error;
    };
    match convert(site, request, &menu) {
        Ok(Some(content)) => Conversion::Made(Classic {
            title: stripslashes(&menu.name),
            name: stripslashes(&menu.slug),
            content: stripslashes(&content),
        }),
        Ok(None) => Conversion::Error,
        Err(NotPorted) => Conversion::NotPorted,
    }
}

/// `get_fallback_classic_menu()`.
fn fallback_menu(site: &dyn Site) -> Option<Term> {
    let all = site.taxonomy_terms("nav_menu");
    let args = [
        ("taxonomy".to_string(), Value::String(b"nav_menu".to_vec())),
        ("hide_empty".to_string(), Value::Bool(false)),
        ("orderby".to_string(), Value::String(b"name".to_vec())),
    ];
    let menus = site.hooks().wp_get_nav_menus(all.clone(), &args);
    if menus.is_empty() {
        return None;
    }
    let slugged = || menus.iter().find(|menu| menu.slug == b"primary");
    // The most recently created: the highest ID.
    let newest = || menus.iter().max_by_key(|menu| menu.id);
    primary_location(site, &all).or_else(|| slugged().or_else(newest).cloned())
}

/// `get_nav_menu_at_primary_location()`.
fn primary_location(site: &dyn Site, menus: &[Term]) -> Option<Term> {
    located_menu(site, menus, "primary")
}

/// `wp_get_nav_menu_object()` of the menu the theme's `nav_menu_locations`
/// assigns to `location`.
pub(crate) fn located_menu(site: &dyn Site, menus: &[Term], location: &str) -> Option<Term> {
    let mods = theme_mods(site)?;
    let locations = mods.get_str("nav_menu_locations")?;
    menu_object(menus, locations.get_str(location)?)
}

/// `wp_get_nav_menu_object( $menu )` among `menus`: by ID, slug or name.
pub(crate) fn menu_object(menus: &[Term], menu: &Value) -> Option<Term> {
    if menu.is_empty() {
        return None;
    }
    let name = menu.to_php_string();
    let id = u64::try_from(menu.to_int()).ok();
    let find = |matches: &dyn Fn(&Term) -> bool| menus.iter().find(|menu| matches(menu)).cloned();
    find(&|menu| Some(menu.id) == id)
        .or_else(|| find(&|menu| menu.slug == name))
        .or_else(|| find(&|menu| menu.name == name))
}

/// `get_theme_mods()`: the current theme's option, or the deprecated one
/// named for the theme.
fn theme_mods(site: &dyn Site) -> Option<Value> {
    let option = |name: &[u8]| site.option(&String::from_utf8_lossy(name));
    let stylesheet = site.option("stylesheet").unwrap_or_default();
    let mods = option(&[b"theme_mods_", &stylesheet[..]].concat()).or_else(|| {
        let theme = site.option("current_theme")?;
        option(&[b"mods_", &theme[..]].concat())
    })?;
    match maybe_unserialize(&mods) {
        Maybe::Value(value) => Some(value),
        _ => None,
    }
}

/// `WP_Classic_To_Block_Menu_Converter::convert()` of `menu`: its blocks,
/// serialized; `None` for a menu without items, `Err` for one not ported.
fn convert(site: &dyn Site, request: &Request, menu: &Term) -> Result<Option<Vec<u8>>, NotPorted> {
    let Some(mut items) = menu_items(site, menu)? else {
        return Ok(None);
    };
    context::apply(site, request, &mut items);
    let content = to_blocks(&items, b"0", 0).ok_or(NotPorted)?;
    Ok((!content.is_empty()).then_some(content))
}

/// `wp_get_nav_menu_items( $menu->term_id )` on the front end: the valid
/// items in menu order, filtered; `None` for a menu that is gone or counts
/// no items.
pub(crate) fn menu_items(
    site: &dyn Site,
    menu: &Term,
) -> Result<Option<Vec<NavMenuItem>>, NotPorted> {
    // The menu is fetched again by its ID, with the count the database has.
    let Some(menu) = site.term(menu.id, "nav_menu").filter(|menu| menu.count > 0) else {
        return Ok(None);
    };
    let query = PostQuery {
        post_types: vec!["nav_menu_item".into()],
        terms: vec![TermFilter {
            taxonomy: "nav_menu".into(),
            terms: vec![menu.id],
            exclude: false,
        }],
        order: vec![(OrderBy::MenuOrder, true)],
        limit: query_limit(-1),
        ..PostQuery::default()
    };
    let mut items = Vec::new();
    for post in site.query(&query) {
        match item::setup(site, &post) {
            Ok(item) => items.push(item),
            Err(Skip::Invalid) => {}
            Err(Skip::NotPorted) => return Err(NotPorted),
        }
    }
    let text = |name: &str| Value::String(name.as_bytes().to_vec());
    let args = [
        ("order", text("ASC")),
        ("orderby", text("menu_order")),
        ("post_type", text("nav_menu_item")),
        ("post_status", text("publish")),
        ("output", text("ARRAY_A")),
        ("output_key", text("menu_order")),
        ("nopaging", Value::Bool(true)),
        ("update_menu_item_cache", Value::Bool(true)),
        ("update_post_term_cache", Value::Bool(false)),
    ]
    .map(|(name, value)| (name.to_string(), value));
    Ok(Some(
        site.hooks().wp_get_nav_menu_items(items, &menu, &args),
    ))
}

/// `to_blocks()` of the items under `parent`, serialized; `None` below the
/// depth blocks nest to.
fn to_blocks(items: &[NavMenuItem], parent: &[u8], depth: usize) -> Option<Vec<u8>> {
    if depth >= MAX_BLOCK_DEPTH {
        return None;
    }
    let mut out = Vec::new();
    for item in items.iter().filter(|item| item.menu_item_parent == parent) {
        let children = to_blocks(items, item.id.to_string().as_bytes(), depth + 1)?;
        let name = match children.is_empty() {
            true => "core/navigation-link",
            false => "core/navigation-submenu",
        };
        let block = Block {
            name: Some(name.into()),
            attrs: Some(attributes(item)),
            inner_blocks: Vec::new(),
            inner_html: Vec::new(),
            inner_content: if children.is_empty() {
                Vec::new()
            } else {
                vec![Some(children)]
            },
        };
        out.extend(serialize_blocks(&[block]));
    }
    Some(out)
}

/// The attributes `to_blocks()` makes of an item.
fn attributes(item: &NavMenuItem) -> Vec<(Key, Value)> {
    let text = |value: &[u8]| Value::String(value.to_vec());
    let optional = |value: &[u8]| {
        if value.is_empty() {
            Value::Null
        } else {
            text(value)
        }
    };
    let kind: Vec<u8> = item
        .kind
        .iter()
        .map(|b| if *b == b'_' { b'-' } else { *b })
        .collect();
    let attrs = [
        // The classes the context gave are never none.
        ("className", text(&item.classes.join(&b' '))),
        ("description", text(&item.description)),
        (
            "id",
            if item.object == b"custom" {
                Value::Null
            } else {
                text(&item.object_id)
            },
        ),
        ("kind", text(&kind)),
        ("label", text(&item.title)),
        ("opensInNewTab", Value::Bool(item.target == b"_blank")),
        ("rel", optional(&item.xfn)),
        ("title", text(&item.attr_title)),
        ("type", text(&item.object)),
        ("url", item.url.as_deref().map_or(Value::Bool(false), text)),
    ];
    attrs
        .into_iter()
        .map(|(name, value)| (Key::String(name.into()), value))
        .collect()
}

#[cfg(test)]
mod tests;
