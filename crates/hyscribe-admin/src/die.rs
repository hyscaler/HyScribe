// SPDX-License-Identifier: AGPL-3.0-or-later

//! `_default_wp_die_handler()`'s page, the one `wp_die()` shows.

const PAGE: &str = include_str!("die.html");

/// The page for `wp_die( $message, $title )`; `message` is HTML.
pub fn page(title: &str, message: &str) -> Vec<u8> {
    let (head, rest) = PAGE.split_once("%title%").unwrap_or_default();
    let (middle, tail) = rest.split_once("%message%").unwrap_or_default();
    [head, title, middle, message, tail].concat().into_bytes()
}
