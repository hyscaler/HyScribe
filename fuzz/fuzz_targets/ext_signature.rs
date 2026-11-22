// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a minisign signature file or public key (ADR 0024) never
//! panics, what it reads keeps the reader's bounds, and no key made up of
//! the input verifies it.

#![no_main]

use hyscribe_ext::signature::{PublicKey, Signature, prehash};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let text = std::str::from_utf8(bytes).ok();
    let key = text.and_then(|text| PublicKey::parse(text.lines().next().unwrap_or(text)));
    if let Some(key) = &key {
        assert_eq!(key.id().len(), 16);
    }
    if let Ok(signature) = Signature::parse(bytes) {
        assert!(bytes.len() <= 4 << 10);
        let comment = signature.trusted_comment().bytes();
        assert!(comment.clone().all(|b| b == b' ' || b.is_ascii_graphic()));
        assert_eq!(signature.key_id().len(), 16);
        assert!(key.is_none_or(|key| !signature.verify(&key, &prehash(bytes))));
    }
});
