// SPDX-License-Identifier: AGPL-3.0-or-later

//! The navigation post `WP_Navigation_Fallback::create_classic_menu_fallback()`
//! makes: the classic menu it picks (the one at the `primary` location, the
//! one slugged `primary`, else the newest) and its custom links as
//! `WP_Classic_To_Block_Menu_Converter` converts them. A menu with an item
//! of a post, a term or an archive, or a link to the page asked for, is not
//! ported: it makes no fallback, and the page list stands in.

use hyscribe_kses::url_scheme;
use hyscribe_model::{OrderBy, Post, PostQuery, Term, TermFilter};
use hyscribe_phpser::{
    Key, Maybe, Value, is_empty_string, maybe_unserialize, php_trim, stripslashes, urldecode,
};

use crate::blocks::{Block, MAX_BLOCK_DEPTH, serialize_blocks};
use crate::excerpt::wp_trim_words;
use crate::formatting::untrailingslashit;
use crate::permalinks::home_path;
use crate::query_loop::query_limit;
use crate::site::{Request, Site};

/// What `wp_insert_post()` is given: the menu's name and slug and its
/// blocks, as it unslashes them.
pub(super) struct Classic {
    pub(super) title: Vec<u8>,
    pub(super) name: Vec<u8>,
    pub(super) content: Vec<u8>,
}

/// A menu item: its ID, its parent as stored, and the attributes of its
/// block.
struct Item {
    id: u64,
    parent: Vec<u8>,
    attrs: Vec<(Key, Value)>,
}

/// `get_fallback_classic_menu()` and the conversion of the menu it finds;
/// `None` for no menu, an empty one, or one not ported.
pub(super) fn classic_menu(site: &dyn Site, request: &Request) -> Option<Classic> {
    let menu = fallback_menu(site)?;
    let content = convert(site, request, &menu)?;
    Some(Classic {
        title: stripslashes(&menu.name),
        name: stripslashes(&menu.slug),
        content: stripslashes(&content),
    })
}

/// `get_fallback_classic_menu()`.
fn fallback_menu(site: &dyn Site) -> Option<Term> {
    let menus = site.taxonomy_terms("nav_menu");
    let at_location = primary_location(site, &menus);
    let slugged = || menus.iter().find(|menu| menu.slug == b"primary");
    // The most recently created: the highest ID.
    let newest = || menus.iter().max_by_key(|menu| menu.id);
    at_location.or_else(|| slugged().or_else(newest).cloned())
}

/// `get_nav_menu_at_primary_location()`: `wp_get_nav_menu_object()` of the
/// menu the theme's `nav_menu_locations` assigns to `primary`, by ID, slug
/// or name.
fn primary_location(site: &dyn Site, menus: &[Term]) -> Option<Term> {
    let mods = theme_mods(site)?;
    let assigned = mods.get_str("nav_menu_locations")?.get_str("primary")?;
    if assigned.is_empty() {
        return None;
    }
    let name = assigned.to_php_string();
    let id = u64::try_from(assigned.to_int()).ok();
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
/// serialized; `None` for a menu without items or one not ported.
fn convert(site: &dyn Site, request: &Request, menu: &Term) -> Option<Vec<u8>> {
    // `wp_get_nav_menu_items()` finds no items of a menu that counts none.
    if menu.count == 0 {
        return None;
    }
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
    let items = site
        .query(&query)
        .iter()
        .map(|post| item(site, request, post))
        .collect::<Option<Vec<Item>>>()?;
    let content = to_blocks(&items, b"0", 0)?;
    (!content.is_empty()).then_some(content)
}

/// `get_post_meta( $id, $key, true )`, empty when unset.
fn meta(site: &dyn Site, post: &Post, key: &str) -> Vec<u8> {
    site.post_meta(post.id, key).unwrap_or_default()
}

/// `wp_setup_nav_menu_item()` of a custom link and the attributes
/// `to_blocks()` makes of it; `None` for any other kind of item, or one
/// that links to the page asked for.
fn item(site: &dyn Site, request: &Request, post: &Post) -> Option<Item> {
    let kind = meta(site, post, "_menu_item_type");
    if [&b"post_type"[..], b"post_type_archive", b"taxonomy"].contains(&&kind[..]) {
        return None;
    }
    let object = meta(site, post, "_menu_item_object");
    let url = meta(site, post, "_menu_item_url");
    let classes = context_classes(
        site,
        request,
        (&kind, &object, &url),
        stored_classes(site, post),
    )?;
    let hooks = site.hooks();
    let description = wp_trim_words(&post.content, 200, b"&hellip;");
    let xfn = meta(site, post, "_menu_item_xfn");
    let id = match &object[..] {
        b"custom" => Value::Null,
        _ => Value::String(meta(site, post, "_menu_item_object_id")),
    };
    let dashed = kind.iter().map(|b| if *b == b'_' { b'-' } else { *b });
    let attrs = [
        ("className", Value::String(classes.join(&b' '))),
        (
            "description",
            Value::String(hooks.nav_menu_description(description)),
        ),
        ("id", id),
        ("kind", Value::String(dashed.collect())),
        ("label", Value::String(post.title.clone())),
        (
            "opensInNewTab",
            Value::Bool(meta(site, post, "_menu_item_target") == b"_blank"),
        ),
        (
            "rel",
            if xfn.is_empty() {
                Value::Null
            } else {
                Value::String(xfn)
            },
        ),
        (
            "title",
            Value::String(hooks.nav_menu_attr_title(post.excerpt.clone())),
        ),
        ("type", Value::String(object)),
        ("url", Value::String(url)),
    ];
    Some(Item {
        id: post.id,
        parent: meta(site, post, "_menu_item_menu_item_parent"),
        attrs: attrs
            .into_iter()
            .map(|(name, value)| (Key::String(name.into()), value))
            .collect(),
    })
}

/// `(array) get_post_meta( $id, '_menu_item_classes', true )`.
fn stored_classes(site: &dyn Site, post: &Post) -> Vec<Vec<u8>> {
    let stored = meta(site, post, "_menu_item_classes");
    match maybe_unserialize(&stored) {
        Maybe::Value(Value::Array(items)) => items
            .iter()
            .map(|(_, value)| value.to_php_string())
            .collect(),
        Maybe::Value(value) => vec![value.to_php_string()],
        Maybe::Plain(_) | Maybe::Invalid(_) => vec![stored],
    }
}

/// `_wp_menu_item_classes_by_context()` for a custom link: the classes
/// added to the stored ones, `array_unique()`d. `None` when the link is
/// the page asked for, which adds the current item and its ancestors.
fn context_classes(
    site: &dyn Site,
    request: &Request,
    (kind, object, url): (&[u8], &[u8], &[u8]),
    mut classes: Vec<Vec<u8>>,
) -> Option<Vec<Vec<u8>>> {
    classes.push(b"menu-item".to_vec());
    classes.push([b"menu-item-type-", kind].concat());
    classes.push([b"menu-item-object-", object].concat());
    if object == b"custom" {
        let home = site.home_url();
        let origin = home.strip_suffix(home_path(&String::from_utf8_lossy(&home)).as_bytes());
        let origin = origin.unwrap_or(&home);
        let scheme = url_scheme(&home).unwrap_or_default();
        let root_relative = untrailingslashit(&request.uri);
        let current_url = [origin, root_relative].concat();
        let hash = url.iter().position(|b| *b == b'#').filter(|at| *at > 0);
        let raw = hash.map_or(url, |at| url.split_at(at).0);
        let item_url = with_scheme(untrailingslashit(raw), &scheme);
        let indexless = untrailingslashit(
            current_url
                .strip_suffix(b"index.php")
                .unwrap_or(&current_url),
        );
        let matches = [current_url.as_slice(), indexless, root_relative];
        let decoded = matches.map(urldecode);
        let current = matches.contains(&item_url.as_slice()) || decoded.contains(&item_url);
        if !is_empty_string(raw) && current {
            return None;
        }
        if item_url == home && request.front_page {
            classes.push(b"current-menu-item".to_vec());
        }
        if untrailingslashit(&item_url) == home {
            classes.push(b"menu-item-home".to_vec());
        }
    }
    let mut unique: Vec<Vec<u8>> = Vec::new();
    for class in classes {
        if !unique.contains(&class) {
            unique.push(class);
        }
    }
    Some(unique)
}

/// `set_url_scheme( $url )`: `url` with the scheme of the home URL, which
/// a request is served under.
fn with_scheme(url: &[u8], scheme: &[u8]) -> Vec<u8> {
    let url = php_trim(url);
    let url = url
        .strip_prefix(b"//")
        .map_or(url.to_vec(), |rest| [b"http://", rest].concat());
    let named = url.windows(3).position(|w| w == b"://").filter(|at| {
        let (name, _) = url.split_at(*at);
        !name.is_empty() && name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    });
    match named {
        Some(at) => [scheme, url.split_at(at).1].concat(),
        None => url,
    }
}

/// `to_blocks()` of the items under `parent`, serialized; `None` below the
/// depth blocks nest to.
fn to_blocks(items: &[Item], parent: &[u8], depth: usize) -> Option<Vec<u8>> {
    if depth >= MAX_BLOCK_DEPTH {
        return None;
    }
    let mut out = Vec::new();
    for item in items.iter().filter(|item| item.parent == parent) {
        let children = to_blocks(items, item.id.to_string().as_bytes(), depth + 1)?;
        let name = match children.is_empty() {
            true => "core/navigation-link",
            false => "core/navigation-submenu",
        };
        let block = Block {
            name: Some(name.into()),
            attrs: Some(item.attrs.clone()),
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

#[cfg(test)]
mod tests;
