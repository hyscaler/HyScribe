// SPDX-License-Identifier: AGPL-3.0-or-later

//! The items of posts, pages, terms and archives and the classes of the
//! page asked for, against the write oracle's `navigation-fallback-*`
//! actions of `reference/writes/actions-navigation.php`.

use super::*;
use crate::site::{Archive, Taxonomy};

/// A page or post of the default install, as the oracle's setup left it.
fn post(id: u64, (title, name, kind): (&str, &str, &str), status: &str, parent: u64) -> Post {
    Post {
        id,
        title: title.into(),
        name: name.into(),
        post_type: kind.into(),
        status: status.into(),
        parent,
        ..Post::default()
    }
}

fn category(id: u64, name: &str, parent: u64) -> Term {
    Term {
        id,
        name: name.into(),
        slug: name.to_lowercase().replace(' ', "-").into(),
        parent,
        ..Term::default()
    }
}

/// The default install over the oracle's categories, with a menu of the
/// name the case gives it: the first post (in `Uncategorized` and the child
/// category), the Sample Page and the Privacy Policy draft.
fn site(menu: &str) -> Menus {
    let mut site = Menus::default();
    site.menu(20, menu, &menu.to_lowercase().replace(' ', "-"));
    site.options = vec![("wp_page_for_privacy_policy", "3".into())];
    site.posts = vec![
        post(1, ("Hello world!", "hello-world", "post"), "publish", 0),
        post(2, ("Sample Page", "sample-page", "page"), "publish", 0),
        post(3, ("Privacy Policy", "privacy-policy", "page"), "draft", 0),
    ];
    site.categories = vec![
        category(1, "Uncategorized", 0),
        category(9001, "Parent Category", 0),
        category(9002, "Child Category", 9001),
    ];
    site.post_categories = vec![1];
    site
}

impl Menus {
    fn option_is(&mut self, name: &'static str, value: &str) {
        self.options.push((name, value.to_string()));
    }

    fn post_mut(&mut self, id: u64) -> &mut Post {
        self.posts.iter_mut().find(|post| post.id == id).unwrap()
    }

    /// An item of an object, as `wp_update_nav_menu_item()` stores it: its
    /// `post_parent` is the object's parent, and `parent` is the index of
    /// the item it is under.
    fn object(&mut self, kind: &str, object: &str, id: u64, parent: Option<u64>) -> &mut Entry {
        let item = 100 + self.entries.len() as u64;
        let above = parent.map_or(0, |index| 100 + index);
        let original = match kind {
            "post_type" => self
                .posts
                .iter()
                .find(|post| post.id == id)
                .map(|post| post.parent),
            "taxonomy" if object == "category" => self
                .categories
                .iter()
                .find(|term| term.id == id)
                .map(|term| term.parent),
            _ => None,
        };
        let entry = self.link(20, item, above, ("", ""));
        entry.post.parent = original.unwrap_or(0);
        entry.set("_menu_item_type", kind);
        entry.set("_menu_item_object", object);
        entry.set("_menu_item_object_id", &id.to_string());
        entry.set("_menu_item_url", "");
        entry
    }

    /// A custom link under the item `parent`.
    fn custom(&mut self, parent: Option<u64>, (title, url): (&str, &str)) -> &mut Entry {
        let item = 100 + self.entries.len() as u64;
        self.link(
            20,
            item,
            parent.map_or(0, |index| 100 + index),
            (title, url),
        )
    }
}

/// What `made_for()` gives against the golden of `case`.
fn matches_golden(case: &str, site: &Menus, request: &Request, menu: (&str, &str)) {
    let classic = made_for(site, request).unwrap();
    let content = String::from_utf8_lossy(&classic.content).into_owned();
    let expected = String::from_utf8(golden(case, "post_content")).unwrap();
    assert_eq!(content, expected, "{case}");
    assert_eq!(classic.title, menu.0.as_bytes());
    assert_eq!(classic.title, golden(case, "post_title"));
    assert_eq!(classic.name, menu.1.as_bytes());
    assert_eq!(classic.name, golden(case, "post_name"));
}

fn home() -> Request {
    Request {
        home: true,
        front_page: true,
        uri: "/".into(),
        ..Request::default()
    }
}

fn singular(id: u64, uri: &str) -> Request {
    Request {
        singular: true,
        queried_object_id: id,
        uri: uri.into(),
        ..Request::default()
    }
}

/// Items of a post, a page, a term and an archive, titled by the post or
/// by what they link to, on the home page.
#[test]
fn objects_are_converted_as_php_does() {
    let mut site = site("Objects");
    site.post_categories = vec![1, 9002];
    site.option_is("page_for_posts", "2");
    site.post_mut(1).title = b"Hello \"world\" -- &amp; <b>more</b>".to_vec();
    site.object("post_type", "page", 2, None);
    let renamed = site.object("post_type", "page", 2, None);
    renamed.post.title = b"Renamed \"page\"".to_vec();
    renamed.set("_menu_item_target", "_blank");
    renamed.set("_menu_item_classes", "a:2:{i:0;s:1:\"x\";i:1;s:1:\"y\";}");
    renamed.set("_menu_item_xfn", "friend");
    let post_item = site.object("post_type", "post", 1, None);
    post_item.post.content = b"A post".to_vec();
    post_item.post.excerpt = b"Tip".to_vec();
    site.object("taxonomy", "category", 1, Some(2));
    site.object("post_type_archive", "post", 0, Some(2));
    site.object("post_type", "page", 3, None);
    site.object("taxonomy", "category", 9002, Some(5));
    site.object("post_type_archive", "page", 0, None);
    site.object("post_type_archive", "post", 0, None).post.title = b"Blog".to_vec();
    matches_golden(
        "navigation-fallback-objects",
        &site,
        &home(),
        ("Objects", "objects"),
    );
}

/// The links under pretty permalinks.
#[test]
fn objects_link_under_pretty_permalinks() {
    let mut site = site("Pretty");
    site.post_categories = vec![1, 9002];
    site.option_is("permalink_structure", "/%postname%/");
    site.object("post_type", "page", 2, None);
    site.object("post_type", "post", 1, None);
    site.object("taxonomy", "category", 9002, None);
    site.object("post_type_archive", "post", 0, None);
    matches_golden(
        "navigation-fallback-pretty",
        &site,
        &home(),
        ("Pretty", "pretty"),
    );
}

/// The posts page and the front page, and the archive of `post` that is
/// the posts page.
#[test]
fn the_posts_page_and_the_front_page() {
    let mut site = site("Posts page");
    site.post_mut(3).status = "publish".into();
    for (name, value) in [
        ("show_on_front", "page"),
        ("page_on_front", "3"),
        ("page_for_posts", "2"),
    ] {
        site.option_is(name, value);
    }
    site.categories.push(category(2, "Same ID", 0));
    site.object("post_type", "page", 2, None);
    site.object("post_type", "page", 3, None);
    site.object("taxonomy", "category", 2, None);
    site.object("post_type_archive", "post", 0, None);
    site.custom(None, ("Home", "http://127.0.0.1:8081/"));
    let request = Request {
        home: true,
        queried_object_id: 2,
        uri: "/?page_id=2".into(),
        ..Request::default()
    };
    let menu = ("Posts page", "posts-page");
    matches_golden("navigation-fallback-posts-page", &site, &request, menu);

    let mut site = self::site("Front only");
    site.post_mut(3).status = "publish".into();
    site.option_is("show_on_front", "page");
    site.option_is("page_on_front", "3");
    site.object("post_type", "page", 3, None);
    site.object("post_type_archive", "post", 0, None);
    let request = Request {
        front_page: true,
        ..singular(3, "/")
    };
    let menu = ("Front only", "front-only");
    matches_golden("navigation-fallback-front-only", &site, &request, menu);

    let mut site = self::site("Front page");
    site.post_mut(3).status = "publish".into();
    site.option_is("show_on_front", "page");
    site.option_is("page_on_front", "3");
    site.option_is("page_for_posts", "2");
    site.object("post_type", "page", 2, None);
    site.object("post_type", "page", 3, None);
    site.object("post_type_archive", "post", 0, None);
    let request = Request {
        front_page: true,
        ..singular(3, "/")
    };
    matches_golden(
        "navigation-fallback-front-page",
        &site,
        &request,
        ("Front page", "front-page"),
    );
}

/// The current page, and the items above it.
#[test]
fn the_current_page_and_its_menu_ancestors() {
    let mut site = site("Current page");
    site.custom(None, ("Top", "https://example.test/"));
    site.object("post_type", "post", 1, Some(0));
    site.object("post_type", "page", 2, Some(1));
    site.custom(Some(2), ("Below", "https://example.test/below"));
    let request = singular(2, "/?page_id=2");
    matches_golden(
        "navigation-fallback-current-page",
        &site,
        &request,
        ("Current page", "current-page"),
    );

    // A page's parent page is an ancestor, and the page the current item.
    let mut site = self::site("Page ancestor");
    let child = site.post_mut(3);
    (child.status, child.parent) = ("publish".into(), 2);
    site.object("post_type", "page", 2, None);
    site.object("post_type", "page", 3, Some(0));
    let request = singular(3, "/?page_id=3");
    matches_golden(
        "navigation-fallback-page-ancestor",
        &site,
        &request,
        ("Page ancestor", "page-ancestor"),
    );
}

/// The categories of the post asked for, and of the category asked for.
#[test]
fn terms_above_the_post_or_the_category_asked_for() {
    let mut site = site("Single post");
    site.post_categories = vec![1, 9002];
    site.object("post_type", "post", 1, None);
    site.object("taxonomy", "category", 9001, None);
    site.object("taxonomy", "category", 9002, Some(1));
    site.object("taxonomy", "category", 1, None);
    site.object("post_type", "page", 2, None);
    site.object("taxonomy", "post_tag", 9002, None);
    let request = singular(1, "/?p=1");
    matches_golden(
        "navigation-fallback-single-post",
        &site,
        &request,
        ("Single post", "single-post"),
    );

    // A category that shares the post's ID is not the current item.
    let mut site = self::site("Single other");
    site.post_categories = vec![9002];
    site.object("post_type", "post", 1, None);
    site.object("taxonomy", "category", 1, None);
    site.object("taxonomy", "category", 9002, None);
    let menu = ("Single other", "single-other");
    matches_golden("navigation-fallback-single-other", &site, &request, menu);

    let mut site = self::site("Category archive");
    site.post_categories = vec![1, 9002];
    site.object("taxonomy", "category", 9001, None);
    site.object("taxonomy", "category", 9002, Some(0));
    site.object("taxonomy", "category", 1, None);
    site.object("post_type", "post", 1, None);
    site.object("taxonomy", "post_tag", 9002, None);
    let request = Request {
        archive: Some(Archive::Term(
            Taxonomy::Category,
            Some(category(9002, "Child Category", 9001)),
        )),
        queried_object_id: 9002,
        uri: "/?cat=9002".into(),
        ..Request::default()
    };
    matches_golden(
        "navigation-fallback-category",
        &site,
        &request,
        ("Category archive", "category-archive"),
    );
}

/// A custom link to the page asked for, with its ancestors, and the home
/// links.
#[test]
fn custom_links_to_the_current_page_and_home() {
    let mut site = site("Current link");
    site.custom(None, ("Top", "https://example.test/"));
    site.custom(Some(0), ("Middle", "https://example.test/middle"));
    let here = site.custom(Some(1), ("Here", "http://127.0.0.1:8081/?page_id=2#top"));
    here.set("_menu_item_classes", "a:1:{i:0;s:4:\"here\";}");
    site.custom(None, ("Elsewhere", "http://127.0.0.1:8081/?page_id=3"));
    let request = singular(2, "/?page_id=2");
    matches_golden(
        "navigation-fallback-current-link",
        &site,
        &request,
        ("Current link", "current-link"),
    );

    let mut site = self::site("Home link");
    site.custom(None, ("Home", "http://127.0.0.1:8081/"));
    site.custom(None, ("Home again", "https://127.0.0.1:8081"));
    site.custom(None, ("Elsewhere", "https://example.test/"));
    site.custom(None, ("Relative", "/"));
    matches_golden(
        "navigation-fallback-home-link",
        &site,
        &home(),
        ("Home link", "home-link"),
    );

    let mut site = self::site("Home paged");
    site.custom(None, ("Home", "http://127.0.0.1:8081/"));
    site.custom(None, ("Elsewhere", "https://example.test/"));
    let request = Request {
        paged: 2,
        uri: "/?paged=2".into(),
        ..home()
    };
    matches_golden(
        "navigation-fallback-home-paged",
        &site,
        &request,
        ("Home paged", "home-paged"),
    );
}

/// Items whose objects are gone, trashed or not registered are left out;
/// a menu of nothing else is the default fallback's.
#[test]
fn invalid_items_are_left_out() {
    let invalid = |site: &mut Menus| {
        site.posts
            .push(post(50, ("Gone", "gone", "page"), "trash", 0));
        site.object("post_type", "page", 50, None);
        site.object("post_type", "page", 9999, None);
        site.object("post_type", "ghost", 1, None);
        site.object("taxonomy", "category", 9999, None);
        site.object("taxonomy", "ghost", 1, None);
        site.object("post_type_archive", "ghost", 0, None);
    };
    let mut only = site("Only invalid");
    invalid(&mut only);
    assert!(matches!(
        classic_menu(&only, &Request::default()),
        Conversion::Error
    ));

    let mut mixed = site("Mixed validity");
    mixed
        .posts
        .push(post(50, ("Gone", "gone", "page"), "trash", 0));
    mixed.object("post_type", "page", 50, None);
    mixed.custom(None, TWITTER);
    mixed.object("post_type", "page", 9999, None);
    mixed.object("post_type", "page", 2, None);
    mixed.object("taxonomy", "category", 9999, None);
    let menu = ("Mixed validity", "mixed-validity");
    matches_golden(
        "navigation-fallback-invalid-mixed",
        &mixed,
        &Request::default(),
        menu,
    );
}

/// What is not ported makes no post: a registered taxonomy with no link
/// here, and an archive of a type whose labels are unknown.
#[test]
fn what_is_not_ported_is_not_written() {
    let not_ported = |kind: &str, object: &str| {
        let mut site = site("Menu");
        site.custom(None, TWITTER);
        site.object(kind, object, 1, None);
        matches!(
            classic_menu(&site, &Request::default()),
            Conversion::NotPorted
        )
    };
    assert!(not_ported("taxonomy", "nav_menu"));
    assert!(not_ported("post_type_archive", "revision"));
    assert!(!not_ported("taxonomy", "category"));
}

/// The two filters of `wp_get_nav_menus()` and `wp_get_nav_menu_items()`
/// run with the arguments WordPress gives them.
#[test]
fn the_menu_filters_run() {
    let mut site = two_menus();
    site.hooks = Some(&Hides);
    // The filter hides the newer menu, so the older one is the newest.
    assert_eq!(made(&site).name, b"chosen-by-location");
    // The menu at the primary location is the database's, not the filter's.
    let mut located = super::located("i:3;");
    located.hooks = Some(&Hides);
    assert_eq!(made(&located).name, b"newer-menu");
    let mut site = Menus::default();
    site.menu(2, "Menu", "menu");
    site.link(2, 1, 0, TWITTER);
    site.link(2, 2, 0, DOCS);
    site.hooks = Some(&Trims);
    let content = String::from_utf8(made(&site).content).unwrap();
    assert!(content.contains("\"label\":\"Docs"));
    assert!(!content.contains("twitter.com"));
}

/// A hook that drops the newest menu.
#[derive(Debug)]
struct Hides;

impl Hooks for Hides {
    fn wp_get_nav_menus(&self, mut menus: Vec<Term>, args: &[(String, Value)]) -> Vec<Term> {
        assert_eq!(args[0].0, "taxonomy");
        assert_eq!(args[1], ("hide_empty".to_string(), Value::Bool(false)));
        assert_eq!(args[2].0, "orderby");
        menus.retain(|menu| menu.id != 3);
        menus
    }
}

/// A hook that drops the first item.
#[derive(Debug)]
struct Trims;

impl Hooks for Trims {
    fn wp_get_nav_menu_items(
        &self,
        mut items: Vec<NavMenuItem>,
        menu: &Term,
        args: &[(String, Value)],
    ) -> Vec<NavMenuItem> {
        assert_eq!((menu.id, menu.count), (2, 2));
        let names: Vec<&str> = args.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(&names[..3], ["order", "orderby", "post_type"]);
        assert_eq!(items.len(), 2);
        items.remove(0);
        items
    }
}

/// An object without a title is named by its ID, and the main query of an
/// archive with no term or a date has no queried object.
#[test]
fn untitled_objects_and_other_archives() {
    let mut site = site("Untitled");
    site.post_mut(2).title = Vec::new();
    site.categories[0].name = Vec::new();
    site.object("post_type", "page", 2, None);
    site.object("taxonomy", "category", 1, None);
    let request = Request {
        archive: Some(Archive::Year),
        uri: "/?m=2026".into(),
        ..Request::default()
    };
    let content = made_for(&site, &request).unwrap().content;
    let content = String::from_utf8(content).unwrap();
    assert!(content.contains("\"label\":\"#2 (no title)\""));
    assert!(content.contains("\"label\":\"#1 (no title)\""));
    assert!(!content.contains("current"));
    // A term archive of no term has none either.
    let none = Request {
        archive: Some(Archive::Term(Taxonomy::Category, None)),
        ..request
    };
    assert!(made_for(&site, &none).is_some());
}

/// Items that are each other's parents end the walk up the menu.
#[test]
fn a_loop_of_parents_ends() {
    let mut site = site("Loop");
    site.custom(None, TWITTER);
    site.custom(Some(2), ("Here", "http://127.0.0.1:8081/?page_id=2"));
    site.custom(Some(1), ("There", "https://example.test/there"));
    let request = singular(2, "/?page_id=2");
    let content = made_for(&site, &request).unwrap().content;
    let content = String::from_utf8(content).unwrap();
    // Only the item at the top is converted.
    assert!(content.contains("twitter.com") && !content.contains("Here"));
}

/// An attachment is singular but has no categories of its own, whatever
/// the categories of its parent: no item is above it.
#[test]
fn an_attachment_has_no_terms_above_it() {
    let mut site = site("Attachment");
    site.post_categories = vec![1, 9002];
    site.posts
        .push(post(77, ("Picture", "picture", "attachment"), "inherit", 1));
    site.object("taxonomy", "category", 1, None);
    site.object("taxonomy", "category", 9002, None);
    site.object("post_type", "post", 1, None);
    let request = Request {
        attachment: true,
        ..singular(77, "/")
    };
    let menu = ("Attachment", "attachment");
    matches_golden("navigation-fallback-attachment", &site, &request, menu);
}
mod nav_menu;
