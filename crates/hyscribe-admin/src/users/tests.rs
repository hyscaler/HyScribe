// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{addslashes, remove_query_arg, urldecode};
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
        let arg = |name: &str| {
            query.split('&').find_map(|pair| {
                let value = pair.strip_prefix(name)?.strip_prefix('=')?;
                Some(String::from_utf8(urldecode(value.as_bytes())).unwrap())
            })
        };
        let args = UserArgs {
            role: arg("role"),
            search: arg("s"),
            orderby: arg("orderby"),
            order: arg("order"),
            paged: arg("paged"),
            clauses: user_order(arg("orderby").as_deref(), arg("order").as_deref(), false).unwrap(),
            ..UserArgs::default()
        };
        // The request's path and query, slashed as `wp_magic_quotes()` does.
        let path = if query.is_empty() {
            "/hs-admin/users".to_string()
        } else {
            format!("/hs-admin/users?{query}")
        };
        let path = String::from_utf8(addslashes(path.as_bytes())).unwrap();
        let ours = users(
            &Role(role),
            &args,
            &String::from_utf8(remove_query_arg(
                &[b"message"],
                format!("{SITE}{path}").as_bytes(),
            ))
            .unwrap(),
            SITE,
            &path,
            &crate::admin_bar::Browser {
                user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
                mobile_hint: None,
            },
        );
        let recorded = String::from_utf8(unhex(f[2])).unwrap();
        let (php, ours) = match ours {
            Answer::Page(page) => (hs_admin_page(&recorded), page),
            Answer::Denied(message) | Answer::Died(message) | Answer::Missing(message) => {
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
        &UserArgs::default(),
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

/// The role filter's meta clauses are named only when roles are asked for,
/// and their order is not ported.
#[test]
fn role_clause_orders_are_declined() {
    for alias in ["wp_usermeta", "mt1", "mt12"] {
        assert_eq!(user_order(Some(alias), None, true), None, "{alias}");
        assert_eq!(
            user_order(Some(alias), None, false),
            Some(vec![("user_login", false)])
        );
    }
    assert_eq!(
        user_order(Some("mt"), None, true),
        Some(vec![("user_login", false)])
    );
    assert_eq!(
        user_order(Some("mtx"), None, true),
        Some(vec![("user_login", false)])
    );
    assert_eq!(user_order(Some("login,meta_value"), None, false), None);
}

/// A link searches its URL, an address its email, a number login and ID.
#[test]
fn searches_pick_their_columns() {
    assert_eq!(search_columns("https://a"), ["user_url"]);
    assert_eq!(search_columns("http://a"), ["user_url"]);
    assert_eq!(search_columns("a@b"), ["user_email"]);
    assert_eq!(search_columns(" 1.5"), ["user_login", "ID"]);
}

/// A role asked for twice is filtered by once.
#[test]
fn roles_are_asked_for_once() {
    let args = UserArgs {
        role: Some("editor, editor".into()),
        clauses: vec![("user_login", false)],
        ..UserArgs::default()
    };
    let Answer::Page(page) = users(
        &Role("users:administrator"),
        &args,
        &format!("{SITE}/hs-admin/users"),
        SITE,
        "/hs-admin/users",
        &crate::admin_bar::Browser::default(),
    ) else {
        panic!("no page");
    };
    assert!(page.contains("id='user-601'") && !page.contains("id='user-602'"));
}
