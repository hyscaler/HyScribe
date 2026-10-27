// SPDX-License-Identifier: AGPL-3.0-or-later

//! `TagProcessor` against WordPress's `WP_HTML_Tag_Processor`: the same
//! operations on the same HTML give the same results and the same updated
//! HTML. Goldens come from `reference/html/golden.sh`; the operation codes
//! are described in `reference/html/golden.php`.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use hyscribe_html::{Attribute, TagProcessor};

fn unhex(text: &str) -> Vec<u8> {
    text.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn bit(value: bool) -> String {
    String::from(if value { "1" } else { "0" })
}

fn run(p: &mut TagProcessor, op: &str) -> String {
    let mut parts = op.split(':');
    let code = parts.next().unwrap();
    let args: Vec<Option<Vec<u8>>> = parts
        .map(|a| if a == "~" { None } else { Some(unhex(a)) })
        .collect();
    let arg = |i: usize| args[i].as_deref().unwrap();
    match code {
        "N" => bit(p.next_tag(args[0].as_deref(), args[1].as_deref())),
        "S" => bit(p.set_attribute(arg(0), arg(1))),
        "B" => bit(p.set_boolean_attribute(arg(0))),
        "R" => bit(p.remove_attribute(arg(0))),
        "A" => bit(p.add_class(arg(0))),
        "D" => bit(p.remove_class(arg(0))),
        "G" => match p.get_attribute(arg(0)) {
            None => "n".into(),
            Some(Attribute::True) => "t".into(),
            Some(Attribute::Value(value)) => format!("s{}", hex(&value)),
        },
        "H" => p.has_class(arg(0)).map_or_else(|| "n".into(), bit),
        "L" => {
            let names: Vec<String> = p.class_list().iter().map(|n| hex(n)).collect();
            format!("l{}", names.join(","))
        }
        "T" => p
            .get_tag()
            .map_or_else(|| "n".into(), |tag| format!("s{}", hex(&tag))),
        "W" => {
            let mut tags = Vec::new();
            for _ in 0..1_000_000 {
                if !p.next_tag(None, None) {
                    break;
                }
                tags.push(hex(&p.get_tag().unwrap()));
            }
            format!("w{}", tags.join(","))
        }
        "U" => format!("s{}", hex(p.get_updated_html())),
        _ => panic!("unknown operation {op}"),
    }
}

fn check(golden: &str, expected_count: usize) {
    let mut failures = Vec::new();
    let mut count = 0;
    for line in golden.lines() {
        let mut fields = line.split('\t');
        let (html, ops, expected) = (
            unhex(fields.next().unwrap()),
            fields.next().unwrap(),
            fields.next().unwrap(),
        );
        let mut p = TagProcessor::new(&html);
        let ours: Vec<String> = ops.split(' ').map(|op| run(&mut p, op)).collect();
        let ours = ours.join(" ");
        if ours != expected {
            let first = ours
                .split(' ')
                .zip(expected.split(' '))
                .zip(ops.split(' '))
                .find(|((a, b), _)| a != b);
            failures.push(format!(
                "{:?}\n  ops:  {}\n  first difference: {:?}",
                String::from_utf8_lossy(&html)
                    .chars()
                    .take(200)
                    .collect::<String>(),
                ops.chars().take(300).collect::<String>(),
                first.map(|((ours, php), op)| (
                    op,
                    String::from_utf8_lossy(&unhex(php.get(1..).unwrap_or_default()))
                        .chars()
                        .take(300)
                        .collect::<String>(),
                    String::from_utf8_lossy(&unhex(ours.get(1..).unwrap_or_default()))
                        .chars()
                        .take(300)
                        .collect::<String>(),
                )),
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
fn matches_wordpress_on_edge_cases() {
    check(include_str!("golden/edge.tsv"), 2276);
}

#[test]
fn matches_wordpress_on_the_default_theme() {
    check(include_str!("golden/theme.tsv"), 226);
}

#[test]
fn matches_wordpress_on_random_edits() {
    check(include_str!("golden/random.tsv"), 3000);
}
