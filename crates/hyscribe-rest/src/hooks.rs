// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's REST hooks at their points, under their names and with their
//! arguments in WordPress's order (ADR 0013, docs/EXTENSIONS.md): plain
//! calls until the extension runtime (M6), so nothing attaches yet and a
//! filter gives its value back, which the checks and the plan still take.

use std::any::Any;

/// `apply_filters( $hook, $value, ...$args )`.
pub(crate) fn apply_filters<T>(_hook: &str, value: T, _args: &[&dyn Any]) -> T {
    value
}

/// `do_action( $hook, ...$args )`.
pub(crate) fn do_action(_hook: &str, _args: &[&dyn Any]) {}
