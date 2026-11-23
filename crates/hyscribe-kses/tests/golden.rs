// SPDX-License-Identifier: AGPL-3.0-or-later

//! Every function against PHP WordPress: the same output bytes for every
//! recorded input. Goldens come from `reference/kses/golden.sh`.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic)]

use hyscribe_kses::{
    ALLOWED_PROTOCOLS, Parts, esc_attr, esc_html, esc_url, parse_url, safecss_filter_attr,
    sanitize_key, url_query, wp_kses_bad_protocol, wp_kses_decode_entities, wp_kses_no_null,
    wp_kses_normalize_entities, wp_parse_url, wp_specialchars, wp_strip_all_tags,
};
use hyscribe_testkit::unhex;

/// Compares `ours` with every line of a golden; "throws" stands for an
/// empty result, as the port returns where PHP throws.
fn check(golden: &str, expected_count: usize, ours: impl Fn(&[u8]) -> Vec<u8>) {
    let mut failures = Vec::new();
    let mut count = 0;
    for line in golden.lines() {
        let (input, expected) = line.split_once('\t').unwrap();
        let input = unhex(input);
        let expected = if expected == "throws" {
            Vec::new()
        } else {
            unhex(expected)
        };
        let got = ours(&input);
        if got != expected {
            failures.push(format!(
                "{:?}\n  php:  {:?}\n  ours: {:?}",
                String::from_utf8_lossy(&input),
                String::from_utf8_lossy(&expected),
                String::from_utf8_lossy(&got)
            ));
        }
        count += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {count} differ:\n{}",
        failures.len(),
        failures
            .iter()
            .take(8)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert_eq!(count, expected_count);
}

#[test]
fn esc_html_matches() {
    check(include_str!("golden/esc-html.tsv"), 1815, |t| {
        esc_html(t).into_bytes()
    });
}

#[test]
fn esc_attr_matches() {
    check(include_str!("golden/esc-attr.tsv"), 1815, |t| {
        esc_attr(t).into_bytes()
    });
}

#[test]
fn normalize_entities_matches() {
    check(
        include_str!("golden/normalize-entities.tsv"),
        1496,
        wp_kses_normalize_entities,
    );
}

#[test]
fn decode_entities_matches() {
    check(include_str!("golden/decode-entities.tsv"), 1311, |t| {
        wp_kses_decode_entities(t).unwrap_or_default()
    });
    // Where PHP throws, the port says so.
    assert_eq!(wp_kses_decode_entities(b"&#99999999999999999999;"), None);
}

#[test]
fn no_null_matches() {
    check(include_str!("golden/no-null.tsv"), 1326, wp_kses_no_null);
}

#[test]
fn bad_protocol_matches() {
    check(include_str!("golden/bad-protocol.tsv"), 1614, |t| {
        wp_kses_bad_protocol(t, &ALLOWED_PROTOCOLS)
    });
}

#[test]
fn esc_url_matches() {
    check(include_str!("golden/esc-url.tsv"), 1312, esc_url);
}

#[test]
fn url_query_matches() {
    check(include_str!("golden/url-query.tsv"), 1312, |url| {
        url_query(url).map_or(b"null".to_vec(), |query| [&b"="[..], &query].concat())
    });
}

/// `serialize()` of `parse_url()`'s array, or "false".
fn serialized(parts: Option<Parts>) -> Vec<u8> {
    let Some(parts) = parts else {
        return b"false".to_vec();
    };
    let string = |text: &[u8]| [format!("s:{}:\"", text.len()).as_bytes(), text, b"\";"].concat();
    let mut entries = Vec::new();
    let mut add =
        |name: &str, value: Vec<u8>| entries.push([string(name.as_bytes()), value].concat());
    let text = [("scheme", &parts.scheme), ("host", &parts.host)];
    for (name, value) in text {
        if let Some(value) = value {
            add(name, string(value));
        }
    }
    if let Some(port) = parts.port {
        add("port", format!("i:{port};").into_bytes());
    }
    for (name, value) in [
        ("user", &parts.user),
        ("pass", &parts.pass),
        ("path", &parts.path),
        ("query", &parts.query),
        ("fragment", &parts.fragment),
    ] {
        if let Some(value) = value {
            add(name, string(value));
        }
    }
    [
        format!("a:{}:{{", entries.len()).into_bytes(),
        entries.concat(),
        b"}".to_vec(),
    ]
    .concat()
}

#[test]
fn parse_url_matches() {
    check(include_str!("golden/wp-parse-url.tsv"), 1312, |url| {
        serialized(wp_parse_url(url))
    });
    check(include_str!("golden/parse-url.tsv"), 1312, |url| {
        serialized(parse_url(url))
    });
}

#[test]
fn strip_all_tags_matches() {
    check(include_str!("golden/strip-all-tags.tsv"), 1602, |t| {
        wp_strip_all_tags(t, false)
    });
    check(
        include_str!("golden/strip-all-tags-breaks.tsv"),
        1494,
        |t| wp_strip_all_tags(t, true),
    );
}

#[test]
fn sanitize_key_matches() {
    check(include_str!("golden/sanitize-key.tsv"), 1399, |t| {
        sanitize_key(t).into_bytes()
    });
}

#[test]
fn safecss_filter_attr_matches() {
    check(
        include_str!("golden/safecss-filter-attr.tsv"),
        1601,
        safecss_filter_attr,
    );
}

/// Inputs at the edges of PCRE's JIT stack and backtrack limits.
#[test]
fn limits_match() {
    let mut count = 0;
    for line in include_str!("golden/limits.tsv").lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [function, prefix, unit, repeat, suffix, expected] = fields[..] else {
            panic!("bad line {line}");
        };
        let input = [
            unhex(prefix),
            unhex(unit).repeat(repeat.parse().unwrap()),
            unhex(suffix),
        ]
        .concat();
        let got = match function {
            "wp_strip_all_tags" => wp_strip_all_tags(&input, false),
            "safecss_filter_attr" => safecss_filter_attr(&input),
            _ => panic!("unknown function {function}"),
        };
        match expected.strip_prefix("len:") {
            _ if expected == "=" => assert!(got == input, "{function} {repeat}: changed"),
            Some(len) => assert_eq!(
                got.len(),
                len.parse::<usize>().unwrap(),
                "{function} {repeat}"
            ),
            None => assert_eq!(got, unhex(expected), "{function} {repeat}"),
        }
        count += 1;
    }
    assert_eq!(count, 22);
}

/// `_wp_specialchars()` keeps quotes and entities, and a text without any
/// special character as it is, invalid UTF-8 too.
#[test]
fn wp_specialchars_keeps_quotes() {
    assert_eq!(wp_specialchars(b"plain"), b"plain");
    assert_eq!(wp_specialchars(b"R&D"), b"R&amp;D");
    assert_eq!(wp_specialchars(b"a<b>\"'"), b"a&lt;b&gt;\"'");
    assert_eq!(wp_specialchars(b"&#039;&apos;"), b"&#039;&amp;apos;");
    assert_eq!(wp_specialchars(b"\xc3&"), b"");
    assert_eq!(wp_specialchars(b"\xc3"), b"\xc3");
}
