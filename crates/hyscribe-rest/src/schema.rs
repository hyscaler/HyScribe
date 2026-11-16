// SPDX-License-Identifier: AGPL-3.0-or-later

//! `rest_validate_value_from_schema()` and `rest_sanitize_value_from_schema()`
//! for every type and keyword but `anyOf`, `pattern`,
//! `patternProperties` and a `format` other than `date-time`, `email` and
//! `uri`, which a
//! schema using them is refused for.

use hyscribe_kses::esc_url_raw;
use hyscribe_phpser::{Key, Pattern, Value, encode, json_encode};
use hyscribe_render::date::strtotime;
use hyscribe_render::formatting::{is_email, sanitize_text_field};

use crate::response::{RestError, object, text};

/// Why a value is not taken: the error WordPress gives, or none when its
/// schema uses a keyword not ported yet.
#[derive(Clone, Debug, PartialEq)]
pub struct Refusal(pub Option<RestError>);

/// The outcome of a check.
pub type Checked<T> = Result<T, Refusal>;

pub(crate) const UNPORTED: [&str; 4] = ["anyOf", "pattern", "patternProperties", "format"];
const TYPES: [&str; 7] = [
    "array", "object", "string", "number", "integer", "boolean", "null",
];

fn invalid<T>(code: &'static str, message: String) -> Checked<T> {
    Err(Refusal(Some(RestError {
        code,
        message,
        data: Value::Null,
    })))
}

fn invalid_type<T>(param: &str, ty: &str) -> Checked<T> {
    Err(Refusal(Some(RestError {
        code: "rest_invalid_type",
        message: format!("{param} is not of type {ty}."),
        data: object([("param", text(param))]),
    })))
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `isset( $schema[ $key ] )`.
fn get<'a>(schema: &'a Value, key: &str) -> Option<&'a Value> {
    schema.get_str(key).filter(|value| **value != Value::Null)
}

/// `! empty( $schema[ $key ] )`.
fn set(schema: &Value, key: &str) -> bool {
    schema.get_str(key).is_some_and(|value| !value.is_empty())
}

fn is_scalar(value: &Value) -> bool {
    matches!(
        value,
        Value::Bool(_) | Value::Int(_) | Value::Float(_) | Value::String(_)
    )
}

fn entries(value: &Value) -> &[(Key, Value)] {
    value.as_array().unwrap_or_default()
}

/// `wp_is_numeric_array()`.
pub(crate) fn is_list(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|entries| entries.iter().all(|(key, _)| matches!(key, Key::Int(_))))
}

/// `wp_parse_list()`: a scalar split at white space and commas, an array's
/// scalars.
pub(crate) fn parse_list(value: &Value) -> Vec<(Key, Value)> {
    if let Value::Array(entries) = value {
        return entries
            .iter()
            .filter(|(_, item)| is_scalar(item))
            .cloned()
            .collect();
    }
    let separator = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r' | b',');
    value
        .to_php_string()
        .split(separator)
        .filter(|part| !part.is_empty())
        .zip(0..)
        .map(|(part, i)| (Key::Int(i), Value::String(part.to_vec())))
        .collect()
}

/// `wp_parse_slug_list()`: [`parse_list`] of `sanitize_title()`, each once.
pub(crate) fn parse_slug_list(value: &Value) -> Vec<Vec<u8>> {
    let mut slugs: Vec<Vec<u8>> = Vec::new();
    for (_, item) in parse_list(value) {
        let slug = hyscribe_render::formatting::sanitize_title(&item.to_php_string());
        if !slugs.contains(&slug) {
            slugs.push(slug);
        }
    }
    slugs
}

fn values(entries: Vec<(Key, Value)>) -> Vec<(Key, Value)> {
    (0..)
        .map(Key::Int)
        .zip(entries.into_iter().map(|(_, value)| value))
        .collect()
}

/// `rest_is_array()`.
fn is_array(value: &Value) -> bool {
    is_scalar(value) || is_list(value)
}

/// `rest_sanitize_array()`.
fn sanitize_array(value: &Value) -> Vec<(Key, Value)> {
    match value {
        Value::Array(entries) => values(entries.clone()),
        value => parse_list(value),
    }
}

/// `rest_is_object()`.
fn is_object(value: &Value) -> bool {
    matches!(value, Value::Array(_)) || *value == text("")
}

/// `rest_sanitize_object()`.
fn sanitize_object(value: &Value) -> Vec<(Key, Value)> {
    entries(value).to_vec()
}

/// `rest_is_integer()`: a number without a fraction. WordPress first takes
/// strings of digits as they are, which as floats come out the same.
fn is_integer(value: &Value) -> bool {
    let number = value.to_float();
    value.is_numeric() && number.floor() == number
}

/// `rest_is_boolean()`.
fn is_boolean(value: &Value) -> bool {
    match value {
        Value::Bool(_) => true,
        Value::String(text) => {
            let text = text.to_ascii_lowercase();
            [&b"false"[..], b"true", b"0", b"1"].contains(&text.as_slice())
        }
        Value::Int(n) => matches!(n, 0 | 1),
        _ => false,
    }
}

/// `rest_sanitize_boolean()`.
fn sanitize_boolean(value: &Value) -> bool {
    match value {
        Value::String(text) => {
            !matches!(text.to_ascii_lowercase().as_slice(), b"false" | b"0" | b"")
        }
        value => !value.is_empty(),
    }
}

/// `rest_get_best_type_for_value()`.
fn best_type<'a>(value: &Value, types: &'a [String]) -> Option<&'a str> {
    if *value == text("")
        && let Some(string) = types.iter().find(|ty| *ty == "string")
    {
        return Some(string);
    }
    let fits = |ty: &str| match ty {
        "array" => is_array(value),
        "object" => is_object(value),
        "integer" => is_integer(value),
        "number" => value.is_numeric(),
        "boolean" => is_boolean(value),
        "string" => matches!(value, Value::String(_)),
        "null" => *value == Value::Null,
        _ => false,
    };
    types.iter().map(String::as_str).find(|ty| fits(ty))
}

/// The type to check `value` as: the schema's, or of a list of types the
/// best for it, else the first not built in, as
/// `rest_handle_multi_type_schema()` picks it.
fn resolve(value: &Value, schema: &Value) -> Result<String, String> {
    let ty = schema.get_str("type").unwrap_or(&Value::Null);
    let Value::Array(types) = ty else {
        return Ok(lossy(&ty.to_php_string()));
    };
    let types: Vec<String> = types
        .iter()
        .map(|(_, ty)| lossy(&ty.to_php_string()))
        .collect();
    best_type(value, &types)
        .or_else(|| {
            types
                .iter()
                .map(String::as_str)
                .find(|ty| !TYPES.contains(ty))
        })
        .map(String::from)
        .ok_or_else(|| types.join(","))
}

/// The formats ported.
const FORMATS: [&[u8]; 3] = [b"date-time", b"email", b"uri"];

/// A `pattern` the byte matcher reads as PCRE's `u` mode does: literals
/// and classes of ASCII, with no anchor or Unicode-aware escape.
fn pattern(schema: &Value) -> Option<Pattern> {
    let pattern = schema.get_str("pattern")?.as_bytes()?;
    let simple = pattern.is_ascii()
        && !pattern
            .windows(2)
            .any(|w| matches!(w, [b'\\', c] if b"wWsSdDbB".contains(c)))
        && !pattern.iter().any(|b| b".^$#".contains(b));
    let pattern = std::str::from_utf8(pattern).ok().filter(|_| simple)?;
    Pattern::new(pattern)
}

/// `rest_validate_json_schema_pattern()`: `pattern` matches somewhere in
/// `text`, which must be UTF-8.
fn matches(pattern: &Pattern, text: &[u8]) -> bool {
    std::str::from_utf8(text).is_ok()
        && (0..=text.len()).any(|at| {
            pattern
                .captures(text.get(at..).unwrap_or_default())
                .is_some()
        })
}

fn unported(schema: &Value) -> Checked<()> {
    let format = schema.get_str("format").and_then(Value::as_bytes);
    let ported = |key: &&str| match *key {
        "format" => format.is_some_and(|f| FORMATS.contains(&f)),
        "pattern" => pattern(schema).is_some(),
        _ => false,
    };
    if UNPORTED
        .iter()
        .any(|key| schema.get_str(key).is_some() && !ported(key))
    {
        return Err(Refusal(None));
    }
    Ok(())
}

/// `rest_parse_date( $date )`: an RFC 3339 date and time, its seconds'
/// fraction left out, read by `strtotime()`.
pub(crate) fn parse_date(date: &[u8]) -> Option<i64> {
    let digits = |from: usize, to: usize| {
        date.get(from..to)
            .is_some_and(|d| d.iter().all(u8::is_ascii_digit))
    };
    let at = |i: usize, allowed: &[u8]| date.get(i).is_some_and(|b| allowed.contains(b));
    let shaped = digits(0, 4)
        && at(4, b"-")
        && digits(5, 7)
        && at(7, b"-")
        && digits(8, 10)
        && at(10, b"Tt ")
        && digits(11, 13)
        && at(13, b":")
        && digits(14, 16)
        && at(16, b":")
        && digits(17, 19);
    let mut rest = date.get(19..).filter(|_| shaped)?;
    if let [b'.', fraction @ ..] = rest {
        let length = fraction.iter().take_while(|b| b.is_ascii_digit()).count();
        rest = fraction.get(length..).filter(|_| length > 0)?;
    }
    // `strtotime()` checks the offset's digits.
    let zone = match rest {
        [] | [b'Z'] | [b'+' | b'-', _, _, b':', _, _] => rest.to_vec(),
        [sign @ (b'+' | b'-'), h1, h2] => vec![*sign, *h1, *h2, b':', b'0', b'0'],
        _ => return None,
    };
    let base = date.get(..19).unwrap_or_default().iter().enumerate();
    let mut text: Vec<u8> = base.map(|(i, b)| if i == 10 { b'T' } else { *b }).collect();
    text.extend(zone);
    strtotime(&String::from_utf8_lossy(&text))
}

/// `$value < $bound` of numbers, as floats: integers past 2^53 apart, which
/// PHP compares exactly, are never a schema's bounds.
fn below(value: &Value, bound: &Value) -> bool {
    value.to_float() < bound.to_float()
}

fn above(value: &Value, bound: &Value) -> bool {
    value.to_float() > bound.to_float()
}

/// `number_format_i18n( $n )` in English of a size limit: rounded,
/// thousands separated.
fn number_format(value: &Value) -> String {
    let digits = format!("{:.0}", value.to_float().abs().round());
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

/// `_n( $single, $plural, $count )` in English.
fn plural<'a>(count: &Value, single: &'a str, plural: &'a str) -> &'a str {
    if count.to_int() == 1 { single } else { plural }
}

/// `count( $value ) < $min` or `> $max` of a schema's limits on a size.
fn sizes(
    size: usize,
    schema: &Value,
    (min, max): (&str, &str),
    param: &str,
    (code_few, code_many, noun, nouns): (&'static str, &'static str, &str, &str),
) -> Checked<()> {
    let size = Value::Int(i64::try_from(size).unwrap_or(i64::MAX));
    if let Some(min) = get(schema, min).filter(|min| below(&size, min)) {
        let noun = plural(min, noun, nouns);
        return invalid(
            code_few,
            format!(
                "{param} must contain at least {} {noun}.",
                number_format(min)
            ),
        );
    }
    if let Some(max) = get(schema, max).filter(|max| above(&size, max)) {
        let noun = plural(max, noun, nouns);
        return invalid(
            code_many,
            format!(
                "{param} must contain at most {} {noun}.",
                number_format(max)
            ),
        );
    }
    Ok(())
}

/// `schema` with the type `ty` set.
fn with_type(schema: &Value, ty: Option<&Value>) -> Value {
    let mut entries = entries(schema).to_vec();
    entries.retain(|(key, _)| key.to_php_string() != b"type");
    entries.push((
        Key::String(b"type".to_vec()),
        ty.cloned().unwrap_or(Value::Null),
    ));
    Value::Array(entries)
}

/// `wp_sprintf()`'s `%l` of two items or more: joined, the last with `and`.
fn listed(items: &[String]) -> String {
    match items {
        [a, b] => format!("{a} and {b}"),
        _ => items
            .split_last()
            .map(|(last, rest)| format!("{}, and {last}", rest.join(", ")))
            .unwrap_or_default(),
    }
}

/// A `oneOf` schema that did not match: its index and itself, and why.
type Mismatch = (i64, Value, RestError);

/// `rest_format_combining_operation_error()`.
fn combining_format(param: &str, (index, schema, error): &Mismatch) -> Refusal {
    let reason = &error.message;
    let message = match get(schema, "title") {
        Some(title) => {
            let title = lossy(&title.to_php_string());
            format!("{param} is not a valid {title}. Reason: {reason}")
        }
        None => format!("{param} does not match the expected format. Reason: {reason}"),
    };
    Refusal(Some(RestError {
        code: "rest_no_matching_schema",
        message,
        data: object([("position", Value::Int(*index))]),
    }))
}

/// `rest_get_combining_operation_error()`.
fn combining_error(value: &Value, param: &str, errors: &[Mismatch]) -> Refusal {
    if let [error] = errors {
        return combining_format(param, error);
    }
    let filtered: Vec<&Mismatch> = errors
        .iter()
        .filter(|(_, _, error)| {
            let other = error.data.get_str("param").map(Value::to_php_string);
            error.code != "rest_invalid_type"
                || other.is_some_and(|other| other != param.as_bytes())
        })
        .collect();
    if let [error] = filtered.as_slice() {
        return combining_format(param, error);
    }
    let objects = filtered
        .first()
        .is_some_and(|(_, schema, _)| schema.get_str("type") == Some(&text("object")));
    if objects {
        // The schema sharing the most properties with the value.
        let mut best: Option<&Mismatch> = None;
        let mut most = 0;
        for error in &filtered {
            let properties = entries(get(&error.1, "properties").unwrap_or(&Value::Null));
            let shared = properties
                .iter()
                .filter(|(name, _)| entries(value).iter().any(|(key, _)| key == name))
                .count();
            if shared > most {
                best = Some(error);
                most = shared;
            }
        }
        if let Some(best) = best {
            return combining_format(param, best);
        }
    }
    let titles: Vec<String> = errors
        .iter()
        .filter_map(|(_, schema, _)| get(schema, "title"))
        .map(|title| lossy(&title.to_php_string()))
        .collect();
    let message = if titles.len() == errors.len() {
        format!("{param} is not a valid {}.", listed(&titles))
    } else {
        format!("{param} does not match any of the expected formats.")
    };
    Refusal(Some(RestError {
        code: "rest_no_matching_schema",
        message,
        data: Value::Null,
    }))
}

/// `rest_find_one_matching_schema()`: the one schema of `oneOf` that
/// `value` matches, none without the keyword.
fn one_matching(value: &Value, schema: &Value, param: &str) -> Checked<Option<Value>> {
    let Some(options) = get(schema, "oneOf") else {
        return Ok(None);
    };
    let mut matching: Vec<(i64, Value)> = Vec::new();
    let mut errors: Vec<Mismatch> = Vec::new();
    for (index, (_, option)) in (0..).zip(entries(options)) {
        let option = match (get(option, "type"), get(schema, "type")) {
            (None, Some(ty)) => with_type(option, Some(ty)),
            _ => option.clone(),
        };
        match validate_value_from_schema(value, &option, param) {
            Ok(()) => matching.push((index, option)),
            Err(Refusal(Some(error))) => errors.push((index, option, error)),
            Err(refusal) => return Err(refusal),
        }
    }
    match matching.as_slice() {
        [] => Err(combining_error(value, param, &errors)),
        [(_, one)] => Ok(Some(one.clone())),
        _ => {
            let positions = matching
                .iter()
                .map(|(index, _)| Value::Int(*index))
                .collect();
            let titles: Vec<String> = matching
                .iter()
                .filter_map(|(_, schema)| get(schema, "title"))
                .map(|title| lossy(&title.to_php_string()))
                .collect();
            let message = if titles.len() == matching.len() {
                format!(
                    "{param} matches {}, but should match only one.",
                    listed(&titles)
                )
            } else {
                format!("{param} matches more than one of the expected formats.")
            };
            Err(Refusal(Some(RestError {
                code: "rest_one_of_multiple_matches",
                message,
                data: object([("positions", crate::response::list(positions))]),
            })))
        }
    }
}

/// `rest_validate_value_from_schema( $value, $schema, $param )`.
pub fn validate_value_from_schema(value: &Value, schema: &Value, param: &str) -> Checked<()> {
    unported(schema)?;
    let typed;
    let schema = match one_matching(value, schema, param)? {
        Some(one) if get(schema, "type").is_none() && get(&one, "type").is_some() => {
            typed = with_type(schema, get(&one, "type"));
            &typed
        }
        _ => schema,
    };
    let ty = match resolve(value, schema) {
        Ok(ty) => ty,
        Err(types) => return invalid_type(param, &types),
    };
    match ty.as_str() {
        "null" if *value != Value::Null => return invalid_type(param, "null"),
        "boolean" if !is_boolean(value) => return invalid_type(param, "boolean"),
        "object" => validate_object(value, schema, param)?,
        "array" => validate_array(value, schema, param)?,
        "number" => validate_number(value, schema, param, "number")?,
        "integer" => {
            validate_number(value, schema, param, "integer")?;
            if !is_integer(value) {
                return invalid_type(param, "integer");
            }
        }
        "string" => validate_string(value, schema, param)?,
        _ => {}
    }
    if set(schema, "enum") {
        validate_enum(value, schema, &ty, param)?;
    }
    let format = schema.get_str("format").and_then(Value::as_bytes);
    if ty == "string" && format == Some(b"email") && !is_email(&value.to_php_string()) {
        return invalid("rest_invalid_email", "Invalid email address.".into());
    }
    if ty == "string"
        && format == Some(b"date-time")
        && parse_date(&value.to_php_string()).is_none()
    {
        return invalid("rest_invalid_date", "Invalid date.".into());
    }
    Ok(())
}

fn validate_object(value: &Value, schema: &Value, param: &str) -> Checked<()> {
    if !is_object(value) {
        return invalid_type(param, "object");
    }
    let value = sanitize_object(value);
    let has = |name: &[u8]| value.iter().any(|(key, _)| key.to_php_string() == name);
    let properties = get(schema, "properties").unwrap_or(&Value::Null);
    let required: Vec<Vec<u8>> = match get(schema, "required") {
        Some(Value::Array(names)) => names.iter().map(|(_, name)| name.to_php_string()).collect(),
        _ => entries(properties)
            .iter()
            .filter(|(_, property)| property.get_str("required") == Some(&Value::Bool(true)))
            .map(|(name, _)| name.to_php_string())
            .collect(),
    };
    if let Some(name) = required.iter().find(|name| !has(name)) {
        let name = lossy(name);
        return invalid(
            "rest_property_required",
            format!("{name} is a required property of {param}."),
        );
    }
    for (property, item) in &value {
        let name = lossy(&property.to_php_string());
        let inner = format!("{param}[{name}]");
        match (
            properties.get(property).filter(|p| **p != Value::Null),
            get(schema, "additionalProperties"),
        ) {
            (Some(property), _) => validate_value_from_schema(item, property, &inner)?,
            (None, Some(Value::Bool(false))) => {
                return invalid(
                    "rest_additional_properties_forbidden",
                    format!("{name} is not a valid property of Object."),
                );
            }
            (None, Some(additional @ Value::Array(_))) => {
                validate_value_from_schema(item, additional, &inner)?;
            }
            (None, _) => {}
        }
    }
    let codes = (
        "rest_too_few_properties",
        "rest_too_many_properties",
        "property",
        "properties",
    );
    sizes(
        value.len(),
        schema,
        ("minProperties", "maxProperties"),
        param,
        codes,
    )
}

fn validate_array(value: &Value, schema: &Value, param: &str) -> Checked<()> {
    if !is_array(value) {
        return invalid_type(param, "array");
    }
    let value = sanitize_array(value);
    if let Some(items) = get(schema, "items") {
        for (index, item) in &value {
            let index = lossy(&index.to_php_string());
            validate_value_from_schema(item, items, &format!("{param}[{index}]"))?;
        }
    }
    let codes = ("rest_too_few_items", "rest_too_many_items", "item", "items");
    sizes(value.len(), schema, ("minItems", "maxItems"), param, codes)?;
    if set(schema, "uniqueItems") && !unique(&value) {
        return invalid(
            "rest_duplicate_items",
            format!("{param} has duplicate items."),
        );
    }
    Ok(())
}

fn validate_number(value: &Value, schema: &Value, param: &str, ty: &str) -> Checked<()> {
    if !value.is_numeric() {
        return invalid_type(param, ty);
    }
    if let Some(multiple) = get(schema, "multipleOf")
        && value.to_float() % multiple.to_float() != 0.0
    {
        let multiple = lossy(&multiple.to_php_string());
        return invalid(
            "rest_invalid_multiple",
            format!("{param} must be a multiple of {multiple}."),
        );
    }
    let (min, max) = (get(schema, "minimum"), get(schema, "maximum"));
    let (open_min, open_max) = (
        set(schema, "exclusiveMinimum"),
        set(schema, "exclusiveMaximum"),
    );
    let out = |message: String| invalid("rest_out_of_bounds", format!("{param} must be {message}"));
    match (min, max) {
        (Some(min), None) => {
            let low = below(value, min) || open_min && !above(value, min);
            if low {
                let or_equal = if open_min { "" } else { " or equal to" };
                return out(format!("greater than{or_equal} {}", min.to_int()));
            }
        }
        (None, Some(max)) => {
            let high = above(value, max) || open_max && !below(value, max);
            if high {
                let or_equal = if open_max { "" } else { " or equal to" };
                return out(format!("less than{or_equal} {}", max.to_int()));
            }
        }
        (Some(min), Some(max)) => {
            let low = below(value, min) || open_min && !above(value, min);
            let high = above(value, max) || open_max && !below(value, max);
            if low || high {
                let ends = |open| if open { "exclusive" } else { "inclusive" };
                return out(format!(
                    "between {} ({}) and {} ({})",
                    min.to_int(),
                    ends(open_min),
                    max.to_int(),
                    ends(open_max)
                ));
            }
        }
        (None, None) => {}
    }
    Ok(())
}

/// `mb_strlen()` of UTF-8, each invalid sequence one character.
fn mb_strlen(text: &[u8]) -> usize {
    text.utf8_chunks()
        .map(|chunk| chunk.valid().chars().count() + usize::from(!chunk.invalid().is_empty()))
        .sum()
}

fn validate_string(value: &Value, schema: &Value, param: &str) -> Checked<()> {
    let Value::String(text) = value else {
        return invalid_type(param, "string");
    };
    let length = Value::Int(i64::try_from(mb_strlen(text)).unwrap_or(i64::MAX));
    if let Some(min) = get(schema, "minLength").filter(|min| below(&length, min)) {
        let noun = plural(min, "character", "characters");
        return invalid(
            "rest_too_short",
            format!(
                "{param} must be at least {} {noun} long.",
                number_format(min)
            ),
        );
    }
    if let Some(max) = get(schema, "maxLength").filter(|max| above(&length, max)) {
        let noun = plural(max, "character", "characters");
        return invalid(
            "rest_too_long",
            format!(
                "{param} must be at most {} {noun} long.",
                number_format(max)
            ),
        );
    }
    if pattern(schema).is_some_and(|pattern| !matches(&pattern, text)) {
        let source = schema.get_str("pattern").map(Value::to_php_string);
        let source = String::from_utf8_lossy(&source.unwrap_or_default()).into_owned();
        return invalid(
            "rest_invalid_pattern",
            format!("{param} does not match pattern {source}."),
        );
    }
    Ok(())
}

/// `rest_are_values_equal()`.
fn equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len()
                && a.iter().all(|(key, item)| {
                    b.iter()
                        .any(|(other, value)| other == key && equal(item, value))
                })
        }
        (Value::Int(_) | Value::Float(_), Value::Int(_) | Value::Float(_)) => {
            a.to_float() == b.to_float()
        }
        _ => a == b,
    }
}

/// `rest_validate_enum()`.
fn validate_enum(value: &Value, schema: &Value, ty: &str, param: &str) -> Checked<()> {
    let choices = entries(schema.get_str("enum").unwrap_or(&Value::Null));
    let sanitized = sanitize_as(value, schema, ty, param);
    sanitized.and_then(|sanitized| {
        match choices.iter().any(|(_, choice)| equal(&sanitized, choice)) {
            true => Ok(()),
            false => not_in_enum(choices, param),
        }
    })
}

/// `rest_not_in_enum` with the choices listed.
fn not_in_enum(choices: &[(Key, Value)], param: &str) -> Checked<()> {
    let shown: Vec<String> = choices
        .iter()
        .map(|(_, choice)| match choice {
            choice if is_scalar(choice) => lossy(&choice.to_php_string()),
            choice => lossy(&json_encode(choice).unwrap_or_default()),
        })
        .collect();
    let message = match shown.len() {
        1 => format!("{param} is not {}.", shown.concat()),
        _ => format!("{param} is not one of {}.", listed(&shown)),
    };
    invalid("rest_not_in_enum", message)
}

/// `rest_stabilize_value()`: arrays' keys in order, all the way down.
fn stabilize(value: &Value) -> Value {
    let Value::Array(entries) = value else {
        return value.clone();
    };
    let mut entries: Vec<(Key, Value)> = entries
        .iter()
        .map(|(key, item)| (key.clone(), stabilize(item)))
        .collect();
    entries.sort_by(|(a, _), (b, _)| match (a, b) {
        (Key::Int(a), Key::Int(b)) => a.cmp(b),
        (a, b) => a.to_php_string().cmp(&b.to_php_string()),
    });
    Value::Array(entries)
}

/// `rest_validate_array_contains_unique_items()`.
fn unique(items: &[(Key, Value)]) -> bool {
    let mut seen: Vec<Vec<u8>> = Vec::new();
    items.iter().all(|(_, item)| {
        let key = encode(&stabilize(item));
        let new = !seen.contains(&key);
        seen.push(key);
        new
    })
}

/// `prepare_value()` of a setting or meta field: the value sanitized by
/// `schema` if it validates, else null.
pub(crate) fn prepare_value(value: &Value, schema: &Value, param: &str) -> Value {
    validate_value_from_schema(value, schema, param)
        .and_then(|()| sanitize_value_from_schema(value, schema, param))
        .unwrap_or(Value::Null)
}

/// `rest_sanitize_value_from_schema( $value, $schema, $param )`.
pub fn sanitize_value_from_schema(value: &Value, schema: &Value, param: &str) -> Checked<Value> {
    unported(schema)?;
    let (typed, sanitized);
    let (value, schema) = match one_matching(value, schema, param)? {
        Some(one) => {
            // PHP goes on with an error, which arrays and objects sanitize as empty.
            sanitized = sanitize_value_from_schema(value, &one, param).unwrap_or(Value::Null);
            let schema = match get(schema, "type") {
                Some(_) => schema,
                None => {
                    typed = with_type(schema, get(&one, "type"));
                    &typed
                }
            };
            (&sanitized, schema)
        }
        None => (value, schema),
    };
    match resolve(value, schema) {
        Ok(ty) => sanitize_as(value, schema, &ty, param),
        Err(_) => Ok(Value::Null),
    }
}

fn sanitize_as(value: &Value, schema: &Value, ty: &str, param: &str) -> Checked<Value> {
    let inner = |key: &Key| format!("{param}[{}]", lossy(&key.to_php_string()));
    Ok(match ty {
        "array" => {
            let items_schema = schema.get_str("items").filter(|items| !items.is_empty());
            let items: Checked<Vec<(Key, Value)>> = sanitize_array(value)
                .into_iter()
                .map(|(index, item)| match items_schema {
                    Some(items) => sanitize_value_from_schema(&item, items, &inner(&index))
                        .map(|item| (index, item)),
                    None => Ok((index, item)),
                })
                .collect();
            return items.and_then(|items| {
                if set(schema, "uniqueItems") && !unique(&items) {
                    return invalid(
                        "rest_duplicate_items",
                        format!("{param} has duplicate items."),
                    );
                }
                Ok(Value::Array(items))
            });
        }
        "object" => {
            let properties = get(schema, "properties").unwrap_or(&Value::Null);
            let additional = get(schema, "additionalProperties");
            let kept: Checked<Vec<(Key, Value)>> = sanitize_object(value)
                .into_iter()
                .filter_map(|(property, item)| {
                    let sanitized = match (
                        properties.get(&property).filter(|p| **p != Value::Null),
                        additional,
                    ) {
                        (Some(schema), _) | (None, Some(schema @ Value::Array(_))) => {
                            sanitize_value_from_schema(&item, schema, &inner(&property))
                        }
                        (None, Some(Value::Bool(false))) => return None,
                        (None, _) => Ok(item),
                    };
                    Some(sanitized.map(|item| (property, item)))
                })
                .collect();
            return kept.map(Value::Array);
        }
        "null" => Value::Null,
        "integer" => Value::Int(value.to_int()),
        "number" => Value::Float(hyscribe_phpser::Float::from_f64(value.to_float())),
        "boolean" => Value::Bool(sanitize_boolean(value)),
        "string" => match schema.get_str("format").and_then(Value::as_bytes) {
            Some(b"email" | b"date-time") => {
                Value::String(sanitize_text_field(&value.to_php_string()))
            }
            Some(b"uri") => Value::String(esc_url_raw(&value.to_php_string())),
            _ => Value::String(value.to_php_string()),
        },
        _ => value.clone(),
    })
}

#[cfg(test)]
mod tests;
