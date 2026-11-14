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
        let schema = object([("type", text("string")), (keyword, text("x"))]);
        assert_eq!(
            validate_value_from_schema(&text("a"), &schema, "p"),
            Err(Refusal(None))
        );
        assert_eq!(
            sanitize_value_from_schema(&text("a"), &schema, "p"),
            Err(Refusal(None))
        );
        let nested = object([("type", text("array")), ("items", schema)]);
        let value = Value::Array(vec![(Key::Int(0), text("a"))]);
        assert_eq!(
            validate_value_from_schema(&value, &nested, "p"),
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
