// SPDX-License-Identifier: AGPL-3.0-or-later

//! The part of WordPress's HTML API that block rendering uses: a port of
//! `WP_HTML_Tag_Processor` (WordPress 7.1.3), byte for byte, with the
//! attribute decoder it relies on. URL attributes go through
//! `hyscribe_kses::esc_url()`.
//!
//! The tokenizer reproduces PHP's handling of comments, doctypes, CDATA
//! lookalikes, processing instructions, the special text elements
//! (`<script>`, `<style>`, `<textarea>`, `<title>` and friends), duplicate
//! attributes and unterminated input, so that edits land where WordPress
//! puts them. Every loop is bounded by the input length.

mod decoder;
mod entities;
mod tag_processor;

pub use decoder::decode_attribute;
pub use tag_processor::{Attribute, CommentType, Namespace, TagProcessor, escape};
