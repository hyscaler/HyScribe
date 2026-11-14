// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Request`'s parameters for a request without a body: the
//! query's, the URL's and the defaults, in that order, checked and
//! sanitized by their route's arguments.

use hyscribe_kses::sanitize_key;
use hyscribe_phpser::{Key, Value, absint, urldecode};
use hyscribe_render::formatting::sanitize_text_field;

use crate::Context;
use crate::response::{RestError, object, text};
use crate::schema::{Checked, Refusal, sanitize_value_from_schema, validate_value_from_schema};

/// The parameters by where they come from.
#[derive(Clone, Debug, Default)]
pub(crate) struct Params {
    /// The regex of the route matched.
    pub(crate) route: &'static str,
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
    fn sources(&self) -> [&[(Key, Value)]; 3] {
        [&self.get, &self.url, &self.defaults]
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
    /// `urldecode()` of a URL parameter, a string.
    Urldecode,
    /// Left as given, for the endpoint to check: a schema whose keywords
    /// are not ported.
    Endpoint,
    /// The endpoint's own sanitizer, which reads the site.
    Site(fn(&Context, &Value) -> Checked<Value>),
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

/// The invalid parameters and the errors each gives.
#[derive(Default)]
struct Invalid(Vec<(String, RestError)>);

impl Invalid {
    /// `rest_invalid_param` of them all, if any.
    fn check(self) -> Result<(), RestError> {
        if self.0.is_empty() {
            return Ok(());
        }
        let key = |name: &str| Key::String(name.as_bytes().to_vec());
        let names: Vec<&str> = self.0.iter().map(|(name, _)| name.as_str()).collect();
        let params = self
            .0
            .iter()
            .map(|(name, error)| (key(name), text(error.message.clone())))
            .collect();
        let details = self
            .0
            .iter()
            .map(|(name, error)| {
                let detail = object([
                    ("code", text(error.code)),
                    ("message", text(error.message.clone())),
                    ("data", error.data.clone()),
                ]);
                (key(name), detail)
            })
            .collect();
        Err(RestError {
            code: "rest_invalid_param",
            message: format!("Invalid parameter(s): {}", names.join(", ")),
            data: object([
                ("status", Value::Int(400)),
                ("params", Value::Array(params)),
                ("details", Value::Array(details)),
            ]),
        })
    }
}

/// An invalid parameter noted. Ported arguments use only ported keywords
/// (`tests::arguments_use_ported_keywords`), so a refusal always carries
/// its error.
fn refused<T>(result: Result<T, Refusal>, name: &str, invalid: &mut Invalid) -> Option<T> {
    result
        .map_err(|Refusal(error)| invalid.0.extend(error.map(|error| (name.into(), error))))
        .ok()
}

/// `has_valid_params()`: every required argument is set, and each one set
/// and validated checks out.
pub(crate) fn has_valid_params(params: &Params, args: &[Arg]) -> Result<(), RestError> {
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
        .filter(|arg| arg.validate)
        .filter_map(|arg| params.get(arg.name).map(|value| (arg, value)));
    for (arg, value) in set {
        let checked = validate_value_from_schema(value, &arg.schema, arg.name);
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
    for source in [&mut params.get, &mut params.url, &mut params.defaults] {
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
                Sanitize::Endpoint => Ok(value.clone()),
                Sanitize::Site(sanitize) => sanitize(context, value),
                Sanitize::Schema => validate_value_from_schema(value, &arg.schema, arg.name)
                    .and_then(|()| sanitize_value_from_schema(&*value, &arg.schema, arg.name)),
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
