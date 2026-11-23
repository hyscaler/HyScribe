// SPDX-License-Identifier: AGPL-3.0-or-later

//! `hyscribe plugin install <name>[@<version>]` and `hyscribe plugin
//! update` (ADR 0024): the key list and index refreshed (revocations
//! applied), a version chosen from the extension's version document, its
//! package downloaded and verified, then installed as a local file is.

use std::path::{Path, PathBuf};

use hyscribe_ext::hyx::Hyx;
use hyscribe_ext::installed::{self, Installed, Vouch};
use hyscribe_ext::manifest::{self, Kind, Manifest};
use hyscribe_ext::origin::{self, Signer};
use hyscribe_ext::signature::{self, PublicKey, Signature};

use crate::index::Index;
use crate::keys::{KeyList, package_key};
use crate::remote::Remote;
use crate::revoke::revoke;
use crate::versions::{Version, Versions, release};

/// The site the commands work on.
pub struct Site<'a> {
    /// The `hyscribe/` folder.
    pub home: &'a Path,
    /// The root keys the key list must be signed by.
    pub roots: &'a [PublicKey],
    /// The clock, in unix seconds.
    pub now: u64,
}

impl Site<'_> {
    fn plugins(&self) -> PathBuf {
        self.home.join("plugins")
    }
}

/// What the operator's command allows.
#[derive(Clone, Copy, Debug, Default)]
pub struct Allow {
    /// `--downgrade`: a version older than the installed one.
    pub downgrade: bool,
    /// `--allow-new-grants`: grants or limits other than those approved.
    pub new_grants: bool,
    /// `--replace-local`: replacing a plugin installed from a local file.
    pub replace_local: bool,
}

/// Which plugins `hyscribe plugin update` works on.
#[derive(Clone, Copy, Debug)]
pub enum Update<'a> {
    /// `--check`: list the offers, install nothing.
    Check,
    /// These ids.
    Ids(&'a [String]),
    /// `--all`: every plugin installed from the registry.
    All,
}

/// The key list and index, fresh from the registry, with the lines of
/// what their revocations marked.
async fn refresh(remote: &Remote<'_>, site: &Site<'_>) -> Result<(KeyList, Index, String), String> {
    let (keys, index) = remote
        .refresh(&site.home.join("registry"), site.roots, site.now)
        .await?;
    let marked = revoke(&site.plugins(), &keys, Some(&index), site.now)?;
    let lines = marked.iter().map(|line| format!("Revoked {line}\n"));
    Ok((keys, index, lines.collect()))
}

/// `hyscribe plugin install <name>[@<version>]`.
pub async fn install(
    remote: &Remote<'_>,
    site: &Site<'_>,
    (name, version): (&str, Option<&str>),
    allow: Allow,
) -> Result<String, String> {
    let (keys, index, mut text) = refresh(remote, site).await?;
    let one = One {
        remote,
        site,
        keys: &keys,
        index: &index,
    };
    text.push_str(&one.install(name, version, allow, false).await?);
    Ok(text)
}

/// `hyscribe plugin update --check | <id>... | --all`: what it did, and
/// whether any plugin failed; an error only when nothing could be done.
pub async fn update(
    remote: &Remote<'_>,
    site: &Site<'_>,
    which: Update<'_>,
    allow: Allow,
) -> Result<(String, bool), String> {
    let (keys, index, mut text) = refresh(remote, site).await?;
    let installed = installed::list(&site.plugins());
    let ids: Vec<String> = match which {
        Update::Check => {
            text.push_str(&offers(&installed, &index));
            return Ok((text, false));
        }
        Update::Ids(ids) => ids.to_vec(),
        Update::All => installed
            .iter()
            .filter(|plugin| from_registry(plugin))
            .map(|plugin| plugin.manifest.id.clone())
            .collect(),
    };
    let one = One {
        remote,
        site,
        keys: &keys,
        index: &index,
    };
    let mut failed = false;
    for id in &ids {
        let done = match installed.iter().find(|plugin| plugin.manifest.id == *id) {
            None => Err(format!("{id} is not installed")),
            Some(plugin) if !from_registry(plugin) => Err(format!(
                "{id} was installed from a local file, so the registry offers it no update; `hyscribe plugin install {id} --replace-local` replaces it with the registry's"
            )),
            Some(_) => one.install(id, None, allow, true).await,
        };
        match done {
            Ok(line) => text.push_str(&line),
            Err(why) => {
                failed = true;
                text.push_str(&format!("error: {why}\n"));
            }
        }
    }
    if ids.is_empty() {
        text.push_str("No plugin installed from the registry.\n");
    }
    Ok((text, failed))
}

/// Whether the registry signed the plugin's package: by name, or a local
/// file with its signature.
fn from_registry(plugin: &Installed) -> bool {
    let read = origin::read(&plugin.folder).ok().flatten();
    let signer = read.and_then(|origin| origin.signer);
    signer.is_some_and(|(signer, _)| signer != Signer::Trusted)
}

/// The SDK and version the index offers a plugin from the registry: its
/// `latest` for the newest SDK this binary links, when not the installed one.
pub fn offer<'i>(plugin: &Installed, index: &'i Index) -> Option<&'i (String, String)> {
    let manifest = &plugin.manifest;
    let entry = index
        .extension(&manifest.id)
        .filter(|_| from_registry(plugin))?;
    let mut linked = manifest::SDK.iter().rev();
    linked
        .find_map(|sdk| entry.latest.iter().find(|(listed, _)| listed == sdk))
        .filter(|(_, version)| *version != manifest.version)
}

/// `--check`: each plugin from the registry with another version in the
/// index for the newest SDK this binary links, and each revoked one.
fn offers(installed: &[Installed], index: &Index) -> String {
    let mut text = String::new();
    for plugin in installed.iter().filter(|plugin| from_registry(plugin)) {
        let manifest = &plugin.manifest;
        let revoked = origin::read(&plugin.folder).ok().flatten();
        if let Some((_, reason)) = revoked.and_then(|origin| origin.revoked) {
            let (id, version) = (&manifest.id, &manifest.version);
            text.push_str(&format!("{id} {version} is revoked: {reason}\n"));
        }
        if let Some((sdk, version)) = offer(plugin, index) {
            text.push_str(&format!(
                "{} {} is installed; {version} is available (SDK {sdk}): `hyscribe plugin update {}`\n",
                manifest.id, manifest.version, manifest.id
            ));
        }
    }
    if text.is_empty() {
        text.push_str("Every plugin from the registry is up to date.\n");
    }
    text
}

/// One install against a refreshed key list and index.
struct One<'a> {
    remote: &'a Remote<'a>,
    site: &'a Site<'a>,
    keys: &'a KeyList,
    index: &'a Index,
}

impl One<'_> {
    /// Installs `name` at `asked`, else its newest compatible version; an
    /// `update` of the installed version only says it is up to date.
    async fn install(
        &self,
        name: &str,
        asked: Option<&str>,
        allow: Allow,
        update: bool,
    ) -> Result<String, String> {
        let entry = self
            .index
            .extension(name)
            .filter(|e| e.kind == Kind::Plugin);
        let entry = entry.ok_or_else(|| format!("{name} is not a plugin in the registry"))?;
        let versions = self.remote.versions(entry).await?;
        let chosen = choose(&versions, self.index, asked)?;
        let plugins = self.site.plugins();
        let installed = installed::list(&plugins);
        let installed = installed
            .into_iter()
            .find(|plugin| plugin.manifest.id == name);
        if let Some(plugin) = &installed {
            if !from_registry(plugin) && !allow.replace_local {
                return Err(format!(
                    "{name} was installed from a local file, not from the registry; --replace-local replaces it with the registry's"
                ));
            }
            if update && plugin.manifest.version == chosen.version && !revoked(plugin) {
                return Ok(format!("{name} {} is up to date.\n", chosen.version));
            }
            older(&versions, plugin, chosen, allow)?;
        }
        let packages = self.site.home.join("packages");
        let (bytes, file) = self.remote.package(name, chosen, &packages).await?;
        let read = Signature::parse(&file).map_err(|e| format!("its signature: {e}"))?;
        let key = package_key(self.keys, Some(self.index), &read)?;
        signature::vouch(&bytes, &key, &file)?;
        let manifest = Hyx::read(&bytes)?.manifest;
        let described = (
            &manifest.id,
            &manifest.version,
            &manifest.sdk,
            &manifest.replaces,
        );
        let listed = (&versions.id, &chosen.version, &chosen.sdk, &chosen.replaces);
        if described != listed || manifest.replaces_versions != chosen.replaces_versions {
            return Err(format!(
                "{name} {}'s manifest is not what its version document says",
                chosen.version
            ));
        }
        let approved = installed.as_ref().and_then(installed::approval);
        let changed = approved.as_ref().filter(|approved| {
            (&approved.grants, approved.limits) != (&manifest.grants, manifest.limits)
        });
        if let Some(approved) = changed.filter(|_| !allow.new_grants) {
            let new = asks(&manifest, Some(approved));
            let new = if new.is_empty() {
                "fewer".into()
            } else {
                new.join("; ")
            };
            return Err(format!(
                "{name} {} asks for other grants or limits than those approved ({new}); --allow-new-grants installs it, and it runs again once activated from the Plugins screen",
                chosen.version
            ));
        }
        let vouch = Vouch::Signed(Signer::Registry, &key, &file);
        installed::install((&plugins, &packages), &bytes, vouch)?;
        Ok(format!(
            "Installed {} {} into plugins/{name}/ from the registry, signed by its key {}, standing in for {}. It asks for: {}. Its page: {}. It runs once activated from the Plugins screen.\n",
            manifest.name,
            manifest.version,
            key.id(),
            manifest.replaces.as_deref().unwrap_or_default(),
            asks(&manifest, None).join("; "),
            self.remote.url(&chosen.page)
        ))
    }
}

/// Whether the plugin's `origin.txt` marks it revoked.
fn revoked(plugin: &Installed) -> bool {
    let read = origin::read(&plugin.folder).ok().flatten();
    read.is_some_and(|origin| origin.revoked.is_some())
}

/// Refuses `chosen` when it comes before the installed version in the
/// version document (or the installed one is not listed), unless allowed.
fn older(
    versions: &Versions,
    plugin: &Installed,
    chosen: &Version,
    allow: Allow,
) -> Result<(), String> {
    let at = |version: &str| versions.versions.iter().position(|v| v.version == version);
    let installed = &plugin.manifest.version;
    let behind = at(installed).is_none_or(|installed| at(&chosen.version) < Some(installed));
    if behind && !allow.downgrade && from_registry(plugin) {
        return Err(format!(
            "{} {} is installed and {} is not newer; --downgrade installs it",
            versions.id, installed, chosen.version
        ));
    }
    Ok(())
}

/// The version `asked`, else the newest one that is not revoked (by the
/// version document or the index), links an SDK this binary has and needs
/// no newer HyScribe; if none fits, why not.
fn choose<'v>(
    versions: &'v Versions,
    index: &Index,
    asked: Option<&str>,
) -> Result<&'v Version, String> {
    let id = &versions.id;
    let binary = release(env!("CARGO_PKG_VERSION")).unwrap_or_default();
    let why = |v: &Version| {
        let revoked = v.revoked.as_deref();
        let revoked = revoked.or_else(|| index.revoked(id, &v.version, &v.sha256));
        let [major, minor, patch] = v.requires_hyscribe;
        if let Some(reason) = revoked {
            Some(format!("{id} {} is revoked: {reason}", v.version))
        } else if !manifest::SDK.contains(&v.sdk.as_str()) {
            Some(format!(
                "{id} {} needs SDK {}, which this HyScribe does not link",
                v.version, v.sdk
            ))
        } else if v.requires_hyscribe > binary {
            Some(format!(
                "{id} {} needs HyScribe {major}.{minor}.{patch} or newer",
                v.version
            ))
        } else {
            None
        }
    };
    let found = match asked {
        Some(asked) => versions.versions.iter().find(|v| v.version == asked),
        None => versions.versions.iter().rev().find(|v| why(v).is_none()),
    };
    match (found, asked) {
        (Some(found), _) => why(found).map_or(Ok(found), Err),
        (None, Some(asked)) => Err(format!("the registry has no version {asked} of {id}")),
        (None, None) => Err(versions
            .versions
            .last()
            .and_then(why)
            .unwrap_or_else(|| format!("the registry has no version of {id}"))),
    }
}

/// What `manifest` asks for, as lines; beside `approved`, only what is new
/// to it, and its limits only when they differ.
fn asks(manifest: &Manifest, approved: Option<&Manifest>) -> Vec<String> {
    let before = approved.map(listed).unwrap_or_default();
    let mut lines: Vec<String> = listed(manifest)
        .into_iter()
        .filter_map(|(label, names)| {
            let had = before.iter().find(|(was, _)| *was == label);
            let had = had.map(|(_, names)| names.as_slice()).unwrap_or_default();
            let new: Vec<String> = names.into_iter().filter(|n| !had.contains(n)).collect();
            (!new.is_empty()).then(|| format!("{label}: {}", new.join(", ")))
        })
        .collect();
    let limits = manifest.limits;
    if approved.is_none_or(|approved| approved.limits != limits) {
        lines.push(format!(
            "limits of {} ms a call, {} ms a request and {} MiB",
            limits.call().as_millis(),
            limits.request().as_millis(),
            limits.memory() >> 20
        ));
    }
    lines
}

/// The grants a manifest lists, with its sensitive hooks.
fn listed(manifest: &Manifest) -> Vec<(&'static str, Vec<String>)> {
    let mut listed = manifest.grants.listed();
    let sensitive: Vec<String> = manifest.grants.sensitive.iter().cloned().collect();
    listed.extend((!sensitive.is_empty()).then_some(("Sensitive hooks", sensitive)));
    listed
}

#[cfg(test)]
mod tests;
