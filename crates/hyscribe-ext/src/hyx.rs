// SPDX-License-Identifier: AGPL-3.0-or-later

//! The package, a `.hyx` file (ADR 0022): a zip archive read strictly from
//! its central directory, each entry checked again against its local
//! header and its CRC as it is inflated. Untrusted input: every size and
//! count is bounded before anything is allocated for it.

use std::collections::BTreeSet;

use crate::manifest::{self, Kind, Manifest};

/// The component's file name at the top of a package and of its folder.
pub const COMPONENT: &str = "extension.wasm";

/// The most entries a package may hold.
const MAX_ENTRIES: usize = 10_000;

/// The most a package may hold uncompressed, and the largest package read.
pub const MAX_TOTAL: u64 = 256 << 20;

/// The most one entry may hold uncompressed.
pub(crate) const MAX_ENTRY: u64 = 64 << 20;

/// How many times its compressed size an entry may grow to.
const MAX_RATIO: u64 = 100;

/// The longest name of an entry, in bytes.
const MAX_NAME: usize = 1_024;

/// What may sit at the top of a package: files, then folders.
const TOP_FILES: [&str; 3] = [manifest::FILE, COMPONENT, "LICENSE"];
const TOP_FOLDERS: [&str; 4] = ["public", "languages", "blocks", "templates"];

const LOCAL: u32 = 0x0403_4b50;
const CENTRAL: u32 = 0x0201_4b50;
const END: u32 = 0x0605_4b50;

/// A package's manifest and its files, by their names in the package.
#[derive(Debug)]
pub struct Hyx {
    /// Its manifest, read.
    pub manifest: Manifest,
    /// Every file it holds, the manifest and the component among them, in
    /// the central directory's order; folders are implied by the names.
    pub files: Vec<(String, Vec<u8>)>,
}

impl Hyx {
    /// Reads a package; anything ADR 0022's format does not allow refuses
    /// it, with the reason.
    pub fn read(bytes: &[u8]) -> Result<Hyx, String> {
        if u64::try_from(bytes.len()).map_or(true, |len| len > MAX_TOTAL) {
            return Err(format!("a package is at most {MAX_TOTAL} bytes"));
        }
        let end = bytes.len().checked_sub(22).ok_or("not a zip archive")?;
        let tail = At(bytes, end);
        let (disk, cd_disk) = (tail.u16(4), tail.u16(6));
        let (here, entries) = (tail.usize16(8), tail.usize16(10));
        let (cd_size, cd_start) = (tail.usize32(12), tail.usize32(16));
        if tail.u32(0) != Some(END) || tail.u16(20) != Some(0) {
            return Err("not a zip archive, or one with a comment".into());
        }
        if disk != Some(0) || cd_disk != Some(0) || here != entries {
            return Err("a zip archive over several disks".into());
        }
        if entries > MAX_ENTRIES {
            return Err(format!("a package holds at most {MAX_ENTRIES} entries"));
        }
        if cd_start.checked_add(cd_size) != Some(end) {
            return Err("the central directory is not where the archive says".into());
        }
        let mut seen = BTreeSet::new();
        let mut listed = Vec::new();
        let mut total = 0;
        let mut at = cd_start;
        for _ in 0..entries {
            let entry = Entry::central(bytes, at, end)?;
            at = entry.next;
            let folded = entry.name.trim_end_matches('/').to_lowercase();
            if !seen.insert(folded) {
                return Err(format!(
                    "{} is named twice, or twice but for case",
                    entry.name
                ));
            }
            total += entry.size;
            if total > MAX_TOTAL {
                return Err(format!("a package holds at most {MAX_TOTAL} bytes"));
            }
            listed.push(entry);
        }
        if at != end {
            return Err("the central directory holds more than its entries".into());
        }
        let mut files = Vec::new();
        for entry in listed
            .into_iter()
            .filter(|entry| !entry.name.ends_with('/'))
        {
            let data = entry.data(bytes)?;
            files.push((entry.name, data));
        }
        let named: BTreeSet<&str> = files.iter().map(|(name, _)| name.as_str()).collect();
        let nested = files.iter().find_map(|(name, _)| {
            let mut parents = name.match_indices('/').filter_map(|(n, _)| name.get(..n));
            parents.find(|parent| named.contains(parent))
        });
        if let Some(parent) = nested {
            return Err(format!("{parent} is a file and a folder"));
        }
        let text = find(&files, manifest::FILE).ok_or("a package has a hyscribe.ext.toml")?;
        let manifest = Manifest::parse(text)?;
        let hyx = Hyx { manifest, files };
        if hyx.manifest.kind == Kind::Plugin && hyx.file(COMPONENT).is_none() {
            return Err(format!("a plugin package has an {COMPONENT}"));
        }
        Ok(hyx)
    }

    /// The contents of the file named `name`.
    pub fn file(&self, name: &str) -> Option<&[u8]> {
        find(&self.files, name)
    }
}

/// Little endian fields of the archive at an offset.
fn find<'a>(files: &'a [(String, Vec<u8>)], name: &str) -> Option<&'a [u8]> {
    let (_, data) = files.iter().find(|(file, _)| file == name)?;
    Some(data)
}

struct At<'a>(&'a [u8], usize);

impl At<'_> {
    fn bytes<const N: usize>(&self, at: usize) -> Option<[u8; N]> {
        let from = self.1.saturating_add(at);
        self.0.get(from..from.saturating_add(N))?.try_into().ok()
    }

    fn u16(&self, at: usize) -> Option<u16> {
        self.bytes(at).map(u16::from_le_bytes)
    }

    fn u32(&self, at: usize) -> Option<u32> {
        self.bytes(at).map(u32::from_le_bytes)
    }

    fn usize16(&self, at: usize) -> usize {
        self.u16(at).map_or(usize::MAX, usize::from)
    }

    fn usize32(&self, at: usize) -> usize {
        let field = self.u32(at).map(usize::try_from);
        field.and_then(Result::ok).unwrap_or(usize::MAX)
    }
}

/// One entry of the central directory, checked.
struct Entry {
    name: String,
    flags: u16,
    deflated: bool,
    crc: u32,
    stored: usize,
    size: u64,
    local: usize,
    next: usize,
}

impl Entry {
    /// The entry at `at`, which must end by `end`.
    fn central(bytes: &[u8], at: usize, end: usize) -> Result<Entry, String> {
        let header = At(bytes, at);
        if header.u32(0) != Some(CENTRAL) {
            return Err("a central directory entry is broken".into());
        }
        let lengths = [28, 30, 32].map(|field| header.usize16(field));
        let next = lengths
            .iter()
            .try_fold(at + 46, |sum, &n| sum.checked_add(n));
        let next = next
            .filter(|&next| next <= end)
            .ok_or("an entry runs past the directory")?;
        let raw = bytes.get(at + 46..at + 46 + lengths[0]).unwrap_or_default();
        let name = std::str::from_utf8(raw).map_err(|_| "an entry's name is not UTF-8")?;
        let flags = header.u16(8).unwrap_or_default();
        let method = header.u16(10).unwrap_or_default();
        let made_on_unix = header.u16(4).unwrap_or_default() >> 8 == 3;
        let mode = header.u32(38).unwrap_or_default() >> 16;
        let size = u64::from(header.u32(24).unwrap_or_default());
        let entry = Entry {
            name: name.to_string(),
            flags,
            deflated: method == 8,
            crc: header.u32(16).unwrap_or_default(),
            stored: header.usize32(20),
            size,
            local: header.usize32(42),
            next,
        };
        let stored = u64::from(header.u32(20).unwrap_or_default());
        if flags & 0x41 != 0 {
            return Err(format!("{name} is encrypted"));
        }
        if !matches!(method, 0 | 8) {
            return Err(format!("{name} is neither stored nor deflated"));
        }
        if made_on_unix && !matches!(mode & 0o170_000, 0 | 0o040_000 | 0o100_000) {
            return Err(format!("{name} is a link or a special file"));
        }
        if header.u16(34) != Some(0) {
            return Err(format!("{name} is on another disk"));
        }
        if !Entry::named(name) {
            return Err(format!("{name} is not a name a package may hold"));
        }
        if size > MAX_ENTRY {
            return Err(format!("{name} is larger than {MAX_ENTRY} bytes"));
        }
        let grows = if method == 0 {
            size != stored
        } else {
            size > stored * MAX_RATIO
        };
        if grows || (name.ends_with('/') && size != 0) {
            return Err(format!("{name} does not have the size it says"));
        }
        Ok(entry)
    }

    /// A relative path under one of the top level names a package allows.
    fn named(name: &str) -> bool {
        let (path, folder) = match name.strip_suffix('/') {
            Some(path) => (path, true),
            None => (name, false),
        };
        let top = path.split('/').next().unwrap_or_default();
        let nested = path.contains('/') || folder;
        name.len() <= MAX_NAME
            && crate::grants::relative(path)
            && !path.chars().any(char::is_control)
            && if nested {
                TOP_FOLDERS.contains(&top)
            } else {
                TOP_FILES.contains(&top)
            }
    }

    /// The entry's bytes, from its local header, inflated and checked.
    fn data(&self, bytes: &[u8]) -> Result<Vec<u8>, String> {
        let name = &self.name;
        let header = At(bytes, self.local);
        let lengths = [26, 28].map(|field| header.usize16(field));
        let start = lengths
            .iter()
            .try_fold(self.local + 30, |sum, &n| sum.checked_add(n));
        let raw = start.and_then(|_| bytes.get(self.local + 30..self.local + 30 + lengths[0]));
        let matches = header.u32(0) == Some(LOCAL)
            && header.u16(6) == Some(self.flags)
            && header.u16(8) == Some(u16::from(self.deflated) * 8)
            && raw == Some(name.as_bytes());
        let data = start
            .and_then(|start| bytes.get(start..start.saturating_add(self.stored)))
            .filter(|_| matches)
            .ok_or_else(|| format!("{name} does not match its local header"))?;
        let limit = usize::try_from(self.size).unwrap_or(usize::MAX);
        let data = if self.deflated {
            miniz_oxide::inflate::decompress_to_vec_with_limit(data, limit)
                .map_err(|_| format!("{name} does not inflate to the size it says"))?
        } else {
            data.to_vec()
        };
        if data.len() != limit || crc32fast::hash(&data) != self.crc {
            return Err(format!("{name} does not have the size or CRC it says"));
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests;
