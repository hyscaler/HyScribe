// SPDX-License-Identifier: AGPL-3.0-or-later

use std::cell::RefCell;

use hyscribe_model::Term;

use super::*;
use crate::outbound::Hooks;
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// One menu item as the database holds it: its menu, its post and its meta.
struct Entry {
    menu: u64,
    post: Post,
    meta: Vec<(&'static str, String)>,
}

/// Classic menus over a site at `http://127.0.0.1:8081`.
#[derive(Default)]
struct Menus {
    options: Vec<(&'static str, String)>,
    terms: Vec<Term>,
    entries: Vec<Entry>,
    hooks: Option<&'static dyn Hooks>,
    /// The `wp_navigation` post that exists, if any.
    existing: Option<Post>,
    /// What `insert_navigation_fallback()` was given.
    inserted: RefCell<Vec<[Vec<u8>; 3]>>,
}

/// A hook that declines to create a fallback.
#[derive(Debug)]
struct Declined;

impl Hooks for Declined {
    fn wp_navigation_should_create_fallback(&self, _create: bool) -> bool {
        false
    }
}

/// A hook that marks what it filters.
#[derive(Debug)]
struct Marked;

impl Hooks for Marked {
    fn nav_menu_attr_title(&self, title: Vec<u8>) -> Vec<u8> {
        [&b"T:"[..], &title].concat()
    }

    fn nav_menu_description(&self, description: Vec<u8>) -> Vec<u8> {
        [&b"D:"[..], &description].concat()
    }
}

impl Site for Menus {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        let option = self.options.iter().find(|(key, _)| *key == name);
        option.map(|(_, value)| value.clone().into_bytes())
    }

    fn home_url(&self) -> Vec<u8> {
        b"http://127.0.0.1:8081".to_vec()
    }

    fn post(&self, _id: u64) -> Option<Post> {
        None
    }

    fn post_terms(&self, _post_id: u64, _taxonomy: &str) -> Vec<Term> {
        Vec::new()
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        let entry = self.entries.iter().find(|entry| entry.post.id == post_id)?;
        let meta = entry.meta.iter().find(|(name, _)| *name == key);
        meta.map(|(_, value)| value.clone().into_bytes())
    }

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        if query.post_types == ["wp_navigation"] {
            return self.existing.iter().cloned().collect();
        }
        if query.post_types != ["nav_menu_item"] {
            return Vec::new();
        }
        // `nopaging`.
        assert_eq!(query.limit, query_limit(-1));
        assert_eq!(query.order, [(OrderBy::MenuOrder, true)]);
        let menu = query.terms[0].terms[0];
        let mut found: Vec<Post> = self
            .entries
            .iter()
            .filter(|entry| entry.menu == menu)
            .map(|entry| entry.post.clone())
            .collect();
        found.sort_by_key(|post| post.menu_order);
        found
    }

    fn taxonomy_terms(&self, taxonomy: &str) -> Vec<Term> {
        assert_eq!(taxonomy, "nav_menu");
        self.terms.clone()
    }

    fn insert_navigation_fallback(&self, title: &[u8], name: &[u8], content: &[u8]) {
        let post = [title.to_vec(), name.to_vec(), content.to_vec()];
        self.inserted.borrow_mut().push(post);
    }

    fn hooks(&self) -> &dyn Hooks {
        self.hooks.unwrap_or(&())
    }
}

impl Menus {
    fn menu(&mut self, id: u64, name: &str, slug: &str) {
        self.terms.push(Term {
            id,
            name: name.into(),
            slug: slug.into(),
            ..Term::default()
        });
    }

    /// A custom link at the next menu order of `menu`, under `parent`.
    fn link(&mut self, menu: u64, id: u64, parent: u64, (title, url): (&str, &str)) -> &mut Entry {
        let meta = vec![
            ("_menu_item_type", "custom".to_string()),
            ("_menu_item_object", "custom".to_string()),
            ("_menu_item_object_id", id.to_string()),
            ("_menu_item_menu_item_parent", parent.to_string()),
            ("_menu_item_url", url.to_string()),
            ("_menu_item_classes", "a:1:{i:0;s:0:\"\";}".to_string()),
        ];
        let post = Post {
            id,
            title: title.into(),
            menu_order: id as i64,
            post_type: "nav_menu_item".into(),
            status: "publish".into(),
            ..Post::default()
        };
        self.terms
            .iter_mut()
            .filter(|term| term.id == menu)
            .for_each(|term| term.count += 1);
        self.entries.push(Entry { menu, post, meta });
        self.entries.last_mut().unwrap()
    }
}

impl Entry {
    fn set(&mut self, key: &'static str, value: &str) {
        self.meta.retain(|(name, _)| *name != key);
        self.meta.push((key, value.to_string()));
    }
}

/// A value of the golden of a write oracle `case`: the cell of the post
/// that is new in it.
fn golden(case: &str, column: &str) -> Vec<u8> {
    let path = format!(
        "{}/../hyscribe-testkit/tests/golden/writes/{case}.tsv",
        env!("CARGO_MANIFEST_DIR")
    );
    let golden = std::fs::read_to_string(path).unwrap();
    let wanted = format!("\t+\t{column}\t");
    let line = golden
        .lines()
        .find(|line| line.starts_with("wp_posts\t") && line.contains(&wanted))
        .unwrap();
    let value = line.split_once(&wanted).unwrap().1;
    let mut unescaped = String::new();
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            unescaped.push(c);
            continue;
        }
        unescaped.push(match chars.next() {
            Some('t') => '\t',
            Some('n') => '\n',
            Some('r') => '\r',
            other => other.unwrap(),
        });
    }
    unescaped.into_bytes()
}

/// The links of the oracle's `$plain` menus, as stored.
const DOCS: (&str, &str) = (
    "Docs &amp; \"News\" <b>bold</b> -- Ελληνικά",
    "https://example.test/docs?a=1&b=2#top",
);
const TWITTER: (&str, &str) = ("twitter.com", "https://twitter.com/wordpress");

fn made(site: &Menus) -> Classic {
    classic_menu(site, &Request::default()).unwrap()
}

/// What `reference/writes/actions-navigation.php` has PHP write for a
/// menu of custom links with a submenu, a description, a title attribute,
/// a new tab, a relation and classes.
#[test]
fn a_classic_menu_is_converted_as_php_does() {
    let mut site = Menus::default();
    site.menu(2, "An older menu", "an-older-menu");
    site.menu(3, "Oracle Menu", "oracle-menu");
    site.link(2, 1, 0, TWITTER);
    site.link(3, 10, 0, ("Home", "https://example.test/"));
    let about = site.link(3, 11, 0, ("About", "https://example.test/about"));
    about.set(
        "_menu_item_classes",
        "a:2:{i:0;s:4:\"tall\";i:1;s:4:\"wide\";}",
    );
    about.set("_menu_item_target", "_blank");
    about.set("_menu_item_xfn", "nofollow");
    about.post.content = b"Who we are".to_vec();
    about.post.excerpt = b"The about page".to_vec();
    site.link(3, 12, 11, ("Team", "https://example.test/team"));
    site.link(3, 13, 12, ("Careers", "https://example.test/careers"));
    site.link(3, 14, 0, ("Contact", "#"));
    let classic = made(&site);
    let case = "navigation-fallback-classic";
    assert_eq!(classic.content, golden(case, "post_content"));
    assert_eq!(classic.title, golden(case, "post_title"));
    assert_eq!(classic.name, golden(case, "post_name"));
}

/// Backslashes the JSON escapes carry are unslashed, as `wp_insert_post()`
/// unslashes what it is given, and so is the menu's name.
#[test]
fn the_fields_are_unslashed() {
    let mut site = Menus::default();
    site.menu(2, "Escapes", "escapes");
    site.link(2, 1, 0, TWITTER);
    site.link(2, 2, 0, DOCS);
    let classic = made(&site);
    let case = "navigation-fallback-escapes";
    assert_eq!(classic.content, golden(case, "post_content"));
    assert_eq!(classic.name, golden(case, "post_name"));
    site.menu(3, "A \\\"slashed\\\" menu", "slashed\\-menu");
    site.link(3, 3, 0, TWITTER);
    let classic = made(&site);
    assert_eq!(classic.title, b"A \"slashed\" menu");
    assert_eq!(classic.name, b"slashed-menu");
}

/// Two menus, the older with the first link and the newer with the second.
fn two_menus() -> Menus {
    let mut site = Menus::default();
    site.menu(2, "Chosen by location", "chosen-by-location");
    site.menu(3, "Newer menu", "newer-menu");
    site.link(2, 1, 0, TWITTER);
    site.link(3, 2, 0, DOCS);
    site
}

/// Two menus and the theme's mods with `value` at the `primary` location.
fn located(value: &str) -> Menus {
    let mut site = two_menus();
    let mods = format!("a:1:{{s:18:\"nav_menu_locations\";a:1:{{s:7:\"primary\";{value}}}}}");
    site.options = vec![
        ("stylesheet", "twentytwentyfive".into()),
        ("theme_mods_twentytwentyfive", mods),
    ];
    site
}

/// The menu at the `primary` location wins over the newest; with none, the
/// newest.
#[test]
fn the_menu_at_the_primary_location_wins() {
    // No mods, no `primary` slug: the newest.
    assert_eq!(made(&two_menus()).name, b"newer-menu");
    let case = "navigation-fallback-primary-location";
    assert_eq!(made(&located("i:2;")).title, golden(case, "post_title"));
    assert_eq!(made(&located("i:2;")).content, golden(case, "post_content"));
    // By slug and by name, the way `wp_get_nav_menu_object()` looks.
    let by_slug = located("s:18:\"chosen-by-location\";");
    assert_eq!(made(&by_slug).name, b"chosen-by-location");
    let by_name = located("s:18:\"Chosen by location\";");
    assert_eq!(made(&by_name).name, b"chosen-by-location");
    // A location that names no menu, or none, leaves the newest.
    for value in ["i:9;", "i:0;", "N;", "s:3:\"abc\";", "i:-4;"] {
        assert_eq!(made(&located(value)).name, b"newer-menu", "{value}");
    }
}

/// The menu slugged `primary` wins over a newer one.
#[test]
fn a_menu_slugged_primary_wins_over_a_newer_one() {
    let mut site = Menus::default();
    site.menu(2, "Primary", "primary");
    site.menu(3, "Newer menu", "newer-menu");
    site.link(2, 1, 0, TWITTER);
    site.link(3, 2, 0, DOCS);
    let case = "navigation-fallback-primary-slug";
    assert_eq!(made(&site).content, golden(case, "post_content"));
}

/// `get_theme_mods()`: the option of the stylesheet, else the deprecated
/// one of the theme's name; neither, or one that does not decode, is none.
#[test]
fn theme_mods_come_from_either_option() {
    let mods = "a:1:{s:18:\"nav_menu_locations\";a:1:{s:7:\"primary\";i:2;}}";
    let mut site = Menus::default();
    site.menu(2, "Located", "located");
    site.menu(3, "Newer", "newer");
    site.link(2, 1, 0, TWITTER);
    site.link(3, 2, 0, DOCS);
    assert_eq!(made(&site).name, b"newer");
    site.options.push(("stylesheet", "child".into()));
    assert_eq!(made(&site).name, b"newer");
    site.options.push(("current_theme", "Child".into()));
    assert_eq!(made(&site).name, b"newer");
    site.options.push(("mods_Child", mods.into()));
    assert_eq!(made(&site).name, b"located");
    site.options
        .push(("theme_mods_child", "not serialized".into()));
    assert_eq!(made(&site).name, b"newer");
    site.options.retain(|(name, _)| *name != "theme_mods_child");
    site.options.push(("theme_mods_child", mods.into()));
    assert_eq!(made(&site).name, b"located");
}

/// No menu, a menu without items, and an item of a post, a term or an
/// archive make no fallback; a link to a page asked for does not either.
#[test]
fn what_is_not_ported_makes_no_fallback() {
    let request = Request::default();
    let mut site = Menus::default();
    assert!(classic_menu(&site, &request).is_none());
    site.menu(2, "Empty", "empty");
    assert!(classic_menu(&site, &request).is_none());
    // Counted but gone: the converter has nothing to convert.
    site.terms[0].count = 1;
    assert!(classic_menu(&site, &request).is_none());
    for kind in ["post_type", "post_type_archive", "taxonomy"] {
        let mut site = Menus::default();
        site.menu(2, "Menu", "menu");
        site.link(2, 1, 0, TWITTER);
        site.link(2, 2, 0, DOCS).set("_menu_item_type", kind);
        assert!(classic_menu(&site, &request).is_none(), "{kind}");
    }
    // Items whose parents are not in the menu leave nothing at the top.
    let mut site = Menus::default();
    site.menu(2, "Menu", "menu");
    site.link(2, 1, 7, TWITTER);
    assert!(classic_menu(&site, &request).is_none());
    // An item with a parent of no integer is not at the top either.
    site.entries[0].set("_menu_item_menu_item_parent", "");
    assert!(classic_menu(&site, &request).is_none());
}

/// A link to the page being asked for adds the current item's classes,
/// which are not ported; the URL is compared as PHP does, with or without
/// the host, the fragment, a trailing slash, `index.php` and encoding.
#[test]
fn a_link_to_the_current_page_makes_no_fallback() {
    let current = |url: &str, uri: &str| {
        let mut site = Menus::default();
        site.menu(2, "Menu", "menu");
        site.link(2, 1, 0, ("Here", url));
        let request = Request {
            uri: uri.into(),
            ..Request::default()
        };
        classic_menu(&site, &request).is_none()
    };
    assert!(current("http://127.0.0.1:8081/about", "/about"));
    assert!(current("https://127.0.0.1:8081/about/", "/about/"));
    assert!(current("http://127.0.0.1:8081/about#team", "/about"));
    assert!(current("/about", "/about"));
    assert!(current("http://127.0.0.1:8081/", "/index.php"));
    assert!(!current("http://127.0.0.1:8081/a%20b", "/a b"));
    assert!(current("http://127.0.0.1:8081/a b", "/a%20b"));
    assert!(current("//127.0.0.1:8081/about", "/about"));
    assert!(!current("http://127.0.0.1:8081/about", "/other"));
    assert!(!current("https://example.test/about", "/about"));
    // A fragment at the start is kept, so it can be the page asked for.
    assert!(current("#", "#"));
    // A fragment alone and an empty or zero link are no page.
    assert!(!current("#", "/about"));
    assert!(!current("", ""));
    assert!(!current("0", "0"));
    assert!(!current("#top", "/about"));
}

/// A link to the home page is the home item, and the current one on the
/// front page when the request has more than the home's own address.
#[test]
fn the_home_link_has_its_classes() {
    let mut site = Menus::default();
    site.menu(2, "Menu", "menu");
    site.link(2, 1, 0, ("Home", "http://127.0.0.1:8081/"));
    site.link(2, 2, 0, ("Home again", "https://127.0.0.1:8081"));
    site.link(2, 3, 0, ("Elsewhere", "https://example.test/"));
    let request = Request {
        front_page: true,
        uri: "/?paged=2".into(),
        ..Request::default()
    };
    let content = classic_menu(&site, &request).unwrap().content;
    let content = String::from_utf8(content).unwrap();
    let class = |label: &str| {
        let after = content
            .split(&format!("\"label\":\"{label}\""))
            .next()
            .unwrap();
        let class = after.rsplit("\"className\":\"").next().unwrap();
        class.split('"').next().unwrap().to_string()
    };
    let home = " menu-item menu-item-type-custom menu-item-object-custom";
    assert_eq!(
        class("Home"),
        format!("{home} current-menu-item menu-item-home")
    );
    assert_eq!(
        class("Home again"),
        format!("{home} current-menu-item menu-item-home")
    );
    assert_eq!(class("Elsewhere"), home);
    // Off the front page, only the home item.
    let other = Request {
        uri: "/other".into(),
        ..Request::default()
    };
    let content = classic_menu(&site, &other).unwrap().content;
    let content = String::from_utf8(content).unwrap();
    assert_eq!(content.matches("current-menu-item").count(), 0);
    assert_eq!(content.matches("menu-item-home").count(), 2);
}

/// Stored classes: an array, a value that is not one, a plain string, a
/// value that does not decode and none at all; no class twice; and a
/// different object and type name their own classes, an ID of its own.
#[test]
fn stored_classes_and_other_objects() {
    let mut site = Menus::default();
    site.menu(2, "Menu", "menu");
    let classes = [
        "a:2:{i:0;s:9:\"menu-item\";i:1;s:1:\"x\";}",
        "i:5;",
        "plain",
        "a:1:{broken",
    ];
    for (id, stored) in classes.iter().enumerate() {
        let item = site.link(2, id as u64 + 1, 0, TWITTER);
        item.set("_menu_item_classes", stored);
    }
    let unset = site.link(2, 9, 0, TWITTER);
    unset.meta.retain(|(name, _)| *name != "_menu_item_classes");
    let other = site.link(2, 10, 0, TWITTER);
    other.set("_menu_item_type", "outer_space");
    other.set("_menu_item_object", "thing");
    other.set("_menu_item_object_id", "77");
    let content = String::from_utf8(made(&site).content).unwrap();
    let base = "menu-item menu-item-type-custom menu-item-object-custom";
    for expected in [
        "\"className\":\"menu-item x menu-item-type-custom menu-item-object-custom\"".to_string(),
        format!("\"className\":\"5 {base}\""),
        format!("\"className\":\"plain {base}\""),
        format!("\"className\":\"a:1:{{broken {base}\""),
        format!("\"className\":\" {base}\""),
        "\"className\":\" menu-item menu-item-type-outer_space menu-item-object-thing\""
            .to_string(),
        "\"id\":\"77\",\"kind\":\"outer-space\"".to_string(),
        "\"type\":\"thing\"".to_string(),
    ] {
        assert!(content.contains(&expected), "{expected}\n{content}");
    }
}

/// `nav_menu_attr_title` and `nav_menu_description` filter each item's.
#[test]
fn the_item_filters_run() {
    let mut site = Menus::default();
    site.menu(2, "Menu", "menu");
    let item = site.link(2, 1, 0, TWITTER);
    item.post.content = b"<b>A</b> long description".to_vec();
    item.post.excerpt = b"Tip".to_vec();
    let plain = String::from_utf8(made(&site).content).unwrap();
    assert!(plain.contains("\"description\":\"A long description\""));
    assert!(plain.contains("\"title\":\"Tip\""));
    site.hooks = Some(&Marked);
    let hooked = String::from_utf8(made(&site).content).unwrap();
    assert!(hooked.contains("\"description\":\"D:A long description\""));
    assert!(hooked.contains("\"title\":\"T:Tip\""));
}

/// A description is trimmed to 200 words.
#[test]
fn a_long_description_is_trimmed() {
    let mut site = Menus::default();
    site.menu(2, "Menu", "menu");
    let words = vec!["word"; 201].join(" ");
    site.link(2, 1, 0, TWITTER).post.content = words.into_bytes();
    let content = String::from_utf8(made(&site).content).unwrap();
    let kept = vec!["word"; 200].join(" ");
    assert!(content.contains(&format!("\"description\":\"{kept}u0026hellip;\"")));
}

/// Menus nest as deep as blocks do, no deeper.
#[test]
fn the_nesting_is_bounded() {
    let chain = |depth: u64| {
        let mut site = Menus::default();
        site.menu(2, "Menu", "menu");
        for id in 1..=depth {
            site.link(2, id, id - 1, TWITTER);
        }
        classic_menu(&site, &Request::default())
    };
    assert!(chain(127).is_some());
    assert!(chain(128).is_none());
}

/// The navigation block with no menu of its own shows the fallback: the
/// newest published `wp_navigation` post if there is one, else the classic
/// menu, written as a post, else the page list.
#[test]
fn the_navigation_block_falls_back_to_the_classic_menu() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let render = |site: &Menus| {
        let html = Renderer::new(&theme, site, &request).render(b"<!-- wp:navigation /-->");
        String::from_utf8(html).unwrap()
    };
    let site = two_menus();
    let html = render(&site);
    assert!(html.contains("wp-block-navigation-link"));
    assert!(html.contains("href=\"https://example.test/docs?a=1u0026b=2#top\""));
    let made = made(&site);
    assert_eq!(
        *site.inserted.borrow(),
        [[made.title, made.name, made.content]]
    );
    // A menu post that exists is the fallback, and nothing is written.
    let menu = Post {
        id: 900,
        content: b"<!-- wp:navigation-link {\"label\":\"Existing\",\"url\":\"/e\"} /-->".to_vec(),
        ..Post::default()
    };
    let existing = Menus {
        existing: Some(menu),
        ..two_menus()
    };
    assert!(render(&existing).contains("Existing"));
    assert!(existing.inserted.borrow().is_empty());
    // The filter can decline the fallback; the page list stands in.
    let declined = Menus {
        hooks: Some(&Declined),
        ..two_menus()
    };
    assert!(!render(&declined).contains("Docs"));
    assert!(declined.inserted.borrow().is_empty());
    // So does a menu not ported.
    let mut unported = two_menus();
    unported.entries[1].set("_menu_item_type", "taxonomy");
    assert!(!render(&unported).contains("Docs"));
    assert!(unported.inserted.borrow().is_empty());
}

/// `set_url_scheme()` puts the scheme of the home URL on a URL that has
/// one of word characters, or has none but two slashes.
#[test]
fn urls_take_the_scheme_of_the_home() {
    let cases: [(&[u8], &[u8]); 9] = [
        (b"http://a/b", b"https://a/b"),
        (b"  ftp://a  ", b"https://a"),
        (b"a_b9://x", b"https://x"),
        (b"//host/p", b"https://host/p"),
        (b"://a", b"://a"),
        (b"a b://x", b"a b://x"),
        (b"a-b://x", b"a-b://x"),
        (b"/relative", b"/relative"),
        (b"", b""),
    ];
    for (url, expected) in cases {
        assert_eq!(with_scheme(url, b"https"), expected, "{url:?}");
    }
}

/// The hook's defaults pass what they are given on.
#[test]
fn the_hooks_pass_on_by_default() {
    assert!(!().wp_navigation_should_create_fallback(false));
    assert!(().wp_navigation_should_create_fallback(true));
}

/// Mods without locations, or locations without `primary`, leave the newest;
/// and a site that writes nothing takes the post as given.
#[test]
fn mods_without_a_primary_location_pick_the_newest() {
    for mods in ["a:0:{}", "a:1:{s:18:\"nav_menu_locations\";a:0:{}}", "i:3;"] {
        let mut site = two_menus();
        site.options = vec![
            ("stylesheet", "twentytwentyfive".into()),
            ("theme_mods_twentytwentyfive", mods.into()),
        ];
        assert_eq!(made(&site).name, b"newer-menu", "{mods}");
    }
    let installed = crate::fixture::DefaultInstall { home: Vec::new() };
    installed.insert_navigation_fallback(b"Title", b"slug", b"");
}
