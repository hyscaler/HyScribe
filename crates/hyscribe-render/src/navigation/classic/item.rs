// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_setup_nav_menu_item()` of a `nav_menu_item` post, as the front end
//! runs it (without the post states only the admin loads): what the item
//! links to and how it is titled. An item whose object is gone is invalid,
//! which `wp_get_nav_menu_items()` leaves out.

use std::sync::OnceLock;

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Maybe, Value, json_decode, maybe_unserialize};

use crate::excerpt::wp_trim_words;
use crate::permalinks::{permalink, term_link};
use crate::post::title_filters;
use crate::site::{NavMenuItem, Site};

/// Every registered post type by name: its fields when shown in REST,
/// else `false`; and the same for taxonomies. Both are exported by
/// `reference/wordpress/export.sh`.
const POST_TYPES: &[u8] =
    include_bytes!("../../../../../wordpress/wp-includes/rest-post-types.json");
const TAXONOMIES: &[u8] =
    include_bytes!("../../../../../wordpress/wp-includes/rest-taxonomies.json");

/// The registry entry of `name` in the JSON `data`, decoded once into `cell`.
fn registered<'a>(cell: &'a OnceLock<Value>, data: &[u8], name: &str) -> Option<&'a Value> {
    let all = cell.get_or_init(|| json_decode(data).unwrap_or(Value::Null));
    let entries: &[(Key, Value)] = all.as_array().unwrap_or_default();
    let found = entries
        .iter()
        .find(|(key, _)| key.to_php_string() == name.as_bytes());
    found.map(|(_, fields)| fields)
}

/// Why an item is left out.
pub(super) enum Skip {
    /// `wp_get_nav_menu_items()` drops it on the front end.
    Invalid,
    /// It is of a kind not ported.
    NotPorted,
}

/// What an item links to: the title of its object, and its URL, `None`
/// for the `false` of an archive without one.
type Target = (Vec<u8>, Option<Vec<u8>>);

/// `wp_setup_nav_menu_item()` of `post`.
pub(super) fn setup(site: &dyn Site, post: &Post) -> Result<NavMenuItem, Skip> {
    let meta = |key: &str| site.post_meta(post.id, key).unwrap_or_default();
    let kind = meta("_menu_item_type");
    let object = meta("_menu_item_object");
    let object_id = meta("_menu_item_object_id");
    let name = String::from_utf8_lossy(&object).into_owned();
    let id = Value::String(object_id.clone()).to_int();
    let own = post.title.clone();
    let linked = match &kind[..] {
        b"post_type" => post_type(site, &name, id),
        b"post_type_archive" => archive(site, &name),
        b"taxonomy" => taxonomy(site, &name, id),
        _ => Ok((Vec::new(), Some(meta("_menu_item_url")))),
    };
    let (original, url) = linked?;
    // A custom link has no object to take a title from.
    let title = if own.is_empty() { original } else { own };
    let hooks = site.hooks();
    let description = wp_trim_words(&post.content, 200, b"&hellip;");
    Ok(NavMenuItem {
        id: post.id,
        menu_item_parent: meta("_menu_item_menu_item_parent"),
        kind,
        object,
        object_id,
        post_parent: post.parent,
        title,
        url,
        target: meta("_menu_item_target"),
        attr_title: hooks.nav_menu_attr_title(post.excerpt.clone()),
        description: hooks.nav_menu_description(description),
        classes: stored_classes(site, post),
        xfn: meta("_menu_item_xfn"),
    })
}

/// `#%d (no title)` for an object without a title.
fn titled(title: Vec<u8>, id: i64) -> Vec<u8> {
    if title.is_empty() {
        return format!("#{id} (no title)").into_bytes();
    }
    title
}

/// An item of a post: invalid when its type is not registered, or its post
/// is gone or trashed.
fn post_type(site: &dyn Site, name: &str, id: i64) -> Result<Target, Skip> {
    static TYPES: OnceLock<Value> = OnceLock::new();
    registered(&TYPES, POST_TYPES, name).ok_or(Skip::Invalid)?;
    let found = u64::try_from(id).ok().and_then(|id| site.post(id));
    let found = found.filter(|post| post.status != "trash");
    let found = found.ok_or(Skip::Invalid)?;
    let title = titled(title_filters(&found.title), id);
    Ok((title, Some(permalink(site, &found))))
}

/// An item of a post type's archive: the posts page or the home page for
/// `post`; no other registered type has an archive.
fn archive(site: &dyn Site, name: &str) -> Result<Target, Skip> {
    static TYPES: OnceLock<Value> = OnceLock::new();
    let fields = registered(&TYPES, POST_TYPES, name).ok_or(Skip::Invalid)?;
    let label = fields.get_path(&["labels", "archives"]);
    let label = label.ok_or(Skip::NotPorted)?.to_php_string();
    // The others have no archive: `get_post_type_archive_link()` is `false`.
    if name != "post" {
        return Ok((label, None));
    }
    let option = |name: &str| Value::String(site.option(name).unwrap_or_default());
    let page_for_posts = option("page_for_posts");
    let url = if option("show_on_front").to_php_string() == b"page" && !page_for_posts.is_empty() {
        // `get_permalink()` of a page that is gone is `false`.
        let posts = u64::try_from(page_for_posts.to_int()).ok();
        posts
            .and_then(|id| site.post(id))
            .map(|page| permalink(site, &page))
    } else {
        Some(site.home_url())
    };
    Ok((label, url))
}

/// An item of a term: invalid when its taxonomy is not registered or its
/// term is gone; only the taxonomies visitors query have links ported.
fn taxonomy(site: &dyn Site, name: &str, id: i64) -> Result<Target, Skip> {
    static TAXONOMIES_REGISTRY: OnceLock<Value> = OnceLock::new();
    registered(&TAXONOMIES_REGISTRY, TAXONOMIES, name).ok_or(Skip::Invalid)?;
    if !["category", "post_tag", "post_format"].contains(&name) {
        return Err(Skip::NotPorted);
    }
    let term = u64::try_from(id).ok().and_then(|id| site.term(id, name));
    let term = term.ok_or(Skip::Invalid)?;
    let link = term_link(site, name, &term);
    Ok((titled(term.name, id), Some(link)))
}

/// `(array) get_post_meta( $id, '_menu_item_classes', true )`.
fn stored_classes(site: &dyn Site, post: &Post) -> Vec<Vec<u8>> {
    let stored = site
        .post_meta(post.id, "_menu_item_classes")
        .unwrap_or_default();
    match maybe_unserialize(&stored) {
        Maybe::Value(Value::Array(items)) => items
            .iter()
            .map(|(_, value)| value.to_php_string())
            .collect(),
        Maybe::Value(value) => vec![value.to_php_string()],
        Maybe::Plain(_) | Maybe::Invalid(_) => vec![stored],
    }
}
