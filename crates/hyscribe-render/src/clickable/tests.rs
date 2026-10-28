// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;

/// Every case of `reference/formatting/golden.sh clickable`, outside and
/// inside the `comment_text` filter.
#[test]
fn matches_php() {
    let golden = include_str!("../../tests/golden/make-clickable.tsv");
    let (home, cases) = golden.split_once('\n').unwrap();
    let mut failures = Vec::new();
    for line in cases.lines() {
        let mut fields = line.split('\t');
        let (input, ugc, php) = (
            unhex(fields.next().unwrap()),
            fields.next().unwrap() == "1",
            unhex(fields.next().unwrap()),
        );
        let ours = make_clickable(&input, home.as_bytes(), ugc);
        if ours != php {
            failures.push(format!(
                "{:?} ugc={ugc}\n  php:  {:?}\n  ours: {:?}",
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
        failures[..failures.len().min(6)].join("\n")
    );
    assert!(cases.lines().count() > 6000);
}

/// Inputs that would make a backtracking matcher quadratic stay fast.
#[test]
fn linear_time() {
    let started = std::time::Instant::now();
    for unit in [
        &b"<a <a "[..],
        b"<a x><a y>",
        b".a://",
        b"(",
        b" a@b.",
        b" www.",
        b"http://e.com/(",
        b"))",
    ] {
        let text = unit.repeat(2_000_000 / unit.len());
        make_clickable(&text, b"http://h", true);
    }
    assert!(started.elapsed().as_secs() < 20, "{:?}", started.elapsed());
}
