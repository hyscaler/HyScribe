// SPDX-License-Identifier: AGPL-3.0-or-later

//! The index, `index.json` (ADR 0024): the extensions and the revoked versions, signed
//! by an index key with the comment `hyscribe-index serial=<n> expires=<unix>`.

use hyscribe_ext::manifest;
use hyscribe_ext::origin::{hex64, reason_text};
use hyscribe_ext::signature::{Signature, prehash};

use crate::json::{self, Object};
use crate::keys::{KeyList, Use};

/// The largest index read, ADR 0019's cap, in bytes.
pub const MAX_BYTES: u64 = 4 << 20;

/// The largest version document, in bytes.
const MAX_DOCUMENT: u64 = 1 << 20;

/// A revoked version.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Revoked {
    /// `id`.
    pub id: String,
    /// `version`.
    pub version: String,
    /// `sha256` of its package.
    pub sha256: String,
    /// `reason`, shown to the operator.
    pub reason: String,
}

/// An index as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Index {
    /// `serial`, which never goes down.
    pub serial: u64,
    /// `expires`: from this second on it is refused.
    pub expires: u64,
    /// `revoked`.
    pub revoked: Vec<Revoked>,
}

impl Index {
    /// Reads an index; any member missing, unknown or out of its bounds
    /// refuses it.
    pub fn parse(bytes: &[u8]) -> Option<Index> {
        let mut index = Object::decode(bytes)?;
        let (serial, expires) = (index.int("serial")?, index.int("expires")?);
        let extensions = index.list("extensions")?.into_iter().map(extension);
        let extensions = extensions.collect::<Option<Vec<_>>>()?;
        let revoked = index.list("revoked")?.into_iter().map(revoked);
        let revoked = revoked.collect::<Option<Vec<_>>>()?;
        index.done()?;
        let mut ids = extensions.clone();
        ids.sort_unstable();
        ids.dedup();
        (ids.len() == extensions.len()).then_some(Index {
            serial,
            expires,
            revoked,
        })
    }

    /// The index in `bytes`, if a key of `keys` allowed to sign `index` at
    /// `now` signed it, and it has not expired at `now`.
    pub fn verify(
        bytes: &[u8],
        signature: &[u8],
        keys: &KeyList,
        now: u64,
    ) -> Result<Index, String> {
        if bytes.len() as u64 > MAX_BYTES {
            return Err(format!("index.json is larger than {MAX_BYTES} bytes"));
        }
        let index = Index::parse(bytes).ok_or("index.json does not read")?;
        let signature = Signature::parse(signature).map_err(|e| format!("index.json: {e}"))?;
        let key = keys.key(&signature.key_id(), Use::Index, now)?;
        let comment = format!(
            "hyscribe-index serial={} expires={}",
            index.serial, index.expires
        );
        if signature.trusted_comment() != comment || !signature.verify(key, &prehash(bytes)) {
            return Err("index.json's signature does not verify".into());
        }
        if now >= index.expires {
            return Err(format!("index.json expired at {}", index.expires));
        }
        Ok(index)
    }

    /// Why the package `sha256` of `id` at `version` is revoked, if it is.
    pub fn revoked(&self, id: &str, version: &str, sha256: &str) -> Option<&str> {
        let found = self.revoked.iter().find(|revoked| {
            (
                revoked.id.as_str(),
                revoked.version.as_str(),
                revoked.sha256.as_str(),
            ) == (id, version, sha256)
        });
        found.map(|revoked| revoked.reason.as_str())
    }
}

/// `{"id", "kind", "latest": {<sdk>: <version>}, "sha256", "size"}`, checked
/// until install by name reads it; its id.
fn extension(value: hyscribe_phpser::Value) -> Option<String> {
    let mut entry = Object::of(value)?;
    let id = entry.text("id").filter(|id| manifest::slug(id))?;
    entry
        .text("kind")
        .filter(|kind| kind == "plugin" || kind == "theme")?;
    let latest = entry
        .object("latest")?
        .rest()
        .into_iter()
        .all(|(sdk, version)| {
            let sdk = match sdk {
                hyscribe_phpser::Key::String(sdk) => String::from_utf8(sdk).ok(),
                hyscribe_phpser::Key::Int(_) => None,
            };
            let version = json::text(version).filter(|version| manifest::version(version));
            sdk.is_some_and(|sdk| manifest::version(&sdk)) && version.is_some()
        });
    entry
        .text("sha256")
        .filter(|sha256| hex64(sha256) && latest)?;
    entry
        .int("size")
        .filter(|size| (1..=MAX_DOCUMENT).contains(size))?;
    entry.done().map(|()| id)
}

/// `{"id", "version", "sha256", "reason"}`.
fn revoked(value: hyscribe_phpser::Value) -> Option<Revoked> {
    let mut entry = Object::of(value)?;
    let revoked = Revoked {
        id: entry.text("id").filter(|id| manifest::slug(id))?,
        version: entry.text("version").filter(|v| manifest::version(v))?,
        sha256: entry.text("sha256").filter(|sha256| hex64(sha256))?,
        reason: entry.text("reason").filter(|reason| reason_text(reason))?,
    };
    entry.done().map(|()| revoked)
}

#[cfg(test)]
mod tests;
