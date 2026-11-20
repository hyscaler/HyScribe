// SPDX-License-Identifier: AGPL-3.0-or-later

//! The fault record, `hyscribe/plugins/<slug>/faults.txt` (ADR 0022): the
//! last three counted faults and, when they came within ten minutes,
//! when the extension was disabled. Never in the database; a record that
//! does not read counts as disabled.

use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{ErrorKind, Read, Write as _};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

/// The record's file name in the extension's folder.
pub const FILE: &str = "faults.txt";

/// The file a new record is written to before it is renamed over the old.
const NEW: &str = ".faults.txt.new";

/// Faults within [`WINDOW`] seconds that disable an extension.
pub const DISABLING: usize = 3;

/// Ten minutes, in seconds.
pub const WINDOW: u64 = 600;

/// The most of a file read, in bytes: a record of 4 lines is far less.
const MAX_BYTES: u64 = 4 << 10;

/// A hook name's largest size, in bytes.
const MAX_HOOK: usize = 255;

/// The kind of a counted fault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The guest's own trap: `unreachable`, a panic, a bad memory access.
    Trap,
    /// A call ran past its deadline or the extension's request budget.
    Timeout,
    /// The instance asked for more memory than its limit.
    Memory,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Trap => "trap",
            Kind::Timeout => "timeout",
            Kind::Memory => "memory",
        }
    }
}

/// One counted fault: when, which kind, on which hook.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counted {
    /// Unix seconds.
    pub at: u64,
    /// Its kind.
    pub kind: Kind,
    /// The hook its callback ran on.
    pub hook: String,
}

/// The record: the last faults, oldest first, and when it was disabled.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    faults: Vec<Counted>,
    disabled: Option<u64>,
}

impl Record {
    /// Reads a record as [`Record::text`] writes it; `None` for anything
    /// else, which the host takes as disabled.
    pub fn parse(bytes: &[u8]) -> Option<Record> {
        let text = std::str::from_utf8(bytes).ok()?.strip_suffix('\n')?;
        let mut record = Record::default();
        for line in text.split('\n') {
            let mut fields = line.split(' ');
            match (fields.next(), record.disabled) {
                (Some("fault"), None) => {
                    let at = seconds(fields.next()?)?;
                    let kind = match fields.next()? {
                        "trap" => Kind::Trap,
                        "timeout" => Kind::Timeout,
                        "memory" => Kind::Memory,
                        _ => return None,
                    };
                    let hook = fields.next().filter(|hook| hook_name(hook))?.to_string();
                    let earlier = record.faults.last().map_or(0, |last| last.at);
                    (at >= earlier && record.faults.len() < DISABLING).then_some(())?;
                    record.faults.push(Counted { at, kind, hook });
                }
                (Some("disabled"), None) => record.disabled = Some(seconds(fields.next()?)?),
                _ => return None,
            }
            fields.next().is_none().then_some(())?;
        }
        let disabling = record.disabling();
        (!record.faults.is_empty() && disabling == record.disabled).then_some(record)
    }

    /// The record as its file holds it.
    pub fn text(&self) -> String {
        let mut text = String::new();
        for fault in &self.faults {
            let _ = writeln!(
                text,
                "fault {} {} {}",
                fault.at,
                fault.kind.name(),
                fault.hook
            );
        }
        if let Some(at) = self.disabled {
            let _ = writeln!(text, "disabled {at}");
        }
        text
    }

    /// Counts a fault, keeping the last three; true when it disables the
    /// extension. A clock that went back counts it at the last fault's time.
    pub fn count(&mut self, mut fault: Counted) -> bool {
        fault.at = self
            .faults
            .last()
            .map_or(fault.at, |last| last.at.max(fault.at));
        self.faults.push(fault);
        if self.faults.len() > DISABLING {
            self.faults.remove(0);
        }
        self.disabled = self.disabling();
        self.disabled.is_some()
    }

    /// When it was disabled, if it is.
    pub fn disabled(&self) -> Option<u64> {
        self.disabled
    }

    /// The faults it keeps, oldest first.
    pub fn faults(&self) -> &[Counted] {
        &self.faults
    }

    /// The time of the third fault, when it came within the window of the first.
    fn disabling(&self) -> Option<u64> {
        match self.faults.as_slice() {
            [first, _, third] if third.at - first.at <= WINDOW => Some(third.at),
            _ => None,
        }
    }
}

/// An integer of at most 20 digits.
fn seconds(field: &str) -> Option<u64> {
    let digits =
        !field.is_empty() && field.len() <= 20 && field.bytes().all(|b| b.is_ascii_digit());
    digits.then(|| field.parse().ok()).flatten()
}

/// Printable ASCII without a space, at most 255 bytes.
fn hook_name(hook: &str) -> bool {
    !hook.is_empty() && hook.len() <= MAX_HOOK && hook.bytes().all(|b| b.is_ascii_graphic())
}

/// The record in `folder`: empty when there is no file, `None` when there
/// is one that does not read (or cannot be read), which counts as disabled.
pub fn read(folder: &Path) -> Option<Record> {
    let mut bytes = Vec::new();
    match fs::File::open(folder.join(FILE)) {
        Ok(file) => file.take(MAX_BYTES).read_to_end(&mut bytes).ok()?,
        Err(e) if e.kind() == ErrorKind::NotFound => return Some(Record::default()),
        Err(_) => return None,
    };
    Record::parse(&bytes)
}

/// Writes `record` whole: a new file with mode `0600` in the same folder,
/// flushed and renamed over the old one, so a crash leaves one or the
/// other. The caller holds the extension's lock.
pub fn write(folder: &Path, record: &Record) -> std::io::Result<()> {
    let new = folder.join(NEW);
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true).mode(0o600);
    let written = options.open(&new).and_then(|mut file| {
        let text = file.write_all(record.text().as_bytes());
        text.and_then(|()| file.sync_all())
    });
    written.and_then(|()| fs::rename(&new, folder.join(FILE)))
}

/// Re-enabling from the Plugins screen: the record is deleted.
pub fn clear(folder: &Path) -> std::io::Result<()> {
    match fs::remove_file(folder.join(FILE)) {
        Err(e) if e.kind() != ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests;
