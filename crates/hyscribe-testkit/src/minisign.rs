// SPDX-License-Identifier: AGPL-3.0-or-later

//! Minisign keys and prehashed signatures for tests (ADR 0024), as the
//! `minisign` tool writes them; never used outside tests.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use blake2::{Blake2b512, Digest as _};
use ring::error::KeyRejected;
use ring::signature::{Ed25519KeyPair, KeyPair as _};

/// A test signing key made from a fixed seed, so signatures are the same
/// each run; its key id is the seed byte repeated.
pub struct Key(pub u8);

impl Key {
    fn pair(&self) -> Result<Ed25519KeyPair, KeyRejected> {
        Ed25519KeyPair::from_seed_unchecked(&[self.0; 32])
    }

    /// Its public key, as minisign prints it.
    pub fn public(&self) -> String {
        let key = self.pair().map(|pair| {
            let public = [b"Ed", &[self.0; 8][..], pair.public_key().as_ref()].concat();
            STANDARD.encode(public)
        });
        key.unwrap_or_default()
    }

    /// Its key id, as minisign prints it.
    pub fn id(&self) -> String {
        format!("{:016X}", u64::from_le_bytes([self.0; 8]))
    }

    /// A signature file for `file` with `trusted` as its trusted comment.
    pub fn sign(&self, file: &[u8], trusted: &str) -> String {
        let signed = self.pair().map(|pair| {
            let signature = pair.sign(&Blake2b512::digest(file));
            let first = [b"ED", &[self.0; 8][..], signature.as_ref()].concat();
            let global = pair.sign(&[signature.as_ref(), trusted.as_bytes()].concat());
            format!(
                "untrusted comment: signature from minisign secret key\n{}\ntrusted comment: {trusted}\n{}\n",
                STANDARD.encode(first),
                STANDARD.encode(global.as_ref())
            )
        });
        signed.unwrap_or_default()
    }
}

/// An online key of a key list: a key, what it may sign
/// (`"index","packages"`), and its first and last second.
pub type Online<'a> = (&'a Key, &'a str, u64, u64);

/// A key list (`keys.json`) of `serial` and `expires`.
pub fn key_list(serial: u64, expires: u64, keys: &[Online<'_>], revoked: &[&str]) -> String {
    let keys: Vec<String> = keys
        .iter()
        .map(|(key, uses, not_before, not_after)| {
            format!(
                r#"{{"id":"{}","public_key":"{}","may_sign":[{uses}],"not_before":{not_before},"not_after":{not_after}}}"#,
                key.id(),
                key.public()
            )
        })
        .collect();
    let revoked: Vec<String> = revoked.iter().map(|id| format!("\"{id}\"")).collect();
    format!(
        r#"{{"serial":{serial},"expires":{expires},"keys":[{}],"revoked":[{}]}}"#,
        keys.join(","),
        revoked.join(",")
    )
}

/// The signatures of a key list by `roots`, with the trusted comment the
/// registry signs it with.
pub fn sign_keys(list: &str, serial: u64, expires: u64, roots: &[&Key]) -> Vec<String> {
    let comment = format!("hyscribe-keys serial={serial} expires={expires}");
    roots
        .iter()
        .map(|root| root.sign(list.as_bytes(), &comment))
        .collect()
}
