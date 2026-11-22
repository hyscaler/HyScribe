// SPDX-License-Identifier: AGPL-3.0-or-later

//! Revocation of installed extensions (ADR 0024): one whose signing key the
//! key list revokes, or whose version the index revokes, gets a `revoked`
//! line in its `origin.txt`, and stops loading. Packages an operator's key
//! signed, or none did, are not the registry's, so nothing revokes them.

use std::path::Path;

use hyscribe_ext::installed;
use hyscribe_ext::origin::{self, Signer};

use crate::index::Index;
use crate::keys::KeyList;

/// Marks revoked at `now` each plugin in `plugins` that `keys` or `index`
/// revokes and is not marked yet; it answers what it marked, as `<id>:
/// <reason>`, or what it could not write.
pub fn revoke(
    plugins: &Path,
    keys: &KeyList,
    index: Option<&Index>,
    now: u64,
) -> Result<Vec<String>, String> {
    let mut marked = Vec::new();
    for plugin in installed::list(plugins) {
        let Ok(Some(mut origin)) = origin::read(&plugin.folder) else {
            continue;
        };
        let Some((signer, key)) = origin.signer.clone() else {
            continue;
        };
        if signer == Signer::Trusted || origin.revoked.is_some() {
            continue;
        }
        let manifest = &plugin.manifest;
        let reason = if keys.revoked.contains(&key) {
            format!("its signing key {key} was revoked")
        } else {
            let revoked =
                index.and_then(|i| i.revoked(&manifest.id, &manifest.version, &origin.sha256));
            let Some(reason) = revoked else { continue };
            reason.to_string()
        };
        marked.push(format!("{}: {reason}", manifest.id));
        origin.revoked = Some((now, reason));
        let written = origin::write(&plugin.folder, &origin);
        written.map_err(|e| format!("cannot mark {} revoked: {e}", manifest.id))?;
    }
    Ok(marked)
}

#[cfg(test)]
mod tests;
