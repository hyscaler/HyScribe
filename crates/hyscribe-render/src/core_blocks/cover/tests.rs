// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;
use sha2::{Digest, Sha256};

use crate::fixture::ThemeTestData;
use crate::site::Request;
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// Every case of `reference/cover/golden.sh`, by hash: the
/// fixture's content is not ours to commit.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let site = ThemeTestData {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../../tests/golden/cover.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let markup = unhex(fields[1]);
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.context.post_id = Some(fields[0].parse().unwrap());
        let ours = renderer.render(&markup);
        let hash: String = Sha256::digest(&ours)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if hash != fields[2] {
            // Only names and hashes: the fixture's content must not reach logs; `RAW=1`
            // on the golden script shows PHP's HTML locally.
            failures.push(format!(
                "{} {}: ours {hash}",
                fields[0],
                String::from_utf8_lossy(&markup)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(8)].join("\n")
    );
    assert_eq!(golden.lines().count(), 112);
}
