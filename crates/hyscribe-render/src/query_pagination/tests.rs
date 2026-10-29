// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{Value, json_decode};
use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::Request;
use crate::template::render_content;
use crate::theme::twentytwentyfive;

/// The `pagination` cases of `reference/core-blocks/golden.sh`: page and
/// page count set on the main query in memory.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let mut failures = Vec::new();
    let mut count = 0;
    for line in golden
        .lines()
        .filter(|line| line.starts_with("pagination\t"))
    {
        let mut fields = line.split('\t').skip(1).map(unhex);
        let (markup, state, php) = (
            fields.next().unwrap(),
            fields.next().unwrap(),
            fields.next().unwrap(),
        );
        let state = json_decode(&state).unwrap();
        let site = DefaultInstall {
            home: state.get_str("home").unwrap().to_php_string(),
        };
        let number = |name| state.get_str(name).unwrap().to_int() as u64;
        let single = state.get_str("single") == Some(&Value::Bool(true));
        let base = if single {
            site.request("p=1")
        } else {
            site.request("")
        };
        let request = Request {
            uri: state.get_str("uri").unwrap().to_php_string(),
            paged: number("paged"),
            max_pages: number("max"),
            ..base
        };
        let ours = render_content(&theme, &site, &request, &markup);
        if ours != php {
            failures.push(format!(
                "{} {}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&request.uri),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
        count += 1;
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(count, 176);
}
