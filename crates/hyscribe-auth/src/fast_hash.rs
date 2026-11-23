// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_fast_hash()`: libsodium's `crypto_generichash()`, keyed BLAKE2b
//! (RFC 7693), as WordPress hashes password reset keys.

use blake2::Blake2bMac;
use blake2::digest::consts::U30;
use blake2::digest::{FixedOutput, KeyInit, Update};
use hyscribe_phpser::hash_equals;

/// The key `wp_fast_hash()` hashes with.
const KEY: &[u8] = b"wp_fast_hash_6.8+";

/// `wp_fast_hash( $message )`: `$generic$` and the URL safe, unpadded
/// base64 of the message's BLAKE2b-240 under `KEY`.
pub fn wp_fast_hash(message: &[u8]) -> String {
    const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    // Only a key over BLAKE2b's 64 bytes fails, and `KEY` has 17.
    Blake2bMac::<U30>::new_from_slice(KEY)
        .map(|mac| {
            let hash = mac.chain(message).finalize_fixed();
            format!("$generic${}", crate::encode(&hash, URL_SAFE, false))
        })
        .unwrap_or_default()
}

/// `wp_verify_fast_hash( $message, $hash )`: `$generic$` hashes compared, else
/// phpass as reset keys were stored before 6.8, then `crypt()`'s bcrypt.
pub fn wp_verify_fast_hash(message: &[u8], hash: &[u8]) -> bool {
    if hash.starts_with(b"$generic$") {
        return hash_equals(hash, wp_fast_hash(message).as_bytes());
    }
    if message.len() > crate::MAX_PASSWORD {
        return false;
    }
    let phpass = hash.starts_with(b"$P$") || hash.starts_with(b"$H$");
    (phpass && crate::phpass(message, hash)) || crate::password_verify(message, hash)
}

#[cfg(test)]
mod tests {
    use super::{wp_fast_hash, wp_verify_fast_hash};

    /// Every case of `reference/auth/fast-hash-golden.php`.
    #[test]
    fn fast_hashes_are_php_s() {
        let golden = include_str!("../tests/golden/fast-hash.tsv");
        for line in golden.lines() {
            let (message, hash) = line.split_once('\t').unwrap();
            let message = hyscribe_testkit::unhex(message);
            assert_eq!(wp_fast_hash(&message), hash, "{line}");
        }
    }

    /// Every case of `reference/auth/verify-fast-hash-golden.php`.
    #[test]
    fn verified_fast_hashes_are_php_s() {
        let golden = include_str!("../tests/golden/verify-fast-hash.tsv");
        for line in golden.lines() {
            let fields: Vec<&str> = line.split('\t').collect();
            let message = hyscribe_testkit::unhex(fields[0]);
            let verified = wp_verify_fast_hash(&message, fields[1].as_bytes());
            assert_eq!(verified, fields[2] == "1", "{line:.80}");
        }
        assert_eq!(golden.lines().count(), 16);
    }
}
