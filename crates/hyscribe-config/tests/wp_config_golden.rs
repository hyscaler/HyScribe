// SPDX-License-Identifier: AGPL-3.0-or-later

//! The wp-config.php reader against PHP itself: every literal it reads must
//! be what PHP defines, and `DB_HOST` must split as `wpdb::parse_db_host`
//! splits it. Goldens come from `reference/wpconfig/golden.sh`.

// Tests fail by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::HashMap;
use std::path::PathBuf;

use hyscribe_config::wpconfig::{self, Value};

fn dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join(name)
}

fn unhex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// What PHP defined, by name: the type letter and the value's text.
fn php_defined(name: &str) -> HashMap<String, (String, String)> {
    let path = dir("golden/wp-config").join(format!("{name}.tsv"));
    std::fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let name = String::from_utf8(unhex(fields[0])).unwrap();
            let value = String::from_utf8(unhex(fields[2])).unwrap();
            (name, (fields[1].to_string(), value))
        })
        .collect()
}

fn as_php(value: &Value) -> Option<(String, String)> {
    let pair = |kind: &str, text: String| Some((kind.to_string(), text));
    match value {
        Value::String(text) => pair("s", text.clone()),
        Value::Int(n) => pair("i", n.to_string()),
        Value::Bool(b) => pair("b", if *b { "1" } else { "0" }.to_string()),
        Value::Null => pair("n", String::new()),
        Value::Computed(_) => None,
    }
}

#[test]
fn literals_match_php_for_the_whole_corpus() {
    let mut files: Vec<_> = std::fs::read_dir(dir("corpus/wp-config"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    files.sort();
    assert_eq!(files.len(), 4);
    let mut compared = 0;
    for file in files {
        let name = file.file_stem().unwrap().to_str().unwrap().to_string();
        let config = wpconfig::parse(&std::fs::read(&file).unwrap()).unwrap();
        let php = php_defined(&name);

        for (constant, (kind, text)) in &php {
            if constant == "$table_prefix" {
                continue;
            }
            let ours = config
                .get(constant)
                .unwrap_or_else(|| panic!("{name}: {constant} not found"));
            if let Some(literal) = as_php(&ours.value) {
                assert_eq!(literal, (kind.clone(), text.clone()), "{name}: {constant}");
                compared += 1;
            }
        }
        for constant in &config.constants {
            // ABSPATH is defined before the file runs, as WordPress does.
            if !constant.conditional && constant.name != "ABSPATH" {
                assert!(
                    php.contains_key(&constant.name),
                    "{name}: PHP lacks {}",
                    constant.name
                );
            }
        }
        if let Some(literal) = config.table_prefix.as_ref().and_then(as_php) {
            assert_eq!(
                Some(&literal),
                php.get("$table_prefix"),
                "{name}: $table_prefix"
            );
        }
    }
    // Guards against the comparison silently skipping everything.
    assert_eq!(compared, 49);
}

#[test]
fn db_host_matches_wpdb() {
    let golden = std::fs::read_to_string(dir("golden/db-host.tsv")).unwrap();
    let mut count = 0;
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let input = String::from_utf8(unhex(fields[0])).unwrap();
        let ours = wpconfig::parse_db_host(&input);
        if fields[1] == "false" {
            assert_eq!(ours, None, "{input}");
        } else {
            let ours = ours.unwrap_or_else(|| panic!("{input}: not parsed"));
            assert_eq!(ours.host.as_bytes(), unhex(fields[1]), "{input}: host");
            assert_eq!(
                ours.port.map(|p| p.to_string()).as_deref().unwrap_or("-"),
                fields[2],
                "{input}: port"
            );
            let socket = ours.socket.map(|s| s.into_bytes());
            let expected = (fields[3] != "-").then(|| unhex(fields[3]));
            assert_eq!(socket, expected, "{input}: socket");
            assert_eq!(ours.ipv6, fields[4] == "1", "{input}: ipv6");
        }
        count += 1;
    }
    assert_eq!(count, 19);
}
