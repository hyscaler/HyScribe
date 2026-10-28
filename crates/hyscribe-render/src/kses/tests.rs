// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;

/// Every case of `reference/kses-post/golden.sh`.
#[test]
fn matches_php() {
    let golden = include_str!("../../tests/golden/kses-post.tsv");
    let mut host = Vec::new();
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let input = unhex(fields.get(1).unwrap());
        let (kind, ours) = match fields[0] {
            "host" => {
                host = input;
                continue;
            }
            "kses" => ("kses", wp_kses_post(&input, &host)),
            _ => ("serialize", serialize_blocks(&parse_blocks(&input))),
        };
        let php = unhex(fields[2]);
        if ours != php {
            failures.push(format!(
                "{kind} {:?}\n  php:  {:?}\n  ours: {:?}",
                String::from_utf8_lossy(&input),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(8)].join("\n")
    );
    assert!(golden.lines().count() > 6000);
}

#[test]
fn refiltering_stops_at_the_depth_limit() {
    let mut kses = Kses {
        allowed: &POST,
        upload_host: b"",
        budget: budget(0),
    };
    assert_eq!(
        kses.refilter(b"x", true, MAX_DEPTH - 1),
        Some(b"x".to_vec())
    );
    assert_eq!(kses.refilter(b"x", true, MAX_DEPTH), None);
    // The comment inside is one level deeper, past the limit.
    let comment = b"<!-- x -->";
    assert_eq!(
        kses.refilter(comment, true, MAX_DEPTH - 1),
        Some(Vec::new())
    );
}

#[test]
fn an_exhausted_budget_drops_the_token() {
    assert_eq!(budget(10), 4256);
    // Each pass costs the content's length and one.
    for (token, budget) in [(&b"<!-- x -->"[..], 0), (b"</ x>", 0), (b"</ >", 1)] {
        let mut kses = Kses {
            allowed: &POST,
            upload_host: b"",
            budget,
        };
        assert_eq!(kses.kses(token, true, 0), b"", "{token:?}");
    }
}

#[test]
fn limits_no_core_list_uses() {
    let allowed = AllowedHtml::from_json(
        br#"{"p":{"title":{"maxlen":5},"lang":{"REQUIRED":true,"Required":true},"cite":{"value_callback":"other"}},"1":true}"#,
    );
    // PHP would keep the short title; unported limits fail closed.
    assert_eq!(
        wp_kses(
            br#"<p title="x" lang="en" cite="http://h/a.pdf">a</p><1 x="y">"#,
            &allowed,
            b"h"
        ),
        br#"<p lang="en">a</p><1>"#
    );
    let nothing = AllowedHtml::from_json(b"null");
    assert_eq!(wp_kses(b"<p>x</p>", &nothing, b""), b"x");
}

/// HTTP requests render on 2 MiB worker stacks.
#[test]
fn the_deepest_blocks_fit_a_worker_stack() {
    let depth = crate::blocks::MAX_BLOCK_DEPTH;
    let document = [
        br#"<!-- wp:group {"a":{"b":"\u003ci\u003e"}} -->"#.repeat(depth),
        b"<!-- /wp:group -->".repeat(depth),
    ]
    .concat();
    let clean = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || wp_kses_post(&document, b""))
        .unwrap()
        .join()
        .unwrap();
    assert!(clean.starts_with(br#"<!-- wp:group {"a":{"b":"\u003ci\u003e"}} -->"#));
}
