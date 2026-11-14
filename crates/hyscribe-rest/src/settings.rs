// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Settings_Controller`: `/wp/v2/settings`, read only, over the
//! settings exported by `reference/wordpress/export.sh`.

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Value, json_decode};

use crate::request::Params;
use crate::response::{Response, RestError};
use crate::schema::prepare_value;
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
                args: Vec::new(),
                permission,
                callback: None,
            },
        ]
    })
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
    let data = settings().iter().map(|(name, setting)| {
        let schema = setting.get_str("schema").cloned().unwrap_or_default();
        let option = setting
            .get_str("option")
            .map(Value::to_php_string)
            .unwrap_or_default();
        let stored = context.site.option(&String::from_utf8_lossy(&option));
        let value = match stored.as_deref() {
            Some(stored) => crate::unserialized(stored),
            None => schema.get_str("default").cloned().unwrap_or_default(),
        };
        // `prepare_value()`: a value its schema does not take is null.
        let param = String::from_utf8_lossy(&name.to_php_string()).into_owned();
        (name.clone(), prepare_value(&value, &schema, &param))
    });
    Ok(Response::new(Value::Array(data.collect()), 200))
}
