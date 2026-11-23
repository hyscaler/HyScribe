// SPDX-License-Identifier: Apache-2.0 OR MIT

//! JSON for the arguments that cross as JSON: query variables, a route's
//! `args`, a post type's arguments. Text is written as it is, UTF-8, with
//! only what JSON requires escaped.

use alloc::string::String;
use core::fmt::Write;

/// A string as a JSON string.
pub fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// An object of `entries`, each a key and its value already as JSON.
pub fn object(entries: &[(&str, &str)]) -> String {
    let mut out = String::from("{");
    for (n, (key, value)) in entries.iter().enumerate() {
        if n > 0 {
            out.push(',');
        }
        out.push_str(&string(key));
        out.push(':');
        out.push_str(value);
    }
    out.push('}');
    out
}

#[cfg(test)]
mod tests;
