// SPDX-License-Identifier: AGPL-3.0-or-later

//! The manifest, `hyscribe.ext.toml` (ADR 0022): what the extension is,
//! what it stands in for, what it may touch and how long it may run.
//! Untrusted input: read strictly, every key known, every name checked.

use std::collections::BTreeSet;

use crate::{Grants, Limits};

mod toml;

use toml::Item;

/// The manifest's file name at the top of a package and of its folder.
pub const FILE: &str = "hyscribe.ext.toml";

/// The most a manifest may hold, in bytes.
pub(crate) const MAX_BYTES: u64 = 64 << 10;

/// The longest name a grant may list, as `faults.txt` records a hook.
const MAX_NAME: usize = 255;

/// The longest text of `[extension]` shown on the Plugins screen.
const MAX_TEXT: usize = 1_000;

/// The SDK versions the host links (ADR 0022).
const SDK: [&str; 1] = ["0.1"];

/// What a package is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A plugin, installed into `hyscribe/plugins/<id>/`.
    Plugin,
    /// A theme, installed into `hyscribe/themes/<id>/` (phase 10).
    Theme,
}

/// A manifest as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    /// `kind`.
    pub kind: Kind,
    /// `id`: the WordPress slug, its folder's name.
    pub id: String,
    /// `version`.
    pub version: String,
    /// `sdk`: the SDK version the component was built against.
    pub sdk: String,
    /// `replaces`: the plugin file `active_plugins` names, `<id>/<file>.php`.
    pub replaces: Option<String>,
    /// `replaces_versions`: the original's versions it stands in for.
    pub replaces_versions: Option<String>,
    /// `compatibility`: `full` or `partial`.
    pub compatibility: Option<String>,
    /// `license`, an SPDX expression.
    pub license: String,
    /// `name`, the plugin's `Plugin Name` header.
    pub name: String,
    /// `description`, its `Description` header.
    pub description: String,
    /// `author`, its `Author` header.
    pub author: String,
    /// `author_uri`, its `Author URI` header.
    pub author_uri: String,
    /// `plugin_uri`, its `Plugin URI` header.
    pub plugin_uri: String,
    /// What it asks for: `[capabilities]`, `[hooks]`, `[meta]`,
    /// `[taxonomies]` and `[rest]`.
    pub grants: Grants,
    /// `[limits]`.
    pub limits: Limits,
}

impl Manifest {
    /// Reads a manifest; anything the format does not allow refuses it,
    /// with the reason.
    pub fn parse(bytes: &[u8]) -> Result<Manifest, String> {
        if bytes.len() as u64 > MAX_BYTES {
            return Err(format!("{FILE} is larger than {MAX_BYTES} bytes"));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| format!("{FILE} is not UTF-8"))?;
        let mut tables = toml::parse(text)?;
        let mut taken = |name: &str| {
            let found = tables.iter().position(|(table, _)| table == name);
            let keys = found.map(|n| tables.remove(n).1).unwrap_or_default();
            Keys(keys, name.into(), None)
        };
        let mut extension = taken("extension");
        let mut capabilities = taken("capabilities");
        let mut hooks = taken("hooks");
        let mut meta = taken("meta");
        let mut taxonomies = taken("taxonomies");
        let mut rest = taken("rest");
        let limits = taken("limits");
        if let Some((table, _)) = tables.first() {
            return Err(format!("{FILE} has no table [{table}]"));
        }
        let given = extension.text("kind");
        let kind_error = extension.2.take();
        let kind = match given.as_deref() {
            Some("plugin") => Some(Kind::Plugin),
            Some("theme") => Some(Kind::Theme),
            _ => None,
        };
        let id = extension.text("id").filter(|id| slug(id));
        let manifest = Manifest {
            kind: kind.unwrap_or(Kind::Plugin),
            id: id.clone().unwrap_or_default(),
            version: extension.required("version", version),
            sdk: extension.required("sdk", |sdk| SDK.contains(&sdk)),
            replaces: extension.text("replaces"),
            replaces_versions: extension.optional("replaces_versions", expression),
            compatibility: extension.optional("compatibility", |c| matches!(c, "full" | "partial")),
            license: extension.required("license", expression),
            name: extension.required("name", shown),
            description: extension.optional("description", shown).unwrap_or_default(),
            author: extension.optional("author", shown).unwrap_or_default(),
            author_uri: extension.optional("author_uri", shown).unwrap_or_default(),
            plugin_uri: extension.optional("plugin_uri", shown).unwrap_or_default(),
            grants: Grants {
                options: capabilities.names("options"),
                post_types: capabilities.names("post_types"),
                mail: capabilities.flag("mail"),
                filesystem: capabilities.names("filesystem"),
                actions: hooks.names("actions"),
                filters: hooks.names("filters"),
                shortcodes: hooks.names("shortcodes"),
                sensitive: hooks.names("sensitive"),
                meta: meta.names("keys"),
                read_taxonomies: taxonomies.names("read"),
                taxonomies: taxonomies.names("write"),
                rest: rest.names("namespaces"),
            },
            limits: limits.limits()?,
        };
        if kind.is_none() {
            return Err(
                kind_error.unwrap_or_else(|| "[extension] kind must be plugin or theme".into())
            );
        }
        if id.is_none() {
            return Err(
                "[extension] id must be a slug: lowercase letters, digits, - and _, at most 100"
                    .into(),
            );
        }
        for keys in [extension, capabilities, hooks, meta, taxonomies, rest] {
            keys.done()?;
        }
        manifest.check()?;
        Ok(manifest)
    }

    /// The checks across keys: a plugin names the file it stands in for in
    /// its own folder, a port is not under GPL-2.0-only (ADR 0004), and the
    /// grants pass [`Grants::check`].
    fn check(&self) -> Result<(), String> {
        let file = self.replaces.as_deref().map(|replaces| {
            let name = replaces.strip_prefix(&self.id)?.strip_prefix('/')?;
            let stem = name.strip_suffix(".php")?;
            slug(stem).then_some(())
        });
        match (self.kind, file) {
            (Kind::Plugin, Some(Some(()))) | (Kind::Theme, None) => {}
            (Kind::Plugin, _) => {
                return Err(format!(
                    "[extension] replaces must be the plugin file {}/<file>.php",
                    self.id
                ));
            }
            (Kind::Theme, Some(_)) => return Err("a theme replaces no plugin file".into()),
        }
        if self.replaces.is_some() && matches!(self.license.as_str(), "GPL-2.0-only" | "GPL-2.0") {
            return Err("a port is never under GPL-2.0-only (ADR 0004)".into());
        }
        let folders = self.grants.filesystem.iter();
        if let Some(folder) = folders
            .clone()
            .find(|folder| !crate::grants::relative(folder))
        {
            return Err(format!(
                "[capabilities] filesystem {folder} is not a folder"
            ));
        }
        self.grants.check()
    }
}

/// A WordPress slug as HyScribe names a folder after it.
pub(crate) fn slug(text: &str) -> bool {
    (1..=100).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Printable ASCII without spaces, as a version or name of a grant.
fn visible(text: &str) -> bool {
    (1..=MAX_NAME).contains(&text.len()) && text.bytes().all(|b| b.is_ascii_graphic())
}

/// A version as a file name may hold it: ASCII letters, digits, `.`, `+`,
/// `_` and `-`, at most 64.
fn version(text: &str) -> bool {
    (1..=64).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'_' | b'-'))
}

/// Printable ASCII, spaces allowed, as an SPDX or version expression.
fn expression(text: &str) -> bool {
    (1..=MAX_NAME).contains(&text.len()) && text.bytes().all(|b| b == b' ' || b.is_ascii_graphic())
}

/// Text the Plugins screen shows: no control characters, not too long.
fn shown(text: &str) -> bool {
    text.len() <= MAX_TEXT && !text.chars().any(char::is_control)
}

/// The keys of one table, taken as they are read, its name, and the first
/// value that did not read; whatever is left over is unknown, and either
/// refuses the manifest.
struct Keys(Vec<(String, Item)>, String, Option<String>);

impl Keys {
    fn take(&mut self, key: &str) -> Option<Item> {
        let found = self.0.iter().position(|(named, _)| named == key)?;
        Some(self.0.remove(found).1)
    }

    /// Keeps the first refusal, `key` not being `what`.
    fn wrong(&mut self, key: &str, what: &str) {
        let why = format!("[{}] {key} must be {what}", self.1);
        self.2.get_or_insert(why);
    }

    fn text(&mut self, key: &str) -> Option<String> {
        match self.take(key) {
            Some(Item::Text(text)) => Some(text),
            None => None,
            Some(_) => {
                self.wrong(key, "a string");
                None
            }
        }
    }

    fn optional(&mut self, key: &str, valid: fn(&str) -> bool) -> Option<String> {
        let text = self.text(key);
        if text.as_deref().is_some_and(|text| !valid(text)) {
            self.wrong(key, "a valid value");
        }
        text
    }

    fn required(&mut self, key: &str, valid: fn(&str) -> bool) -> String {
        let text = self.optional(key, valid).unwrap_or_default();
        if text.is_empty() {
            self.wrong(key, "given");
        }
        text
    }

    fn flag(&mut self, key: &str) -> bool {
        match self.take(key) {
            None => false,
            Some(Item::Bool(flag)) => flag,
            Some(_) => {
                self.wrong(key, "true or false");
                false
            }
        }
    }

    fn names(&mut self, key: &str) -> BTreeSet<String> {
        match self.take(key) {
            None => BTreeSet::new(),
            Some(Item::List(names)) if names.iter().all(|name| visible(name)) => {
                names.into_iter().collect()
            }
            Some(_) => {
                self.wrong(key, "a list of names without spaces");
                BTreeSet::new()
            }
        }
    }

    fn limits(self) -> Result<Limits, String> {
        let mut asked = Vec::new();
        for (key, item) in &self.0 {
            let Item::Int(value) = item else {
                return Err(format!("[limits] {key} must be an integer"));
            };
            asked.push((key.as_str(), *value));
        }
        Limits::asked(&asked)
    }

    fn done(self) -> Result<(), String> {
        match (self.2, self.0.first()) {
            (Some(why), _) => Err(why),
            (None, Some((key, _))) => Err(format!("[{}] has no key {key}", self.1)),
            (None, None) => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
