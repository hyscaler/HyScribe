// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;
use std::sync::OnceLock;

use hyscribe_auth::caps::{CapPost, CapabilityData, User};
use hyscribe_testkit::{SqlValue, unhex};

use super::*;

/// The default install's options.
fn options() -> &'static HashMap<String, Vec<u8>> {
    static OPTIONS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let version = hyscribe_testkit::default_install_versions().pop().unwrap();
        let dump = hyscribe_testkit::default_install(&version).unwrap();
        dump.table("wp_options")
            .filter_map(|row| match (&row[1], &row[2]) {
                (SqlValue::Text(name), SqlValue::Text(value)) => {
                    Some((String::from_utf8_lossy(name).into_owned(), value.clone()))
                }
                _ => None,
            })
            .collect()
    })
}

/// User 1 with a role's capabilities, on HyScribe's constants, as
/// `reference/admin-menu/as-role.php` sets PHP up.
struct Role(&'static str);

impl CapabilityData for Role {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        options().get(name).cloned()
    }
    fn user_meta(&self, _: u64, key: &str) -> Option<Vec<u8>> {
        (key == "wp_capabilities")
            .then(|| format!("a:1:{{s:{}:\"{}\";b:1;}}", self.0.len(), self.0).into_bytes())
    }
    fn user_exists(&self, id: u64) -> bool {
        id == 1
    }
    fn post(&self, _: u64) -> Option<CapPost> {
        None
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn comment_post(&self, _: u64) -> Option<u64> {
        None
    }
    fn term_taxonomy(&self, _: u64) -> Option<String> {
        None
    }
    fn constant(&self, name: &str) -> bool {
        name == "DISALLOW_UNFILTERED_HTML"
    }
}

impl MenuUser for Role {
    fn can(&self, cap: &str) -> bool {
        User::load(self, "wp_", 1).has_cap(self, cap, &[])
    }
    fn moderated_comments(&self) -> u64 {
        0
    }
    fn transient(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

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
        parent_file,
        submenu_file,
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
        let ours = admin_menu(&user, &screen).filter(|_| allowed);
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
}

fn dashboard() -> Screen<'static> {
    Screen {
        pagenow: "index.php",
        typenow: "",
        parent_file: "index.php",
        submenu_file: None,
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
    )
    .unwrap();
    assert!(menu.contains("count-1\"><span class=\"pending-count\" aria-hidden=\"true\">1</span><span class=\"comments-in-moderation-text screen-reader-text\">1 Comment in moderation"));
    assert!(menu.contains("Site Health <span class=\"menu-counter site-health-counter count-3\"><span class=\"count\">3</span></span>"));
    let menu = admin_menu(&Caps(caps, 123, None), &dashboard()).unwrap();
    assert!(menu.contains("aria-hidden=\"true\">123</span>"));
    for transient in [None, Some("[]"), Some("not json")] {
        let menu = admin_menu(&Caps(caps, 1234, transient), &dashboard()).unwrap();
        assert!(menu.contains("aria-hidden=\"true\">1,234</span>"));
        assert!(menu.contains("site-health-counter count-0"));
    }
}

/// A role that may add users but not list them gets them under Profile,
/// and one that may list them but not add them has no Add User.
#[test]
fn adding_users_without_listing_them() {
    let menu = admin_menu(&Caps(&["read", "list_users"], 0, None), &dashboard()).unwrap();
    assert!(menu.contains("All Users</a></li><li><a href='profile'>Profile</a></li>"));
    let menu = admin_menu(&Caps(&["read", "create_users"], 0, None), &dashboard()).unwrap();
    assert!(menu.contains("<li class=\"wp-first-item\"><a href='profile' class=\"wp-first-item\">Profile</a></li><li><a href='user-new'>Add User</a></li>"));
}

/// A page the user may not reach under its parent is turned away, even
/// when the parent also lists it.
#[test]
fn a_denied_page_under_its_parent() {
    let mut menu = build(&Caps(&["read", "edit_posts"], 0, None));
    menu.submenu_nopriv
        .push(("tools.php".into(), "tools.php".into()));
    let screen = Screen {
        pagenow: "tools.php",
        typenow: "",
        parent_file: "tools.php",
        submenu_file: None,
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
        parent_file,
        submenu_file: None,
    }
}

/// `get_admin_page_parent()` finds the parent a screen does not give, by
/// its file or its post type, and leaves one with a query alone.
#[test]
fn the_parent_is_found_as_php_finds_it() {
    let admin = Role("administrator");
    let current = |screen: &Screen| {
        let menu = admin_menu(&admin, screen).unwrap();
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
    let menu = admin_menu(&user, &dashboard()).unwrap();
    let tools = menu
        .split("<li")
        .find(|item| item.contains("menu-icon-tools"))
        .unwrap();
    assert!(tools.contains("<a href='import'"), "{tools}");
    let mut parent = String::new();
    let built = {
        let mut menu = build(&user);
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
            (1, item("Read", "read", "index.php", "x", "", "")),
            (2, item("Things", "nope", "x.php", "x", "", "")),
        ],
        submenu: Submenu(vec![(
            "x.php".into(),
            vec![sub("All", "read", "x.php?post_type=thing")],
        )]),
        menu_nopriv: Vec::new(),
        submenu_nopriv: Vec::new(),
    };
    assert!(!can_access(
        &menu,
        &user,
        &screen_of("x.php", "thing", ""),
        &mut String::new()
    ));
}
