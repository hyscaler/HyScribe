// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use hyscribe_phpser::{encode, json_decode};
use hyscribe_testkit::unhex;

fn php(parsed: Parsed) -> Vec<u8> {
    let key = |name: &str| Key::String(name.as_bytes().to_vec());
    encode(&Value::Array(vec![
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
    ]))
}

/// The PHP style engine on every style attribute in the theme and
/// synthetic ones (`reference/supports/style-golden.sh`).
#[test]
fn every_golden_case() {
    let golden = include_str!("../../tests/golden/style-engine.tsv");
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let style = json_decode(&unhex(fields[2])).unwrap();
        assert_eq!(
            php(parse_block_styles(&style, fields[1] == "1")),
            unhex(fields[3]),
            "{}",
            String::from_utf8_lossy(&unhex(fields[2]))
        );
    }
    assert!(golden.lines().count() > 300);
}
