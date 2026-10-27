// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP `serialize()` and `unserialize()` for WordPress data.
//!
//! WordPress stores arrays and objects in options and meta as PHP serialized
//! strings. This crate reads and writes that format with two guarantees:
//!
//! - **Data only.** Serialized objects, `Serializable` payloads and enums are
//!   kept as opaque values. Nothing is ever instantiated, so the PHP object
//!   injection bug class cannot exist here.
//! - **Byte-exact round trips.** For any input produced by PHP's `serialize()`,
//!   `encode(&decode(input)?) == input`. Floats keep their original text so
//!   values written with any `serialize_precision` survive unchanged.
//!
//! [`is_serialized`] and [`maybe_unserialize`] reproduce the WordPress
//! functions of the same name.

mod decode;
mod encode;
mod error;
mod float;
mod json;
mod value;
mod wp;

pub use decode::{MAX_DEPTH, decode};
pub use encode::encode;
pub use error::{Error, ErrorKind};
pub use float::Float;
pub use json::{JSON_MAX_DEPTH, json_decode};
pub use value::{Key, Value};
pub use wp::{Maybe, is_serialized, maybe_unserialize};
