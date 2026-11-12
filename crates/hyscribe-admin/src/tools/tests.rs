// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::tests::{Role, SITE, first_difference};

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
        let php = hs_admin_page(&String::from_utf8(unhex(f[2])).unwrap());
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
