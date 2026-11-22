// SPDX-License-Identifier: AGPL-3.0-or-later

//! The hosted registry as HyScribe reads it (ADR 0024): the key list two of
//! the compiled-in root keys sign, the index an online key of that list
//! signs, their verified copies kept in `hyscribe/registry/`, the version
//! documents, and installs by name. Only `remote` reaches the network, and
//! only the registry's host; nothing here reaches the database.

pub mod index;
pub mod install;
mod json;
pub mod kept;
pub mod keys;
pub mod remote;
pub mod revoke;
pub mod versions;

/// The offline root keys, as minisign prints them (ADR 0024); tests pass
/// their own, never configuration.
///
/// PLACEHOLDER: the three root keys do not exist yet. With none
/// listed no key list verifies, so no registry signature is accepted.
pub const ROOTS: [&str; 0] = [];

#[cfg(test)]
mod tests;
