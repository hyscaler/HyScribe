// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{decode, json_decode};
use hyscribe_testkit::unhex;

use super::*;

fn error_json(refusal: Refusal) -> Vec<u8> {
    let error = refusal.0.expect("refused as unported");
    let error = object([
        ("code", text(error.code)),
        ("message", text(error.message)),
        ("data", error.data),
    ]);
    json_encode(&error).unwrap()
}

/// Every case of `reference/rest/schema-golden.php`.
#[test]
fn values_are_checked_as_php_checks_them() {
    let golden = include_str!("../../tests/golden/schema.tsv");
    for line in golden.lines() {
        let mut fields = line.split('\t').map(unhex);
        let mut field = || fields.next().unwrap();
        let (schema, value, validated, sanitized) = (field(), field(), field(), field());
        let schema = json_decode(&schema).unwrap();
        let value = decode(&value).unwrap();
        let ours = validate_value_from_schema(&value, &schema, "p")
            .map_or_else(error_json, |()| b"true".to_vec());
        let case = format!("{schema:?} {value:?}");
        assert_eq!(lossy(&ours), lossy(&validated), "{case}");
        let ours = sanitize_value_from_schema(&value, &schema, "p")
            .map_or_else(error_json, |value| encode(&value));
        assert_eq!(lossy(&ours), lossy(&sanitized), "{case}");
    }
    assert!(golden.lines().count() > 140);
}

#[test]
fn keywords_not_ported_are_refused() {
    for keyword in UNPORTED {
        // A pattern the byte matcher would read otherwise than PCRE's `u`.
        let value = if keyword == "pattern" { r"\w" } else { "x" };
        let schema = object([("type", text("string")), (keyword, text(value))]);
        assert_eq!(
            validate_value_from_schema(&text("a"), &schema, "p"),
            Err(Refusal(None))
        );
        assert_eq!(
            sanitize_value_from_schema(&text("a"), &schema, "p"),
            Err(Refusal(None))
        );
        let nested = object([("type", text("array")), ("items", schema.clone())]);
        let value = Value::Array(vec![(Key::Int(0), text("a"))]);
        assert_eq!(
            validate_value_from_schema(&value, &nested, "p"),
            Err(Refusal(None))
        );
        let one_of = object([("oneOf", Value::Array(vec![(Key::Int(0), schema)]))]);
        assert_eq!(
            sanitize_value_from_schema(&text("a"), &one_of, "p"),
            Err(Refusal(None))
        );
    }
}

/// `rest_parse_date()`'s pattern: seconds required, a fraction of them
/// left out, `t` or a space for `T`, an offset of hours alone or hours and
/// minutes with a colon.
#[test]
fn dates_are_parsed_as_rest_parse_date_parses_them() {
    let at = |date: &str| parse_date(date.as_bytes());
    let base = at("2013-03-14T08:00:00+02:00");
    assert!(base.is_some());
    for same in [
        "2013-03-14T08:00:00+02",
        "2013-03-14t08:00:00.25+02:00",
        "2013-03-14 06:00:00Z",
    ] {
        assert_eq!(at(same), base, "{same}");
    }
    for wrong in [
        "2013-03-14T08:00",
        "2013-03-14T08:00:00.",
        "2013-03-14T08:00:00+0200",
        "2013-03-14T08:00:00+0a",
        "2013-03-14T08:00:00+02:0a",
        "2013-03-14X08:00:00",
        "13-03-14T08:00:00",
    ] {
        assert_eq!(at(wrong), None, "{wrong}");
    }
}

/// What `schema.tsv`'s addresses leave unsaid: where PHP's `str_repeat()`
/// of a negative count throws, which no answer is made of, a last new line,
/// and the forms of a mapped address.
#[test]
fn ip_addresses_are_read_as_php_reads_them() {
    let is = |ip: &str| is_ip_address(ip.as_bytes());
    for fatal in [
        "::1:2:3:4:5:6:7:8:9",
        "1:2:3:4:5:6:7:8:9::",
        "1:2:3:4:5:6:7::8:9",
    ] {
        assert_eq!(is(fatal), None, "{fatal}");
    }
    assert_eq!(is_ip_address(b"\xff::1"), Some(false));
    assert_eq!(is("10.0.0.1\n"), Some(true));
    assert_eq!(is("10.0.0.1\n\n"), Some(false));
    assert_eq!(is("1.2.3"), Some(false));
    assert_eq!(is("1:2:3:4:5:6:1.2.3.4"), Some(true));
    assert_eq!(is("1:2:3:4:5:6:1.2.3.04"), Some(false));
    assert_eq!(is("1:2:3:4:5:6:1.2.3.256"), Some(false));
    assert_eq!(is("::ffff:1.2.3.4"), Some(true));
    assert_eq!(is("ffff::"), Some(true));
    assert_eq!(is("1::2:3"), Some(true));
    let schema = json_decode(br#"{"type":"string","format":"ip"}"#).unwrap();
    let fatal = Value::String(b"1:2:3:4:5:6:7::8:9".to_vec());
    assert_eq!(
        validate_value_from_schema(&fatal, &schema, "p"),
        Err(Refusal(None))
    );
}
