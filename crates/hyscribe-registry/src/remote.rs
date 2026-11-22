// SPDX-License-Identifier: AGPL-3.0-or-later

//! The hosted registry over HTTPS (ADR 0024): its files fetched through
//! `hyscribe-fetch` from its host only, each verified before it is used or
//! kept. Nothing is sent but the request: no site, no installed list.

use std::path::Path;
use std::time::Duration;

use hyscribe_ext::signature::{MAX_BYTES as MAX_SIGNATURE, PublicKey};
use hyscribe_fetch::{Args, Client, validate_url};

use crate::index::{self, Entry, Index};
use crate::kept::{self, State, Verified};
use crate::keys::{self, KeyList};
use crate::versions::{self, Version, Versions};

/// The registry at a base URL.
pub struct Remote<'a> {
    client: &'a Client,
    base: String,
    host: String,
}

/// What every request says it is.
fn user_agent() -> String {
    format!("HyScribe/{}", env!("CARGO_PKG_VERSION"))
}

impl<'a> Remote<'a> {
    /// The registry at `base`, an `https:` URL ending in `/`, fetched
    /// through `client`.
    pub fn new(client: &'a Client, base: &str) -> Result<Remote<'a>, String> {
        let target = validate_url(base.as_bytes(), b"");
        let target = target.filter(|target| target.https && base.ends_with('/'));
        let target = target.ok_or_else(|| format!("{base} is not an https: registry URL"))?;
        Ok(Remote {
            client,
            base: base.into(),
            host: target.host,
        })
    }

    /// The URL of `path` under the base.
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// The file at `path` under the base, at most `max` bytes, under ADR
    /// 0019's limits; none when the registry answers 404.
    async fn get(&self, path: &str, max: u64) -> Result<Option<Vec<u8>>, String> {
        let url = self.url(path);
        let args = Args {
            timeout: Duration::from_secs(30),
            redirection: 5,
            user_agent: user_agent(),
            limit_response_size: usize::try_from(max + 1).ok(),
        };
        let allowed = |host: &str| host == self.host;
        let fetched = self.client.get_within(url.as_bytes(), b"", args, &allowed);
        let fetched = fetched.await;
        let fetched = fetched.map_err(|e| format!("cannot fetch {url}: {:?}", e.kind))?;
        match fetched.status {
            404 => Ok(None),
            200 if fetched.body.len() as u64 <= max => Ok(Some(fetched.body)),
            200 => Err(format!("{url} is larger than {max} bytes")),
            status => Err(format!("{url} answered {status}")),
        }
    }

    /// The file at `path`, which must be there.
    async fn need(&self, path: &str, max: u64) -> Result<Vec<u8>, String> {
        let found = self.get(path, max).await?;
        found.ok_or_else(|| format!("{} is not there", self.url(path)))
    }

    /// The key list and index, fetched, verified against `roots` at `now`
    /// and kept in `folder` in place of older ones.
    pub async fn refresh(
        &self,
        folder: &Path,
        roots: &[PublicKey],
        now: u64,
    ) -> Result<(KeyList, Index), String> {
        let keys = self.need("v1/keys.json", keys::MAX_BYTES).await?;
        let mut signatures = Vec::new();
        for n in 1..=3 {
            let path = format!("v1/keys.json.{n}.minisig");
            signatures.push(self.get(&path, MAX_SIGNATURE).await?);
        }
        let found: Vec<Vec<u8>> = signatures.iter().flatten().cloned().collect();
        let list = KeyList::verify(&keys, &found, roots, now)?;
        let index = self.need("v1/index.json", index::MAX_BYTES).await?;
        let signature = self.need("v1/index.json.minisig", MAX_SIGNATURE).await?;
        let read = Index::verify(&index, &signature, &list, now)?;
        let verified = Verified {
            keys: (&keys, &signatures),
            index: (&index, &signature),
            serials: State {
                keys: list.serial,
                index: read.serial,
            },
        };
        kept::keep(folder, &verified)?;
        Ok((list, read))
    }

    /// The version document of `entry`, checked against it.
    pub async fn versions(&self, entry: &Entry) -> Result<Versions, String> {
        let path = format!("v1/extensions/{}.json", entry.id);
        let document = self.need(&path, versions::MAX_BYTES).await?;
        Versions::verify(&document, entry)
    }

    /// The package `id` at `version`, downloaded through `packages/` and
    /// checked against its size and SHA-256, with its signature file.
    pub async fn package(
        &self,
        id: &str,
        version: &Version,
        packages: &Path,
    ) -> Result<(Vec<u8>, Vec<u8>), String> {
        let name = format!("{id}-{}.hyx", version.version);
        let path = format!("v1/packages/{id}/{name}");
        let url = self.url(&path);
        let to = packages.join(format!(".download-{id}-{}", version.version));
        let created = std::fs::create_dir_all(packages);
        created.map_err(|e| format!("cannot write {}: {e}", packages.display()))?;
        let agent = user_agent();
        let downloaded = self
            .client
            .download(url.as_bytes(), &agent, (version.size, &to));
        let sha256 = downloaded.await;
        let sha256 = sha256.map_err(|e| format!("cannot download {url}: {e:?}"))?;
        let bytes = std::fs::read(&to).unwrap_or_default();
        let _ = std::fs::remove_file(&to);
        if sha256 != version.sha256 || bytes.len() as u64 != version.size {
            return Err(format!(
                "{name} is not the package its version document names"
            ));
        }
        let signature = self.need(&format!("{path}.minisig"), MAX_SIGNATURE).await?;
        Ok((bytes, signature))
    }
}

#[cfg(test)]
mod tests;
