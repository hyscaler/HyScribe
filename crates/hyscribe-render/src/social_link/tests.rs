// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::formatting::{antispambot, is_email};
use crate::site::Request;
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// `html` with decimal references turned back into their ASCII
/// characters, which `antispambot()` draws at random.
fn decoded(html: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = html;
    while let Some(&b) = rest.first() {
        let reference = rest
            .strip_prefix(b"&#")
            .and_then(|r| {
                let len = r.iter().take_while(|c| c.is_ascii_digit()).count();
                (r.get(len) == Some(&b';')).then(|| (len, r.get(..len)))
            })
            .and_then(|(len, digits)| {
                let n: u8 = std::str::from_utf8(digits?).ok()?.parse().ok()?;
                Some((len, n))
            });
        match reference {
            Some((len, n)) => {
                out.push(n);
                rest = rest.get(len + 3..).unwrap_or_default();
            }
            None => {
                out.push(b);
                rest = rest.get(1..).unwrap_or_default();
            }
        }
    }
    out
}

/// Every case of `reference/social-link/golden.sh`.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../tests/golden/social-link.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let input = unhex(fields[1]);
        if fields[0] == "email" {
            assert_eq!(
                is_email(&input),
                fields[2] == "1",
                "{:?}",
                String::from_utf8_lossy(&input)
            );
            continue;
        }
        let php = unhex(fields[2]);
        let ours = Renderer::new(&theme, &site, &request).render(&input);
        if decoded(&ours) != decoded(&php) {
            failures.push(format!(
                "{}\n  php:  {}\n  ours: {}",
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
        failures[..failures.len().min(4)].join("\n")
    );
    assert_eq!(golden.lines().count(), 50);
}

/// Each character is drawn on its own, `@` always encoded.
#[test]
fn antispambot_draws_each_character() {
    let drawn: Vec<Vec<u8>> = (0..64).map(|_| antispambot(b"ab@cd.io")).collect();
    assert!(
        drawn
            .iter()
            .all(|d| decoded(d) == b"ab@cd.io" && d.windows(5).any(|w| w == b"&#64;"))
    );
    assert!(drawn.iter().any(|d| d.starts_with(b"a")));
    assert!(drawn.iter().any(|d| d.starts_with(b"&#97;")));
}
