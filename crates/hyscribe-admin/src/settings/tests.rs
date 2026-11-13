// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use hyscribe_phpser::remove_query_arg;

use super::*;
use crate::admin_bar::Browser;
use crate::tests::{Role, SITE, first_difference};

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

/// The request for `path` from `browser`.
fn request<'a>(path: &'a str, canonical: &'a str, browser: &'a Browser<'a>) -> Request<'a> {
    Request {
        canonical,
        origin: SITE,
        uri: path,
        browser,
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
        let request = request(&path, &canonical, &browser);
        let screen = match f[2] {
            "general" => general(&site, &request),
            "writing" => Some(writing(&site, &request)),
            _ => reading(&site, &request),
        };
        let recorded = String::from_utf8(unhex(f[3])).unwrap();
        let (php, ours) = match screen {
            Some(Answer::Page(page)) => (hs_admin_page(&recorded), page),
            Some(Answer::Denied(message)) => {
                let page = crate::die::page(true, "WordPress &rsaquo; Error", message);
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
        reading(&site, &request(path, SITE, &browser)).is_some()
    };
    assert!(!show("s1unset:administrator") && !show("s1hidden:administrator"));
    assert!(show("s1static:administrator") && show("s1nopages:administrator"));
    let site = Role("s1icon:administrator");
    let path = "/hs-admin/options-general";
    assert!(general(&site, &request(path, SITE, &browser)).is_none());
}

/// Without `WP_HOME` and `WP_SITEURL` the addresses can be changed, and
/// the site address is explained.
#[test]
fn addresses_without_constants_can_change() {
    let site = Role("s1undefined:administrator");
    let path = "/hs-admin/options-general";
    let browser = Browser::default();
    let Some(Answer::Page(page)) = general(&site, &request(path, SITE, &browser)) else {
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

/// Uploads need iOS 6 or later on an Apple device.
#[test]
fn uploads_follow_the_device() {
    let can = |agent: &'static [u8]| {
        general::can_upload(&Browser {
            user_agent: agent,
            mobile_hint: None,
        })
    };
    assert!(can(b"iPhone; CPU iPhone OS 6_0 like Mac OS X Mobile"));
    assert!(can(b"iPod; CPU OS 12 like Mac OS X Mobile"));
    assert!(!can(b"iPhone; CPU iPhone OS 5_1 like Mac OS X Mobile"));
    assert!(!can(b"iPhone; Mobile"));
    assert!(!can(b"iPhone; CPU OS 9_0 Mobile"));
    assert!(can(b"Macintosh; iPhone OS 5_1 like Mac OS X"));
}
