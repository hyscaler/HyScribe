// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::tests::{Role, SITE, first_difference};

/// Every page of `reference/users-list/golden.sh`; a refusal is `403`.
#[test]
fn pages_are_php_s() {
    let golden = include_str!("../../tests/golden/users-list.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (variant, query) = f[0].split_once('?').unwrap_or((f[0], ""));
        let variant = match variant {
            "paged" => "upaged",
            "hidecols" => "uhidden",
            other => other,
        };
        let role: &'static str = Box::leak(format!("{variant}:{}", f[1]).into_boxed_str());
        let path = "/hs-admin/users";
        let ours = users(
            &Role(role),
            &format!("{SITE}{path}"),
            SITE,
            path,
            &crate::admin_bar::Browser {
                user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
                mobile_hint: None,
            },
        );
        let recorded = String::from_utf8(unhex(f[2])).unwrap();
        let (php, ours) = match ours {
            Answer::Page(page) => (hs_admin_page(&recorded), page),
            Answer::Denied(message) => {
                let page = crate::die::page(true, "WordPress &rsaquo; Error", message);
                (
                    recorded,
                    format!("403\t{}", String::from_utf8(page).unwrap()),
                )
            }
            Answer::Redirect(to) => (hs_admin_page(&recorded), format!("302\t{to}")),
        };
        if ours != php {
            failures.push(format!("{role}?{query}: {}", first_difference(&php, &ours)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// One who may list users but not change them sees no links to do so.
#[test]
fn listing_alone_offers_no_changes() {
    let Answer::Page(page) = users(
        &Role("users:list_users"),
        &format!("{SITE}/hs-admin/users"),
        SITE,
        "/hs-admin/users",
        &crate::admin_bar::Browser::default(),
    ) else {
        panic!("no page");
    };
    assert!(!page.contains("id=\"new_role\"") && !page.contains(">Add User</a>"));
    assert!(!page.contains("Send password reset") && !page.contains("user-edit"));
    assert!(page.contains("<strong>editor1</strong><br />"));
}

/// Roles need a name, and a capability among a user's roles is no role.
#[test]
fn roles_are_named() {
    let roles = role_names(
        b"a:3:{s:3:\"one\";a:1:{s:4:\"name\";s:3:\"One\";}s:3:\"two\";a:0:{}s:5:\"three\";a:1:{s:4:\"name\";i:3;}}",
    );
    assert_eq!(roles, [("one".to_string(), "One".to_string())]);
    let list = role_list(b"a:2:{s:3:\"one\";b:1;s:10:\"edit_posts\";b:1;}", &roles);
    assert_eq!(list, [("one".to_string(), "One".to_string())]);
}
