// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

/// PHP's `urlencode()`, `rawurlencode()` and `urldecode()` of the same
/// strings.
#[test]
fn encodes_like_php() {
    let text = b"http://Example.test/?p=1&x=a b~*";
    assert_eq!(
        urlencode(text),
        b"http%3A%2F%2FExample.test%2F%3Fp%3D1%26x%3Da+b%7E%2A"
    );
    assert_eq!(
        rawurlencode(text),
        b"http%3A%2F%2FExample.test%2F%3Fp%3D1%26x%3Da%20b~%2A"
    );
    assert_eq!(urlencode(b"AZaz09-_.!"), b"AZaz09-_.%21");
    assert_eq!(urldecode(b"a+b%41%4%zz%"), b"a bA%4%zz%");
}

/// `map_deep()` encodes every value, nested ones too, and no key.
#[test]
fn encodes_deep_values() {
    let value = parse_str(b"a%20b=c%20d&x[y%20z]=~%20");
    let string = |s: &[u8]| Value::String(s.to_vec());
    let encoded = |c: &[u8], tilde: &[u8]| {
        Value::Array(vec![
            (Key::String(b"a_b".to_vec()), string(c)),
            (
                Key::String(b"x".to_vec()),
                Value::Array(vec![(Key::String(b"y z".to_vec()), string(tilde))]),
            ),
        ])
    };
    assert_eq!(urlencode_deep(value.clone()), encoded(b"c+d", b"%7E+"));
    assert_eq!(rawurlencode_deep(value), encoded(b"c%20d", b"~%20"));
}

/// Long names, deep nesting and many pairs stay fast.
#[test]
fn linear_time() {
    let started = std::time::Instant::now();
    let deep = [&b"a"[..], &b"[x]".repeat(64), b"=1&"]
        .concat()
        .repeat(20_000);
    let unclosed = [&b"a["[..], &b"[".repeat(4_000_000)].concat();
    let appends = b"a[]=1&".repeat(200_000);
    for query in [deep, unclosed, appends, b"%".repeat(4_000_000)] {
        parse_str(&query);
        add_query_arg(&[(b"x", None)], &[b"/?", &query[..]].concat());
    }
    assert!(started.elapsed().as_secs() < 20, "{:?}", started.elapsed());
}
