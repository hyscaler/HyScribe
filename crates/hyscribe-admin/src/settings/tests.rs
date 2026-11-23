// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use hyscribe_phpser::remove_query_arg;
use hyscribe_render::date::format_unix;

use super::*;
use crate::admin_bar::Browser;
use crate::tests::{BROWSER, Role, SITE, first_difference, request};

/// The query `reference/settings/golden.sh` asks the `s1referer` page
/// with, slashed as `wp_magic_quotes()` leaves `REQUEST_URI`.
const REFERER_QUERY: &str = "?x=1&y=\\'q\\'";

/// The user agents `reference/settings/golden.sh` sends.
fn agent(variant: &str) -> &'static [u8] {
    match variant {
        "s1android" => b"Mozilla/5.0 (Linux; Android 14) Mobile",
        "s1ios7" => b"Mozilla/5.0 (iPhone; CPU iPhone OS 7_0 like Mac OS X) Mobile",
        "s1ios5" => b"Mozilla/5.0 (iPad; CPU OS 5_1 like Mac OS X) Mobile",
        _ => b"curl",
    }
}

/// Every page of `reference/settings/golden.sh`, the `s1referer` one asked
/// with a query its form's referer field escapes.
#[test]
fn pages_are_php_s() {
    let golden = include_str!("../../tests/golden/settings.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let role: &'static str = Box::leak(format!("{}:{}", f[0], f[1]).into_boxed_str());
        let query = if f[0] == "s1referer" {
            REFERER_QUERY
        } else {
            ""
        };
        let path = format!("/hs-admin/options-{}{query}", f[2]);
        let site = Role(role);
        let browser = Browser {
            user_agent: agent(f[0]),
            mobile_hint: None,
        };
        // `wp_admin_canonical_url()` rebuilds the query.
        let canonical = remove_query_arg(&[b"updated"], format!("{SITE}{path}").as_bytes());
        let canonical = text(&canonical);
        let request = request(&canonical, &path, &browser);
        let screen = match f[2] {
            "general" => general(&site, &request),
            "writing" => Some(writing(&site, &request)),
            _ => reading(&site, &request),
        };
        let recorded = String::from_utf8(unhex(f[3])).unwrap();
        let (php, ours) = match screen {
            Some(Answer::Page(page)) => (hs_admin_page(&recorded), page),
            Some(Answer::Denied(message)) => {
                let page = crate::die::page(true, "HyScribe &rsaquo; Error", message);
                (
                    recorded,
                    format!("403\t{}", String::from_utf8(page).unwrap()),
                )
            }
            _ => panic!("{role}: no page"),
        };
        if ours != php {
            failures.push(format!(
                "{role} {}: {}",
                f[2],
                first_difference(&php, &ours)
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// What PHP would save before showing, and the Site Icon, are not ported.
#[test]
fn unported_states_have_no_page() {
    let browser = Browser::default();
    let show = |variant: &'static str| {
        let site = Role(variant);
        let path = "/hs-admin/options-reading";
        reading(&site, &request(SITE, path, &browser)).is_some()
    };
    assert!(!show("s1unset:administrator") && !show("s1hidden:administrator"));
    assert!(show("s1static:administrator") && show("s1nopages:administrator"));
    let site = Role("s1icon:administrator");
    let path = "/hs-admin/options-general";
    assert!(general(&site, &request(SITE, path, &browser)).is_none());
}

/// Without `WP_HOME` and `WP_SITEURL` the addresses can be changed, and
/// the site address is explained.
#[test]
fn addresses_without_constants_can_change() {
    let site = Role("s1undefined:administrator");
    let path = "/hs-admin/options-general";
    let browser = Browser::default();
    let Some(Answer::Page(page)) = general(&site, &request(SITE, path, &browser)) else {
        panic!("no page");
    };
    assert!(!page.contains("disabled='disabled'") && !page.contains("code disabled"));
    assert!(page.contains("value=\"http://127.0.0.1:8081\" class=\"regular-text code\" />\n\t<p class=\"description\" id=\"home-description\">\n\t\tEnter the same address here unless you"));
}

/// A zone named in another case, or none PHP knows, tells no transitions,
/// and only a name the list leaves out as spelled shows on its own.
#[test]
fn zones_by_other_names() {
    let site = Role("administrator");
    let info = general::zone_info(&site, "asia/kolkata");
    assert!(info.contains("in standard time.\t<br />\n\t\t</span>"));
    assert!(general::zone_info(&site, "Mars/Base").is_empty());
    // Sydney's summer time ends on 2027-04-04 at 03:00, 16:00 UTC the day before.
    let sydney = general::zone_info(&site, "Australia/Sydney");
    assert!(sydney.contains("currently in daylight saving time."));
    assert!(sydney.contains(" Standard time begins on: <code>April 3, 2027 4:00 pm</code>."));
    assert!(
        general::zone_choice("Asia/Calcutta")
            .starts_with("<option selected=\"selected\" value=\"Asia/Calcutta\"")
    );
    assert!(general::zone_choice("asia/calcutta").starts_with("<optgroup label=\"Africa\""));
    assert!(!general::zone_choice("asia/calcutta").contains("selected"));
}

/// The Formatting options are for sites installed before database 32453.
#[test]
fn formatting_is_for_old_sites() {
    assert!(writing::formatting(&Role("s1db:administrator")).is_empty());
    assert!(!writing::formatting(&Role("s1writing:administrator")).is_empty());
}

/// The `settings_errors` transient the save of an `o5` variant kept, as
/// the write oracle's goldens record them.
pub(crate) fn kept_errors(variant: &str) -> Option<Vec<u8>> {
    let error = |setting: &str, code: &str, message: &str, kind: &str| {
        let field = |name: &str, value: &str| {
            let key = Key::String(name.as_bytes().to_vec());
            (key, Value::String(value.as_bytes().to_vec()))
        };
        Value::Array(vec![
            field("setting", setting),
            field("code", code),
            field("message", message),
            field("type", kind),
        ])
    };
    let saved = |message| error("general", "settings_updated", message, "success");
    let errors = match variant {
        "o5saved" | "o5expired" | "o5now" => vec![saved("Settings saved.")],
        "o5failed" => vec![error(
            "general",
            "settings_updated",
            "Settings save failed.",
            "error",
        )],
        "o5permalink" => vec![saved("Permalink structure updated.")],
        "o5email" => vec![
            error(
                "general",
                "settings_updated",
                "The timezone you have entered is not valid. Please select a valid timezone.",
                "error",
            ),
            error(
                "new_admin_email",
                "invalid_new_admin_email",
                "The email address entered did not appear to be a valid email address. Please enter a valid email address.",
                "error",
            ),
        ],
        "o5structure" => vec![error(
            "permalink_structure",
            "invalid_permalink_structure",
            "A structure tag is required when using custom permalinks. <a href=\"https://wordpress.org/documentation/article/customize-permalinks/#choosing-your-permalink-structure\">Learn more</a>",
            "error",
        )],
        "o5kinds" => vec![
            error("a", "one", "Updated.", "updated"),
            error("a", "two", "Told.", "info"),
            error("a", "three", "Other.", "other"),
        ],
        "o5bad" => return Some(b"x".to_vec()),
        _ => return None,
    };
    let list = errors.into_iter().enumerate();
    let list = list.map(|(i, error)| (Key::Int(i as i64), error)).collect();
    Some(hyscribe_phpser::encode(&Value::Array(list)))
}

/// The screens `reference/admin-saves/golden.sh` recorded after each save,
/// with the notices the save kept, which an expired transient no longer shows.
#[test]
fn pages_after_saves_are_php_s() {
    let golden = include_str!("../../tests/golden/admin-saves-after.tsv");
    let mut failures = Vec::new();
    let mut pages = 0;
    for line in golden.lines().filter(|line| line.starts_with("options-")) {
        let [case, "200", page] = line.split('\t').collect::<Vec<_>>()[..] else {
            panic!("{line}");
        };
        pages += 1;
        let (variant, screen, query) = match case {
            "options-adminhash-bad" => ("default", "general", "?updated=false"),
            "options-dismiss" => ("default", "general", "?updated=true"),
            "options-general-email" => ("o5email", "general", "?settings-updated=true"),
            "options-privacy" => ("o5failed", "privacy", "?settings-updated=true"),
            "options-permalink" => ("o5permalink", "permalink", "?settings-updated=true"),
            "options-permalink-error" => ("o5structure", "permalink", "?settings-updated=true"),
            other => (
                "o5saved",
                other.trim_start_matches("options-"),
                "?settings-updated=true",
            ),
        };
        let role: &'static str = Box::leak(format!("{variant}:administrator").into_boxed_str());
        let path = format!("/hs-admin/options-{screen}{query}");
        let canonical = remove_query_arg(
            &[b"updated", b"settings-updated"],
            format!("{SITE}{path}").as_bytes(),
        );
        let canonical = text(&canonical);
        let request = request(&canonical, &path, &BROWSER);
        let site = Role(role);
        let ours = match screen {
            "general" => general(&site, &request),
            "writing" => Some(writing(&site, &request)),
            "reading" => reading(&site, &request),
            "discussion" => Some(crate::settings_more::discussion(&site, &request)),
            "media" => Some(crate::settings_more::media(&site, &request)),
            "permalink" => Some(crate::settings_more::permalink(&site, &request)),
            _ => Some(crate::settings_more::privacy(&site, &request)),
        };
        let Some(Answer::Page(ours)) = ours else {
            panic!("{case}: no page");
        };
        let now = 1_794_500_000;
        let php = hs_admin_page(&String::from_utf8(unhex(page)).unwrap())
            .replace("%ymd%", &format_unix("Y/m/d", now, Zone::Utc))
            .replace("%ym%", &format_unix("Y/m", now, Zone::Utc));
        if ours != php {
            failures.push(format!("{case}: {}", first_difference(&php, &ours)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(pages, 11);
    let site = Role("o5expired:administrator");
    let path = "/hs-admin/options-writing?settings-updated=true";
    let Answer::Page(page) = writing(&site, &request(SITE, path, &BROWSER)) else {
        panic!("no page");
    };
    assert!(!page.contains("settings-error"));
}

/// `settings_errors()` gives `updated` notices the success class and other
/// core types their `notice-` class, and prints nothing of a transient that
/// is not a list.
#[test]
fn kept_notices_by_type() {
    let page = |variant: &'static str| {
        let site = Role(variant);
        let path = "/hs-admin/options-writing?settings-updated=1";
        let Answer::Page(page) = writing(&site, &request(SITE, path, &BROWSER)) else {
            panic!("no page");
        };
        page
    };
    let kinds = page("o5kinds:administrator");
    for notice in [
        "<div id='setting-error-one' class='notice notice-success settings-error is-dismissible'>",
        "<div id='setting-error-two' class='notice notice-info settings-error is-dismissible'>",
        "<div id='setting-error-three' class='notice other settings-error is-dismissible'>",
    ] {
        assert!(kinds.contains(notice), "{notice}");
    }
    assert!(!page("o5bad:administrator").contains("settings-error"));
    // A transient expires only after its timeout.
    assert!(page("o5now:administrator").contains("Settings saved."));
}
