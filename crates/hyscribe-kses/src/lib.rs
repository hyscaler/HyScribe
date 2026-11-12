// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's escaping and sanitizing functions, ported byte for byte from
//! WordPress 7.1.3 (`wp-includes/kses.php` and `formatting.php`) with the
//! default filters and a UTF-8 blog charset. PHP WordPress is the oracle:
//! `reference/kses/golden.sh` records its output for every function.
//!
//! The PCRE patterns are matched by hand, with PCRE's semantics, in linear
//! time. Where PCRE gives up (its JIT stack or backtrack limit), PHP returns
//! an error value that WordPress turns into an empty result or a dropped
//! declaration; the port reproduces those limits too.

mod css;
mod entities;
mod escape;
mod php;
mod protocol;
mod strip;
mod url;

pub use css::safecss_filter_attr;
pub use entities::{wp_kses_decode_entities, wp_kses_normalize_entities};
pub use escape::{esc_attr, esc_html, esc_textarea, sanitize_key};
pub use php::contains as str_contains;
pub use protocol::{
    ALLOWED_PROTOCOLS, wp_kses_bad_protocol, wp_kses_no_null, wp_kses_no_null_keep_slash_zero,
};
pub use strip::{collapse_breaks, strip_tags, wp_strip_all_tags};
pub use url::{
    Parts, deep_replace, esc_url, esc_url_protocols, esc_url_raw, parse_url, url_host, url_query,
    url_scheme, wp_parse_url,
};
