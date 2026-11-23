// SPDX-License-Identifier: AGPL-3.0-or-later

//! Where an installed extension came from, `plugins/<id>/origin.txt` (ADR
//! 0024): who signed its package and the package's SHA-256, then, once the
//! registry revokes it, when and why, so it stays refused across restarts.
//! A package cannot carry a file of this name.

use std::fmt::Write as _;
use std::io::ErrorKind;
use std::path::Path;

use crate::faults;
use crate::installed::{read_at_most, replace};
use crate::signature::is_key_id;

/// The file's name in the extension's folder.
pub const FILE: &str = "origin.txt";

/// The most of the file read, in bytes.
const MAX_BYTES: u64 = 4 << 10;

/// The longest reason for a revocation, in bytes.
const MAX_REASON: usize = 1_000;

/// Who signed the package: the first word of the file's first line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signer {
    /// A key of the registry's key list, for a package installed by name.
    Registry,
    /// A key of the registry's key list, for a local file with its signature.
    Signed,
    /// A key in `EXTENSION_TRUSTED_KEYS`.
    Trusted,
}

impl Signer {
    fn word(self) -> &'static str {
        match self {
            Signer::Registry => "registry",
            Signer::Signed => "signed",
            Signer::Trusted => "trusted",
        }
    }
}

/// The file as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    /// Who signed it and the key's id, none for an `unsigned` package.
    pub signer: Option<(Signer, String)>,
    /// The package's SHA-256, in lowercase hex.
    pub sha256: String,
    /// When the registry revoked it (unix seconds), and why.
    pub revoked: Option<(u64, String)>,
}

impl Origin {
    /// Reads the file as [`Origin::text`] writes it; `None` for anything else.
    pub fn parse(bytes: &[u8]) -> Option<Origin> {
        let text = std::str::from_utf8(bytes).ok()?.strip_suffix('\n')?;
        let (first, revoked) = match text.split_once('\n') {
            Some((first, second)) => (first, Some(revocation(second)?)),
            None => (text, None),
        };
        let fields: Vec<&str> = first.split(' ').collect();
        let (signer, sha256) = match fields.as_slice() {
            ["unsigned", sha256] => (None, *sha256),
            [word, key, sha256] if is_key_id(key) => {
                let signer = [Signer::Registry, Signer::Signed, Signer::Trusted];
                let signer = signer.into_iter().find(|signer| signer.word() == *word)?;
                (Some((signer, key.to_string())), *sha256)
            }
            _ => return None,
        };
        hex64(sha256).then(|| Origin {
            signer,
            sha256: sha256.into(),
            revoked,
        })
    }

    /// The file's text.
    pub fn text(&self) -> String {
        let mut text = match &self.signer {
            Some((signer, key)) => format!("{} {key} {}\n", signer.word(), self.sha256),
            None => format!("unsigned {}\n", self.sha256),
        };
        if let Some((at, reason)) = &self.revoked {
            let _ = writeln!(text, "revoked {at} {reason}");
        }
        text
    }
}

/// `revoked <unix seconds> <reason>`.
fn revocation(line: &str) -> Option<(u64, String)> {
    let (at, reason) = line.strip_prefix("revoked ")?.split_once(' ')?;
    let at = faults::seconds(at)?;
    reason_text(reason).then(|| (at, reason.into()))
}

/// A revocation's reason as kept: printable ASCII, spaces allowed, at most
/// [`MAX_REASON`] bytes.
pub fn reason_text(reason: &str) -> bool {
    (1..=MAX_REASON).contains(&reason.len())
        && reason.bytes().all(|b| b == b' ' || b.is_ascii_graphic())
}

/// 64 lowercase hex digits, as a SHA-256.
pub fn hex64(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The file in `folder`: none when there is none (an extension installed
/// before ADR 0024), an error when it does not read, which refuses it.
pub fn read(folder: &Path) -> Result<Option<Origin>, String> {
    match read_at_most(&folder.join(FILE), MAX_BYTES) {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{FILE} cannot be read: {e}")),
        Ok(bytes) => Origin::parse(&bytes)
            .map(Some)
            .ok_or_else(|| format!("{FILE} does not read")),
    }
}

/// Writes `origin` whole, renamed into place.
pub fn write(folder: &Path, origin: &Origin) -> std::io::Result<()> {
    replace(&folder.join(FILE), origin.text().as_bytes())
}

#[cfg(test)]
mod tests;
