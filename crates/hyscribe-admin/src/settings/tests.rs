// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::tests::{Role, SITE, first_difference};

/// The user agents `reference/settings/golden.sh` sends.
fn agent(variant: &str) -> &'static [u8] {
    match variant {
        "s1android" => b"Mozilla/5.0 (Linux; Android 14) Mobile",
        "s1ios7" => b"Mozilla/5.0 (iPhone; CPU iPhone OS 7_0 like Mac OS X) Mobile",
        "s1ios5" => b"Mozilla/5.0 (iPad; CPU OS 5_1 like Mac OS X) Mobile",
        _ => b"curl",
    }
}

/// Every page of `reference/settings/golden.sh`.
#[test]
fn pages_are_php_s() {
    let golden = include_str!("../../tests/golden/settings.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let role: &'static str = Box::leak(format!("{}:{}", f[0], f[1]).into_boxed_str());
        let path = format!("/hs-admin/options-{}", f[2]);
        let site = Role(role);
        let browser = Browser {
            user_agent: agent(f[0]),
            mobile_hint: None,
        };
        let canonical = format!("{SITE}{path}");
        let screen = match f[2] {
            "general" => general(&site, &canonical, SITE, &path, &browser),
            "writing" => Some(writing(&site, &canonical, SITE, &path, &browser)),
            _ => reading(&site, &canonical, SITE, &path, &browser),
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
        reading(&site, SITE, SITE, path, &browser).is_some()
    };
    assert!(!show("s1unset:administrator") && !show("s1hidden:administrator"));
    assert!(show("s1static:administrator") && show("s1nopages:administrator"));
    let site = Role("s1icon:administrator");
    let path = "/hs-admin/options-general";
    assert!(general(&site, SITE, SITE, path, &browser).is_none());
}

/// Without `WP_HOME` and `WP_SITEURL` the addresses can be changed, and
/// the site address is explained.
#[test]
fn addresses_without_constants_can_change() {
    let site = Role("s1undefined:administrator");
    let path = "/hs-admin/options-general";
    let Some(Answer::Page(page)) = general(&site, SITE, SITE, path, &Browser::default()) else {
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
