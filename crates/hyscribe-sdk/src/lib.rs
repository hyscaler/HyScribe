// SPDX-License-Identifier: Apache-2.0 OR MIT

//! HyScribe's extension SDK 0.2 (ADR 0022): a WordPress plugin ported to
//! Rust and built as a WebAssembly component for `wasm32-wasip2`.
//!
//! WordPress's functions keep their names and argument order
//! (docs/EXTENSIONS.md): [`hooks::on_filter`] is `add_filter()`,
//! [`hooks::filter`] is `apply_filters()`, [`options::get`] is
//! `get_option()`, and so on. Every call is checked by the host against the
//! extension's manifest; nothing is granted unless the manifest names it.
//!
//! A plugin implements [`Plugin`], numbers its callbacks with
//! [`callbacks!`] and exports itself with [`plugin!`]:
//!
//! ```ignore
//! #![no_std]
//! extern crate alloc;
//! use hyscribe_sdk::{Plugin, Value, hooks};
//!
//! hyscribe_sdk::callbacks! {
//!     enum Callback {
//!         Content => content,
//!     }
//! }
//!
//! fn content(args: Vec<Value>) -> Result<Value, String> { ... }
//!
//! struct Mine;
//! impl Plugin for Mine {
//!     fn init() -> Result<(), String> {
//!         hooks::on_filter("the_content", Callback::Content, 10, 1).map_err(|e| format!("{e:?}"))
//!     }
//!     fn activate() -> Result<(), String> { Ok(()) }
//!     fn deactivate() -> Result<(), String> { Ok(()) }
//!     fn invoke(callback: u32, args: Vec<Value>) -> Result<Value, String> {
//!         Callback::invoke(callback, args)
//!     }
//! }
//! hyscribe_sdk::plugin!(Mine);
//! ```

#![cfg_attr(target_arch = "wasm32", no_std)]

extern crate alloc;

pub mod json;
mod value;

pub use value::Value;

#[doc(hidden)]
pub use alloc::string::String as __String;
#[doc(hidden)]
pub use alloc::vec::Vec as __Vec;

#[cfg(target_arch = "wasm32")]
mod guest;

#[cfg(target_arch = "wasm32")]
pub use guest::*;

/// What the host calls in a plugin (`guest`).
pub trait Plugin {
    /// Runs once when the package is loaded: adds hooks, registers post
    /// types, routes, shortcodes and blocks. Never per request.
    fn init() -> Result<(), __String>;

    /// `register_activation_hook()`.
    fn activate() -> Result<(), __String>;

    /// `register_deactivation_hook()`.
    fn deactivate() -> Result<(), __String>;

    /// Runs callback `callback` with its arguments.
    fn invoke(callback: u32, args: __Vec<Value>) -> Result<Value, __String>;
}

/// Numbers a plugin's callbacks at compile time, so a fresh instance runs
/// any of them without running `init` again: an enum whose variants name
/// the functions, each `fn(Vec<Value>) -> Result<Value, String>`, and its
/// `invoke( $number, $args )`.
#[macro_export]
macro_rules! callbacks {
    ($vis:vis enum $name:ident { $($variant:ident => $call:path),* $(,)? }) => {
        /// The plugin's callbacks, by number.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        $vis enum $name {
            $(
                #[allow(missing_docs)]
                $variant,
            )*
        }

        impl $name {
            /// Runs callback `number` with `args`.
            $vis fn invoke(
                number: u32,
                args: $crate::__Vec<$crate::Value>,
            ) -> ::core::result::Result<$crate::Value, $crate::__String> {
                $(
                    if number == $name::$variant as u32 {
                        return $call(args);
                    }
                )*
                Err("no such callback".into())
            }
        }

        impl ::core::convert::From<$name> for u32 {
            fn from(callback: $name) -> u32 {
                callback as u32
            }
        }
    };
}
