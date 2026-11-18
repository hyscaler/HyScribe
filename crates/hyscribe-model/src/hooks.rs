// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's hooks under their names and argument order (docs/EXTENSIONS.md):
//! plain calls until the extension runtime, so a filter gives its value back.

use std::any::Any;

/// `apply_filters( $hook, $value, ...$args )`.
pub fn apply_filters<T>(_hook: &str, value: T, _args: &[&dyn Any]) -> T {
    value
}

/// The actions fired, by name, for tests to read.
#[cfg(feature = "hook-log")]
pub static FIRED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// `do_action( $hook, ...$args )`.
#[cfg_attr(not(feature = "hook-log"), allow(unused_variables))]
pub fn do_action(hook: &str, _args: &[&dyn Any]) {
    #[cfg(feature = "hook-log")]
    FIRED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(hook.to_string());
}
