// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::admin_bar::Browser;
use crate::tests::{Role, SITE, first_difference};

/// Every page of `reference/settings-more/golden.sh`, today's date the test
/// site's.
#[test]
fn pages_are_php_s() {
    let golden = include_str!("../../tests/golden/settings-more.tsv");
    let now = 1_794_500_000;
    let (mut failures, mut pages) = (Vec::new(), 0);
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let role: &'static str = match f[0] {
            "default" => Box::leak(f[2].to_string().into_boxed_str()),
            variant => Box::leak(format!("{variant}:{}", f[2]).into_boxed_str()),
        };
        let file = f[1].trim_end_matches(".php");
        let path = format!("/hs-admin/{file}");
        let screen = match file {
            "options-discussion" => discussion,
            "options-media" => media,
            "options-permalink" => permalink,
            _ => privacy,
        };
        let browser = Browser {
            user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
            mobile_hint: None,
        };
        let request = Request {
            canonical: &format!("{SITE}{path}"),
            origin: SITE,
            uri: &path,
            browser: &browser,
        };
        let ours = screen(&Role(role), &request);
        let recorded = String::from_utf8(unhex(f[3])).unwrap();
        let Answer::Page(ours) = ours else {
            let refusal = "Sorry, you are not allowed to access this page.";
            assert!(recorded.starts_with("403\t") && recorded.contains(refusal));
            continue;
        };
        let php = hs_admin_page(&recorded)
            .replace("%ymd%", &format_unix("Y/m/d", now, Zone::Utc))
            .replace("%ym%", &format_unix("Y/m", now, Zone::Utc));
        pages += 1;
        if ours != php {
            failures.push(format!("{role} {file}: {}", first_difference(&php, &ours)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(pages, 20);
}
