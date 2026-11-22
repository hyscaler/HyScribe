// SPDX-License-Identifier: AGPL-3.0-or-later

//! The key list, `keys.json` (ADR 0024): the online keys and the revoked ones,
//! signed by two roots with the comment `hyscribe-keys serial=<n> expires=<unix>`.

use hyscribe_ext::signature::{PublicKey, Signature, is_key_id, prehash};

use crate::json::{self, Object};

/// The largest key list read, in bytes.
pub const MAX_BYTES: u64 = 64 << 10;

/// The longest an online key may sign: a year, a leap year's.
const YEAR: u64 = 366 * 86_400;

/// What an online key may sign.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Use {
    /// The index.
    Index,
    /// Packages.
    Packages,
}

/// An online key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OnlineKey {
    /// The key.
    pub key: PublicKey,
    /// What it may sign.
    pub uses: Vec<Use>,
    /// The first second it may sign at.
    pub not_before: u64,
    /// The last second it may sign at.
    pub not_after: u64,
}

/// A key list as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyList {
    /// `serial`, which never goes down.
    pub serial: u64,
    /// `expires`: from this second on it is refused.
    pub expires: u64,
    /// `keys`.
    pub keys: Vec<OnlineKey>,
    /// `revoked`: the ids of keys that count for nothing.
    pub revoked: Vec<String>,
}

impl KeyList {
    /// Reads a key list; any member missing, unknown or out of its bounds
    /// refuses it.
    pub fn parse(bytes: &[u8]) -> Option<KeyList> {
        let mut list = Object::decode(bytes)?;
        let (serial, expires) = (list.int("serial")?, list.int("expires")?);
        let keys = list.list("keys")?.into_iter().map(online_key);
        let keys = keys.collect::<Option<Vec<_>>>()?;
        let revoked = list.list("revoked")?.into_iter().map(json::text);
        let revoked = revoked.collect::<Option<Vec<_>>>()?;
        list.done()?;
        let mut ids: Vec<String> = keys.iter().map(|key| key.key.id()).collect();
        ids.sort();
        ids.dedup();
        let unique = ids.len() == keys.len();
        let revoked_ids = revoked.iter().all(|id| is_key_id(id));
        (unique && revoked_ids).then_some(KeyList {
            serial,
            expires,
            keys,
            revoked,
        })
    }

    /// The key list in `bytes`, if two distinct `roots` signed it among
    /// `signatures` (others are passed over) and it has not expired at `now`.
    pub fn verify(
        bytes: &[u8],
        signatures: &[Vec<u8>],
        roots: &[PublicKey],
        now: u64,
    ) -> Result<KeyList, String> {
        if bytes.len() as u64 > MAX_BYTES {
            return Err(format!("keys.json is larger than {MAX_BYTES} bytes"));
        }
        let list = KeyList::parse(bytes).ok_or("keys.json does not read")?;
        let comment = format!(
            "hyscribe-keys serial={} expires={}",
            list.serial, list.expires
        );
        let hash = prehash(bytes);
        let mut signers: Vec<&PublicKey> = Vec::new();
        for signature in signatures.iter().filter_map(|s| Signature::parse(s).ok()) {
            let root = roots.iter().find(|root| root.id() == signature.key_id());
            let root = root.filter(|root| !signers.contains(root));
            let root = root.filter(|root| signature.verify(root, &hash));
            if let Some(root) = root.filter(|_| signature.trusted_comment() == comment) {
                signers.push(root);
            }
        }
        if signers.len() < 2 {
            return Err("keys.json is not signed by two of the root keys".into());
        }
        if now >= list.expires {
            return Err(format!("keys.json expired at {}", list.expires));
        }
        Ok(list)
    }

    /// The key `id`, allowed `what` at `at`, and not revoked.
    pub fn key(&self, id: &str, what: Use, at: u64) -> Result<&PublicKey, String> {
        let found = self.keys.iter().find(|key| key.key.id() == id);
        let found = found.ok_or_else(|| format!("key {id} is not in the key list"))?;
        if self.revoked.iter().any(|revoked| revoked == id) {
            return Err(format!("key {id} is revoked"));
        }
        if !found.uses.contains(&what) {
            let what = format!("{what:?}").to_lowercase();
            return Err(format!("key {id} may not sign {what}"));
        }
        if !(found.not_before..=found.not_after).contains(&at) {
            return Err(format!("key {id} could not sign at {at}"));
        }
        Ok(&found.key)
    }
}

/// `{"id", "public_key", "may_sign", "not_before", "not_after"}`, the id
/// the key's own, its window at most a year.
fn online_key(value: hyscribe_phpser::Value) -> Option<OnlineKey> {
    let mut key = Object::of(value)?;
    let id = key.text("id")?;
    let public = PublicKey::parse(&key.text("public_key")?)?;
    let uses = key
        .list("may_sign")?
        .into_iter()
        .map(|value| match json::text(value)?.as_str() {
            "index" => Some(Use::Index),
            "packages" => Some(Use::Packages),
            _ => None,
        });
    let uses = uses.collect::<Option<Vec<_>>>()?;
    let (not_before, not_after) = (key.int("not_before")?, key.int("not_after")?);
    key.done()?;
    let window = not_after.checked_sub(not_before).is_some_and(|w| w <= YEAR);
    (id == public.id() && window).then_some(OnlineKey {
        key: public,
        uses,
        not_before,
        not_after,
    })
}

#[cfg(test)]
mod tests;
