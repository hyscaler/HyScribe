// SPDX-License-Identifier: AGPL-3.0-or-later

//! An extension's version document, `v1/extensions/<id>.json` (ADR 0024):
//! its versions, oldest first. It is not signed itself; the signed index
//! gives its size and SHA-256, which it must match.

use hyscribe_ext::manifest;
use hyscribe_ext::origin::{hex64, reason_text};
use hyscribe_ext::signature::sha256;
use hyscribe_phpser::Value;

use crate::index::Entry;
use crate::json::{self, Object};

/// The largest version document, in bytes.
pub const MAX_BYTES: u64 = 1 << 20;

/// The largest package, the package format's limit, in bytes.
const MAX_PACKAGE: u64 = hyscribe_ext::hyx::MAX_TOTAL;

/// The longest `replaces`, `replaces_versions` or `page`, in bytes.
const MAX_TEXT: usize = 255;

/// One version of an extension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    /// `version`, as its manifest gives it.
    pub version: String,
    /// `sdk`, as its manifest gives it.
    pub sdk: String,
    /// `requires_hyscribe`: the oldest HyScribe release it runs on.
    pub requires_hyscribe: [u64; 3],
    /// `replaces`, as its manifest gives it.
    pub replaces: Option<String>,
    /// `replaces_versions`, as its manifest gives it.
    pub replaces_versions: Option<String>,
    /// `size` of its package, in bytes.
    pub size: u64,
    /// `sha256` of its package.
    pub sha256: String,
    /// `released`, in unix seconds.
    pub released: u64,
    /// `page`: its page on the registry's site, a path under the base URL.
    pub page: String,
    /// `revoked`: why, if it is.
    pub revoked: Option<String>,
}

/// A version document as read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Versions {
    /// `id`.
    pub id: String,
    /// `versions`, oldest first: their `released` times never go down.
    pub versions: Vec<Version>,
}

impl Versions {
    /// Reads a version document; any member missing, unknown or out of its
    /// bounds refuses it, as do a version listed twice and versions out of
    /// order.
    pub fn parse(bytes: &[u8]) -> Option<Versions> {
        let mut document = Object::decode(bytes)?;
        let id = document.text("id").filter(|id| manifest::slug(id))?;
        let versions = document.list("versions")?.into_iter().map(version);
        let versions = versions.collect::<Option<Vec<_>>>()?;
        document.done()?;
        let later = versions.iter().skip(1);
        let ordered = versions
            .iter()
            .zip(later)
            .all(|(a, b)| a.released <= b.released);
        let mut names: Vec<&str> = versions.iter().map(|v| v.version.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        (ordered && names.len() == versions.len()).then_some(Versions { id, versions })
    }

    /// The version document in `bytes`, if it is the one `entry` of the
    /// signed index names: its size, its SHA-256 and its id.
    pub fn verify(bytes: &[u8], entry: &Entry) -> Result<Versions, String> {
        let name = format!("extensions/{}.json", entry.id);
        if bytes.len() as u64 != entry.size || sha256(bytes) != entry.sha256 {
            return Err(format!("{name} is not the one the index names"));
        }
        let versions = Versions::parse(bytes).ok_or(format!("{name} does not read"))?;
        if versions.id != entry.id {
            return Err(format!("{name} is of {}", versions.id));
        }
        Ok(versions)
    }
}

/// A HyScribe release number, three decimal numbers: `0.1.0`.
pub fn release(text: &str) -> Option<[u64; 3]> {
    let numbers = text.split('.').map(|n| {
        let digits = n.bytes().all(|b| b.is_ascii_digit());
        n.parse().ok().filter(|_| digits)
    });
    numbers.collect::<Option<Vec<u64>>>()?.try_into().ok()
}

/// A path under the base URL: letters, digits and `-._~/`, relative, no
/// empty or `..` segment.
fn path(text: &str) -> bool {
    let chars = text
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~' | b'/'));
    let mut segments = text.strip_suffix('/').unwrap_or(text).split('/');
    let segments = segments.all(|s| !s.is_empty() && s != ".." && s != ".");
    chars && segments && text.len() <= MAX_TEXT
}

/// A string member that passes `check`, or `null` for none.
fn optional(entry: &mut Object, name: &str, check: fn(&str) -> bool) -> Option<Option<String>> {
    match entry.take(name)? {
        Value::Null => Some(None),
        value => json::text(value).filter(|text| check(text)).map(Some),
    }
}

/// Printable ASCII, spaces allowed, at most [`MAX_TEXT`] bytes.
fn shown(text: &str) -> bool {
    text.len() <= MAX_TEXT && reason_text(text)
}

/// `{"version", "sdk", "requires_hyscribe", "replaces", "replaces_versions",
/// "size", "sha256", "released", "page", "revoked"}`.
fn version(value: Value) -> Option<Version> {
    let mut entry = Object::of(value)?;
    let version = Version {
        version: entry.text("version").filter(|v| manifest::version(v))?,
        sdk: entry.text("sdk").filter(|sdk| manifest::version(sdk))?,
        requires_hyscribe: release(&entry.text("requires_hyscribe")?)?,
        replaces: optional(&mut entry, "replaces", shown)?,
        replaces_versions: optional(&mut entry, "replaces_versions", shown)?,
        size: entry
            .int("size")
            .filter(|size| (1..=MAX_PACKAGE).contains(size))?,
        sha256: entry.text("sha256").filter(|sha256| hex64(sha256))?,
        released: entry.int("released")?,
        page: entry.text("page").filter(|page| path(page))?,
        revoked: optional(&mut entry, "revoked", reason_text)?,
    };
    entry.done().map(|()| version)
}

#[cfg(test)]
mod tests;
