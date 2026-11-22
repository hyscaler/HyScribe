// SPDX-License-Identifier: AGPL-3.0-or-later

//! Minisign signatures in the prehashed form only (ADR 0024): Ed25519 over
//! the BLAKE2b-512 of the file, and a global Ed25519 signature over that
//! signature and the trusted comment. Untrusted input: read strictly, four
//! lines, each field its exact length.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use blake2::{Blake2b512, Digest as _};
use ring::signature::{ED25519, UnparsedPublicKey};

use crate::{manifest, origin};

/// The most a signature file may hold, in bytes.
pub const MAX_BYTES: u64 = 4 << 10;

const UNTRUSTED: &str = "untrusted comment: ";
const TRUSTED: &str = "trusted comment: ";

/// A minisign public key: its 8 byte id and its Ed25519 key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey {
    id: [u8; 8],
    key: [u8; 32],
}

impl PublicKey {
    /// Reads a public key as minisign prints it: the base64 of `Ed`, the
    /// key id and the key.
    pub fn parse(text: &str) -> Option<PublicKey> {
        let bytes: [u8; 42] = decoded(text)?;
        let (algorithm, rest) = bytes.split_at(2);
        let (id, key) = rest.split_at(8);
        let id = id.try_into().unwrap_or_default();
        let key = key.try_into().unwrap_or_default();
        (algorithm == b"Ed").then_some(PublicKey { id, key })
    }

    /// Its key id, as minisign prints it.
    pub fn id(&self) -> String {
        key_id(self.id)
    }
}

/// A key id as minisign prints it: the little endian number in uppercase hex.
fn key_id(id: [u8; 8]) -> String {
    format!("{:016X}", u64::from_le_bytes(id))
}

/// Whether `text` is a key id as minisign prints it: 16 uppercase hex digits.
pub fn is_key_id(text: &str) -> bool {
    text.len() == 16 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'A'..=b'F'))
}

/// A signature file, read but not yet verified.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signature {
    key_id: [u8; 8],
    signature: [u8; 64],
    trusted: String,
    global: [u8; 64],
}

impl Signature {
    /// Reads a signature file: an untrusted comment, the signature, a
    /// trusted comment and the global signature, the comments printable
    /// ASCII; anything else, the legacy form among it, refuses it.
    pub fn parse(bytes: &[u8]) -> Result<Signature, &'static str> {
        if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_BYTES) {
            return Err("a signature file is at most 4 KiB");
        }
        let text = std::str::from_utf8(bytes).map_err(|_| "a signature file is ASCII text")?;
        let lines: Vec<&str> = text
            .strip_suffix('\n')
            .unwrap_or(text)
            .split('\n')
            .collect();
        let &[untrusted, first, trusted, global] = lines.as_slice() else {
            return Err("a signature file has four lines");
        };
        comment(untrusted, UNTRUSTED).ok_or("the first line is not an untrusted comment")?;
        let trusted = comment(trusted, TRUSTED).ok_or("the third line is not a trusted comment")?;
        let first: [u8; 74] = decoded(first).ok_or("the signature line does not read")?;
        let global = decoded(global).ok_or("the global signature line does not read")?;
        let (algorithm, rest) = first.split_at(2);
        let (key_id, signature) = rest.split_at(8);
        match algorithm {
            b"ED" => Ok(Signature {
                key_id: key_id.try_into().unwrap_or_default(),
                signature: signature.try_into().unwrap_or([0; 64]),
                trusted: trusted.into(),
                global,
            }),
            b"Ed" => Err("a signature of the whole file (minisign's legacy form) is refused"),
            _ => Err("not a minisign Ed25519 signature"),
        }
    }

    /// The id of the key that made it.
    pub fn key_id(&self) -> String {
        key_id(self.key_id)
    }

    /// Its trusted comment, which the global signature covers.
    pub fn trusted_comment(&self) -> &str {
        &self.trusted
    }

    /// Whether `key` made both signatures, of the file whose [`prehash`] is
    /// `prehash` and of the trusted comment.
    pub fn verify(&self, key: &PublicKey, prehash: &[u8; 64]) -> bool {
        let (trusted, global) = (self.trusted.as_bytes(), &self.global);
        self.key_id == key.id && verified(&key.key, prehash, &self.signature, trusted, global)
    }
}

/// The comment after `prefix`, if the line has it and it is printable ASCII.
fn comment<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(prefix);
    rest.filter(|rest| rest.bytes().all(|b| b == b' ' || b.is_ascii_graphic()))
}

/// Exactly `N` bytes in canonical base64.
fn decoded<const N: usize>(line: &str) -> Option<[u8; N]> {
    STANDARD.decode(line).ok()?.try_into().ok()
}

fn verified(
    key: &[u8; 32],
    prehash: &[u8],
    signature: &[u8],
    trusted: &[u8],
    global: &[u8],
) -> bool {
    let key = UnparsedPublicKey::new(&ED25519, key);
    let covered = [signature, trusted].concat();
    key.verify(prehash, signature).is_ok() && key.verify(&covered, global).is_ok()
}

/// The BLAKE2b-512 of a file, which a prehashed signature signs.
pub fn prehash(bytes: &[u8]) -> [u8; 64] {
    Blake2b512::digest(bytes).into()
}

/// The SHA-256 of a file, in lowercase hex.
pub fn sha256(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    digest.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}

/// What a package's signature says of it, its trusted comment: `hyscribe-package
/// id=<id> version=<version> sdk=<sdk> sha256=<hex> signed=<unix>`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Claim {
    /// The manifest's `id`.
    pub id: String,
    /// The manifest's `version`.
    pub version: String,
    /// The manifest's `sdk`.
    pub sdk: String,
    /// The package's SHA-256.
    pub sha256: String,
    /// When it was signed, in unix seconds.
    pub signed: u64,
}

impl Claim {
    /// Reads a package signature's trusted comment, each field in its place.
    pub fn parse(comment: &str) -> Option<Claim> {
        let fields: Vec<&str> = comment
            .strip_prefix("hyscribe-package ")?
            .split(' ')
            .collect();
        let &[id, version, sdk, sha256, signed] = fields.as_slice() else {
            return None;
        };
        let signed = signed.strip_prefix("signed=")?;
        let digits = (1..=20).contains(&signed.len()) && signed.bytes().all(|b| b.is_ascii_digit());
        Some(Claim {
            id: id
                .strip_prefix("id=")
                .filter(|id| manifest::slug(id))?
                .into(),
            version: version
                .strip_prefix("version=")
                .filter(|v| manifest::version(v))?
                .into(),
            sdk: sdk
                .strip_prefix("sdk=")
                .filter(|sdk| manifest::version(sdk))?
                .into(),
            sha256: sha256
                .strip_prefix("sha256=")
                .filter(|h| origin::hex64(h))?
                .into(),
            signed: digits.then(|| signed.parse().ok()).flatten()?,
        })
    }
}

/// What the signature file `signature` by `key` says of the package
/// `bytes`, once both its signatures verify and its trusted comment names a
/// package of `bytes`' SHA-256.
pub fn vouch(bytes: &[u8], key: &PublicKey, signature: &[u8]) -> Result<Claim, String> {
    let signature = Signature::parse(signature).map_err(|e| format!("its signature: {e}"))?;
    if !signature.verify(key, &prehash(bytes)) {
        return Err(format!(
            "its signature does not verify with key {}",
            key.id()
        ));
    }
    let claim = Claim::parse(signature.trusted_comment());
    let claim = claim.ok_or("its signature's trusted comment names no package")?;
    if claim.sha256 != sha256(bytes) {
        return Err("its signature is of another package".into());
    }
    Ok(claim)
}

#[cfg(test)]
mod tests;
