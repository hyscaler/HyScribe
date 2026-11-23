// SPDX-License-Identifier: AGPL-3.0-or-later

//! `HtmlProcessor` against WordPress's `WP_HTML_Processor`: the same
//! fragments walked token by token give the same tokens, breadcrumbs and
//! errors. The golden comes from `reference/html/processor-golden.php`.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use hyscribe_html::{HtmlProcessor, Namespace, ProcessorError};
use hyscribe_testkit::unhex;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn optional(value: Option<String>) -> String {
    value.map_or_else(|| "~".into(), |v| hex(v.as_bytes()))
}

/// The tokens and error as `processor-golden.php` prints them.
fn walk(html: &[u8]) -> (String, String) {
    let mut p = HtmlProcessor::create_fragment(html);
    let mut tokens = Vec::new();
    if html.len() > 4096 {
        let mut count = 0;
        while p.next_token() {
            count += 1;
        }
        let error = match p.get_last_error() {
            Some(ProcessorError::ExceededMaxBookmarks) => "exceeded-max-bookmarks:",
            _ => "~",
        };
        return (format!("count:{count}"), error.to_string());
    }
    while p.next_token() {
        let namespace: &[u8] = match p.get_namespace() {
            Namespace::Html => b"html",
            Namespace::Svg => b"svg",
            Namespace::Math => b"math",
        };
        tokens.push(
            [
                optional(p.get_token_type()),
                optional(p.get_token_name()),
                String::from(if p.is_tag_closer() { "1" } else { "0" }),
                String::from(if p.is_virtual() { "1" } else { "0" }),
                hex(namespace),
                p.get_current_depth().to_string(),
                hex(p.get_breadcrumbs().join("/").as_bytes()),
                hex(&p.get_modifiable_text()),
            ]
            .join("."),
        );
    }
    let error = match p.get_last_error() {
        None => "~".to_string(),
        Some(ProcessorError::Unsupported(message)) => {
            format!("unsupported:{}", hex(message.as_bytes()))
        }
        Some(ProcessorError::ExceededMaxBookmarks) => "exceeded-max-bookmarks:".to_string(),
    };
    (tokens.join(","), error)
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    hex(&Sha256::digest(bytes))
}

/// The HTML of the long cases, which the golden holds by hash with their
/// token counts.
fn long_cases() -> Vec<Vec<u8>> {
    vec![
        b"<span>".repeat(10001),
        b"<span>x</span>".repeat(6000),
        b"<table><td>".repeat(10001),
        b"<table><colgroup><td></td>".repeat(10001),
        b"<table><tbody><col>".repeat(10001),
    ]
}

#[test]
fn matches_wordpress() {
    let golden = include_str!("golden/processor.tsv");
    let long = long_cases();
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let html = match fields[0].strip_prefix("sha256:") {
            Some(hash) => long
                .iter()
                .find(|html| sha256(html) == hash)
                .unwrap()
                .clone(),
            None => unhex(fields[0]),
        };
        let (tokens, error) = walk(&html);
        if tokens != fields[1] || error != fields[2] {
            let first = tokens
                .split(',')
                .zip(fields[1].split(','))
                .position(|(ours, php)| ours != php);
            failures.push(format!(
                "{:?}: token {first:?} differs; ours {error}, php {}",
                String::from_utf8_lossy(&html),
                fields[2]
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} differ:\n{}",
        failures.len(),
        golden.lines().count(),
        failures.join("\n")
    );
}

#[test]
fn a_stopped_processor_stays_stopped() {
    let mut p = HtmlProcessor::create_fragment(b"<table>x");
    while p.next_token() {}
    assert!(p.get_last_error().is_some());
    assert!(!p.next_token());
}

#[test]
fn before_the_first_token_nothing_is_open() {
    let p = HtmlProcessor::create_fragment(b"</svg>");
    assert_eq!(p.get_namespace(), Namespace::Html);
    assert!(!p.is_tag_closer());
}
