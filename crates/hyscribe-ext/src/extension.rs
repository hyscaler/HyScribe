// SPDX-License-Identifier: AGPL-3.0-or-later

//! A loaded extension: its component linked once into an `InstancePre`,
//! what its `init` registered, and its fault record.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use hyscribe_phpser::Value;
use wasmtime::Engine;
use wasmtime::component::{Component, ComponentExportIndex, Instance, InstancePre};

use crate::faults::{self, Counted, Record};
use crate::host::{Phase, State, classify, refused};
use crate::manifest::Kind;
use crate::value::{Lifted, Lowered};
use crate::{Fault, Grants, Host, Limits, ObjectKind};

/// What a component's SDK names start with.
const SDK: &str = "hyscribe:sdk/";

/// The interfaces only the `theme` world imports (SDK 0.3, ADR 0023).
const THEME_ONLY: [&str; 3] = ["template", "theme-setup", "customize"];

/// `guest.invoke`'s arguments and answer.
pub(crate) type Args<'a> = (u32, &'a [Lowered<'a>]);
pub(crate) type Answered = (Result<Lifted, String>,);

/// What `guest.init`, `guest.activate` and `guest.deactivate` answer.
pub(crate) type Done = (Result<(), String>,);

/// `theme-guest.render`'s argument and answer.
pub(crate) type Rendered = (Result<Vec<u8>, String>,);

/// What the host needs to load a package; the manifest's parsing and the
/// package's unpacking are the lifecycle's (MVP 11.3).
pub struct Package<'a> {
    /// A plugin, or a theme of the `theme` world.
    pub kind: Kind,
    /// The WordPress slug: its folder's name and its text domain.
    pub slug: &'a str,
    /// The manifest's `sdk`, which the component's imports and export name.
    pub sdk: &'a str,
    /// The component's bytes, compiled in memory, never read back compiled.
    pub wasm: &'a [u8],
    /// What the administrator approved.
    pub grants: Grants,
    /// Its limits, the defaults or what `[limits]` asked.
    pub limits: Limits,
    /// Its folder, `hyscribe/plugins/<slug>/`, where `faults.txt` lives.
    pub folder: &'a Path,
}

/// `add_action()` or `add_filter()` from `init`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hook {
    /// The hook's name.
    pub hook: String,
    /// Added as an action rather than a filter.
    pub action: bool,
    /// The guest's number for its callback.
    pub callback: u32,
    /// `$priority`.
    pub priority: i32,
    /// `$accepted_args`.
    pub accepted_args: u32,
}

/// `register_taxonomy()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Taxonomy {
    /// `$taxonomy`.
    pub name: String,
    /// `$object_type`.
    pub object_types: Vec<String>,
    /// `$args`, decoded.
    pub args: Value,
}

/// `register_meta()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MetaKey {
    /// `$object_type`.
    pub kind: ObjectKind,
    /// `object_subtype`.
    pub subtype: String,
    /// `$meta_key`.
    pub key: String,
    /// `$args`, decoded.
    pub args: Value,
}

/// `register_rest_route()`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Route {
    /// `$route_namespace`, one the manifest grants.
    pub namespace: String,
    /// `$route`, a pattern starting with `/`.
    pub path: String,
    /// `methods`.
    pub methods: Vec<String>,
    /// `callback`.
    pub callback: u32,
    /// `permission_callback`, always run first.
    pub permission: u32,
    /// `args`, decoded.
    pub args: Value,
}

/// `register_block_type()` from the package's `block.json`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// The block's folder inside the package.
    pub path: String,
    /// `render_callback`, for a dynamic block.
    pub render: Option<u32>,
}

/// A template file a theme has (`theme-setup.template-file`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemplateFile {
    /// Its path in the theme, such as `template-parts/header.php`.
    pub file: String,
    /// A page template's `Template Name`.
    pub name: Option<String>,
    /// A page template's `Template Post Type`.
    pub post_types: Vec<String>,
}

/// What `init` registered.
#[derive(Debug, Default)]
pub(crate) struct Registrations {
    pub(crate) hooks: Vec<Hook>,
    pub(crate) post_types: Vec<(String, Value)>,
    pub(crate) taxonomies: Vec<Taxonomy>,
    pub(crate) meta: Vec<MetaKey>,
    pub(crate) routes: Vec<Route>,
    pub(crate) shortcodes: Vec<(String, u32)>,
    pub(crate) blocks: Vec<Block>,
    pub(crate) templates: Vec<TemplateFile>,
}

/// The guest's exports beside `init`, and a theme's `render`.
struct Exports {
    invoke: ComponentExportIndex,
    activate: ComponentExportIndex,
    deactivate: ComponentExportIndex,
    render: Option<ComponentExportIndex>,
}

/// A loaded extension.
pub struct Extension {
    pub(crate) slug: Arc<str>,
    pub(crate) grants: Arc<Grants>,
    pub(crate) limits: Limits,
    folder: PathBuf,
    pre: InstancePre<State>,
    exports: Exports,
    pub(crate) registered: Registrations,
    /// The fault record, under the extension's lock.
    record: Mutex<Record>,
    disabled: AtomicBool,
}

impl Host {
    /// Checks the grants, reads the fault record (a disabled extension, or
    /// one whose record does not read, is not loaded), compiles the
    /// component, links it against the SDK only (no WASI) and runs its
    /// `init` once, keeping what it registers.
    pub fn load(&self, package: Package<'_>) -> Result<Extension, Fault> {
        package.grants.check().map_err(Fault::Refused)?;
        let record = faults::read(package.folder).ok_or(Fault::Disabled)?;
        if record.disabled().is_some() {
            return Err(Fault::Disabled);
        }
        let (component, pre) = Component::new(&self.engine, package.wasm)
            .and_then(|component| {
                let pre = self.linker.instantiate_pre(&component);
                pre.map(|pre| (component, pre))
            })
            .map_err(refused)?;
        let types = component.component_type();
        let mut imports = types.imports(&self.engine).map(|(name, _)| name);
        if let Some(name) = imports.find(|name| !of_sdk(name, package.sdk)) {
            return Err(Fault::Refused(format!(
                "{name} is not of SDK {}, which its manifest names",
                package.sdk
            )));
        }
        let mut imports = types.imports(&self.engine).map(|(name, _)| name);
        let theme_only = |name: &&str| THEME_ONLY.iter().any(|only| of(name, only));
        if let Some(name) = imports
            .find(theme_only)
            .filter(|_| package.kind == Kind::Plugin)
        {
            return Err(Fault::Refused(format!(
                "{name} is a theme's, not a plugin's"
            )));
        }
        let theme_guest = format!("{SDK}theme-guest@{}.0", package.sdk);
        let render = match package.kind {
            Kind::Plugin => None,
            Kind::Theme => {
                let found = component.get_export_index(None, &theme_guest);
                let render = found.and_then(|at| component.get_export_index(Some(&at), "render"));
                Some(render.ok_or(Fault::Refused(format!("no {theme_guest}")))?)
            }
        };
        let guest = format!("{SDK}guest@{}.0", package.sdk);
        let found = component.get_export_index(None, &guest).and_then(|guest| {
            let export = |name| component.get_export_index(Some(&guest), name);
            let all = export("init").zip(export("invoke"));
            let all = all.zip(export("activate")).zip(export("deactivate"));
            all.map(|(((init, invoke), activate), deactivate)| {
                let exports = Exports {
                    invoke,
                    activate,
                    deactivate,
                    render: None,
                };
                (init, exports)
            })
        });
        let (init, mut exports) = found.ok_or(Fault::Refused(format!("no {guest}")))?;
        exports.render = render;
        let (grants, slug) = (Arc::new(package.grants), Arc::from(package.slug));
        let mut store = State::store(
            &self.engine,
            (&grants, &slug),
            Phase::Init,
            package.limits.memory(),
            package.limits.call(),
        );
        let answer = pre
            .instantiate(&mut store)
            .and_then(|instance| {
                instance
                    .get_typed_func::<Args<'_>, Answered>(&mut store, &exports.invoke)
                    .and_then(|_| {
                        instance.get_typed_func::<(), Done>(&mut store, &exports.activate)
                    })
                    .and_then(|_| {
                        instance.get_typed_func::<(), Done>(&mut store, &exports.deactivate)
                    })
                    .and_then(|_| match &exports.render {
                        Some(render) => {
                            let typed =
                                instance.get_typed_func::<(&str,), Rendered>(&mut store, render);
                            typed.map(drop)
                        }
                        None => Ok(()),
                    })
                    .and_then(|()| instance.get_typed_func::<(), Done>(&mut store, &init))
            })
            .and_then(|init| init.call(&mut store, ()));
        match answer {
            Ok((Ok(()),)) => {}
            Ok((Err(why),)) => return Err(Fault::Refused(why)),
            Err(e) => return Err(classify(&e, store.data()).1),
        }
        Ok(Extension {
            slug,
            grants,
            limits: package.limits,
            folder: package.folder.to_path_buf(),
            pre,
            exports,
            registered: store.into_data().registered,
            record: Mutex::new(record),
            disabled: AtomicBool::new(false),
        })
    }
}

/// An SDK import's interface and version.
fn sdk_import(name: &str) -> Option<(&str, &str)> {
    name.strip_prefix(SDK).and_then(|name| name.split_once('@'))
}

/// Whether an import is the SDK's interface `interface`, any version.
pub(crate) fn of(name: &str, interface: &str) -> bool {
    sdk_import(name).is_some_and(|(named, _)| named == interface)
}

/// Whether an import is one of SDK `sdk`'s interfaces, any patch version.
pub(crate) fn of_sdk(name: &str, sdk: &str) -> bool {
    sdk_import(name).is_some_and(|(_, version)| {
        version
            .strip_prefix(sdk)
            .is_some_and(|patch| patch.starts_with('.'))
    })
}

impl Extension {
    /// Its slug.
    pub fn slug(&self) -> &str {
        &self.slug
    }

    /// Whether it was disabled since it loaded.
    pub fn disabled(&self) -> bool {
        self.disabled.load(Ordering::Relaxed)
    }

    /// Its fault record as it stands.
    pub fn record(&self) -> Record {
        self.record
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// The hooks its `init` added.
    pub fn hooks(&self) -> &[Hook] {
        &self.registered.hooks
    }

    /// The post types it registered, with their arguments.
    pub fn post_types(&self) -> &[(String, Value)] {
        &self.registered.post_types
    }

    /// The taxonomies it registered.
    pub fn taxonomies(&self) -> &[Taxonomy] {
        &self.registered.taxonomies
    }

    /// The meta keys it registered.
    pub fn meta(&self) -> &[MetaKey] {
        &self.registered.meta
    }

    /// Its REST routes.
    pub fn routes(&self) -> &[Route] {
        &self.registered.routes
    }

    /// Its shortcodes and their callbacks.
    pub fn shortcodes(&self) -> &[(String, u32)] {
        &self.registered.shortcodes
    }

    /// Its blocks.
    pub fn blocks(&self) -> &[Block] {
        &self.registered.blocks
    }

    /// A theme's template files.
    pub(crate) fn templates(&self) -> &[TemplateFile] {
        &self.registered.templates
    }

    /// A fresh instance in `phase`, its first call bounded by `budget`.
    pub(crate) fn instantiate(
        &self,
        engine: &Engine,
        phase: Phase,
        budget: Duration,
    ) -> (wasmtime::Store<State>, wasmtime::Result<Instance>) {
        let mut store = State::store(
            engine,
            (&self.grants, &self.slug),
            phase,
            self.limits.memory(),
            budget,
        );
        let instance = self.pre.instantiate(&mut store);
        (store, instance)
    }

    /// A theme's `render`.
    pub(crate) fn render(&self) -> Option<&ComponentExportIndex> {
        self.exports.render.as_ref()
    }

    /// The index of `invoke`, or of `activate` or `deactivate`.
    pub(crate) fn export(&self, lifecycle: Option<bool>) -> &ComponentExportIndex {
        match lifecycle {
            None => &self.exports.invoke,
            Some(true) => &self.exports.activate,
            Some(false) => &self.exports.deactivate,
        }
    }

    /// Counts a fault; three within ten minutes disable it. The record is
    /// rewritten under the extension's lock; a failed write is reported.
    pub(crate) fn count(&self, fault: Counted) -> Option<Fault> {
        let mut record = self.record.lock().unwrap_or_else(PoisonError::into_inner);
        if record.count(fault) {
            self.disabled.store(true, Ordering::Relaxed);
        }
        let written = faults::write(&self.folder, &record);
        written.err().map(|e| Fault::Unrecorded(e.to_string()))
    }
}

/// The active extensions, in load order, and each hook's callbacks across
/// them by priority, then in the order added, as WordPress runs them.
pub struct Extensions {
    pub(crate) list: Vec<Arc<Extension>>,
    pub(crate) chains: BTreeMap<String, Vec<(usize, usize)>>,
}

impl Extensions {
    /// The active set, in the order the extensions load.
    pub fn new(list: Vec<Arc<Extension>>) -> Extensions {
        let mut chains: BTreeMap<String, Vec<(i32, usize, usize)>> = BTreeMap::new();
        for (ext, extension) in list.iter().enumerate() {
            for (n, hook) in extension.registered.hooks.iter().enumerate() {
                let chain = chains.entry(hook.hook.clone()).or_default();
                chain.push((hook.priority, ext, n));
            }
        }
        let chains = chains
            .into_iter()
            .map(|(hook, mut chain)| {
                chain.sort();
                (
                    hook,
                    chain.into_iter().map(|(_, ext, n)| (ext, n)).collect(),
                )
            })
            .collect();
        Extensions { list, chains }
    }

    /// The extensions, in load order.
    #[cfg(test)]
    pub(crate) fn list(&self) -> &[Arc<Extension>] {
        &self.list
    }

    /// The registration of a chain's entry.
    pub(crate) fn hook(&self, (ext, n): (usize, usize)) -> Option<&Hook> {
        let extension = self.list.get(ext);
        extension.and_then(|extension| extension.registered.hooks.get(n))
    }
}
