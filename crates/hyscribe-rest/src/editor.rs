// SPDX-License-Identifier: AGPL-3.0-or-later

//! The registries the block and site editors read: the block types, the
//! block patterns and their categories, the theme's global styles, and the
//! themes. What core and the
//! theme register is exported by `reference/rest/registered.php`.

mod block_types;
pub(crate) mod global_styles;
mod patterns;
pub(crate) mod themes;

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Value, json_decode};

use crate::Endpoint;

const CORE: &[u8] = include_bytes!("../../../wordpress/wp-includes/rest-registered.json");
const THEME: &[u8] = include_bytes!("../../../themes/twentytwentyfive/registered.json");

/// The endpoints of the route `regex`, if ported here.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    block_types::endpoints(regex)
        .or_else(|| global_styles::endpoints(regex))
        .or_else(|| patterns::endpoints(regex))
        .or_else(|| themes::endpoints(regex))
}

fn decoded(json: &'static [u8], cell: &'static OnceLock<Value>) -> &'static Value {
    cell.get_or_init(|| json_decode(json).unwrap_or_default())
}

/// The list `name` of what core registers.
fn core(name: &str) -> &'static [(Key, Value)] {
    static CORE_DATA: OnceLock<Value> = OnceLock::new();
    let data = decoded(CORE, &CORE_DATA).get_str(name);
    data.and_then(Value::as_array).unwrap_or_default()
}

/// Whether core registers the block `name` with a render callback.
pub(crate) fn is_dynamic(name: &str) -> bool {
    core("block_types").iter().any(|(_, block)| {
        block.get_str("name").and_then(Value::as_bytes) == Some(name.as_bytes())
            && block.get_str("is_dynamic") == Some(&Value::Bool(true))
    })
}

/// The list `name` of what the active theme registers.
fn theme(name: &str) -> &'static [(Key, Value)] {
    static THEME_DATA: OnceLock<Value> = OnceLock::new();
    let data = decoded(THEME, &THEME_DATA).get_str(name);
    data.and_then(Value::as_array).unwrap_or_default()
}

#[cfg(test)]
mod tests;
