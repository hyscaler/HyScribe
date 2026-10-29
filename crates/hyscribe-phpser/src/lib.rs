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

mod cast;
mod decode;
mod encode;
mod error;
mod float;
mod json;
mod md5;
mod query;
mod value;
mod wp;

pub use cast::{absint, float_to_string, is_empty_string, isset, string_to_float};
pub use decode::{MAX_DEPTH, decode};
pub use encode::encode;
pub use error::{Error, ErrorKind};
pub use float::Float;
pub use json::{JSON_MAX_DEPTH, json_decode, json_encode, json_encode_unescaped};
pub use md5::{bin2hex, md5};
pub use query::{
    add_query_arg, add_query_values, build_query, parse_str, rawurlencode, rawurlencode_deep,
    remove_query_arg, urldecode, urlencode, urlencode_deep,
};
pub use value::{Key, Value, lookup, wp_array_set};
pub use wp::{Maybe, is_serialized, maybe_unserialize, php_trim};
