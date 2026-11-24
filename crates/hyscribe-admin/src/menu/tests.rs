// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;
use crate::tests::Role;

/// A recorded screen: its globals, and the capabilities its own file asks
/// for (any of them), before the menu's check.
fn screen(user: &Role, file: &str) -> (Screen<'static>, Vec<&'static str>) {
    let users = if user.can("list_users") {
        "users.php"
    } else {
        "profile.php"
    };
    let edit_users = if user.can("edit_users") {
        "users.php"
    } else {
        "profile.php"
    };
    let screen = |pagenow, typenow, parent_file, submenu_file| Screen {
        pagenow,
        typenow,
        taxonomy: "",
        parent_file: Some(parent_file).filter(|p: &&str| !p.is_empty()),
        submenu_file,
        plugin_page: None,
    };
    match file {
        "" | "index.php" => (screen("index.php", "", "index.php", None), vec![]),
        "edit.php" => (
            screen("edit.php", "post", "edit.php", Some("edit.php")),
            vec!["edit_posts"],
        ),
        "post-new.php" => (
            screen("post-new.php", "post", "edit.php", Some("post-new.php")),
            vec!["edit_posts"],
        ),
        "edit.php?post_type=page" => (
            screen(
                "edit.php",
                "page",
                "edit.php?post_type=page",
                Some("edit.php?post_type=page"),
            ),
            vec!["edit_pages"],
        ),
        "post-new.php?post_type=page" => (
            screen(
                "post-new.php",
                "page",
                "edit.php?post_type=page",
                Some("post-new.php?post_type=page"),
            ),
            vec!["edit_pages"],
        ),
        "edit-tags.php?taxonomy=category" => (
            screen(
                "edit-tags.php",
                "post",
                "edit.php",
                Some("edit-tags.php?taxonomy=category"),
            ),
            vec!["manage_categories"],
        ),
        "edit-tags.php?taxonomy=post_tag" => (
            screen(
                "edit-tags.php",
                "post",
                "edit.php",
                Some("edit-tags.php?taxonomy=post_tag"),
            ),
            vec!["manage_post_tags"],
        ),
        "upload.php" => (
            screen("upload.php", "attachment", "upload.php", None),
            vec!["upload_files"],
        ),
        "media-new.php" => (
            screen("media-new.php", "", "upload.php", None),
            vec!["upload_files"],
        ),
        "edit-comments.php" => (
            screen("edit-comments.php", "", "", None),
            vec!["edit_posts"],
        ),
        "themes.php" => (
            screen("themes.php", "", "themes.php", None),
            vec!["switch_themes", "edit_theme_options"],
        ),
        "site-editor.php" => (
            screen("site-editor.php", "", "themes.php", None),
            vec!["edit_theme_options"],
        ),
        "users.php" => (
            screen("users.php", "", "users.php", None),
            vec!["list_users"],
        ),
        "user-new.php" => (
            screen("user-new.php", "", users, None),
            vec!["create_users"],
        ),
        "profile.php" => (
            screen("profile.php", "", edit_users, Some("profile.php")),
            vec![],
        ),
        "tools.php" => (screen("tools.php", "", "tools.php", None), vec![]),
        "import.php" => (screen("import.php", "", "tools.php", None), vec!["import"]),
        "export.php" => (screen("export.php", "", "tools.php", None), vec!["export"]),
        "site-health.php" => (
            screen("site-health.php", "", "tools.php", None),
            vec!["view_site_health_checks"],
        ),
        "options-privacy.php" => (
            screen("options-privacy.php", "", "options-general.php", None),
            vec!["manage_privacy_options"],
        ),
        "plugins.php" => (
            screen("plugins.php", "", "plugins.php", None),
            vec!["activate_plugins"],
        ),
        "update-core.php" => (
            screen("update-core.php", "", "index.php", None),
            vec![
                "update_core",
                "update_plugins",
                "update_themes",
                "update_languages",
            ],
        ),
        options => {
            let pagenow: &'static str = Box::leak(options.to_string().into_boxed_str());
            (
                screen(pagenow, "", "options-general.php", None),
                vec!["manage_options"],
            )
        }
    }
}

/// PHP's menu under hs-admin's links, which drop `.php`.
fn hs_admin(menu: &str) -> String {
    let mut out = String::new();
    let mut rest = menu;
    while let Some(at) = rest.find("href='") {
        let (head, tail) = rest.split_at(at + 6);
        out.push_str(head);
        let end = tail.find(['\'', '?']).unwrap_or(tail.len());
        let (file, tail) = tail.split_at(end);
        out.push_str(file.strip_suffix(".php").unwrap_or(file));
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// Every case of `reference/admin-menu/golden.sh`.
#[test]
fn the_admin_menu_is_php_s() {
    let golden = include_str!("../../tests/golden/menu.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let role: &'static str = Box::leak(f[0].to_string().into_boxed_str());
        let user = Role(role);
        let (screen, caps) = screen(&user, f[1]);
        let allowed = caps.is_empty() || caps.iter().any(|cap| user.can(cap));
        let ours = admin_menu(&user, &screen, Look::Block).filter(|_| allowed);
        let php = String::from_utf8(unhex(f[3])).unwrap();
        match (f[2], ours) {
            ("403", None) => {}
            ("200", Some(ours)) if ours == hs_admin(&php) => {}
            (status, ours) => {
                let ours = ours.unwrap_or_default();
                let expected = hs_admin(&php);
                let at = ours
                    .bytes()
                    .zip(expected.bytes())
                    .position(|(a, b)| a != b)
                    .unwrap_or(ours.len().min(expected.len()));
                let near = |t: &str| {
                    t.get(at.saturating_sub(100)..(at + 150).min(t.len()))
                        .unwrap_or_default()
                        .to_string()
                };
                failures.push(format!(
                    "{role} {} {status}\n  php:  {:?}\n  ours: {:?}",
                    f[1],
                    near(&expected),
                    near(&ours)
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        golden.lines().count(),
        failures
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// A user with just these capabilities, these comments in moderation and
/// this cached Site Health result.
struct Caps(&'static [&'static str], u64, Option<&'static str>);

impl MenuUser for Caps {
    fn can(&self, cap: &str) -> bool {
        self.0.contains(&cap)
    }
    fn moderated_comments(&self) -> u64 {
        self.1
    }
    fn transient(&self, name: &str) -> Option<Vec<u8>> {
        assert_eq!(name, "health-check-site-status-result");
        self.2.map(|json| json.as_bytes().to_vec())
    }
    fn plugin_pages(&self) -> Vec<crate::menu::PluginPage> {
        Vec::new()
    }
}

fn dashboard() -> Screen<'static> {
    Screen {
        pagenow: "index.php",
        typenow: "",
        taxonomy: "",
        parent_file: Some("index.php"),
        submenu_file: None,
        plugin_page: None,
    }
}

/// The counts are PHP's: one comment, thousands, and the critical issues
/// of the cached Site Health check, or none when it is missing or empty.
#[test]
fn counts_read_as_php_writes_them() {
    let caps: &[&str] = &["read", "edit_posts", "view_site_health_checks"];
    let menu = admin_menu(
        &Caps(caps, 1, Some(r#"{"good":5,"recommended":2,"critical":3}"#)),
        &dashboard(),
        Look::Block,
    )
    .unwrap();
    assert!(menu.contains("count-1\"><span class=\"pending-count\" aria-hidden=\"true\">1</span><span class=\"comments-in-moderation-text screen-reader-text\">1 Comment in moderation"));
    assert!(menu.contains("Site Health <span class=\"menu-counter site-health-counter count-3\"><span class=\"count\">3</span></span>"));
    let menu = admin_menu(&Caps(caps, 123, None), &dashboard(), Look::Block).unwrap();
    assert!(menu.contains("aria-hidden=\"true\">123</span>"));
    for transient in [None, Some("[]"), Some("not json")] {
        let menu = admin_menu(&Caps(caps, 1234, transient), &dashboard(), Look::Block).unwrap();
        assert!(menu.contains("aria-hidden=\"true\">1,234</span>"));
        assert!(menu.contains("site-health-counter count-0"));
    }
}

/// The cached critical count prints as PHP prints it, rounded for its
/// number and escaped where PHP leaves it raw.
#[test]
fn odd_site_health_counts() {
    let caps: &[&str] = &["read", "edit_posts", "view_site_health_checks"];
    for (json, class, number) in [
        (r#"{"critical":-3}"#, "-3", "-3"),
        (r#"{"critical":2.7}"#, "2.7", "3"),
        (r#"{"good":1}"#, "", "0"),
        (r#"{"critical":"1\"><b>"}"#, "1&quot;&gt;&lt;b&gt;", "1"),
    ] {
        let menu = admin_menu(&Caps(caps, 0, Some(json)), &dashboard(), Look::Block).unwrap();
        let expected =
            format!("site-health-counter count-{class}\"><span class=\"count\">{number}</span>");
        let at = menu.find("site-health-counter").unwrap();
        assert!(
            menu.contains(&expected),
            "{json}: {}",
            menu.get(at..at + 120).unwrap()
        );
    }
}

/// A role that may add users but not list them gets them under Profile,
/// and one that may list them but not add them has no Add User.
#[test]
fn adding_users_without_listing_them() {
    let menu = admin_menu(
        &Caps(&["read", "list_users"], 0, None),
        &dashboard(),
        Look::Block,
    )
    .unwrap();
    assert!(menu.contains("All Users</a></li><li><a href='profile'>Profile</a></li>"));
    let menu = admin_menu(
        &Caps(&["read", "create_users"], 0, None),
        &dashboard(),
        Look::Block,
    )
    .unwrap();
    assert!(menu.contains("<li class=\"wp-first-item\"><a href='profile' class=\"wp-first-item\">Profile</a></li><li><a href='user-new'>Add User</a></li>"));
}

/// A page the user may not reach under its parent is turned away, even
/// when the parent also lists it.
#[test]
fn a_denied_page_under_its_parent() {
    let mut menu = build(&Caps(&["read", "edit_posts"], 0, None), Look::Block);
    menu.submenu_nopriv
        .push(("tools.php".into(), "tools.php".into()));
    let screen = Screen {
        pagenow: "tools.php",
        typenow: "",
        taxonomy: "",
        parent_file: Some("tools.php"),
        submenu_file: None,
        plugin_page: None,
    };
    let user = Caps(&["read", "edit_posts"], 0, None);
    assert!(!can_access(&menu, &user, &screen, &mut String::new()));
}

fn screen_of(
    pagenow: &'static str,
    typenow: &'static str,
    parent_file: &'static str,
) -> Screen<'static> {
    Screen {
        pagenow,
        typenow,
        taxonomy: "",
        parent_file: Some(parent_file).filter(|p| !p.is_empty()),
        submenu_file: None,
        plugin_page: None,
    }
}

/// `get_admin_page_parent()` finds the parent a screen does not give, by
/// its file or its post type, and leaves one with a query alone.
#[test]
fn the_parent_is_found_as_php_finds_it() {
    let admin = Role("administrator");
    let current = |screen: &Screen| {
        let menu = admin_menu(&admin, screen, Look::Block).unwrap();
        let at = menu.find("wp-has-current-submenu").unwrap();
        menu.get(at..)
            .unwrap()
            .split("id=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_string()
    };
    assert_eq!(current(&screen_of("media-new.php", "", "")), "menu-media");
    assert_eq!(
        current(&screen_of("media-new.php", "", "index.php")),
        "menu-media"
    );
    assert_eq!(
        current(&screen_of("post-new.php", "page", "")),
        "menu-pages"
    );
    // A parent with a query stays, whatever else the file matches.
    let menu = admin_menu(
        &admin,
        &screen_of("edit.php", "", "edit.php?post_type=page"),
        Look::Block,
    )
    .unwrap();
    assert!(menu.contains(
        "wp-has-current-submenu wp-menu-open menu-top menu-icon-page\" id=\"menu-pages\""
    ));
}

/// A menu the user may not open, with submenu items they may, becomes
/// the first of them: Tools for someone who may import and export.
#[test]
fn a_menu_moves_to_its_first_item_the_user_may_reach() {
    let user = Caps(&["read", "import", "export"], 0, None);
    let menu = admin_menu(&user, &dashboard(), Look::Block).unwrap();
    let tools = menu
        .split("<li")
        .find(|item| item.contains("menu-icon-tools"))
        .unwrap();
    assert!(tools.contains("<a href='import'"), "{tools}");
    let mut parent = String::new();
    let built = {
        let mut menu = build(&user, Look::Block);
        prune(&mut menu, &user);
        menu
    };
    assert!(can_access(
        &built,
        &user,
        &screen_of("import.php", "", ""),
        &mut parent
    ));
    assert!(!can_access(
        &built,
        &user,
        &screen_of("theme-editor.php", "", ""),
        &mut String::new()
    ));
}

/// A screen of a post type whose list is in the menu goes by the menu
/// item's capability.
#[test]
fn a_typed_screen_needs_its_menu_item() {
    let user = Caps(&["read"], 0, None);
    let menu = Menu {
        items: vec![
            (1.0, item("Read", "read", "index.php", "x", "", "")),
            (2.0, item("Things", "nope", "x.php", "x", "", "")),
        ],
        submenu: Submenu(vec![(
            "x.php".into(),
            vec![sub("All", "read", "x.php?post_type=thing")],
        )]),
        ..Menu::default()
    };
    assert!(!can_access(
        &menu,
        &user,
        &screen_of("x.php", "thing", ""),
        &mut String::new()
    ));
}

/// Text inside a tag is not the title's, even when the tag never closes.
#[test]
fn root_text_leaves_tags_out() {
    assert_eq!(root_text("A <b>x</b> B "), "A  B");
    assert_eq!(root_text("a <b>c"), "a");
}

/// Links to hs-admin's screens lose `.php` in every form a page prints
/// them; other `.php` names and paths are kept.
#[test]
fn links_name_screens_without_php() {
    for (html, clean) in [
        (
            "<a href=\"/hs-admin/edit.php?post_type=page\">",
            "<a href=\"/hs-admin/edit?post_type=page\">",
        ),
        (
            "<a href='/hs-admin/users.php'>",
            "<a href='/hs-admin/users'>",
        ),
        (
            "/hs-admin/post.php#x /hs-admin/index.php",
            "/hs-admin/post#x /hs-admin/index",
        ),
        (
            "{\"url\":\"http:\\/\\/x\\/hs-admin\\/async-upload.php\"}",
            "{\"url\":\"http:\\/\\/x\\/hs-admin\\/async-upload\"}",
        ),
        (
            "redirect_to=http%3A%2F%2Fx%2Fhs-admin%2Fedit.php%3Fpost_type%3Dpage&amp;a=1",
            "redirect_to=http%3A%2F%2Fx%2Fhs-admin%2Fedit%3Fpost_type%3Dpage&amp;a=1",
        ),
        (
            "<form action=\"options.php\" method=\"post\">",
            "<form action=\"options\" method=\"post\">",
        ),
        ("<form action='comment.php'>", "<form action='comment'>"),
        (
            "<a href=\"edit-tags.php?taxonomy=category\">",
            "<a href=\"edit-tags?taxonomy=category\">",
        ),
        ("<a href='user-edit.php'>", "<a href='user-edit'>"),
        // Not a screen: other folders, longer names, another extension.
        (
            "/hs-admin/css/x.php /hs-admin/.php",
            "/hs-admin/css/x.php /hs-admin/.php",
        ),
        (
            "/hs-admin/a.phpx /hs-admin/a.php_ /hs-admin/a.php-b",
            "/hs-admin/a.phpx /hs-admin/a.php_ /hs-admin/a.php-b",
        ),
        (
            "/hs-admin/a.php.bak /hs-admin/a.php/b",
            "/hs-admin/a.php.bak /hs-admin/a.php/b",
        ),
        (
            "href=\"http://x/wp-comments-post.php\" action=\"/wp-cron.php\"",
            "href=\"http://x/wp-comments-post.php\" action=\"/wp-cron.php\"",
        ),
        (
            "/wp-admin/edit.php hs-admin/edit.php",
            "/wp-admin/edit.php hs-admin/edit.php",
        ),
        ("é/hs-admin/edit.phpé", "é/hs-admin/edité"),
        ("/hs-admin/edit.php", "/hs-admin/edit"),
    ] {
        assert_eq!(clean_links(html), clean, "{html}");
    }
}

/// A user with these capabilities and the active extensions' pages.
struct Paged(&'static [&'static str], Vec<crate::menu::PluginPage>);

impl MenuUser for Paged {
    fn can(&self, cap: &str) -> bool {
        self.0.contains(&cap)
    }
    fn moderated_comments(&self) -> u64 {
        0
    }
    fn transient(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn plugin_pages(&self) -> Vec<crate::menu::PluginPage> {
        self.1.clone()
    }
}

fn plugin(
    parent: Option<&str>,
    cap: &str,
    slug: &str,
    position: Option<i32>,
) -> crate::menu::PluginPage {
    crate::menu::PluginPage {
        parent: parent.map(String::from),
        page_title: slug.into(),
        menu_title: slug.into(),
        capability: cap.into(),
        slug: slug.into(),
        icon: String::new(),
        position,
    }
}

fn asked(pagenow: &'static str, page: &'static str) -> Screen<'static> {
    Screen {
        pagenow,
        typenow: "",
        taxonomy: "",
        parent_file: None,
        submenu_file: None,
        plugin_page: Some(page),
    }
}

/// Extensions' pages go where `add_menu_page()` and `add_submenu_page()`
/// put them: a taken position moved a little past it, a free one kept,
/// first in a submenu, under a menu that moved; denied ones left out and
/// refused, as `user_can_access_admin_page()` refuses them.
#[test]
fn extension_pages_take_php_s_places() {
    let user = Paged(
        &["read", "import", "export", "manage_options"],
        vec![
            plugin(None, "read", "ext-a", Some(80)),
            crate::menu::PluginPage {
                icon: "dashicons-x".into(),
                ..plugin(None, "read", "ext-b", Some(3))
            },
            plugin(None, "nope", "ext-locked", None),
            plugin(Some("ext-locked"), "read", "ext-open", None),
            plugin(None, "nope", "ext-hidden", None),
            plugin(Some("options-general.php"), "read", "ext-first", Some(0)),
            plugin(Some("options-general.php"), "nope", "ext-denied", None),
            plugin(Some("tools.php"), "read", "ext-tool", None),
            // Taken places, each moved by its slug's and title's hash.
            plugin(None, "read", "ext-g", Some(80)),
            plugin(None, "read", "ext-f", Some(3)),
            plugin(None, "read", "ext-e", None),
            plugin(None, "read", "ext-h", Some(101)),
        ],
    );
    let menu = admin_menu(&user, &dashboard(), Look::Block).unwrap();
    let at = |text: &str| menu.find(text).unwrap_or_else(|| panic!("{text}"));
    assert!(at("menu-dashboard") < at("toplevel_page_ext-b"));
    assert!(at("toplevel_page_ext-b") < at("wp-menu-separator"));
    assert!(at("menu-settings") < at("toplevel_page_ext-a"));
    // 80.07212 and 80.25761, 3 and 3.27759, then 100, 101.02914 and 102.
    assert!(at("toplevel_page_ext-g") < at("toplevel_page_ext-a"));
    assert!(at("toplevel_page_ext-b") < at("toplevel_page_ext-f"));
    assert!(at("toplevel_page_ext-a") < at("toplevel_page_ext-locked"));
    assert!(at("toplevel_page_ext-locked") < at("toplevel_page_ext-h"));
    assert!(at("toplevel_page_ext-h") < at("toplevel_page_ext-e"));
    assert!(at("\n\t<a href='admin?page=ext-a'") > 0);
    assert!(!menu.contains("ext-hidden") && !menu.contains("ext-denied"));
    assert!(at("dashicons-before dashicons-x") > at("toplevel_page_ext-b"));
    // Under a menu the user may not open, its first page is not copied.
    assert!(at("admin?page=ext-open") > at("toplevel_page_ext-locked"));
    assert!(!menu.contains("admin?page=ext-locked"));
    assert!(at("<a href='admin?page=ext-first' class=\"wp-has-submenu") > 0);
    assert!(at("<li class=\"wp-first-item\"><a href='options-general?page=ext-first'") > 0);
    assert!(at("<a href='import?page=ext-tool'>ext-tool</a>") > 0);
    let current = admin_menu(
        &user,
        &asked("options-general.php", "ext-first"),
        Look::Block,
    );
    assert!(current.unwrap().contains(
        "<li class=\"wp-first-item current\"><a href='options-general?page=ext-first' class=\"wp-first-item current\" aria-current=\"page\">"
    ));
    for (pagenow, page) in [
        ("admin.php", "ext-hidden"),
        ("options-general.php", "ext-denied"),
        ("admin.php", "ext-unknown"),
    ] {
        assert!(
            admin_menu(&user, &asked(pagenow, page), Look::Block).is_none(),
            "{page}"
        );
    }
    assert!(admin_menu(&user, &asked("admin.php", "ext-b"), Look::Block).is_some());
    assert_eq!(
        plugin_page_hook(&user, "options-general.php", "ext-first").as_deref(),
        Some("settings_page_ext-first")
    );
    assert_eq!(plugin_page_hook(&user, "tools.php", "ext-first"), None);
    let commands = commands(&user, Look::Block, &|path| format!("/x/{path}"));
    let url = |name: &str| {
        let found = commands.iter().find(|(_, _, named)| named == name);
        found.map(|(_, url, _)| url.as_str())
    };
    assert_eq!(url("ext-b"), Some("/x/admin?page=ext-b"));
    assert_eq!(
        url("options-general.php-ext-first"),
        Some("/x/options-general?page=ext-first")
    );
}
