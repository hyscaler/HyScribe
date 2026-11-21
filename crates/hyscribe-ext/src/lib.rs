// SPDX-License-Identifier: AGPL-3.0-or-later

//! The extension host (ADR 0022): WebAssembly components of the SDK's
//! `plugin` world under wasmtime, behind their manifest's grants.
//!
//! - One [`Host`] per process: the engine, the SDK's host functions and the
//!   epoch ticker
//! - One [`Extension`] per package: compiled once into an `InstancePre`,
//!   its `init` run once to register its hooks, routes, shortcodes and blocks
//! - One [`Request`] per request: a fresh instance per extension, made the
//!   first time a hook reaches it, each call bounded by its deadline, the
//!   extension's budget for the request and its memory limit
//!
//! Every host call checks its grant before it touches anything. A callback
//! that traps or overruns keeps the value as it was and ends the extension's
//! part of the request; three such faults within ten minutes disable it,
//! recorded in its folder's `faults.txt`.

mod calls;
mod extension;
pub mod faults;
mod grants;
mod host;
pub mod hyx;
pub mod installed;
mod limits;
pub mod manifest;
mod request;
mod site;
mod value;

pub use extension::{Block, Extension, Extensions, Hook, MetaKey, Package, Route, Taxonomy};
pub use grants::Grants;
pub use host::{Host, TICK};
pub use limits::{CEILING, DEFAULT, Limits};
pub use manifest::Manifest;
pub use request::{Report, Request, Rest};
pub use site::{Asset, AssetKind, Found, Message, ObjectKind, Post, Site};

/// What went wrong in an extension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fault {
    /// Not loaded: its manifest, its component or its `init` was refused.
    Refused(String),
    /// A trap other than the two below. Counted.
    Trap(String),
    /// A call ran past its deadline or the extension's request budget. Counted.
    Timeout,
    /// The instance asked for more memory than its limit. Counted.
    Memory,
    /// The callback answered an error; the instance carries on.
    Guest(String),
    /// A value that cannot cross: the callback was not called, or its
    /// answer was not taken.
    Value(String),
    /// Disabled after too many faults, or its fault record does not read.
    Disabled,
    /// The fault record could not be written; the count holds in memory.
    Unrecorded(String),
}

#[cfg(test)]
extern crate self as hyscribe_ext;

#[cfg(test)]
mod tests;
