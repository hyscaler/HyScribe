// SPDX-License-Identifier: AGPL-3.0-or-later

//! The embedder's extensions (MVP 11.3): the host, made when an extension
//! first needs it, each installed extension loaded once per installed
//! version with the grants its approval holds, and the set `active_plugins`
//! names with the active theme's component last (ADR 0023), kept until one
//! of them changes.

use std::collections::BTreeMap;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use crate::hyx::{COMPONENT, MAX_ENTRY};
use crate::installed::{self, APPROVED, Installed};
use crate::{Extension, Extensions, Fault, Host, Package, Report, Request, Site, manifest, origin};

/// The files an extension loads from; a change to any is a new version.
const FILES: [&str; 4] = [manifest::FILE, APPROVED, COMPONENT, origin::FILE];

/// Each of [`FILES`] as it stands: device, inode, size and modification
/// time, none for a missing one.
type Version = Vec<Option<(u64, u64, u64, i64, i64)>>;

/// What loading an extension gave: none when it is not installed or needs
/// approving, which is no fault.
type Loaded = Option<Result<Arc<Extension>, Fault>>;

/// The active theme, as the `stylesheet` and `template` options name it.
#[derive(Clone, Copy, Debug)]
pub struct Active<'a> {
    /// `stylesheet`: the theme whose component runs, a child's own.
    pub stylesheet: &'a str,
    /// `template`: its parent, or itself.
    pub template: &'a str,
}

#[derive(Default)]
struct Cache {
    /// Each extension by its plugin file, a theme by `stylesheet:template`:
    /// the version loaded and what it gave.
    loaded: BTreeMap<String, (Version, Loaded)>,
    /// The last set made.
    set: Option<Arc<Extensions>>,
}

/// The extensions installed in a plugins folder and a themes folder, as
/// requests run them.
pub struct Runtime {
    plugins: PathBuf,
    themes: PathBuf,
    host: OnceLock<Result<Host, Fault>>,
    cache: Mutex<Cache>,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("plugins", &self.plugins)
            .field("themes", &self.themes)
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
        extension: file
            .split(['/', ':'])
            .next()
            .unwrap_or_default()
            .to_string(),
        hook: "load".into(),
        callback: None,
        fault,
    }
}

impl Runtime {
    /// The extensions installed in `plugins` and `themes`; nothing is read
    /// until a request names one.
    pub fn new(plugins: &Path, themes: &Path) -> Runtime {
        Runtime {
            plugins: plugins.to_path_buf(),
            themes: themes.to_path_buf(),
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

    /// The extension `key` names (a plugin file, or a theme's stylesheet
    /// and template with a `:`), whose files stand at `now` and which
    /// `find` finds, as approved, loaded anew when its files changed, which
    /// are all a request reads while they do not; the second value is a
    /// fault met anew.
    fn active(
        &self,
        cache: &mut Cache,
        key: &str,
        now: Option<Version>,
        find: impl FnOnce() -> Option<Installed>,
    ) -> (Option<Arc<Extension>>, Option<Report>) {
        let cached = cache.loaded.get(key);
        if let Some((_, loaded)) = cached.filter(|(seen, _)| now.as_ref() == Some(seen)) {
            return (loaded.clone().and_then(Result::ok), None);
        }
        let found = find();
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
            Some(now) => cache.loaded.insert(key.to_string(), (now, loaded)),
            None => cache.loaded.remove(key),
        };
        (extension, fault.map(|fault| loading(key, fault)))
    }

    /// The plugin standing in for `file`.
    fn plugin(&self, cache: &mut Cache, file: &str) -> (Option<Arc<Extension>>, Option<Report>) {
        let id = file.split_once('/').map(|(id, _)| id);
        let folder = id
            .filter(|id| manifest::slug(id))
            .map(|id| self.plugins.join(id));
        let now = folder.map(|folder| version(&folder));
        self.active(cache, file, now, || installed::find(&self.plugins, file))
    }

    /// The active theme's component, with its parent installed beside it
    /// when it is a child.
    fn theme(
        &self,
        cache: &mut Cache,
        theme: Active<'_>,
    ) -> (Option<Arc<Extension>>, Option<Report>) {
        let (stylesheet, template) = (theme.stylesheet, theme.template);
        let now = manifest::slug(stylesheet).then(|| {
            let mut now = version(&self.themes.join(stylesheet));
            let parent = manifest::slug(template).then(|| version(&self.themes.join(template)));
            now.extend(parent.into_iter().flatten());
            now
        });
        let find = || installed::theme(&self.themes, stylesheet, template);
        self.active(cache, &format!("{stylesheet}:{template}"), now, find)
    }

    /// The extensions active in a request, the plugin files `files` names
    /// (as `active_plugins` lists them) in their order, then the theme,
    /// each installed, approved and loading; with what failed to load since
    /// it last changed, for the log.
    fn set(&self, files: &[String], theme: Option<Active<'_>>) -> (Arc<Extensions>, Vec<Report>) {
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        let mut reports = Vec::new();
        let mut list = Vec::new();
        let plugins = files.iter().map(|file| self.plugin(&mut cache, file));
        let all: Vec<_> = plugins.collect();
        let theme = theme.map(|theme| self.theme(&mut cache, theme));
        for (extension, report) in all.into_iter().chain(theme) {
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

    /// A request on `site` over the extensions active for `files` and the
    /// active `theme`; none when there are none.
    pub fn request(
        &self,
        files: &[String],
        theme: Option<Active<'_>>,
        site: Arc<dyn Site>,
    ) -> (Option<Request>, Vec<Report>) {
        let (set, reports) = self.set(files, theme);
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
        let (set, _) = self.set(files, None);
        let mut list = set.list.clone();
        list.push(Arc::new(extension));
        let set = Arc::new(Extensions::new(list));
        Ok(Request::new(host, &set, site))
    }
}

#[cfg(test)]
mod tests;
