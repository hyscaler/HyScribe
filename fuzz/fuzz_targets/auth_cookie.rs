// SPDX-License-Identifier: AGPL-3.0-or-later

//! The auth cookie, nonces and fast hashes from a request never panic; a
//! cookie the site signed round trips and verifies, and one changed byte of
//! its signature never does; a nonce verifies only for its own tick.

#![no_main]

use hyscribe_auth::cookie::{generate_auth_cookie, parse_auth_cookie};
use hyscribe_auth::{
    create_nonce, password_needs_rehash, verify_nonce, wp_fast_hash, wp_verify_fast_hash,
};
use libfuzzer_sys::fuzz_target;

const PASS: &[u8] = b"$2y$10$abcdefghijklmnopqrstuuvwxyz0123456789ABCDEFGHIJKLMN";
const NOW: i64 = 1_700_000_000;

fuzz_target!(|data: &[u8]| {
    let (salt, rest) = data.split_at(data.len().min(8));
    if let Some(cookie) = parse_auth_cookie(rest) {
        let _ = cookie.is_valid(PASS, salt, NOW, true);
    }
    let _ = verify_nonce(rest, salt, NOW / 43_200, "log-out", 1, b"token");
    let _ = wp_verify_fast_hash(salt, rest);
    let _ = password_needs_rehash(rest);
    assert!(wp_verify_fast_hash(rest, wp_fast_hash(rest).as_bytes()));

    // WordPress logins never hold `|`, which separates the cookie's parts.
    let login: Vec<u8> = rest.iter().copied().filter(|&b| b != b'|').collect();
    let signed = generate_auth_cookie(&login, PASS, NOW, b"token", salt);
    let cookie = parse_auth_cookie(&signed).expect("a signed cookie parses");
    assert!(cookie.username == login && cookie.is_valid(PASS, salt, NOW, false));
    let mut forged = signed.clone();
    let last = forged.len() - 1;
    forged[last] ^= 1;
    let forged = parse_auth_cookie(&forged).expect("still four parts");
    assert!(!forged.is_valid(PASS, salt, NOW, false));

    let nonce = create_nonce(salt, 7, "log-out", 1, &login);
    assert_eq!(
        verify_nonce(nonce.as_bytes(), salt, 7, "log-out", 1, &login),
        1
    );
    assert_eq!(
        verify_nonce(nonce.as_bytes(), salt, 8, "log-out", 1, &login),
        2
    );
    assert_eq!(
        verify_nonce(nonce.as_bytes(), salt, 9, "log-out", 1, &login),
        0
    );
    assert_eq!(verify_nonce(b"", salt, 7, "log-out", 1, &login), 0);
});
