// SPDX-License-Identifier: AGPL-3.0-or-later

//! Compound File Binary files (Word, Excel and PowerPoint 97 to 2003, and
//! their kin), typed as libmagic's `file_trycdf()` types them: by the
//! application name in the summary information, else by the names of the
//! directory's entries. Only well formed files with 512 byte sectors are
//! read, and a stream is read only when libmagic reads it; any other shape
//! is [`Unported`], never guessed.

use super::{le16, le32};
use crate::meta::Unported;

/// The first eight bytes of a Compound File Binary file.
pub(super) const MAGIC: [u8; 8] = [0xd0, 0xcf, 0x11, 0xe0, 0xa1, 0xb1, 0x1a, 0xe1];

/// The sector size, and the size of a sector of the short stream.
const SECTOR: usize = 512;
const SHORT: usize = 64;
/// A stream shorter than this lives in the short stream.
const CUTOFF: usize = 4096;
/// The end of a chain of sectors.
const END: usize = 0xffff_fffe;
/// The ids from here on are negative: free, or a table's own sector.
const NEGATIVE: usize = 0x8000_0000;
/// libmagic's limit on a chain's length, which bounds every stream read.
const CHAIN_MAX: usize = 10_000;
/// A directory entry's size, and its kinds.
const ENTRY: usize = 128;
const STORAGE: u8 = 1;
const STREAM: u8 = 2;
const ROOT: u8 = 5;
/// The class of the root entry of a Windows Installer package.
const MSI: [u8; 16] = [
    0x84, 0x10, 0x0c, 0, 0, 0, 0, 0, 0xc0, 0, 0, 0, 0, 0, 0, 0x46,
];

/// What an application name of the summary information, in lower case,
/// makes the file.
const APPS: [(&str, &str); 9] = [
    ("word", "application/msword"),
    ("excel", "application/vnd.ms-excel"),
    ("powerpoint", "application/vnd.ms-powerpoint"),
    ("crystal reports", "application/x-rpt"),
    ("advanced installer", "application/vnd.ms-msi"),
    ("installshield", "application/vnd.ms-msi"),
    ("microsoft patch compiler", "application/vnd.ms-msi"),
    ("nant", "application/vnd.ms-msi"),
    ("windows installer", "application/vnd.ms-msi"),
];

/// What a word inside an entry's name makes the file, in lower case.
const NAMES: [(&str, &str); 5] = [
    ("book", "application/vnd.ms-excel"),
    ("workbook", "application/vnd.ms-excel"),
    ("worddocument", "application/msword"),
    ("powerpoint", "application/vnd.ms-powerpoint"),
    ("digitalsignature", "application/vnd.ms-msi"),
];

/// The streams and storages that type a file with no summary information,
/// in libmagic's order: any one entry (name and kind) of a group gives it.
const SECTIONS: [(&[(&str, u8)], &str); 6] = [
    (
        &[("EncryptedPackage", STREAM), ("EncryptedSummary", STREAM)],
        "application/encrypted",
    ),
    (&[("mfbu_header", STREAM)], "application/quickbooks"),
    (
        &[("Book", STREAM), ("Workbook", STREAM)],
        "application/vnd.ms-excel",
    ),
    (&[("WordDocument", STREAM)], "application/msword"),
    (&[("PowerPoint", STREAM)], "application/vnd.ms-powerpoint"),
    (
        &[
            ("__properties_version1.0", STREAM),
            ("__recip_version1.0_#00000000", STORAGE),
        ],
        "application/vnd.ms-outlook",
    ),
];

/// The type libmagic gives a Compound File Binary file, or [`Unported`].
pub(super) fn cfb(b: &[u8]) -> Result<Vec<&'static str>, Unported> {
    typed(b).map(|mime| vec![mime]).ok_or(Unported)
}

/// The little endian words of `bytes`.
fn words(bytes: &[u8]) -> Vec<usize> {
    let (words, _) = bytes.as_chunks::<4>();
    words
        .iter()
        .map(|w| u32::from_le_bytes(*w) as usize)
        .collect()
}

/// The sectors of the chain from `first` in `table` up to its end mark:
/// `None` for one that leaves the table or outruns libmagic's limit.
fn chain(table: &[usize], first: usize) -> Option<Vec<usize>> {
    let (mut ids, mut id) = (Vec::new(), first);
    while id != END {
        if ids.len() >= CHAIN_MAX {
            return None;
        }
        ids.push(id);
        id = *table.get(id)?;
    }
    Some(ids)
}

/// A directory entry's kind, first sector and size.
fn kind(entry: &[u8]) -> u8 {
    entry.get(66).copied().unwrap_or_default()
}

fn start(entry: &[u8]) -> usize {
    le32(entry, 116).unwrap_or_default()
}

fn size(entry: &[u8]) -> usize {
    le32(entry, 120).unwrap_or_default()
}

/// Whether the entry's name, 31 characters at most, is `name`.
fn named(entry: &[u8], name: &str) -> bool {
    name.bytes()
        .chain([0])
        .enumerate()
        .all(|(at, c)| le16(entry, at * 2) == Some(usize::from(c)))
}

/// The low bytes of an entry's name up to the first NUL, as libmagic reads
/// its 32 characters; `None` for a name with no end.
fn low_bytes(entry: &[u8]) -> Option<Vec<u8>> {
    let mut chars: Vec<u8> = entry.iter().take(64).step_by(2).copied().collect();
    let end = chars.iter().position(|c| *c == 0)?;
    chars.truncate(end);
    Some(chars)
}

/// Whether `hay` holds `needle`, a lower case word, in any case.
fn holds(hay: &[u8], needle: &str) -> bool {
    hay.windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// The type an application name makes the file, from the printable
/// characters of a string property up to its first NUL.
fn app_type(data: &[u8], wide: bool) -> Option<&'static str> {
    if data.len() <= 1 {
        return None;
    }
    let name: Vec<u8> = data
        .iter()
        .step_by(if wide { 2 } else { 1 })
        .copied()
        .take_while(|c| *c != 0)
        .filter(|c| (0x20..0x7f).contains(c))
        .take(1023)
        .collect();
    APPS.iter()
        .find(|(word, _)| holds(&name, word))
        .map(|(_, mime)| *mime)
}

/// The type the application name in summary information `s` gives, after
/// `known`: the outer `None` where libmagic fails or may read otherwise.
fn properties(s: &[u8], known: Option<&'static str>) -> Option<Option<&'static str>> {
    let offs = le32(s, 0x2c)?;
    let section = s.get(offs..offs.saturating_add(8))?;
    let end = offs.saturating_add(le32(section, 0).unwrap_or_default());
    let first = offs.saturating_add(8);
    if end > s.len() || first >= end {
        return None;
    }
    let (mut found, mut scanned) = (known, false);
    for i in 0..le32(section, 4).unwrap_or_default() {
        let table = first.saturating_add(i.saturating_mul(8));
        let at = le32(s, table.saturating_add(4)).unwrap_or_default();
        let id = le32(s, table).unwrap_or_default();
        let q = first.saturating_add(at.checked_sub(8)?);
        if end.saturating_sub(q) < 4 {
            return None;
        }
        let (left, ty) = (end - q, le32(s, q).unwrap_or_default());
        // The room the value needs after its type.
        let need = match ty {
            1 | 0x47 => 4,
            2 => 6,
            3 | 4 | 0x13 | 0x1e | 0x1f => 8,
            5 | 0x40 => 12,
            _ => return None,
        };
        if left < need {
            return None;
        }
        if matches!(ty, 0x1e | 0x1f) {
            let len = le32(s, q + 4).unwrap_or_default();
            if left - 8 < len {
                return None;
            }
            if id == 0x12 && found.is_none() {
                // Names may share their bytes: one scan keeps the time linear.
                if scanned {
                    return None;
                }
                scanned = true;
                let data = s.get(q + 8..q + 8 + len).unwrap_or_default();
                found = app_type(data, ty == 0x1f);
            }
        }
    }
    Some(found)
}

/// A file's tables and directory.
struct Cfb<'a> {
    b: &'a [u8],
    fat: Vec<usize>,
    ssat: Vec<usize>,
    dir: Vec<&'a [u8]>,
    mini: Option<Vec<u8>>,
}

impl<'a> Cfb<'a> {
    /// The sector with id `id`, whole.
    fn sector(&self, id: usize) -> Option<&'a [u8]> {
        let at = id.saturating_add(1).saturating_mul(SECTOR);
        self.b.get(at..at.saturating_add(SECTOR))
    }

    /// The bytes of sectors `ids`.
    fn read(&self, ids: &[usize]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        for id in ids {
            out.extend_from_slice(self.sector(*id)?);
        }
        Some(out)
    }

    /// The root entry, the first of its kind.
    fn root(&self) -> Option<&'a [u8]> {
        self.dir.iter().find(|entry| kind(entry) == ROOT).copied()
    }

    /// The tables and directory of a file of 512 byte sectors, a cut-off of
    /// 4096 and no table sectors beyond the header's 109.
    fn open(b: &'a [u8]) -> Option<Self> {
        let head = b.get(..SECTOR).unwrap_or_default();
        let word = |at| le32(head, at).unwrap_or_default();
        let half = |at| le16(head, at).unwrap_or_default();
        if half(0x1c) != 0xfffe || half(0x1e) != 9 || half(0x20) != 6 {
            return None;
        }
        if word(0x38) != CUTOFF || word(0x48) != 0 {
            return None;
        }
        let mut cfb = Cfb {
            b,
            fat: Vec::new(),
            ssat: Vec::new(),
            dir: Vec::new(),
            mini: None,
        };
        for id in words(head.get(0x4c..).unwrap_or_default()) {
            if id >= NEGATIVE {
                break;
            }
            cfb.fat.extend(words(cfb.sector(id)?));
        }
        let ids = chain(&cfb.fat, word(0x3c))?;
        cfb.ssat = words(&cfb.read(&ids)?);
        for id in chain(&cfb.fat, word(0x30))? {
            cfb.dir.extend(
                cfb.sector(id)?
                    .as_chunks::<ENTRY>()
                    .0
                    .iter()
                    .map(|e| &e[..]),
            );
        }
        if let Some(root) = cfb.root().filter(|root| start(root) < NEGATIVE) {
            if size(root) == 0 {
                return None;
            }
            let ids = chain(&cfb.fat, start(root))?;
            cfb.mini = Some(cfb.read(&ids)?);
        }
        Some(cfb)
    }

    /// The last entry of kind `kind_of` named `name`.
    fn find(&self, name: &str, kind_of: u8) -> Option<&'a [u8]> {
        let wanted = |entry: &&&[u8]| kind(entry) == kind_of && named(entry, name);
        self.dir.iter().rev().find(wanted).copied()
    }

    /// The bytes of the sectors of a stream, short or long by its size as
    /// libmagic chooses; `None` where libmagic's read differs.
    fn stream(&self, entry: &[u8]) -> Option<Vec<u8>> {
        if size(entry) >= CUTOFF {
            return self.read(&chain(&self.fat, start(entry))?);
        }
        let mini = self.mini.as_ref()?;
        let mut out = Vec::new();
        for id in chain(&self.ssat, start(entry))? {
            let at = id.saturating_mul(SHORT);
            out.extend_from_slice(mini.get(at..at.saturating_add(SHORT))?);
        }
        Some(out)
    }

    /// The type of the first entry whose name holds a word that makes
    /// one, else a generic Office file.
    fn by_names(&self) -> Option<&'static str> {
        for entry in &self.dir {
            let name = low_bytes(entry)?;
            let hit = NAMES.iter().find(|(word, _)| holds(&name, word));
            if let Some((_, mime)) = hit {
                return Some(mime);
            }
        }
        Some("application/vnd.ms-office")
    }

    /// The type the streams a file has give it, else the generic storage.
    fn by_sections(&self) -> &'static str {
        let has = |(name, kind): &(&str, u8)| self.find(name, *kind).is_some();
        let section = SECTIONS.iter().find(|(names, _)| names.iter().any(has));
        section.map_or("application/x-ole-storage", |(_, mime)| mime)
    }
}

/// Whether libmagic's tar test, which runs first, may find the checksum
/// field (read as `from_oct()` reads it) equal to the header's sum.
fn tar(head: &[u8]) -> bool {
    let field = head.get(148..156).unwrap_or_default();
    let stored = field
        .iter()
        .skip_while(|c| matches!(c, b'\t'..=b'\r' | b' '))
        .take_while(|c| (b'0'..=b'7').contains(c))
        .fold(0, |sum, c| sum * 8 + usize::from(c - b'0'));
    let blank = |(at, c): (usize, &u8)| {
        if (148..156).contains(&at) {
            32
        } else {
            usize::from(*c)
        }
    };
    stored == head.iter().enumerate().map(blank).sum::<usize>()
}

/// Whether libmagic's SIMH tape test, which runs first too, may find the
/// record the first word opens closed by the same length.
fn tape(b: &[u8]) -> bool {
    let record = le32(&MAGIC, 0).unwrap_or_default() & 0xff_ffff;
    le32(b, 4 + record).is_some_and(|n| {
        let n = n & 0xff_ffff;
        n + (n & 1) == record
    })
}

/// What libmagic types the file `b` that starts as a Compound File Binary
/// does, `None` where this does not read it as libmagic does.
fn typed(b: &[u8]) -> Option<&'static str> {
    if tar(b.get(..SECTOR)?) || tape(b) {
        return None;
    }
    let cfb = Cfb::open(b)?;
    // A Hangul Word Processor file marks itself in its `FileHeader`.
    if let Some(entry) = cfb.find("FileHeader", STREAM)
        && cfb.stream(entry)?.starts_with(b"HWP Document File")
    {
        return Some("application/x-hwp");
    }
    let summary = cfb
        .find("\u{5}SummaryInformation", STREAM)
        .or_else(|| cfb.find("\u{5}DocumentSummaryInformation", STREAM));
    let Some(entry) = summary else {
        return Some(cfb.by_sections());
    };
    let installer = cfb
        .root()
        .filter(|root| root.get(80..96) == Some(&MSI[..]))
        .map(|_| "application/x-msi");
    match properties(&cfb.stream(entry)?, installer)? {
        Some(mime) => Some(mime),
        None => cfb.by_names(),
    }
}
