// SPDX-License-Identifier: AGPL-3.0-or-later

//! The embedder's extensions (MVP 11.3): the host, made when an extension
//! first needs it, each installed extension loaded once per installed
//! version with the grants its approval holds, and the set `active_plugins`
//! names, kept until one of them changes.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::hyx::{COMPONENT, MAX_ENTRY};
use crate::installed::{self, APPROVED, Installed};
use crate::{Extension, Extensions, Fault, Host, Package, Report, Request, Site, manifest};

/// The files an extension loads from; a change to any is a new version.
const FILES: [&str; 3] = [manifest::FILE, APPROVED, COMPONENT];

/// Each of [`FILES`] as it stands: device, inode, size and modification
/// time, none for a missing one.
type Version = Vec<Option<(u64, u64, u64, i64, i64)>>;

/// What loading an extension gave: none when it is not installed or needs
/// approving, which is no fault.
type Loaded = Option<Result<Arc<Extension>, Fault>>;

#[derive(Default)]
struct Cache {
    /// Each extension by its plugin file: the version loaded and what it gave.
    loaded: BTreeMap<String, (Version, Loaded)>,
    /// The last set made.
    set: Option<Arc<Extensions>>,
}

/// The extensions installed in a plugins folder, as requests run them.
pub struct Runtime {
    plugins: PathBuf,
    host: OnceLock<Result<Host, Fault>>,
    cache: Mutex<Cache>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("plugins", &self.plugins)
            .finish_non_exhaustive()
    }
}

fn version(folder: &Path) -> Version {
    let stat = |name: &&str| std::fs::metadata(folder.join(name)).ok();
    let each = FILES.iter().map(stat);
    each.map(|m| m.map(|m| (m.dev(), m.ino(), m.size(), m.mtime(), m.mtime_nsec())))
        .collect()
}

/// A report of what loading `file`'s extension met.
fn loading(file: &str, fault: Fault) -> Report {
    Report {
        extension: file.split('/').next().unwrap_or_default().to_string(),
        hook: "load".into(),
        callback: None,
        fault,
    }
}

impl Runtime {
    /// The extensions installed in `plugins`; nothing is read until a
    /// request names one.
    pub fn new(plugins: &Path) -> Runtime {
        Runtime {
            plugins: plugins.to_path_buf(),
            host: OnceLock::new(),
            cache: Mutex::default(),
        }
    }

    /// The plugins folder, `hyscribe/plugins/`.
    pub fn plugins(&self) -> &Path {
        &self.plugins
    }

    /// The extension of `installed`, with the grants and limits of
    /// `manifest`, its component read no larger than a package's entry,
    /// and the host it loaded in, made at the first load.
    fn load(
        &self,
        installed: &Installed,
        manifest: manifest::Manifest,
    ) -> Result<(Extension, &Host), Fault> {
        let path = installed.folder.join(COMPONENT);
        let wasm = installed::read_at_most(&path, MAX_ENTRY);
        let wasm = wasm.map_err(|e| Fault::Refused(format!("{}: {e}", path.display())))?;
        let host = self.host.get_or_init(Host::new).as_ref();
        host.map_err(Clone::clone).and_then(|host| {
            let package = Package {
                kind: manifest.kind,
                sdk: &manifest.sdk,
                slug: &manifest.id,
                wasm: &wasm,
                grants: manifest.grants,
                limits: manifest.limits,
                folder: &installed.folder,
            };
            host.load(package).map(|extension| (extension, host))
        })
    }

    /// The extension standing in for `file`, as approved, loaded anew when
    /// its files changed, which are all a request reads while they do not;
    /// the second value is a fault met anew.
    fn active(&self, cache: &mut Cache, file: &str) -> (Option<Arc<Extension>>, Option<Report>) {
        let id = file.split_once('/').map(|(id, _)| id);
        let now = id
            .filter(|id| manifest::slug(id))
            .map(|id| version(&self.plugins.join(id)));
        let cached = cache.loaded.get(file);
        if let Some((_, loaded)) = cached.filter(|(seen, _)| now.as_ref() == Some(seen)) {
            return (loaded.clone().and_then(Result::ok), None);
        }
        let found = installed::find(&self.plugins, file);
        let loaded: Loaded = found.and_then(|found| {
            let approved = installed::approval(&found)?;
            Some(
                self.load(&found, approved)
                    .map(|(extension, _)| Arc::new(extension)),
            )
        });
        let fault = loaded.clone().and_then(Result::err);
        let extension = loaded.clone().and_then(Result::ok);
        match now {
            Some(now) => cache.loaded.insert(file.to_string(), (now, loaded)),
            None => cache.loaded.remove(file),
        };
        (extension, fault.map(|fault| loading(file, fault)))
    }

    /// The extensions active in a request, the plugin files `files` names
    /// (as `active_plugins` lists them) in their order, each installed,
    /// approved and loading; with what failed to load since it last
    /// changed, for the log.
    fn set(&self, files: &[String]) -> (Arc<Extensions>, Vec<Report>) {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        let mut reports = Vec::new();
        let mut list = Vec::new();
        for file in files {
            let (extension, report) = self.active(&mut cache, file);
            list.extend(extension);
            reports.extend(report);
        }
        let same = cache.set.as_ref().filter(|set| {
            let ptrs = set.list.iter().map(Arc::as_ptr);
            ptrs.eq(list.iter().map(Arc::as_ptr))
        });
        let set = match same {
            Some(set) => set.clone(),
            None => {
                let set = Arc::new(Extensions::new(list));
                cache.set = Some(set.clone());
                set
            }
        };
        (set, reports)
    }

    /// A request on `site` over the extensions active for `files`; none
    /// when there are none.
    pub fn request(&self, files: &[String], site: Arc<dyn Site>) -> (Option<Request>, Vec<Report>) {
        let (set, reports) = self.set(files);
        // An extension in the set was loaded, so the host was made.
        let host = self.host.get().and_then(|host| host.as_ref().ok());
        let host = host.filter(|_| !set.list.is_empty());
        (host.map(|host| Request::new(host, &set, site)), reports)
    }

    /// The request activating `installed`: the extensions active for
    /// `files` and, last, `installed`'s, loaded with the grants its
    /// activation approves.
    pub fn activating(
        &self,
        files: &[String],
        installed: &Installed,
        site: Arc<dyn Site>,
    ) -> Result<Request, Fault> {
        let (extension, host) = self.load(installed, installed.manifest.clone())?;
        let (set, _) = self.set(files);
        let mut list = set.list.clone();
        list.push(Arc::new(extension));
        let set = Arc::new(Extensions::new(list));
        Ok(Request::new(host, &set, site))
    }
}

#[cfg(test)]
mod tests;
