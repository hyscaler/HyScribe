// SPDX-License-Identifier: AGPL-3.0-or-later

//! `_wp_menu_item_classes_by_context()`: the classes a menu's items get
//! from the page being asked for: which of them is the current one, and
//! which are its parents and ancestors. No registered post type has an
//! archive, so no item is the current post type archive.

use std::collections::HashMap;

use hyscribe_kses::url_scheme;
use hyscribe_model::{Post, Term};
use hyscribe_phpser::{Value, is_empty_string, php_trim, urldecode};

use crate::formatting::untrailingslashit;
use crate::navigation::post_ancestors;
use crate::permalinks::home_path;
use crate::site::{Archive, NavMenuItem, Request, Site};

/// `get_queried_object()`.
enum Queried {
    None,
    Post(Box<Post>),
    Term(&'static str, Term),
}

/// The ancestors `_wp_menu_item_classes_by_context()` finds of what is
/// asked for.
struct Ancestry {
    /// The queried post's ancestors, if its type is hierarchical.
    posts: Vec<u64>,
    /// `$possible_object_parents`: the categories of a queried post.
    object_parents: Vec<u64>,
    /// `$possible_taxonomy_ancestors`: those categories and their
    /// ancestors, or the ancestors of the category asked for.
    categories: Vec<u64>,
}

/// What the loop over the items collects for the loop over their parents.
#[derive(Default)]
struct Active {
    object: Vec<u8>,
    ancestor_items: Vec<u64>,
    parent_items: Vec<u64>,
    parent_objects: Vec<u64>,
}

struct Context<'a> {
    site: &'a dyn Site,
    request: &'a Request,
    queried: Queried,
    ancestry: Ancestry,
    page_for_posts: i64,
}

/// `(int)` of a stored value.
fn int(value: &[u8]) -> i64 {
    Value::String(value.to_vec()).to_int()
}

/// A non-negative ID, `0` for what no ID is.
fn id_of(value: i64) -> u64 {
    u64::try_from(value).unwrap_or_default()
}

/// `_wp_menu_item_classes_by_context( $menu_items )`.
pub(super) fn apply(site: &dyn Site, request: &Request, items: &mut [NavMenuItem]) {
    let queried = queried_object(site, request);
    let ancestry = ancestry(site, request, &queried);
    let option = |name: &str| int(&site.option(name).unwrap_or_default());
    let page_for_posts = option("page_for_posts");
    let context = Context {
        site,
        request,
        queried,
        ancestry,
        page_for_posts,
    };
    let (front_page, privacy_policy) = (
        option("page_on_front"),
        option("wp_page_for_privacy_policy"),
    );
    let mut active = Active::default();
    for item in items.iter_mut() {
        let mut classes = std::mem::take(&mut item.classes);
        classes.push(b"menu-item".to_vec());
        classes.push([b"menu-item-type-", &item.kind[..]].concat());
        classes.push([b"menu-item-object-", &item.object[..]].concat());
        let object_id = int(&item.object_id);
        let is_post_type = item.kind == b"post_type";
        if is_post_type && front_page == object_id {
            classes.push(b"menu-item-home".to_vec());
        }
        if is_post_type && privacy_policy == object_id {
            classes.push(b"menu-item-privacy-policy".to_vec());
        }
        context.item(item, &mut classes, &mut active);
        if page_for_posts != 0 && is_post_type && !context.is_page() && page_for_posts == object_id
        {
            classes.push(b"current_page_parent".to_vec());
        }
        item.classes = unique(classes);
    }
    for list in [
        &mut active.ancestor_items,
        &mut active.parent_items,
        &mut active.parent_objects,
    ] {
        list.retain(|id| *id != 0);
    }
    for item in items.iter_mut() {
        context.parent_classes(item, &active);
    }
}

/// The queried post, or the term of a term archive.
fn queried_object(site: &dyn Site, request: &Request) -> Queried {
    match &request.archive {
        Some(Archive::Term(taxonomy, Some(term))) => Queried::Term(taxonomy.name(), term.clone()),
        Some(_) => Queried::None,
        None => request
            .queried_object_id
            .ne(&0)
            .then(|| site.post(request.queried_object_id))
            .flatten()
            .map_or(Queried::None, |post| Queried::Post(Box::new(post))),
    }
}

/// Each class once, in the order first met: `array_unique()`.
fn unique(classes: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    let mut unique: Vec<Vec<u8>> = Vec::new();
    for class in classes {
        if !unique.contains(&class) {
            unique.push(class);
        }
    }
    unique
}

/// The terms of a queried post's `category` taxonomy (the one hierarchical
/// taxonomy of `post`) and their ancestors, or the ancestors of the
/// category asked for, and the posts above a queried page.
fn ancestry(site: &dyn Site, request: &Request, queried: &Queried) -> Ancestry {
    let mut ancestry = Ancestry {
        posts: Vec::new(),
        object_parents: Vec::new(),
        categories: Vec::new(),
    };
    let mut to_ancestor: HashMap<u64, u64> = site
        .taxonomy_terms("category")
        .iter()
        .filter(|term| term.parent != 0)
        .map(|term| (term.id, term.parent))
        .collect();
    // Each ancestor is walked up once: it leaves the map as it is used.
    let mut walk = |start: u64, into: &mut Vec<u64>| {
        let mut at = start;
        while at != 0 {
            into.push(at);
            at = to_ancestor.remove(&at).unwrap_or(0);
        }
    };
    match queried {
        Queried::Post(post) if post.post_type == "page" => {
            ancestry.posts = post_ancestors(site, post);
        }
        Queried::Post(post) if request.singular && post.post_type == "post" => {
            let terms = site.post_terms(post.id, "category");
            ancestry.object_parents = terms.iter().map(|term| term.id).collect();
            for term in &terms {
                walk(term.id, &mut ancestry.categories);
            }
        }
        Queried::Term("category", term) => walk(term.id, &mut ancestry.categories),
        _ => {}
    }
    ancestry.object_parents.retain(|id| *id != 0);
    ancestry
}

/// The item's ancestors in its menu: `_menu_item_menu_item_parent` of the
/// item, of that, and so on, each once.
fn menu_ancestors(site: &dyn Site, db_id: u64, into: &mut Vec<u64>) {
    let mut at = db_id;
    loop {
        let parent = site
            .post_meta(at, "_menu_item_menu_item_parent")
            .unwrap_or_default();
        at = id_of(int(&parent));
        if at == 0 || into.contains(&at) {
            return;
        }
        into.push(at);
    }
}

impl Context<'_> {
    /// `is_page()`: a page asked for, not the posts page.
    fn is_page(&self) -> bool {
        self.request.singular
            && matches!(&self.queried, Queried::Post(post) if post.post_type == "page")
    }

    /// The first loop's classes of one item.
    fn item(&self, item: &NavMenuItem, classes: &mut Vec<Vec<u8>>, active: &mut Active) {
        let request = self.request;
        let object_id = int(&item.object_id);
        let is_post_type = item.kind == b"post_type";
        let queried_post_type = match &self.queried {
            Queried::Post(post) => post.post_type.as_bytes(),
            _ => b"",
        };
        let is_term_archive = matches!(
            &self.queried,
            Queried::Term(taxonomy, _) if taxonomy.as_bytes() == item.object
        );
        let queried_here = object_id == i64::try_from(request.queried_object_id).unwrap_or(-1);
        if request.singular
            && item.kind == b"taxonomy"
            && self.ancestry.object_parents.contains(&id_of(object_id))
        {
            // A term of the post being asked for.
            active.parent_objects.push(id_of(object_id));
            active.parent_items.push(item.id);
            active.object = queried_post_type.to_vec();
        } else if queried_here
            && ((self.page_for_posts != 0
                && is_post_type
                && request.home
                && self.page_for_posts == object_id)
                || (is_post_type && request.singular)
                || (item.kind == b"taxonomy" && is_term_archive))
        {
            classes.push(b"current-menu-item".to_vec());
            menu_ancestors(self.site, item.id, &mut active.ancestor_items);
            if is_post_type && item.object == b"page" {
                classes.push(b"page_item".to_vec());
                classes.push([b"page-item-", &item.object_id[..]].concat());
                classes.push(b"current_page_item".to_vec());
            }
            active.parent_items.push(id_of(int(&item.menu_item_parent)));
            active.parent_objects.push(item.post_parent);
            active.object.clone_from(&item.object);
        } else if item.object == b"custom" {
            self.custom_link(item, classes, active);
        }
    }

    /// The classes of a custom link, which is current when it links to the
    /// page asked for.
    fn custom_link(&self, item: &NavMenuItem, classes: &mut Vec<Vec<u8>>, active: &mut Active) {
        let (site, request) = (self.site, self.request);
        let home = site.home_url();
        let origin = home.strip_suffix(home_path(&String::from_utf8_lossy(&home)).as_bytes());
        let origin = origin.unwrap_or(&home);
        let scheme = url_scheme(&home).unwrap_or_default();
        let root_relative = untrailingslashit(&request.uri);
        let current_url = [origin, root_relative].concat();
        let url = item.url.as_deref().unwrap_or_default();
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
            classes.push(b"current-menu-item".to_vec());
            menu_ancestors(site, item.id, &mut active.ancestor_items);
            let current = [
                untrailingslashit(&current_url),
                untrailingslashit(indexless),
            ];
            if current.contains(&home.as_slice()) {
                classes.push(b"current_page_item".to_vec());
            }
            active.parent_items.push(id_of(int(&item.menu_item_parent)));
            active.parent_objects.push(item.post_parent);
            active.object.clone_from(&item.object);
        } else if item_url == home && request.front_page {
            classes.push(b"current-menu-item".to_vec());
        }
        if untrailingslashit(&item_url) == home {
            classes.push(b"menu-item-home".to_vec());
        }
    }

    /// The second loop's classes of one item: those of its position
    /// above the page asked for.
    fn parent_classes(&self, item: &mut NavMenuItem, active: &Active) {
        let object_id = int(&item.object_id);
        let id = id_of(object_id);
        let mut classes = std::mem::take(&mut item.classes);
        let ancestral = match &self.queried {
            Queried::Post(post) => {
                let listed = self.ancestry.posts.contains(&id) && id != post.id;
                let term = item.kind == b"taxonomy"
                    && item.object == b"category"
                    && self.ancestry.categories.contains(&id);
                (item.kind == b"post_type" && listed || term)
                    .then(|| format!("current-{}-ancestor", post.post_type))
            }
            Queried::Term(taxonomy, term) => {
                let listed = item.kind == b"taxonomy"
                    && item.object == b"category"
                    && self.ancestry.categories.contains(&id)
                    && id != term.id;
                listed.then(|| format!("current-{taxonomy}-ancestor"))
            }
            Queried::None => None,
        };
        classes.extend(ancestral.map(String::into_bytes));
        if active.ancestor_items.contains(&item.id) {
            classes.push(b"current-menu-ancestor".to_vec());
        }
        if active.parent_items.contains(&item.id) {
            classes.push(b"current-menu-parent".to_vec());
        }
        if active.parent_objects.contains(&id) {
            classes.push([b"current-", &active.object[..], b"-parent"].concat());
        }
        if item.kind == b"post_type" && item.object == b"page" {
            // Back compat classes for pages to match `wp_page_menu()`.
            for (class, back_compat) in [
                (&b"current-menu-parent"[..], &b"current_page_parent"[..]),
                (b"current-menu-ancestor", b"current_page_ancestor"),
            ] {
                if classes.iter().any(|c| c == class) {
                    classes.push(back_compat.to_vec());
                }
            }
        }
        item.classes = unique(classes);
    }
}

/// `set_url_scheme( $url )`: `url` with the scheme of the home URL, which
/// a request is served under.
pub(super) fn with_scheme(url: &[u8], scheme: &[u8]) -> Vec<u8> {
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
