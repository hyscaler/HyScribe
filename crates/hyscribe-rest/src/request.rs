// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Request`'s parameters for a request without a body: the
//! query's, the URL's and the defaults, in that order, checked and
//! sanitized by their route's arguments.

use hyscribe_kses::sanitize_key;
use hyscribe_phpser::{Key, Value};

use crate::response::{RestError, object, text};
use crate::schema::{Refusal, sanitize_value_from_schema, validate_value_from_schema};

/// The parameters by where they come from.
#[derive(Clone, Debug, Default)]
pub(crate) struct Params {
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

    /// `$request['context']`, a string after sanitizing.
    pub(crate) fn context(&self) -> Vec<u8> {
        self.get("context")
            .map(Value::to_php_string)
            .unwrap_or_default()
    }
}

/// How an argument is sanitized.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Sanitize {
    /// `sanitize_key()`.
    Key,
    /// `rest_parse_request_arg()`: validated, then sanitized, by its schema.
    Schema,
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

/// The arguments' defaults.
pub(crate) fn defaults(args: &[Arg]) -> Vec<(Key, Value)> {
    args.iter()
        .filter_map(|arg| {
            let default = arg.schema.get_str("default")?;
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
/// (`types::tests`), so a refusal always carries its error.
fn refused<T>(result: Result<T, Refusal>, name: &str, invalid: &mut Invalid) -> Option<T> {
    result
        .map_err(|Refusal(error)| invalid.0.extend(error.map(|error| (name.into(), error))))
        .ok()
}

/// `has_valid_params()`: each argument set and validated checks out.
pub(crate) fn has_valid_params(params: &Params, args: &[Arg]) -> Result<(), RestError> {
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
pub(crate) fn sanitize_params(params: &mut Params, args: &[Arg]) -> Result<(), RestError> {
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
