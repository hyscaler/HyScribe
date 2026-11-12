// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;
use crate::tests::{Role, SITE, first_difference};

/// The PHP oracle's page under hs-admin's URLs, without the update notice
/// hs-admin leaves out until an open question is decided.
fn hs_admin(page: &str) -> String {
    let nag = "<div class=\"notice notice-warning update-nag inline\">";
    let page = match page.split_once(nag) {
        Some((head, tail)) => format!("{head}{}", tail.split_once("</div>").unwrap().1),
        None => page.to_string(),
    };
    let mut out = String::new();
    let mut rest = page.as_str();
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
    let mut page = out.replace("/wp-login.php", "/hs-login");
    for prefix in ["href='", "href=\"", "\"url\":\""] {
        let mut out = String::new();
        let mut rest = page.as_str();
        while let Some(at) = rest.find(prefix) {
            let (head, tail) = rest.split_at(at + prefix.len());
            out.push_str(head);
            let end = tail.find(['\'', '?', '"']).unwrap_or(tail.len());
            let (file, tail) = tail.split_at(end);
            let local = file.strip_suffix(".php").filter(|f| !f.contains('/'));
            out.push_str(local.unwrap_or(file));
            rest = tail;
        }
        out.push_str(rest);
        page = out;
    }
    page
}

/// Every case of `reference/admin-page/golden.sh`.
#[test]
fn the_tools_screen_is_php_s() {
    let golden = include_str!("../../tests/golden/pages.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let role: &'static str = Box::leak(f[0].to_string().into_boxed_str());
        let ours = tools(
            &Role(role),
            &format!("{SITE}/hs-admin/tools"),
            SITE,
            &Browser {
                user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
                mobile_hint: None,
            },
        )
        .unwrap();
        let php = hs_admin(&String::from_utf8(unhex(f[2])).unwrap());
        if ours != php {
            failures.push(format!("{role} {}{}", f[1], first_difference(&php, &ours)));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
