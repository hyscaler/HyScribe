// SPDX-License-Identifier: AGPL-3.0-or-later

//! Images (ADR 0015): the header and metadata readers WordPress uses on an
//! upload, and the sizes `wp_create_image_subsizes()` makes of it, pure and
//! synchronous. Bytes go in; the files' bytes and the metadata come out.
#![forbid(unsafe_code)]

pub mod codec;
pub mod editor;
pub mod header;
pub mod hooks;
pub mod iptc;
pub mod meta;
pub mod pixels;
pub mod pool;
pub mod subsizes;
pub mod upload;

#[cfg(test)]
mod tests;
