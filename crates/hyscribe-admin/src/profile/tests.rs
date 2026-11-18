// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{addslashes, parse_str};
use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::admin_bar::Browser;
use crate::page::REMOVABLE;
use crate::tests::{BROWSER, Role, first_difference, request_at};

const CHROME: &[u8] = b"Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/137.0.0.0 Safari/537.36";
const CHROME136: &[u8] = b"Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36";

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
        // The `localhost` variants ask for the site by that name; the
        // `chrome` ones as Chromium 137 and 136 do.
        let site = if f[0].starts_with("localhost") {
            "http://localhost:8081".to_string()
        } else {
            Role(role).site()
        };
        let canonical = format!("{site}{path}");
        let agent: &[u8] = match f[0] {
            variant if variant.ends_with("chrome136") => CHROME136,
            variant if variant.ends_with("chrome") => CHROME,
            _ => BROWSER.user_agent,
        };
        let browser = Browser {
            user_agent: agent,
            ..BROWSER
        };
        let request = request_at(&site, &canonical, path, &browser);
        let ours = profile(&Role(role), &EditArgs::default(), &request).unwrap();
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
        let site = Role(role).site();
        let canonical = remove_query_arg(&REMOVABLE, format!("{site}{path}").as_bytes());
        let canonical = String::from_utf8(canonical).unwrap();
        let request = request_at(&site, &canonical, &path, &BROWSER);
        let query = hs_admin_page(f[1].split_once('?').map_or("", |(_, query)| query));
        let mut args = EditArgs::default();
        let mut update = None;
        for (key, value) in parse_str(query.as_bytes()).as_array().unwrap() {
            let value = Some(String::from_utf8(value.to_php_string()).unwrap());
            match key.to_php_string().as_slice() {
                b"user_id" => args.user_id = value,
                b"update" => update = value,
                _ => args.referer = value,
            }
        }
        let ours = if f[1].starts_with("user-new.php") {
            let user_id = args.user_id.as_deref();
            crate::user_new::user_new(&Role(role), (update.as_deref(), user_id), &request)
        } else if f[1].starts_with("profile.php") {
            profile(&Role(role), &args, &request).unwrap()
        } else {
            user_edit(&Role(role), &args, &request).unwrap()
        };
        let recorded = String::from_utf8(unhex(f[3])).unwrap();
        let die = |status: u16, message: &str| {
            let page = crate::die::page(true, "WordPress &rsaquo; Error", message);
            format!("{status}\t{}", String::from_utf8(page).unwrap())
        };
        let (php, ours) = match ours {
            Answer::Page(page) => (hs_admin_page(&recorded), page),
            Answer::Denied(message) => (recorded, die(403, message)),
            Answer::Died(message) | Answer::Missing(message) | Answer::Conflict(message) => {
                (recorded, die(500, message))
            }
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

/// Passwords PHP would write back, or warn of, are not ported; the others
/// are read as stored.
#[test]
fn application_passwords_are_read_as_php_s() {
    let items = |stored: &[u8]| apps::items(Some(stored.to_vec()));
    assert!(apps::items(None).unwrap().is_empty());
    assert!(items(b"s:3:\"abc\";").unwrap().is_empty());
    assert!(items(b"a:0:{}").unwrap().is_empty());
    let one =
        |entries: &[&str]| format!("a:1:{{i:0;a:{}:{{{}}}}}", entries.len(), entries.concat());
    let (name, uuid) = ("s:4:\"name\";s:1:\"n\";", "s:4:\"uuid\";s:1:\"u\";");
    let (created, used) = ("s:7:\"created\";i:5;", "s:9:\"last_used\";N;");
    let (ip, no_ip) = ("s:7:\"last_ip\";s:1:\"x\";", "s:7:\"last_ip\";N;");
    for stored in [
        one(&[name, uuid, created, used, ip]),
        one(&[name, uuid, created, used, no_ip]),
    ] {
        assert_eq!(items(stored.as_bytes()).unwrap().len(), 1);
    }
    for stored in [
        // No uuid: PHP makes one and writes the passwords back.
        one(&[name, created, used, ip]),
        // No name, a name that is no string, a time or an address of
        // another type, an item that is no array.
        one(&[uuid, created, used, ip]),
        one(&["s:4:\"name\";i:1;", uuid, created, used, ip]),
        one(&[name, uuid, "s:7:\"created\";s:1:\"5\";", used, ip]),
        one(&[name, uuid, created, "s:9:\"last_used\";d:1;", ip]),
        one(&[name, uuid, created, used, "s:7:\"last_ip\";i:5;"]),
        "a:1:{i:0;s:1:\"x\";}".to_string(),
    ] {
        assert!(items(stored.as_bytes()).is_none(), "{stored}");
    }
}
