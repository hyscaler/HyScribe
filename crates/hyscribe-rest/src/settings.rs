// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Settings_Controller`: `/wp/v2/settings`, over the settings
//! exported by `reference/wordpress/export.sh`. An update (MVP 4.3) gives
//! the plan `hyscribe-http` writes with `sanitize_option()`,
//! `update_option()` and `delete_option()`.

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Value, json_decode};

use crate::posts::write::{Outcome, Plan, Write, Writing};
use crate::request::{Arg, Params, Sanitize};
use crate::response::{Response, RestError};
use crate::schema::{prepare_value, validate_value_from_schema};
use crate::taxonomies::forbidden;
use crate::{Context, Endpoint};

const SETTINGS: &[u8] = include_bytes!("../../../wordpress/wp-includes/rest-settings.json");

/// Each setting by name: its option and schema.
fn settings() -> &'static [(Key, Value)] {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| json_decode(SETTINGS).unwrap_or(Value::Null))
        .as_array()
        .unwrap_or_default()
}

/// The endpoints of the route `regex`, if ported.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    (regex == "/wp/v2/settings").then(|| {
        vec![
            Endpoint {
                args: Vec::new(),
                permission,
                callback: Some(item),
            },
            Endpoint {
                args: args(),
                permission,
                callback: None,
            },
        ]
    })
}

/// A setting's option and schema.
fn option_and_schema(setting: &Value) -> (String, Value) {
    let option = setting.get_str("option").map(Value::to_php_string);
    let option = String::from_utf8_lossy(&option.unwrap_or_default()).into_owned();
    (
        option,
        setting.get_str("schema").cloned().unwrap_or_default(),
    )
}

/// `get_endpoint_args_for_item_schema( WP_REST_Server::EDITABLE )`: each
/// setting by its schema, without its default; null is taken as given.
fn args() -> Vec<Arg> {
    static NAMES: OnceLock<Vec<String>> = OnceLock::new();
    let names = NAMES.get_or_init(|| {
        let names = settings().iter().map(|(name, _)| name.to_php_string());
        names
            .map(|name| String::from_utf8_lossy(&name).into_owned())
            .collect()
    });
    let arg = |(name, (_, setting)): (&'static String, &'static (Key, Value))| {
        let mut schema = option_and_schema(setting).1;
        if let Value::Array(entries) = &mut schema {
            entries.retain(|(key, _)| key.to_php_string() != b"default");
        }
        Arg {
            name: name.as_str(),
            schema,
            validate: true,
            sanitize: Sanitize::Nullable,
        }
    };
    names.iter().zip(settings()).map(arg).collect()
}

/// A setting an update writes.
#[derive(Clone, Debug, PartialEq)]
pub struct Setting {
    /// The option.
    pub option: String,
    /// `update_option()` of the value, `delete_option()` for none.
    pub value: Option<Value>,
    /// The default `register_setting()` gave, which `get_option()` reads
    /// for the option when it is not stored.
    pub default: Option<Value>,
}

/// What an update writes: each setting, and the setting whose stored value
/// stopped it, if any.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SettingsSave {
    /// The settings, in the order they are registered.
    pub options: Vec<Setting>,
    /// The setting PHP refuses to reset, the options before it written.
    pub invalid: Option<String>,
}

/// The ported write of the route `regex`'s endpoint `index`, if any.
pub(crate) fn writing(regex: &str, index: usize) -> Option<Writing> {
    (regex == "/wp/v2/settings" && index == 1).then(|| Writing::plain(plan, finish))
}

/// `update_item()` up to its writes: each setting the request gives, in
/// the order they are registered, until one PHP refuses to reset.
fn plan(context: &Context, params: &Params) -> Result<Plan, RestError> {
    let mut save = SettingsSave::default();
    for (name, setting) in settings() {
        let name = String::from_utf8_lossy(&name.to_php_string()).into_owned();
        if !params.has(&name) {
            continue;
        }
        let (option, schema) = option_and_schema(setting);
        let value = params.get(&name).cloned();
        if value.is_none() {
            // `get_option( $option, false )`.
            let stored = context.site.option(&option);
            let stored = stored.map_or(Value::Bool(false), |stored| crate::unserialized(&stored));
            if validate_value_from_schema(&stored, &schema, "").is_err() {
                save.invalid = Some(name);
                break;
            }
        }
        // Only the settings registered with a default have one here.
        let default = schema
            .get_str("default")
            .filter(|default| **default != Value::Null);
        save.options.push(Setting {
            option,
            value,
            default: default.cloned(),
        });
    }
    Ok(Plan::new(
        context,
        Write::Settings(Box::new(save)),
        Value::Null,
    ))
}

/// The answer once written: the settings as they are, or PHP's error for
/// a setting it refused to reset.
fn finish(
    context: &Context,
    params: &mut Params,
    plan: Plan,
    outcome: Outcome,
) -> Result<Response, RestError> {
    if let Write::Settings(save) = plan.write
        && let Some(name) = save.invalid
    {
        let message = format!(
            "The {name} property has an invalid stored value, and cannot be updated to null."
        );
        return Err(RestError {
            message,
            ..RestError::status("rest_invalid_stored_value", "", 500)
        });
    }
    match outcome {
        Outcome::Cached(cached) => cached_item(context, &cached),
        _ => item(context, params),
    }
}

/// `get_item_permissions_check()`.
fn permission(context: &Context, _: &Params) -> Result<(), RestError> {
    if context.can("manage_options") {
        Ok(())
    } else {
        Err(forbidden(context))
    }
}

/// `get_item()`: each option, or its default, as its schema takes it.
fn item(context: &Context, _: &Params) -> Result<Response, RestError> {
    cached_item(context, &[])
}

/// [`item`] with the values of `cached` for their options, as PHP's
/// options cache holds them once written.
fn cached_item(context: &Context, cached: &[(String, Value)]) -> Result<Response, RestError> {
    let data = settings().iter().map(|(name, setting)| {
        let (option, schema) = option_and_schema(setting);
        let written = cached.iter().find(|(known, _)| *known == option);
        let stored = context.site.option(&option);
        let value = match (written, stored.as_deref()) {
            (Some((_, value)), _) => value.clone(),
            (None, Some(stored)) => crate::unserialized(stored),
            (None, None) => schema.get_str("default").cloned().unwrap_or_default(),
        };
        // `prepare_value()`: a value its schema does not take is null.
        let param = String::from_utf8_lossy(&name.to_php_string()).into_owned();
        (name.clone(), prepare_value(&value, &schema, &param))
    });
    Ok(Response::new(Value::Array(data.collect()), 200))
}
