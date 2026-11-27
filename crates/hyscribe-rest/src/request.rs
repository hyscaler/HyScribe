// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Request`'s parameters: the JSON body's, the form's, the
//! query's, the URL's and the defaults, in that order, checked and
//! sanitized by their route's arguments.

use hyscribe_kses::sanitize_key;
use hyscribe_phpser::{Key, Value, absint, is_empty_string, php_trim, urldecode};
use hyscribe_render::formatting::sanitize_text_field;

use crate::Context;
use crate::response::{NOT_PORTED, RestError, object, text};
use crate::schema::{Checked, Refusal, sanitize_value_from_schema, validate_value_from_schema};

/// The parameters by where they come from.
#[derive(Clone, Debug, Default)]
pub(crate) struct Params {
    /// The regex of the route matched.
    pub(crate) route: &'static str,
    /// The JSON body's, for a JSON content type.
    pub(crate) json: Vec<(Key, Value)>,
    /// `$_POST` and a form body's, for a method that takes a body.
    pub(crate) post: Vec<(Key, Value)>,
    pub(crate) get: Vec<(Key, Value)>,
    pub(crate) url: Vec<(Key, Value)>,
    pub(crate) defaults: Vec<(Key, Value)>,
}

fn find<'a>(source: &'a [(Key, Value)], key: &str) -> Option<&'a Value> {
    let key = Key::String(key.as_bytes().to_vec());
    source
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, value)| value)
}

impl Params {
    fn sources(&self) -> [&[(Key, Value)]; 5] {
        [&self.json, &self.post, &self.get, &self.url, &self.defaults]
    }

    /// `set_param( $key, $value )`: in each source but the defaults that
    /// has it, else in the first.
    pub(crate) fn set(&mut self, key: &str, value: Value) {
        let name = Key::String(key.as_bytes().to_vec());
        let mut found = false;
        for source in [&mut self.json, &mut self.post, &mut self.get, &mut self.url] {
            for (_, held) in source.iter_mut().filter(|(known, _)| *known == name) {
                *held = value.clone();
                found = true;
            }
        }
        if !found {
            self.json.push((name, value));
        }
    }

    /// `get_param( $key )`: the first set.
    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        self.sources()
            .into_iter()
            .find_map(|source| find(source, key).filter(|value| **value != Value::Null))
    }

    /// `has_param( $key )`.
    pub(crate) fn has(&self, key: &str) -> bool {
        self.sources()
            .into_iter()
            .any(|source| find(source, key).is_some())
    }

    /// `$request[ $key ]` as text, empty when not set.
    pub(crate) fn text(&self, key: &str) -> Vec<u8> {
        self.get(key).map(Value::to_php_string).unwrap_or_default()
    }

    /// `$request['context']`, a string after sanitizing.
    pub(crate) fn context(&self) -> Vec<u8> {
        self.text("context")
    }
}

/// How an argument is sanitized.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Sanitize {
    /// `sanitize_key()`.
    Key,
    /// `absint()`.
    Absint,
    /// `sanitize_text_field()`.
    TextField,
    /// `rest_parse_request_arg()`: validated, then sanitized, by its schema.
    Schema,
    /// `rest_sanitize_request_arg()`: sanitized by its schema.
    Value,
    /// A setting's: null as given, else as [`Sanitize::Schema`].
    Nullable,
    /// `urldecode()` of a URL parameter, a string.
    Urldecode,
    /// Left as given, for the endpoint to check: a schema whose keywords
    /// are not ported.
    Endpoint,
    /// Not ported: a request that has it is answered 501 before its checks.
    NotPorted,
    /// `check_meta_is_array()` validates it, and it is left as given.
    IsArray,
    /// The endpoint's own sanitizer, which reads the site.
    Site(fn(&Context, &Value) -> Checked<Value>),
    /// The endpoint's own sanitizer, which reads the other parameters as
    /// they were before sanitizing.
    Params(fn(&Params, &Value) -> Checked<Value>),
    /// A `validate_callback` returning false (`Invalid parameter.`) or the
    /// value its `sanitize_callback` gives, reading the site.
    Callback(fn(&Context, &Value) -> Option<Value>),
    /// A `validate_callback` reading the site and the request, whose
    /// error is the parameter's, then `rest_sanitize_request_arg()`.
    Validated(fn(&Context, &Params, &Value) -> Result<(), RestError>),
}

/// A route's argument: its schema, whether `rest_validate_request_arg()`
/// validates it and how it is sanitized.
#[derive(Clone, Debug)]
pub(crate) struct Arg {
    pub(crate) name: &'static str,
    pub(crate) schema: Value,
    pub(crate) validate: bool,
    pub(crate) sanitize: Sanitize,
}

/// The arguments' defaults, those set.
pub(crate) fn defaults(args: &[Arg]) -> Vec<(Key, Value)> {
    args.iter()
        .filter_map(|arg| {
            let default = arg
                .schema
                .get_str("default")
                .filter(|v| **v != Value::Null)?;
            Some((Key::String(arg.name.as_bytes().to_vec()), default.clone()))
        })
        .collect()
}

/// The invalid parameters and the errors each gives, none for a
/// `validate_callback` that returned false.
#[derive(Default)]
struct Invalid(Vec<(String, Option<RestError>)>);

/// `rest_invalid_param` for `name`, as a `validate_callback` returning
/// false gives it.
pub(crate) fn invalid_param(name: &str) -> RestError {
    Invalid(vec![(name.into(), None)]).error()
}

impl Invalid {
    /// `rest_invalid_param` of them all, if any.
    fn check(self) -> Result<(), RestError> {
        if self.0.is_empty() {
            return Ok(());
        }
        Err(self.error())
    }

    /// `rest_invalid_param` of them all.
    fn error(self) -> RestError {
        let key = |name: &str| Key::String(name.as_bytes().to_vec());
        let names: Vec<&str> = self.0.iter().map(|(name, _)| name.as_str()).collect();
        let message = |error: &Option<RestError>| {
            error
                .as_ref()
                .map_or("Invalid parameter.".into(), |error| error.message.clone())
        };
        let params = self
            .0
            .iter()
            .map(|(name, error)| (key(name), text(message(error))))
            .collect();
        let details = self
            .0
            .iter()
            .filter_map(|(name, error)| {
                let error = error.as_ref()?;
                let detail = object([
                    ("code", text(error.code)),
                    ("message", text(error.message.clone())),
                    ("data", error.data.clone()),
                ]);
                Some((key(name), detail))
            })
            .collect();
        RestError {
            code: "rest_invalid_param",
            message: format!("Invalid parameter(s): {}", names.join(", ")),
            data: object([
                ("status", Value::Int(400)),
                ("params", Value::Array(params)),
                ("details", Value::Array(details)),
            ]),
        }
    }
}

/// An invalid parameter noted. Ported arguments use only ported keywords
/// (`tests::arguments_use_ported_keywords`), so a refusal always carries
/// its error.
fn refused<T>(result: Result<T, Refusal>, name: &str, invalid: &mut Invalid) -> Option<T> {
    result
        .map_err(|Refusal(error)| {
            invalid
                .0
                .extend(error.map(|error| (name.into(), Some(error))))
        })
        .ok()
}

/// `has_valid_params()`: every required argument is set, and each one set
/// and validated checks out; a callback's error that is not ported stops it.
pub(crate) fn has_valid_params(
    context: &Context,
    params: &Params,
    args: &[Arg],
) -> Result<(), RestError> {
    let missing: Vec<&str> = args
        .iter()
        .filter(|arg| arg.schema.get_str("required") == Some(&Value::Bool(true)))
        .filter(|arg| params.get(arg.name).is_none())
        .map(|arg| arg.name)
        .collect();
    if !missing.is_empty() {
        let names = missing.iter().map(|name| text(*name)).collect();
        return Err(RestError {
            code: "rest_missing_callback_param",
            message: format!("Missing parameter(s): {}", missing.join(", ")),
            data: object([
                ("status", Value::Int(400)),
                ("params", crate::response::list(names)),
            ]),
        });
    }
    let mut invalid = Invalid::default();
    let set = args
        .iter()
        .filter_map(|arg| params.get(arg.name).map(|value| (arg, value)));
    for (arg, value) in set {
        let checked = match arg.sanitize {
            Sanitize::IsArray => {
                if !matches!(value, Value::Array(_)) {
                    invalid.0.push((arg.name.into(), None));
                }
                continue;
            }
            Sanitize::Validated(check) => match check(context, params, value) {
                Err(error) if error.code == NOT_PORTED => return Err(error),
                checked => checked.map_err(|error| Refusal(Some(error))),
            },
            _ if arg.validate => validate_value_from_schema(value, &arg.schema, arg.name),
            _ => continue,
        };
        refused(checked, arg.name, &mut invalid);
    }
    invalid.check()
}

/// `sanitize_params()`: every parameter of every source with an argument.
pub(crate) fn sanitize_params(
    context: &Context,
    params: &mut Params,
    args: &[Arg],
) -> Result<(), RestError> {
    let mut invalid = Invalid::default();
    let given = params.clone();
    let sources = [
        &mut params.json,
        &mut params.post,
        &mut params.get,
        &mut params.url,
        &mut params.defaults,
    ];
    for source in sources {
        for (key, value) in source.iter_mut() {
            let name = key.to_php_string();
            let Some(arg) = args.iter().find(|arg| arg.name.as_bytes() == name) else {
                continue;
            };
            let sanitized = match arg.sanitize {
                // Validated first, so a string.
                Sanitize::Key => Ok(text(sanitize_key(&value.to_php_string()))),
                Sanitize::Absint => {
                    Ok(Value::Int(i64::try_from(absint(value)).unwrap_or(i64::MAX)))
                }
                Sanitize::TextField => Ok(text(sanitize_text_field(&value.to_php_string()))),
                Sanitize::Urldecode => Ok(text(urldecode(&value.to_php_string()))),
                Sanitize::Endpoint | Sanitize::NotPorted | Sanitize::IsArray => Ok(value.clone()),
                Sanitize::Nullable if *value == Value::Null => Ok(Value::Null),
                Sanitize::Site(sanitize) => sanitize(context, value),
                Sanitize::Params(sanitize) => sanitize(&given, value),
                Sanitize::Schema | Sanitize::Nullable => {
                    validate_value_from_schema(value, &arg.schema, arg.name)
                        .and_then(|()| sanitize_value_from_schema(&*value, &arg.schema, arg.name))
                }
                Sanitize::Value | Sanitize::Validated(_) => {
                    sanitize_value_from_schema(value, &arg.schema, arg.name)
                }
                Sanitize::Callback(check) => {
                    let checked = check(context, value);
                    if checked.is_none() {
                        invalid.0.push((arg.name.into(), None));
                    }
                    checked.ok_or(Refusal(None))
                }
            };
            if let Some(sanitized) = refused(sanitized, arg.name, &mut invalid) {
                *value = sanitized;
            }
        }
    }
    invalid.check()
}

/// An argument of `schema`, JSON, checked and sanitized as `sanitize`
/// says; `rest_validate_request_arg()` validates it when `validate`.
pub(crate) fn arg(name: &'static str, schema: &str, validate: bool, sanitize: Sanitize) -> Arg {
    Arg {
        name,
        schema: hyscribe_phpser::json_decode(schema.as_bytes()).unwrap_or_default(),
        validate,
        sanitize,
    }
}

/// An argument `rest_parse_request_arg()` checks by its schema.
pub(crate) fn typed(name: &'static str, json: &str) -> Arg {
    arg(name, json, false, Sanitize::Schema)
}

/// A string argument checked by its schema.
pub(crate) fn string_arg(name: &'static str) -> Arg {
    typed(name, r#"{"type":"string"}"#)
}

/// A write's required argument, whose value is never read: a write given
/// arguments is not ported.
pub(crate) fn required(name: &'static str) -> Arg {
    arg(name, r#"{"required":true}"#, false, Sanitize::Endpoint)
}

/// `sanitize_title()` of a slug, as the controllers' `sanitize_slug()`.
pub(crate) fn slug(_: &Context, slug: &Value) -> Checked<Value> {
    Ok(text(hyscribe_render::formatting::sanitize_title(
        &slug.to_php_string(),
    )))
}

/// A delete's `force`, false unless given.
pub(crate) fn force_arg() -> Arg {
    typed("force", r#"{"type":"boolean","default":false}"#)
}

/// `absint( ( $page - 1 ) * $per_page )`: the product of PHP's integers,
/// which becomes a float past the largest integer and wraps when cast back.
pub(crate) fn page_offset(page: u64, per_page: u64) -> u64 {
    let before = page.saturating_sub(1);
    match before
        .checked_mul(per_page)
        .filter(|n| i64::try_from(*n).is_ok())
    {
        Some(offset) => offset,
        None => absint(&Value::Float(hyscribe_phpser::Float::from_f64(
            before as f64 * per_page as f64,
        ))),
    }
}

/// `(int) ceil( ( $offset / $per_page ) + 1 )`, the page an offset falls on:
/// PHP's `/` gives an integer for an even division and a float otherwise,
/// the sum is a float where it overflows, and `ceil()` takes it as one, so
/// the page loses digits past 2^53 and wraps to a negative number past the
/// largest integer.
pub(crate) fn page_of_offset(offset: u64, per_page: u64) -> i64 {
    let offset = i64::try_from(offset).unwrap_or(i64::MAX);
    let per_page = i64::try_from(per_page).unwrap_or(i64::MAX);
    let sum = if offset % per_page == 0 {
        let whole = offset / per_page;
        // Only the largest integer overflows, and as a float it absorbs the 1.
        whole.checked_add(1).map_or(whole as f64, |sum| sum as f64)
    } else {
        offset as f64 / per_page as f64 + 1.0
    };
    Value::Float(hyscribe_phpser::Float::from_f64(sum.ceil())).to_int()
}

/// `get_collection_params()`: the context, the page, its size and the
/// search.
pub(crate) fn collection_args() -> Vec<Arg> {
    vec![
        crate::types::context_arg(),
        arg(
            "page",
            r#"{"type":"integer","default":1,"minimum":1}"#,
            true,
            Sanitize::Absint,
        ),
        arg(
            "per_page",
            r#"{"type":"integer","default":10,"minimum":1,"maximum":100}"#,
            true,
            Sanitize::Absint,
        ),
        arg("search", r#"{"type":"string"}"#, true, Sanitize::TextField),
    ]
}

/// `get_content_type()`'s value: the header up to a `;` past its start,
/// lower case and trimmed, if it has a `/`.
pub(crate) fn content_type(header: Option<&[u8]>) -> Option<Vec<u8>> {
    let value = header.filter(|value| !is_empty_string(value))?;
    let value = match value.iter().position(|&b| b == b';') {
        Some(at) if at > 0 => value.get(..at).unwrap_or_default(),
        _ => value,
    };
    let value = value.to_ascii_lowercase();
    value.contains(&b'/').then(|| php_trim(&value).to_vec())
}

/// `wp_is_json_media_type()`: `application/json`, or a `+json` subtype,
/// with `+oembed` or not, after the start, white space or a comma.
pub(crate) fn is_json(media: &[u8]) -> bool {
    // PCRE's `\s`, and its class `[\w!#\$&-\^\.\+]`.
    let space = |b: u8| matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let class =
        |b: u8| b.is_ascii_alphanumeric() || b"_!#$.+".contains(&b) || (b'&'..=b'^').contains(&b);
    let ends = |rest: &[u8]| {
        rest.first()
            .is_none_or(|&b| space(b) || b == b';' || b == b',')
    };
    let json = |rest: &[u8]| {
        rest.strip_prefix(b"json")
            .is_some_and(|rest| ends(rest) || rest.strip_prefix(b"+oembed").is_some_and(ends))
    };
    let lower = media.to_ascii_lowercase();
    (0..lower.len()).any(|at| {
        let before = at.checked_sub(1).and_then(|before| lower.get(before));
        let start = before.is_none_or(|&b| space(b) || b == b',');
        let rest = lower.get(at..).unwrap_or_default();
        let subtype = rest.strip_prefix(b"application/").filter(|_| start);
        subtype.is_some_and(|subtype| {
            (0..subtype.len()).any(|cut| {
                let (prefix, rest) = subtype.split_at(cut);
                let suffixed =
                    prefix.len() > 1 && prefix.ends_with(b"+") && prefix.iter().all(|&b| class(b));
                (cut == 0 || suffixed) && json(rest)
            })
        })
    })
}

/// `array_merge( $first, $second )`: string keys of `second` replace
/// `first`'s, integer keys are numbered afresh.
pub(crate) fn array_merge(
    first: Vec<(Key, Value)>,
    second: Vec<(Key, Value)>,
) -> Vec<(Key, Value)> {
    let mut merged: Vec<(Key, Value)> = Vec::new();
    let mut next = 0;
    for (key, value) in first.into_iter().chain(second) {
        if let Key::Int(_) = key {
            merged.push((Key::Int(next), value));
            next += 1;
            continue;
        }
        match merged.iter_mut().find(|(known, _)| *known == key) {
            Some((_, held)) => *held = value,
            None => merged.push((key, value)),
        }
    }
    merged
}
