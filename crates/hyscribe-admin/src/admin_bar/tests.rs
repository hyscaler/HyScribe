// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;
use crate::menu::link;
use crate::tests::{Role, options};

const SITE: &str = "http://127.0.0.1:8081";

impl BarSite for Role {
    fn home_url(&self) -> String {
        format!("{SITE}/")
    }
    fn admin_url(&self, path: &str) -> String {
        format!("{SITE}/hs-admin/{}", link(path))
    }
    fn blogname(&self) -> String {
        String::from_utf8_lossy(&options()["blogname"]).into_owned()
    }
    fn logout_url(&self) -> String {
        format!("{SITE}/hs-login?action=logout&amp;_wpnonce=%nonce:log-out%")
    }
    fn names(&self) -> (String, String) {
        ("admin".into(), "admin".into())
    }
    fn avatar(&self, size: i64) -> String {
        let user = hyscribe_model::User {
            display_name: b"admin".to_vec(),
            login: b"admin".to_vec(),
            email: b"admin@default-install.test".to_vec(),
            ..hyscribe_model::User::default()
        };
        let avatar =
            hyscribe_render::user_avatar(&|name| options().get(name).cloned(), &user, size);
        String::from_utf8(avatar).unwrap()
    }
}

/// PHP's admin bar under hs-admin's URLs.
fn hs_admin(bar: &str) -> String {
    let mut out = String::new();
    let mut rest = bar;
    while let Some(at) = rest.find("/wp-admin/") {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        out.push_str("/hs-admin/");
        let tail = tail.get(10..).unwrap();
        let end = tail.find(['\'', '?', '"']).unwrap_or(tail.len());
        let (file, tail) = tail.split_at(end);
        out.push_str(file.strip_suffix(".php").unwrap_or(file));
        rest = tail;
    }
    out.push_str(rest);
    out.replace("/wp-login.php", "/hs-login")
}

/// Every case of `reference/admin-bar/golden.sh`.
#[test]
fn the_admin_bar_is_php_s() {
    let golden = include_str!("../../tests/golden/admin-bar.tsv");
    let mut failures = Vec::new();
    for line in golden.lines().filter(|line| !line.ends_with('\t')) {
        let f: Vec<&str> = line.split('\t').collect();
        let role: &'static str = Box::leak(f[0].to_string().into_boxed_str());
        let ours = admin_bar(&Role(role), b"Mozilla/5.0 (X11; Linux x86_64)");
        let php = hs_admin(&String::from_utf8(unhex(f[2])).unwrap());
        if ours != php {
            let at = ours
                .bytes()
                .zip(php.bytes())
                .position(|(a, b)| a != b)
                .unwrap_or(ours.len().min(php.len()));
            let near = |t: &str| {
                t.get(at.saturating_sub(100)..(at + 150).min(t.len()))
                    .unwrap_or_default()
                    .to_string()
            };
            failures.push(format!(
                "{role} {}\n  php:  {:?}\n  ours: {:?}",
                f[1],
                near(&php),
                near(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// A site and user as a test needs them.
struct Fake {
    blogname: &'static str,
    names: (&'static str, &'static str),
    moderated: u64,
    avatar: bool,
}

impl MenuUser for Fake {
    fn can(&self, cap: &str) -> bool {
        matches!(cap, "read" | "edit_posts")
    }
    fn moderated_comments(&self) -> u64 {
        self.moderated
    }
    fn transient(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

impl BarSite for Fake {
    fn home_url(&self) -> String {
        "https://www.example.test/".into()
    }
    fn admin_url(&self, path: &str) -> String {
        format!("https://www.example.test/hs-admin/{}", link(path))
    }
    fn blogname(&self) -> String {
        self.blogname.into()
    }
    fn logout_url(&self) -> String {
        "https://www.example.test/hs-login?action=logout".into()
    }
    fn names(&self) -> (String, String) {
        (self.names.0.into(), self.names.1.into())
    }
    fn avatar(&self, size: i64) -> String {
        if self.avatar {
            format!("<img size='{size}'/>")
        } else {
            String::new()
        }
    }
}

fn fake() -> Fake {
    Fake {
        blogname: "Site",
        names: ("Ann", "ann"),
        moderated: 1,
        avatar: false,
    }
}

/// The user's login shows when it is not their name, one comment is one,
/// and without an avatar the account item has no avatar class.
#[test]
fn names_counts_and_avatars() {
    let bar = admin_bar(&fake(), b"");
    assert!(bar.contains("<span class='display-name'>Ann</span><span class='username'>ann</span>"));
    assert!(bar.contains("1 Comment in moderation"));
    assert!(bar.contains("<li role='group' id='wp-admin-bar-my-account' class=\"menupop\">"));
    let bar = admin_bar(
        &Fake {
            avatar: true,
            ..fake()
        },
        b"",
    );
    assert!(bar.contains("class=\"menupop with-avatar\""));
    assert!(bar.contains("<img size='28'/>") && bar.contains("<img size='64'/>"));
}

/// Apple systems get their own shortcut label.
#[test]
fn the_shortcut_follows_the_system() {
    let bar = admin_bar(&fake(), b"Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)");
    assert!(bar.contains("<kbd>⌘K</kbd>"));
}

/// `wp_html_excerpt()` of the site name, and the home URL without one.
#[test]
fn site_names_are_cut_as_php_cuts_them() {
    let name = |blogname| {
        let bar = admin_bar(&Fake { blogname, ..fake() }, b"");
        let at = bar.find("id='wp-admin-bar-site-name'").unwrap();
        let title = bar.get(at..).unwrap().split("'>").nth(1).unwrap();
        title.split("</a>").next().unwrap().to_string()
    };
    assert_eq!(name(""), "example.test/");
    assert_eq!(name("<b>Bold</b> site"), "Bold site");
    assert_eq!(
        name("A site with a name longer than forty chars"),
        "A site with a name longer than forty cha&hellip;"
    );
    let cut = format!("{}&amp; tail", "a".repeat(36));
    assert_eq!(
        name(Box::leak(cut.into_boxed_str())),
        format!("{}&hellip;", "a".repeat(36))
    );
    let kept = format!("{} &am; tail", "a".repeat(35));
    assert_eq!(
        name(Box::leak(kept.into_boxed_str())),
        format!("{} &am;&hellip;", "a".repeat(35))
    );

    assert_eq!(html_excerpt("ab &c d", 7, "…"), "ab &c d");
    assert_eq!(html_excerpt("ab &cdefghij", 9, "…"), "ab…");
    assert_eq!(html_excerpt("ab &cdefghij", 11, "…"), "ab &cdefghi…");
    assert_eq!(html_excerpt("ab &cdefghZZ", 10, "…"), "ab…");
}

/// A node under a parent that is not there is left out, as `_bind()`
/// leaves it.
#[test]
fn orphans_are_left_out() {
    let mut bar = Bar::default();
    bar.add(
        "lost",
        "nowhere",
        String::new(),
        String::new(),
        Meta::default(),
    );
    let root = bar.bind();
    assert!(
        bar.nodes
            .get(root)
            .is_some_and(|root| root.children.is_empty())
    );
}
