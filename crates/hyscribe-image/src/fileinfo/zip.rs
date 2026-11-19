// SPDX-License-Identifier: AGPL-3.0-or-later

//! ZIP files: OpenDocument and EPUB by their stored `mimetype` entry,
//! Office Open XML by the folders of its parts, any other archive as ZIP.

use crate::meta::Unported;

/// The OpenDocument types WordPress allows, by their `mimetype` entry.
const ODF: [&str; 7] = [
    "application/vnd.oasis.opendocument.text",
    "application/vnd.oasis.opendocument.presentation",
    "application/vnd.oasis.opendocument.spreadsheet",
    "application/vnd.oasis.opendocument.graphics",
    "application/vnd.oasis.opendocument.chart",
    "application/vnd.oasis.opendocument.database",
    "application/vnd.oasis.opendocument.formula",
];

/// Office Open XML's folder and the type of its plain document.
const OOXML: [(&str, &str); 3] = [
    (
        "word/",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    (
        "xl/",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    (
        "ppt/",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
];

/// Names that make libmagic give an archive a type of its own.
const SPECIAL: [&str; 5] = [
    "META-INF/",
    "mimetype",
    "AndroidManifest.xml",
    "classes",
    "_rels/",
];

fn le16(b: &[u8], at: usize) -> Option<usize> {
    let bytes = b.get(at..at + 2)?;
    Some(usize::from(u16::from_le_bytes(bytes.try_into().ok()?)))
}

fn le32(b: &[u8], at: usize) -> Option<usize> {
    let bytes = b.get(at..at + 4)?;
    usize::try_from(u32::from_le_bytes(bytes.try_into().ok()?)).ok()
}

/// The entry names of the central directory, `None` for an archive that
/// is cut off or not consistent.
fn names(b: &[u8]) -> Option<Vec<&[u8]>> {
    let from = b.len().saturating_sub(22 + 0xffff);
    let tail = b.get(from..)?;
    let end = from + tail.windows(4).rposition(|w| w == b"PK\x05\x06")?;
    let count = le16(b, end + 10)?;
    let mut at = le32(b, end + 16)?;
    let mut names = Vec::new();
    for _ in 0..count {
        if b.get(at..at + 4)? != b"PK\x01\x02" {
            return None;
        }
        let (name, extra, comment) = (le16(b, at + 28)?, le16(b, at + 30)?, le16(b, at + 32)?);
        names.push(b.get(at + 46..at + 46 + name)?);
        at += 46 + name + extra + comment;
    }
    Some(names)
}

/// The types libmagic may give an archive that starts as a ZIP does.
pub(super) fn zip(b: &[u8]) -> Result<Vec<&'static str>, Unported> {
    let first = le16(b, 26)
        .and_then(|len| b.get(30..30 + len))
        .ok_or(Unported)?;
    if first == b"mimetype" {
        // Stored, with no extra field: its text follows the name.
        let (method, extra, size) = (le16(b, 8), le16(b, 28), le32(b, 22));
        let (Some(0), Some(0), Some(size)) = (method, extra, size) else {
            return Err(Unported);
        };
        let text = b.get(38..38 + size).ok_or(Unported)?;
        if text == b"application/epub+zip" {
            return Ok(vec!["application/epub+zip"]);
        }
        return ODF
            .iter()
            .find(|mime| mime.as_bytes() == text)
            .map(|mime| vec![*mime])
            .ok_or(Unported);
    }
    let names = names(b).filter(|names| !names.is_empty()).ok_or(Unported)?;
    let has = |prefix: &str| names.iter().any(|n| n.starts_with(prefix.as_bytes()));
    if has("[Content_Types].xml") {
        // Libmagic names a document by its parts, or leaves it a ZIP.
        let found: Vec<&str> = OOXML
            .iter()
            .filter(|(folder, _)| has(folder))
            .map(|(_, mime)| *mime)
            .collect();
        return match found.as_slice() {
            [mime] if !has("word/vbaProject") && !has("xl/vbaProject") => {
                Ok(vec![mime, "application/zip"])
            }
            _ => Err(Unported),
        };
    }
    if SPECIAL.iter().any(|special| has(special)) {
        return Err(Unported);
    }
    // An extra field of the first entry that starts with 0xCAFE marks a JAR.
    let cafe =
        le16(b, 28).and_then(|extra| b.get(30 + first.len()..30 + first.len() + extra.min(2)));
    Ok(vec![match cafe {
        Some(b"\xfe\xca") => "application/java-archive",
        _ => "application/zip",
    }])
}
