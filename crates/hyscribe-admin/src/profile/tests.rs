// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{addslashes, parse_str};
use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::tests::{Role, SITE, first_difference};

const BROWSER: Browser = Browser {
    user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
    mobile_hint: None,
};

/// Every page of `reference/profile/golden.sh`.
#[test]
fn pages_are_php_s() {
    let golden = include_str!("../../tests/golden/profile.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let variant = match f[0] {
            "default" | "noavatars" => f[0].to_string(),
            other => format!("p{other}"),
        };
        let role: &'static str = Box::leak(format!("{variant}:{}", f[1]).into_boxed_str());
        let path = "/hs-admin/profile";
        let request = Request {
            canonical: &format!("{SITE}{path}"),
            origin: SITE,
            uri: path,
            browser: &BROWSER,
        };
        let ours = profile(&Role(role), &request);
        let recorded = String::from_utf8(unhex(f[2])).unwrap();
        let Answer::Page(ours) = ours else {
            panic!("{role}: no page");
        };
        let php = hs_admin_page(&recorded);
        if ours != php {
            failures.push(format!("{role}: {}", first_difference(&php, &ours)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every page of `reference/user-edit/golden.sh`: a refusal is its status
/// and `wp_die()`'s page.
#[test]
fn edit_and_add_pages_are_php_s() {
    let golden = include_str!("../../tests/golden/user-edit.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let role: &'static str = Box::leak(format!("{}:{}", f[0], f[2]).into_boxed_str());
        // The request under hs-admin's URLs, slashed as `wp_magic_quotes()`
        // does.
        let path = hs_admin_page(&format!("/wp-admin/{}", f[1]));
        let path = String::from_utf8(addslashes(path.as_bytes())).unwrap();
        let canonical = remove_query_arg(&[b"message"], format!("{SITE}{path}").as_bytes());
        let request = Request {
            canonical: &String::from_utf8(canonical).unwrap(),
            origin: SITE,
            uri: &path,
            browser: &BROWSER,
        };
        let ours = if f[1].starts_with("user-new.php") {
            crate::user_new::user_new(&Role(role), &request)
        } else {
            let query = hs_admin_page(f[1].split_once('?').map_or("", |(_, query)| query));
            let mut args = EditArgs::default();
            for (key, value) in parse_str(query.as_bytes()).as_array().unwrap() {
                let value = Some(String::from_utf8(value.to_php_string()).unwrap());
                match key.to_php_string().as_slice() {
                    b"user_id" => args.user_id = value,
                    _ => args.referer = value,
                }
            }
            user_edit(&Role(role), &args, &request)
        };
        let recorded = String::from_utf8(unhex(f[3])).unwrap();
        let die = |status: u16, message: &str| {
            let page = crate::die::page(true, "WordPress &rsaquo; Error", message);
            format!("{status}\t{}", String::from_utf8(page).unwrap())
        };
        let (php, ours) = match ours {
            Answer::Page(page) => (hs_admin_page(&recorded), page),
            Answer::Denied(message) => (recorded, die(403, message)),
            Answer::Died(message) => (recorded, die(500, message)),
            Answer::Redirect(to) => (recorded, to),
        };
        if ours != php {
            failures.push(format!(
                "{role} {}: {}",
                f[1],
                first_difference(&php, &ours)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A pending change shows unless it is to the current address, an address
/// missing or of another type shown as PHP casts it.
#[test]
fn pending_changes_show_as_php_s() {
    let site = Role("administrator");
    let current = b"admin@default-install.test";
    let notice = |stored: &[u8]| pending_email(&site, stored, current);
    assert!(
        notice(b"").is_empty() && notice(b"a:0:{}").is_empty() && notice(b"s:1:\"x\";").is_empty()
    );
    assert!(notice(b"a:1:{s:8:\"newemail\";s:26:\"admin@default-install.test\";}").is_empty());
    assert!(notice(b"a:1:{s:4:\"hash\";s:1:\"x\";}").contains("<code></code>"));
    assert!(notice(b"a:1:{s:8:\"newemail\";i:5;}").contains("<code>5</code>"));
}

/// The toolbar shows on the site when unset or `true` only.
#[test]
fn the_toolbar_preference_is_php_s() {
    assert!(admin_bar_pref(None) && admin_bar_pref(Some(b"true")));
    assert!(
        !admin_bar_pref(Some(b""))
            && !admin_bar_pref(Some(b"1"))
            && !admin_bar_pref(Some(b"false"))
    );
}

/// Both orders of the names are offered only with a first and a last name.
#[test]
fn display_names_need_both_names_for_both_orders() {
    let user = User {
        login: b"ann".to_vec(),
        display_name: b"ann".to_vec(),
        first_name: b"Ann".to_vec(),
        ..User::default()
    };
    let options = display_options(&user, "ann");
    assert!(options.contains(">Ann</option>") && !options.contains("Ann </option>"));
    assert_eq!(options.matches("<option").count(), 2);
    let last_only = User {
        first_name: Vec::new(),
        last_name: b"Lee".to_vec(),
        ..user
    };
    let options = display_options(&last_only, "ann");
    assert!(options.contains(">Lee</option>") && !options.contains(">Ann</option>"));
}

/// A display name none of the choices gives comes first, chosen.
#[test]
fn another_display_name_comes_first() {
    let user = User {
        login: b"ann".to_vec(),
        display_name: b"The Ann".to_vec(),
        ..User::default()
    };
    let options = display_options(&user, "ann");
    assert!(
        options
            .trim_start()
            .starts_with("<option  selected='selected'>The Ann</option>")
    );
}

/// Capabilities that do not unserialize to an array list none.
#[test]
fn unreadable_capabilities_list_none() {
    assert!(additional_caps(b"", &[]).is_empty());
    assert!(additional_caps(b"s:1:\"x\";", &[]).is_empty());
}
