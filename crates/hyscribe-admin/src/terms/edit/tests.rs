// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{addslashes, remove_query_arg};
use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::admin_bar::Browser;
use crate::tests::{BROWSER, Role, SITE, first_difference};

const MOBILE: Browser = Browser {
    user_agent: b"Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) Mobile",
    mobile_hint: None,
};

/// `answer` as `reference/term-edit/golden.sh` keeps it.
fn kept(answer: Answer) -> String {
    let died = |status, message| {
        let page = crate::die::page(true, "HyScribe &rsaquo; Error", message);
        format!("{status}\t{}", String::from_utf8(page).unwrap())
    };
    match answer {
        Answer::Page(page) => page,
        Answer::Redirect(to) => format!("302\t{to}"),
        Answer::Denied(message) => died(403, message),
        Answer::Died(message) => died(500, message),
        Answer::Missing(_) | Answer::Conflict(_) => unreachable!(),
    }
}

/// Every page of `reference/term-edit/golden.sh`.
#[test]
fn pages_are_php_s() {
    let golden = include_str!("../../../tests/golden/term-edit.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (variant, query) = f[0].split_once('?').unwrap();
        let query = hs_admin_page(query);
        let browser = if variant == "mobile" {
            &MOBILE
        } else {
            &BROWSER
        };
        let role = match variant {
            "terms" => format!("terms:{}", f[1]),
            _ => f[1].to_string(),
        };
        let role: &'static str = Box::leak(role.into_boxed_str());
        // The request's path and query, slashed as `wp_magic_quotes()` does.
        let path = format!("/hs-admin/term?{query}");
        let path = String::from_utf8(addslashes(path.as_bytes())).unwrap();
        let canonical =
            remove_query_arg(&[b"message", b"error"], format!("{SITE}{path}").as_bytes());
        let canonical = String::from_utf8(canonical).unwrap();
        let args = TermEditArgs::parse(&query, None).unwrap();
        let ours = kept(term_edit(
            &Role(role),
            &args,
            &crate::tests::request(&canonical, &path, browser),
        ));
        let recorded = String::from_utf8(unhex(f[2])).unwrap();
        let php = hs_admin_page(&recorded);
        if ours != php {
            failures.push(format!("{role}?{query}: {}", first_difference(&php, &ours)));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The arguments not ported, and a taxonomy not listed, are refused; the
/// `Referer` sent fills the original referer field.
#[test]
fn arguments_are_parsed() {
    for query in [
        "tag_ID=1",
        "taxonomy=nav_menu&tag_ID=1",
        "taxonomy=category&post_type=page",
        "taxonomy=category&x=1",
        "taxonomy[]=category",
    ] {
        assert!(TermEditArgs::parse(query, None).is_none(), "{query}");
    }
    let referer = Some(format!("{SITE}/hs-admin/edit-tags?taxonomy=category"));
    let args = TermEditArgs::parse("taxonomy=category&tag_ID=1", referer).unwrap();
    let path = "/hs-admin/term?taxonomy=category&tag_ID=1";
    let Answer::Page(page) = term_edit(
        &Role("administrator"),
        &args,
        &crate::tests::request(path, path, &BROWSER),
    ) else {
        panic!("no page");
    };
    let field = "name=\"_wp_original_http_referer\" value=\"http://127.0.0.1:8081/hs-admin/edit-tags?taxonomy=category\"";
    assert!(page.contains(field), "{page}");
    // A page that is its own referer has none.
    for referer in [path.to_string(), format!("{SITE}{path}")] {
        let args = TermEditArgs::parse("taxonomy=category&tag_ID=1", Some(referer)).unwrap();
        let request = crate::tests::request(path, path, &BROWSER);
        let Answer::Page(page) = term_edit(&Role("administrator"), &args, &request) else {
            panic!("no page");
        };
        assert!(page.contains("name=\"_wp_original_http_referer\" value=\"\""));
    }
}
