// SPDX-License-Identifier: AGPL-3.0-or-later

//! The bounds an extension runs in (ADR 0022): the defaults, or what its
//! manifest's `[limits]` asks for, never past the host's fixed ceiling.

use std::time::Duration;

/// A call's time, an extension's time in one request and an instance's
/// linear memory. Made only by [`Limits::default`] or [`Limits::asked`], so
/// no extension runs past the ceiling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub(crate) call_ms: u64,
    pub(crate) request_ms: u64,
    pub(crate) memory_mib: u64,
}

/// What an extension gets unless its manifest asks for more.
pub const DEFAULT: Limits = Limits {
    call_ms: 50,
    request_ms: 250,
    memory_mib: 16,
};

/// The most a manifest may ask for: a constant of the host, not a setting.
pub const CEILING: Limits = Limits {
    call_ms: 500,
    request_ms: 2_000,
    memory_mib: 64,
};

impl Default for Limits {
    fn default() -> Self {
        DEFAULT
    }
}

impl Limits {
    /// The manifest's `[limits]` entries: each an integer from its default
    /// to its ceiling, one left out keeping its default. Anything else
    /// refuses the package, with the reason; nothing is clamped.
    pub fn asked(entries: &[(&str, i64)]) -> Result<Limits, String> {
        entries
            .iter()
            .try_fold(DEFAULT, |mut limits, &(key, value)| {
                let (field, low, high) = match key {
                    "call_ms" => (&mut limits.call_ms, DEFAULT.call_ms, CEILING.call_ms),
                    "request_ms" => (
                        &mut limits.request_ms,
                        DEFAULT.request_ms,
                        CEILING.request_ms,
                    ),
                    "memory_mib" => (
                        &mut limits.memory_mib,
                        DEFAULT.memory_mib,
                        CEILING.memory_mib,
                    ),
                    _ => return Err(format!("[limits] has no key {key}")),
                };
                match u64::try_from(value) {
                    Ok(value) if (low..=high).contains(&value) => {
                        *field = value;
                        Ok(limits)
                    }
                    _ => Err(format!("[limits] {key} must be from {low} to {high}")),
                }
            })
    }

    /// A call's deadline.
    pub fn call(&self) -> Duration {
        Duration::from_millis(self.call_ms)
    }

    /// The time all of an extension's calls in one request may take.
    pub fn request(&self) -> Duration {
        Duration::from_millis(self.request_ms)
    }

    /// An instance's linear memory, in bytes.
    pub fn memory(&self) -> usize {
        usize::try_from(self.memory_mib << 20).unwrap_or(usize::MAX)
    }
}

#[cfg(test)]
mod tests;
