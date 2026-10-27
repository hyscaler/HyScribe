// SPDX-License-Identifier: AGPL-3.0-or-later

//! The style engine's parsing step and `_wp_to_kebab_case` against PHP.
//! Goldens come from `reference/supports/style-golden.sh`.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use hyscribe_phpser::{Key, Value, encode, json_decode};
use hyscribe_render::style::{Parsed, kebab_case, parse_block_styles};

fn unhex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// `$parsed` as PHP's array, for `serialize()`.
fn php(parsed: Parsed) -> Value {
    let key = |name: &str| Key::String(name.as_bytes().to_vec());
    Value::Array(vec![
        (
            key("classnames"),
            Value::Array(
                parsed
                    .classnames
                    .into_iter()
                    .zip(0..)
                    .map(|(c, i)| (Key::Int(i), Value::String(c.into_bytes())))
                    .collect(),
            ),
        ),
        (
            key("declarations"),
            Value::Array(
                parsed
                    .declarations
                    .into_iter()
                    .map(|(p, v)| (key(&p), v))
                    .collect(),
            ),
        ),
    ])
}

#[test]
fn matches_the_php_style_engine() {
    let golden = include_str!("golden/style-engine.tsv");
    let mut failures = Vec::new();
    let (mut words, mut styles) = (0, 0);
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields[0] == "k" {
            let input = String::from_utf8(unhex(fields[1])).unwrap();
            let expected = String::from_utf8(unhex(fields[2])).unwrap();
            let ours = kebab_case(&input);
            if ours != expected {
                failures.push(format!("kebab {input:?}: php {expected:?}, ours {ours:?}"));
            }
            words += 1;
        } else {
            let json = unhex(fields[2]);
            let style = json_decode(&json).unwrap();
            let expected = String::from_utf8_lossy(&unhex(fields[3])).into_owned();
            let ours = String::from_utf8_lossy(&encode(&php(parse_block_styles(
                &style,
                fields[1] == "1",
            ))))
            .into_owned();
            if ours != expected {
                failures.push(format!(
                    "style {} ({}):\n  php:  {expected}\n  ours: {ours}",
                    String::from_utf8_lossy(&json),
                    fields[1]
                ));
            }
            styles += 1;
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(words > 400 && styles > 300, "{words} {styles}");
}
