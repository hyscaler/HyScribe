// SPDX-License-Identifier: AGPL-3.0-or-later

//! Whose signature vouches for a local package (ADR 0024): a key in
//! `EXTENSION_TRUSTED_KEYS`, or an online key of the registry's key list
//! kept in `registry/`, verified against the compiled-in roots, the kept
//! index's revocations applied to the installed plugins first.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use hyscribe_ext::installed::read_at_most;
use hyscribe_ext::origin::Signer;
use hyscribe_ext::signature::{self, Claim, MAX_BYTES, PublicKey, Signature};
use hyscribe_registry::keys::Use;
use hyscribe_registry::{kept, revoke};

/// The clock, in unix seconds.
pub(crate) fn now() -> u64 {
    let since = SystemTime::now().duration_since(UNIX_EPOCH);
    since.map_or(0, |since| since.as_secs())
}

/// The compiled-in root keys; tests swap in their own, never configuration.
#[cfg(not(test))]
const ROOTS: &[&str] = &hyscribe_registry::ROOTS;

/// The public keys of `hyscribe_testkit::minisign::Key` 1 to 3.
#[cfg(test)]
const ROOTS: &[&str] = &[
    "RWQBAQEBAQEBAYqI4910CfGV/VLbLTy6XXLKZwm/HZQSG/N0iAG0D29c",
    "RWQCAgICAgICAoE5dw6ofRdfVqNUZsNMfszLjYqRtO43ol32D1uPybOU",
    "RWQDAwMDAwMDA+1JKMYo0cLG6ukDOJBZlWEpWSc6XGP5NjbBRhSshzfR",
];

/// Who vouches for `bytes` by the signature file at `path`: the signer,
/// the key and the file, once it verifies.
pub(crate) fn vouched(
    home: &Path,
    env: &HashMap<String, String>,
    bytes: &[u8],
    path: &Path,
    now: u64,
) -> Result<(Signer, PublicKey, Vec<u8>), String> {
    let file = match read_at_most(path, MAX_BYTES) {
        Ok(file) => file,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(format!("no signature {} beside it", path.display()));
        }
        Err(e) => return Err(format!("cannot read {}: {e}", path.display())),
    };
    let signature = Signature::parse(&file).map_err(|e| format!("{}: {e}", path.display()))?;
    let trusted = trusted_keys(home, env)?;
    let (signer, key) = match trusted
        .into_iter()
        .find(|key| key.id() == signature.key_id())
    {
        Some(key) => (Signer::Trusted, key),
        None => (Signer::Signed, registry_key(home, &signature, now)?),
    };
    signature::vouch(bytes, &key, &file)?;
    Ok((signer, key, file))
}

/// The set up site's `EXTENSION_TRUSTED_KEYS`, none before setup.
fn trusted_keys(home: &Path, env: &HashMap<String, String>) -> Result<Vec<PublicKey>, String> {
    let config = hyscribe_config::load(home, env).map_err(|e| e.to_string())?;
    let listed = config.map(|config| config.extension_trusted_keys);
    let keys = listed.unwrap_or_default().into_iter().map(|key| {
        PublicKey::parse(&key)
            .ok_or_else(|| format!("EXTENSION_TRUSTED_KEYS: {key} is not a minisign public key"))
    });
    keys.collect()
}

/// The registry key that signed `signature`, by the kept key list, allowed
/// to sign packages when it says it signed; a version the kept index
/// revokes is refused.
fn registry_key(home: &Path, signature: &Signature, now: u64) -> Result<PublicKey, String> {
    let folder = home.join("registry");
    let roots: Vec<PublicKey> = ROOTS.iter().copied().filter_map(PublicKey::parse).collect();
    let keys = kept::keys(&folder, &roots, now)?.ok_or(
        "no key list is kept in registry/ to check its signature against: copy the registry's v1/keys.json and its signatures there",
    )?;
    let index = kept::index(&folder, &keys, now)?;
    revoke::revoke(&home.join("plugins"), &keys, index.as_ref(), now)?;
    let claim = Claim::parse(signature.trusted_comment());
    let claim = claim.ok_or("its signature's trusted comment names no package")?;
    let revoked = index.as_ref();
    let revoked = revoked.and_then(|index| index.revoked(&claim.id, &claim.version, &claim.sha256));
    if let Some(reason) = revoked {
        return Err(format!(
            "{} {} is revoked: {reason}",
            claim.id, claim.version
        ));
    }
    keys.key(&signature.key_id(), Use::Packages, claim.signed)
        .cloned()
}

/// How the install tells who signed the package.
pub(crate) fn told(signer: Signer, key: &PublicKey) -> String {
    if signer == Signer::Trusted {
        format!(
            "signed by key {}, which EXTENSION_TRUSTED_KEYS trusts",
            key.id()
        )
    } else {
        format!("signed by the registry's key {}", key.id())
    }
}
