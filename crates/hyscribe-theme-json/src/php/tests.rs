// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::Float;

use super::*;
use crate::tests::{arg, check, text};

fn float(value: &Value) -> f64 {
    number(value).unwrap()
}

#[test]
fn matches_php_numbers() {
    check("round", |a| {
        let places = match a[1] {
            Value::Int(n) => n as u32,
            _ => unreachable!(),
        };
        Value::Float(Float::from_f64(round(float(&a[0]), places)))
    });
    check("float_string", |a| text(&float_string(float(&a[0]))));
    check("to_float", |a| {
        Value::Float(Float::from_f64(to_float(arg(a, 0))))
    });
    check("is_numeric", |a| Value::Bool(is_numeric(&a[0])));
}

#[test]
fn special_floats() {
    assert_eq!(float_string(f64::NAN), "NAN");
    assert_eq!(float_string(f64::INFINITY), "INF");
    assert_eq!(float_string(f64::NEG_INFINITY), "-INF");
    assert!(round(f64::NAN, 2).is_nan());
    assert_eq!(round(f64::INFINITY, 2), f64::INFINITY);
}

fn json(text: &str) -> Value {
    hyscribe_phpser::json_decode(text.as_bytes()).unwrap()
}

#[test]
fn keys_like_php() {
    assert_eq!(key("7"), Key::Int(7));
    assert_eq!(key("-7"), Key::Int(-7));
    assert_eq!(key("0"), Key::Int(0));
    for text in ["07", "-0", "", "-", "1a", "99999999999999999999", "+1"] {
        assert_eq!(key(text), Key::String(text.as_bytes().to_vec()), "{text}");
    }
    assert_eq!(key_str(&Key::Int(-3)), "-3");
    assert_eq!(key_str(&Key::String(b"a".to_vec())), "a");
}

#[test]
fn array_paths() {
    let mut tree = json(r#"{"a": {"b": 1, "n": null}, "s": "x"}"#);
    assert_eq!(get(&tree, &["a", "b"]), Some(&Value::Int(1)));
    assert_eq!(get(&tree, &["a", "n"]), Some(&Value::Null));
    assert_eq!(get(&tree, &["a", "x"]), None);
    assert_eq!(get(&tree, &["s", "x"]), None);
    assert_eq!(get(&tree, &[]), Some(&tree));
    assert!(get_mut(&mut tree, &["s", "x"]).is_none());

    set(&mut tree, &["a", "c", "d"], Value::Int(2));
    set(&mut tree, &["s", "t"], Value::Int(3));
    set(&mut tree, &["a", "b"], Value::Int(4));
    set(&mut tree, &[], Value::Int(5));
    assert_eq!(
        tree,
        json(r#"{"a": {"b": 4, "n": null, "c": {"d": 2}}, "s": {"t": 3}}"#)
    );
    let mut scalar = Value::Int(1);
    set(&mut scalar, &["a"], Value::Int(2));
    assert_eq!(scalar, Value::Int(1));

    unset(&mut tree, &["a", "c"]);
    unset(&mut tree, &["missing", "x"]);
    unset(&mut tree, &[]);
    assert_eq!(tree, json(r#"{"a": {"b": 4, "n": null}, "s": {"t": 3}}"#));

    let mut base = json(r#"{"a": {"b": 1, "c": [1, 2]}, "d": 1, "e": {"f": 1}}"#);
    replace_recursive(
        &mut base,
        &json(r#"{"a": {"c": [9]}, "d": {"x": 1}, "e": 2, "g": 3}"#),
    );
    assert_eq!(
        base,
        json(r#"{"a": {"b": 1, "c": [9, 2]}, "d": {"x": 1}, "e": 2, "g": 3}"#)
    );
    let mut scalar = Value::Int(1);
    replace_recursive(&mut scalar, &json(r#"{"a": 1}"#));
    assert_eq!(scalar, Value::Int(1));

    let mut base = json(r#"{"a": {"b": 1}, "c": 2}"#);
    replace(&mut base, Some(&json(r#"{"a": {"x": 1}, "d": 3}"#)));
    replace(&mut base, None);
    assert_eq!(base, json(r#"{"a": {"x": 1}, "c": 2, "d": 3}"#));
}

#[test]
fn emptiness() {
    for value in ["null", "false", "0", "0.0", "\"\"", "\"0\"", "[]"] {
        assert!(empty(Some(&json(value))), "{value}");
    }
    for value in ["true", "1", "0.5", "\"a\"", "\"00\"", "[0]"] {
        assert!(!empty(Some(&json(value))), "{value}");
    }
    assert!(empty(None));
    assert!(!empty(Some(&Value::Enum(Vec::new()))));
    assert!(isset(Some(&Value::Int(0))));
    assert!(!isset(Some(&Value::Null)));
    assert!(!isset(None));
}

#[test]
fn strings() {
    assert_eq!(as_str(&Value::String(vec![0xff])), None);
    assert_eq!(as_str(&Value::Int(1)), None);
    assert_eq!(entries(&Value::Int(1)), []);
    assert_eq!(trim(" \t\n\r\0\x0bx \u{a0}"), "x \u{a0}");
    assert_eq!(number(&text("1.5")), Some(1.5));
    assert_eq!(number(&text("x")), None);
    assert_eq!(number(&Value::Int(2)), Some(2.0));
    assert_eq!(number(&Value::Null), None);
    assert_eq!(to_string(&Value::Int(2)), Some("2".to_owned()));
    assert_eq!(to_string(&json("1.0")), Some("1".to_owned()));
    assert_eq!(to_string(&Value::Bool(true)), None);
    for (value, expected) in [
        ("true", "1"),
        ("false", ""),
        ("null", ""),
        ("[1]", "Array"),
        ("2", "2"),
        ("\"s\"", "s"),
    ] {
        assert_eq!(php_string(&json(value)), expected, "{value}");
    }
}
