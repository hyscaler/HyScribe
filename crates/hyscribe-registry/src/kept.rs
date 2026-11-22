// SPDX-License-Identifier: AGPL-3.0-or-later

//! The verified copies kept in `hyscribe/registry/` (ADR 0024): the key
//! list and its root signatures, the index and its signature, each verified
//! again when read, and `state`, the highest serials accepted, so neither
//! can go back. An offline server can copy them in by hand.

use std::io::ErrorKind;
use std::path::Path;

use hyscribe_ext::installed::{read_at_most, replace};
use hyscribe_ext::signature::{MAX_BYTES as MAX_SIGNATURE, PublicKey};

use crate::index::{self, Index};
use crate::keys::{self, KeyList};

/// The highest serials accepted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    /// The key list's.
    pub keys: u64,
    /// The index's.
    pub index: u64,
}

impl State {
    /// `keys <serial>` and `index <serial>`, each on its own line.
    pub fn parse(bytes: &[u8]) -> Option<State> {
        let text = std::str::from_utf8(bytes).ok()?;
        let (keys, index) = text.strip_suffix('\n')?.split_once('\n')?;
        let serial = |line: &str, name: &str| -> Option<u64> {
            let digits = line.strip_prefix(name)?.strip_prefix(' ')?;
            let plain =
                (1..=20).contains(&digits.len()) && digits.bytes().all(|b| b.is_ascii_digit());
            plain.then(|| digits.parse().ok()).flatten()
        };
        Some(State {
            keys: serial(keys, "keys")?,
            index: serial(index, "index")?,
        })
    }

    /// The file's text.
    pub fn text(&self) -> String {
        format!("keys {}\nindex {}\n", self.keys, self.index)
    }
}

/// A kept file, none when there is none.
fn file(folder: &Path, name: &str, max: u64) -> Result<Option<Vec<u8>>, String> {
    match read_at_most(&folder.join(name), max) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("registry/{name} cannot be read: {e}")),
    }
}

/// `state`, all zeros when there is none; one that does not read refuses
/// everything kept.
fn state(folder: &Path) -> Result<State, String> {
    let read = file(folder, "state", 64)?;
    let state = read.map(|bytes| State::parse(&bytes).ok_or("registry/state does not read"));
    state.unwrap_or(Ok(State::default())).map_err(String::from)
}

/// Keeps `serial` as the highest accepted, by `set`, unless one is higher.
fn raise(folder: &Path, serial: u64, set: fn(&mut State) -> &mut u64) -> Result<(), String> {
    let mut state = state(folder)?;
    let kept = set(&mut state);
    if serial < *kept {
        return Err(format!(
            "it is older than one already accepted (serial {serial} < {kept})"
        ));
    }
    if serial > *kept {
        *kept = serial;
        let written = replace(&folder.join("state"), state.text().as_bytes());
        written.map_err(|e| format!("registry/state cannot be written: {e}"))?;
    }
    Ok(())
}

/// The kept key list, verified against `roots` at `now`: none when there
/// is none, refused when it does not verify or goes back.
pub fn keys(folder: &Path, roots: &[PublicKey], now: u64) -> Result<Option<KeyList>, String> {
    let Some(bytes) = file(folder, "keys.json", keys::MAX_BYTES)? else {
        return Ok(None);
    };
    let mut signatures = Vec::new();
    for n in 1..=3 {
        signatures.extend(file(
            folder,
            &format!("keys.json.{n}.minisig"),
            MAX_SIGNATURE,
        )?);
    }
    let list = KeyList::verify(&bytes, &signatures, roots, now)?;
    let raised = raise(folder, list.serial, |state| &mut state.keys);
    raised.map_err(|e| format!("registry/keys.json: {e}"))?;
    Ok(Some(list))
}

/// The kept index, verified against `keys` at `now`: none when there is
/// none, refused when it does not verify or goes back.
pub fn index(folder: &Path, keys: &KeyList, now: u64) -> Result<Option<Index>, String> {
    let Some(bytes) = file(folder, "index.json", index::MAX_BYTES)? else {
        return Ok(None);
    };
    let signature = file(folder, "index.json.minisig", MAX_SIGNATURE)?;
    let signature = signature.ok_or("registry/index.json has no index.json.minisig beside it")?;
    let index = Index::verify(&bytes, &signature, keys, now)?;
    let raised = raise(folder, index.serial, |state| &mut state.index);
    raised.map_err(|e| format!("registry/index.json: {e}"))?;
    Ok(Some(index))
}

#[cfg(test)]
mod tests;
