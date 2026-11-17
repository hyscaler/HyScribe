// SPDX-License-Identifier: AGPL-3.0-or-later

//! The operator documentation's key tables (docs/operators/configuration.md)
//! and `.env.example` against the loader, so neither can drift: every key
//! `from_map` reads is documented, every documented key is read, and every
//! documented default is the loader's.

// Tests fail by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;

use hyscribe_config::{ConfigError, from_map};

fn read(path: &str) -> String {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::read_to_string(root.join(path)).unwrap()
}

/// The keys the loader reads: the upper case string literals of its source.
fn loader_keys() -> BTreeSet<String> {
    let source = read("crates/hyscribe-config/src/lib.rs");
    source
        .split('"')
        .skip(1)
        .step_by(2)
        .filter(|s| {
            s.len() > 2
                && s.starts_with(|c: char| c.is_ascii_uppercase())
                && s.bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        })
        .map(String::from)
        .collect()
}

/// The documented keys and their default cells: rows `| `KEY` | default | ... |`.
fn documented() -> BTreeMap<String, String> {
    let page = read("docs/operators/configuration.md");
    let mut keys = BTreeMap::new();
    for line in page.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if cells.len() < 4 || !cells[1].starts_with('`') {
            continue;
        }
        let key = cells[1].trim_matches('`').to_string();
        assert!(
            keys.insert(key.clone(), cells[2].to_string()).is_none(),
            "{key} documented twice"
        );
    }
    keys
}

fn minimal() -> HashMap<String, String> {
    [("DB_NAME", "wordpress"), ("DB_USER", "wordpress")]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn every_key_is_documented_and_every_documented_key_is_read() {
    let read = loader_keys();
    let documented: BTreeSet<String> = documented().into_keys().collect();
    assert!(read.contains("DB_NAME") && read.contains("NONCE_SALT"));
    assert_eq!(
        read.difference(&documented).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "read but not documented"
    );
    assert_eq!(
        documented.difference(&read).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "documented but not read"
    );
}

#[test]
fn documented_defaults_are_the_loaders() {
    let base = from_map(&minimal()).unwrap();
    for (key, default) in documented() {
        let mut values = minimal();
        match default.as_str() {
            "required" => {
                values.remove(&key);
                assert_eq!(from_map(&values), Err(ConfigError::Missing(leak(&key))));
            }
            "not set" => {
                assert!(!["DB_NAME", "DB_USER"].contains(&key.as_str()));
            }
            literal => {
                let value = literal
                    .strip_prefix('`')
                    .and_then(|v| v.strip_suffix('`'))
                    .unwrap_or_else(|| panic!("{key}: default {literal} is not `value`"));
                values.insert(key.clone(), value.to_string());
                assert_eq!(from_map(&values).as_ref(), Ok(&base), "{key}={value}");
            }
        }
    }
}

#[test]
fn env_example_names_only_keys_the_loader_reads() {
    let read = loader_keys();
    let example = read_example();
    for key in &example {
        assert!(read.contains(key), ".env.example names {key}");
    }
    for key in &read {
        assert!(example.contains(key), ".env.example lacks {key}");
    }
}

/// Keys set or commented out (`KEY=` or `# KEY=`) in `.env.example`.
fn read_example() -> BTreeSet<String> {
    read(".env.example")
        .lines()
        .map(|line| line.trim_start_matches(['#', ' ']))
        .filter_map(|line| line.split_once('=').map(|(key, _)| key))
        .filter(|key| {
            !key.is_empty()
                && key
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        })
        .map(String::from)
        .collect()
}

/// `ConfigError::Missing` holds a `&'static str`.
fn leak(key: &str) -> &'static str {
    Box::leak(key.to_string().into_boxed_str())
}
