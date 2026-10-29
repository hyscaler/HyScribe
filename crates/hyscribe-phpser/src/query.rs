// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP's query string functions: `parse_str()`, which also fills `$_GET`,
//! and the URL encodings. `parse_str()` takes untrusted input, so the fuzz
//! target `phpser_parse_str` covers it.

use crate::value::{Key, Value};

/// `max_input_vars`: pairs past this many are dropped.
const MAX_INPUT_VARS: usize = 1000;

/// `max_input_nesting_level`: a name with more `[...]` groups is dropped.
const MAX_NESTING: usize = 64;

/// `parse_str( $query, $result )`: the pairs as nested arrays, as
/// `php_default_treat_data()` and `php_register_variable_ex()` build them.
pub fn parse_str(query: &[u8]) -> Value {
    Value::Array(parse_pairs(query))
}

fn parse_pairs(query: &[u8]) -> Vec<(Key, Value)> {
    // The query is a C string to PHP.
    let query = query.split(|&b| b == 0).next().unwrap_or_default();
    let mut result = Vec::new();
    let pairs = query.split(|&b| b == b'&').filter(|pair| !pair.is_empty());
    for pair in pairs.take(MAX_INPUT_VARS) {
        let (name, value) = match pair.iter().position(|&b| b == b'=') {
            Some(at) => (
                pair.get(..at).unwrap_or_default(),
                pair.get(at + 1..).unwrap_or_default(),
            ),
            None => (pair, &b""[..]),
        };
        register(&mut result, &urldecode(name), urldecode(value));
    }
    result
}

/// `php_register_variable_ex()`: `name`, decoded, read as a base name and
/// `[...]` groups. Names are C strings, so they end at a NUL byte.
fn register(result: &mut Vec<(Key, Value)>, name: &[u8], value: Vec<u8>) {
    let name = name.split(|&b| b == 0).next().unwrap_or_default();
    let spaces = name.iter().take_while(|&&b| b == b' ').count();
    let name = name.get(spaces..).unwrap_or_default();
    let bracket = name.iter().position(|&b| b == b'[');
    // PHP names hold no ` `, `.` or `[`; the base name ends at its first `[`.
    let mangle = |part: &[u8]| -> Vec<u8> {
        part.iter()
            .map(|&b| {
                if matches!(b, b' ' | b'.' | b'[') {
                    b'_'
                } else {
                    b
                }
            })
            .collect()
    };
    let base = mangle(
        name.get(..bracket.unwrap_or(name.len()))
            .unwrap_or_default(),
    );
    if base.is_empty() {
        return;
    }
    let mut base = base;
    let mut path: Vec<Option<Vec<u8>>> = Vec::new();
    if let Some(bracket) = bracket {
        let mut at = bracket;
        loop {
            if path.len() >= MAX_NESTING {
                return;
            }
            let start = at + 1;
            let skip = usize::from(name.get(start) == Some(&b' '));
            if name.get(start + skip) == Some(&b']') {
                path.push(None);
                at = start + skip;
            } else {
                let Some(close) = name
                    .get(start..)
                    .and_then(|rest| rest.iter().position(|&b| b == b']'))
                    .map(|p| start + p)
                else {
                    // Not an index: on the first group the `[` and the rest
                    // join the base name; deeper, the rest is dropped.
                    if path.is_empty() {
                        base.push(b'_');
                        base.extend(mangle(name.get(start..).unwrap_or_default()));
                    }
                    break;
                };
                path.push(Some(name.get(start..close).unwrap_or_default().to_vec()));
                at = close;
            }
            if name.get(at + 1) != Some(&b'[') {
                break;
            }
            at += 1;
        }
    }
    insert(result, Some(&base), &path, Value::String(value));
}

/// Puts `value` at `key` and then `path` inside `array`, making arrays on
/// the way and replacing what is not one; `None` appends at the next
/// integer key.
fn insert(
    array: &mut Vec<(Key, Value)>,
    key: Option<&[u8]>,
    path: &[Option<Vec<u8>>],
    value: Value,
) {
    let place = |old: Value| match path.split_first() {
        None => value,
        Some((next, rest)) => {
            let mut inner = match old {
                Value::Array(inner) => inner,
                _ => Vec::new(),
            };
            insert(&mut inner, next.as_deref(), rest, value);
            Value::Array(inner)
        }
    };
    match key {
        Some(key) => set(array, Key::from_php_string(key.to_vec()), place),
        None => {
            if let Some(next) = next_index(array) {
                array.push((Key::Int(next), place(Value::Null)));
            }
        }
    }
}

/// `$array[ $key ] = f( $array[ $key ] )`: in place, or appended.
fn set(array: &mut Vec<(Key, Value)>, key: Key, f: impl FnOnce(Value) -> Value) {
    for (k, v) in array.iter_mut() {
        if *k == key {
            *v = f(std::mem::take(v));
            return;
        }
    }
    array.push((key, f(Value::Null)));
}

/// `zend_hash_next_index_insert()`'s key: one past the largest integer key,
/// 0 for none, and none past `PHP_INT_MAX`.
fn next_index(array: &[(Key, Value)]) -> Option<i64> {
    let largest = array
        .iter()
        .filter_map(|(key, _)| match key {
            Key::Int(n) => Some(*n),
            Key::String(_) => None,
        })
        .max();
    match largest {
        None => Some(0),
        Some(n) => n.checked_add(1),
    }
}

/// `add_query_arg( $args, $uri )`: each `(key, value)` set in the
/// query, `None` removing the key, and the existing pairs re-encoded.
pub fn add_query_arg(args: &[(&[u8], Option<&[u8]>)], uri: &[u8]) -> Vec<u8> {
    let args: Vec<(Key, Value)> = args
        .iter()
        .map(|&(key, value)| {
            let value = value.map_or(Value::Bool(false), |v| Value::String(v.to_vec()));
            (Key::from_php_string(key.to_vec()), value)
        })
        .collect();
    add_query_values(&args, uri)
}

/// `add_query_arg( $args, $uri )` with PHP values, arrays among them;
/// `false` removes a key.
pub fn add_query_values(args: &[(Key, Value)], uri: &[u8]) -> Vec<u8> {
    let (uri, fragment) = match uri.iter().position(|&b| b == b'#') {
        Some(at) => (
            uri.get(..at).unwrap_or_default(),
            uri.get(at..).unwrap_or_default(),
        ),
        None => (uri, &b""[..]),
    };
    let starts = |prefix: &[u8]| {
        uri.get(..prefix.len())
            .is_some_and(|p| p.eq_ignore_ascii_case(prefix))
    };
    let protocol: &[u8] = if starts(b"http://") {
        b"http://"
    } else if starts(b"https://") {
        b"https://"
    } else {
        b""
    };
    let uri = uri.get(protocol.len()..).unwrap_or_default();
    let (base, query) = match uri.iter().position(|&b| b == b'?') {
        Some(at) => (
            uri.get(..=at).unwrap_or_default().to_vec(),
            uri.get(at + 1..).unwrap_or_default(),
        ),
        None if !protocol.is_empty() || !uri.contains(&b'=') => ([uri, b"?"].concat(), &b""[..]),
        None => (Vec::new(), uri),
    };
    let mut pairs: Vec<(Key, Value)> = parse_pairs(query)
        .into_iter()
        .map(|(key, value)| (key, urlencode_deep(value)))
        .collect();
    // A removed key is set to `false` and unset after the others are set.
    for (key, value) in args {
        set(&mut pairs, key.clone(), |_| value.clone());
    }
    pairs.retain(|(_, value)| *value != Value::Bool(false));
    let query = build_query(&pairs);
    let query = trim_question_marks(&query, true);
    let joined = [protocol, &base, &without_empty_values(query), fragment].concat();
    let joined = trim_question_marks(&joined, false);
    drop_question_mark_before_hash(joined)
}

/// `remove_query_arg( $keys, $uri )`, which removes one key at a time.
pub fn remove_query_arg(keys: &[&[u8]], uri: &[u8]) -> Vec<u8> {
    keys.iter().fold(uri.to_vec(), |uri, &key| {
        add_query_arg(&[(key, None)], &uri)
    })
}

/// `build_query( $data )`: `_http_build_query()` without encoding.
pub fn build_query(pairs: &[(Key, Value)]) -> Vec<u8> {
    http_build_query(pairs, b"")
}

fn http_build_query(pairs: &[(Key, Value)], parent: &[u8]) -> Vec<u8> {
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for (key, value) in pairs {
        let key = match key {
            Key::Int(n) => n.to_string().into_bytes(),
            Key::String(key) => key.clone(),
        };
        // `! empty( $key )`, so the children of a key `0` lose their prefix.
        let key = if crate::is_empty_string(parent) {
            key
        } else {
            [parent, b"%5B", &key, b"%5D"].concat()
        };
        // The values are strings and arrays.
        match value {
            Value::Array(children) => parts.push(http_build_query(children, &key)),
            value => parts.push([&key[..], b"=", &value.to_php_string()].concat()),
        }
    }
    parts.join(&b'&')
}

/// `map_deep( $value, 'urlencode' )`.
pub fn urlencode_deep(value: Value) -> Value {
    map_deep(value, urlencode)
}

/// `map_deep( $value, 'rawurlencode' )`.
pub fn rawurlencode_deep(value: Value) -> Value {
    map_deep(value, rawurlencode)
}

/// `map_deep()`: `encode` applied to every value, array keys left as they are.
fn map_deep(value: Value, encode: fn(&[u8]) -> Vec<u8>) -> Value {
    match value {
        Value::Array(pairs) => Value::Array(
            pairs
                .into_iter()
                .map(|(k, v)| (k, map_deep(v, encode)))
                .collect(),
        ),
        value => Value::String(encode(&value.to_php_string())),
    }
}

/// `trim( $text, '?' )`, or `rtrim()` when `both` is false.
fn trim_question_marks(text: &[u8], both: bool) -> &[u8] {
    let end = text.iter().rposition(|&b| b != b'?').map_or(0, |at| at + 1);
    let start = if both {
        text.iter().position(|&b| b != b'?').unwrap_or(end)
    } else {
        0
    };
    text.get(start..end).unwrap_or_default()
}

/// `preg_replace( '#=(&|$)#', '$1', $query )`, where `$` also matches
/// before a final newline.
fn without_empty_values(query: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(query.len());
    for (at, &b) in query.iter().enumerate() {
        let rest = query.get(at + 1..).unwrap_or_default();
        if b == b'=' && (rest.is_empty() || rest == b"\n" || rest.first() == Some(&b'&')) {
            continue;
        }
        out.push(b);
    }
    out
}

/// `str_replace( '?#', '#', $text )`.
fn drop_question_mark_before_hash(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for (at, &b) in text.iter().enumerate() {
        if b != b'?' || text.get(at + 1) != Some(&b'#') {
            out.push(b);
        }
    }
    out
}

/// `urldecode()`: `+` as a space and `%XX` as its byte; any other `%` stays.
pub fn urldecode(text: &[u8]) -> Vec<u8> {
    let hex = |b: u8| char::from(b).to_digit(16);
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while let Some(&b) = text.get(at) {
        let pair = text
            .get(at + 1)
            .copied()
            .and_then(hex)
            .zip(text.get(at + 2).copied().and_then(hex));
        match (b, pair) {
            (b'+', _) => out.push(b' '),
            (b'%', Some((high, low))) => {
                out.push(u8::try_from(high * 16 + low).unwrap_or_default());
                at += 2;
            }
            _ => out.push(b),
        }
        at += 1;
    }
    out
}

/// `urlencode()`: letters, digits and `-_.` kept, spaces as `+`.
pub fn urlencode(text: &[u8]) -> Vec<u8> {
    percent_encode(text, false)
}

/// `rawurlencode()`: letters, digits and `-_.~` kept.
pub fn rawurlencode(text: &[u8]) -> Vec<u8> {
    percent_encode(text, true)
}

fn percent_encode(text: &[u8], raw: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for &b in text {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => out.push(b),
            b'~' if raw => out.push(b),
            b' ' if !raw => out.push(b'+'),
            _ => out.extend(format!("%{b:02X}").into_bytes()),
        }
    }
    out
}

#[cfg(test)]
mod tests;
