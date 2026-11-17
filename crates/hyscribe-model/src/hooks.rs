// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's hooks under their names and argument order (docs/EXTENSIONS.md):
//! plain calls until the extension runtime, so a filter gives its value back.

use std::any::Any;

/// `apply_filters( $hook, $value, ...$args )`.
pub fn apply_filters<T>(_hook: &str, value: T, _args: &[&dyn Any]) -> T {
    value
}

/// `do_action( $hook, ...$args )`.
pub fn do_action(_hook: &str, _args: &[&dyn Any]) {}
