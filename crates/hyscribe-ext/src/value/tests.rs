// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::Key;

use super::*;

fn owned((kind, int, float, text): Lowered<'_>) -> Lifted {
    (kind, int, float, text.to_owned())
}

fn round(value: &Value) -> Option<Value> {
    lower(value).and_then(|crossing| lift(owned(crossing.lowered())))
}

#[test]
fn scalars_and_text_cross_as_they_are() {
    let cases = [
        (Value::Null, (Kind::Null, 0, 0.0, "")),
        (Value::Bool(true), (Kind::Boolean, 1, 0.0, "")),
        (Value::Bool(false), (Kind::Boolean, 0, 0.0, "")),
        (Value::Int(-3), (Kind::Int, -3, 0.0, "")),
        (
            Value::Float(Float::from_f64(1.5)),
            (Kind::Float, 0, 1.5, ""),
        ),
        (Value::String("é".into()), (Kind::Text, 0, 0.0, "é")),
    ];
    for (value, lowered) in cases {
        let crossing = lower(&value);
        assert_eq!(crossing.as_ref().map(Crossing::lowered), Some(lowered));
        assert_eq!(round(&value), Some(value));
    }
}

#[test]
fn arrays_and_objects_cross_as_json() {
    let array = Value::Array(vec![
        (Key::Int(0), Value::Int(1)),
        (Key::String("a".into()), Value::String("b/c".into())),
    ]);
    let crossing = lower(&array);
    assert_eq!(
        crossing.as_ref().map(Crossing::lowered),
        Some((Kind::Json, 0, 0.0, r#"{"0":1,"a":"b\/c"}"#))
    );
    assert_eq!(round(&array), Some(array));
    let object = Value::Object {
        class: "WP_Post".into(),
        properties: vec![(Key::String("ID".into()), Value::Int(1))],
    };
    assert_eq!(
        round(&object),
        Some(Value::Array(vec![(
            Key::String("ID".into()),
            Value::Int(1)
        )]))
    );
}

#[test]
fn what_cannot_cross_does_not() {
    assert!(lower(&Value::String(vec![0xff])).is_none());
    assert!(lower(&Value::Enum("A:B".into())).is_none());
    let values = [Value::Int(1), Value::String(vec![0xff])];
    assert!(lower_all(&values).is_none());
    assert_eq!(lower_all(&values[..1]).map(|all| all.len()), Some(1));
    assert_eq!(lift((Kind::Json, 0, 0.0, "{".into())), None);
    assert_eq!(
        lift_all(vec![(Kind::Int, 2, 0.0, String::new())]),
        Some(vec![Value::Int(2)])
    );
    assert_eq!(
        lift_all(vec![
            (Kind::Int, 2, 0.0, String::new()),
            (Kind::Json, 0, 0.0, "x".into())
        ]),
        None
    );
}
