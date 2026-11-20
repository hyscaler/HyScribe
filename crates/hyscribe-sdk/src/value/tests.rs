// SPDX-License-Identifier: Apache-2.0 OR MIT

use super::*;

#[test]
fn reads_what_it_holds() {
    assert_eq!(Value::from("a").text(), Some("a"));
    assert_eq!(Value::from(String::from("b")), Value::Text("b".into()));
    assert_eq!(Value::Int(3).text(), None);
    assert_eq!(Value::from(3).int(), Some(3));
    assert_eq!(Value::Text("3".into()).int(), None);
    assert_eq!(Value::from(true), Value::Bool(true));
}

crate::callbacks! {
    enum Callback {
        First => first,
        Second => second,
    }
}

fn first(_: Vec<Value>) -> Result<Value, String> {
    Ok(Value::Int(1))
}

fn second(args: Vec<Value>) -> Result<Value, String> {
    args.into_iter().next().ok_or_else(|| "no argument".into())
}

#[test]
fn callbacks_are_numbered_in_order() {
    assert_eq!(u32::from(Callback::First), 0);
    assert_eq!(u32::from(Callback::Second), 1);
    assert_eq!(Callback::invoke(0, vec![]), Ok(Value::Int(1)));
    assert_eq!(Callback::invoke(1, vec![Value::Null]), Ok(Value::Null));
    assert_eq!(Callback::invoke(1, vec![]), Err("no argument".into()));
    assert_eq!(Callback::invoke(2, vec![]), Err("no such callback".into()));
}
