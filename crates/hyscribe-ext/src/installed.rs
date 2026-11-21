// SPDX-License-Identifier: AGPL-3.0-or-later

//! Installed plugins, each in `hyscribe/plugins/<id>/` (DEPLOYMENT.md):
//! what `hyscribe plugin install` unpacks there, and what the Plugins
//! screen lists, activates and deactivates.

use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use crate::hyx::{COMPONENT, Hyx};
use crate::manifest::{self, Kind, Manifest};
use crate::{Host, Package, faults};

/// The manifest as the administrator approved it at activation, beside
/// the installed one; a package cannot hold a file of this name.
pub const APPROVED: &str = "approved.ext.toml";

/// An installed plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installed {
    /// Its manifest.
    pub manifest: Manifest,
    /// Its folder, `hyscribe/plugins/<id>/`.
    pub folder: PathBuf,
    /// Whether its grants and limits are those approved at its activation.
    pub approved: bool,
    /// Whether its fault record disables it, or does not read (ADR 0022).
    pub disabled: bool,
}

/// The plugins installed in `plugins`, by folder name; a folder whose
/// manifest does not read, or names another id, is not one.
pub fn list(plugins: &Path) -> Vec<Installed> {
    let mut names: Vec<String> = fs::read_dir(plugins)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    names
        .iter()
        .filter_map(|name| read(plugins, name))
        .collect()
}

/// The installed plugin whose `replaces` is `file`, `<id>/<file>.php`.
pub fn find(plugins: &Path, file: &str) -> Option<Installed> {
    let (id, _) = file.split_once('/')?;
    read(plugins, id).filter(|installed| installed.manifest.replaces.as_deref() == Some(file))
}

/// The plugin in `plugins/<id>/`, if one is installed there.
fn read(plugins: &Path, id: &str) -> Option<Installed> {
    if !manifest::slug(id) {
        return None;
    }
    let folder = plugins.join(id);
    let manifest = manifest_in(&folder, manifest::FILE)?;
    if manifest.id != id || manifest.kind != Kind::Plugin {
        return None;
    }
    let approved = manifest_in(&folder, APPROVED).is_some_and(|approved| {
        (approved.grants, approved.limits) == (manifest.grants.clone(), manifest.limits)
    });
    let disabled = faults::read(&folder).is_none_or(|record| record.disabled().is_some());
    Some(Installed {
        manifest,
        folder,
        approved,
        disabled,
    })
}

/// The manifest as approved at the plugin's activation, whose grants and
/// limits it runs with; none while it needs approving.
pub fn approval(installed: &Installed) -> Option<Manifest> {
    let approved = installed.approved.then_some(&installed.folder);
    approved.and_then(|folder| manifest_in(folder, APPROVED))
}

fn manifest_in(folder: &Path, name: &str) -> Option<Manifest> {
    let bytes = read_at_most(&folder.join(name), manifest::MAX_BYTES).ok()?;
    Manifest::parse(&bytes).ok()
}

/// The file at `path`, reading one byte past `max` at most, so a caller
/// can tell a file that is larger.
pub fn read_at_most(path: &Path, max: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    let read = fs::File::open(path).and_then(|f| f.take(max + 1).read_to_end(&mut bytes));
    read.map(|_| bytes)
}

/// Approves the plugin's grants and limits as its manifest now has them:
/// a copy of it, written whole and renamed into place.
pub fn approve(installed: &Installed) -> std::io::Result<()> {
    let bytes = fs::read(installed.folder.join(manifest::FILE))?;
    replace(&installed.folder.join(APPROVED), &bytes)
}

/// Takes the approval back, once the plugin is deactivated.
pub fn revoke(installed: &Installed) -> std::io::Result<()> {
    gone(fs::remove_file(installed.folder.join(APPROVED)))
}

/// A file or folder that was not there counts as removed or moved.
fn gone(done: std::io::Result<()>) -> std::io::Result<()> {
    match done {
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        done => done,
    }
}

/// Writes `bytes` to a new file beside `path`, flushed, then renamed over it.
fn replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut new = path.as_os_str().to_owned();
    new.push(".new");
    let mut options = OpenOptions::new();
    let written = write_with(options.create(true).truncate(true), Path::new(&new), bytes);
    written
        .and_then(|file| file.sync_all())
        .and_then(|()| fs::rename(&new, path))
}

/// Opens `path` for writing with `options`, mode 0644, and writes `bytes`.
fn write_with(options: &mut OpenOptions, path: &Path, bytes: &[u8]) -> std::io::Result<fs::File> {
    let opened = options.write(true).mode(0o644).open(path);
    opened.and_then(|mut file| file.write_all(bytes).map(|()| file))
}

/// `hyscribe plugin install`: reads the package, loads its component with
/// its grants (its `init` run once), so a refused package changes nothing;
/// keeps the package in `packages/` as `<id>-<version>.hyx`, then unpacks
/// it into `plugins/<id>/` in place of an older one, keeping that one's
/// fault record and approval.
pub fn install((plugins, packages): (&Path, &Path), bytes: &[u8]) -> Result<Manifest, String> {
    let hyx = Hyx::read(bytes)?;
    let manifest = &hyx.manifest;
    if manifest.kind != Kind::Plugin {
        return Err("not a plugin package".into());
    }
    let id = manifest.id.as_str();
    let staging = plugins.join(format!(".{id}.new"));
    let package = Package {
        slug: id,
        wasm: hyx.file(COMPONENT).unwrap_or_default(),
        grants: manifest.grants.clone(),
        limits: manifest.limits,
        folder: &staging,
    };
    let cannot = |e: std::io::Error| format!("cannot install into {}: {e}", plugins.display());
    fs::create_dir_all(plugins).map_err(cannot)?;
    let loaded = Host::new().and_then(|host| host.load(package));
    loaded.map_err(|fault| format!("the component does not load: {fault:?}"))?;
    let name = format!("{id}-{}.hyx", manifest.version);
    let unpacked = fs::create_dir_all(packages)
        .and_then(|()| replace(&packages.join(name), bytes))
        .and_then(|()| unpack(plugins, &staging, &hyx));
    unpacked.map_err(|e| {
        let _ = fs::remove_dir_all(&staging);
        cannot(e)
    })?;
    Ok(hyx.manifest)
}

/// Writes the package's files into `staging`, then swaps it in for the
/// plugin's folder.
fn unpack(plugins: &Path, staging: &Path, hyx: &Hyx) -> std::io::Result<()> {
    let folder = plugins.join(&hyx.manifest.id);
    let old = plugins.join(format!(".{}.old", hyx.manifest.id));
    let write = |(name, data): &(String, Vec<u8>)| {
        let path = staging.join(name);
        let parent = path.parent().unwrap_or(staging);
        fs::create_dir_all(parent)
            .and_then(|()| write_with(OpenOptions::new().create_new(true), &path, data).map(drop))
    };
    let keep = |kept: &&str| gone(fs::rename(folder.join(kept), staging.join(kept)));
    gone(fs::remove_dir_all(staging))
        .and_then(|()| gone(fs::remove_dir_all(&old)))
        .and_then(|()| fs::create_dir_all(staging))
        .and_then(|()| hyx.files.iter().try_for_each(write))
        .and_then(|()| [faults::FILE, APPROVED].iter().try_for_each(keep))
        .and_then(|()| gone(fs::rename(&folder, &old)))
        .and_then(|()| fs::rename(staging, &folder))
        .and_then(|()| gone(fs::remove_dir_all(&old)))
}

#[cfg(test)]
mod tests;
