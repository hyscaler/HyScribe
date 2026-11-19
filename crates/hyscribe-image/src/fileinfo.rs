// SPDX-License-Identifier: AGPL-3.0-or-later

//! What PHP's `finfo_file( FILEINFO_MIME_TYPE )` (libmagic, `fileinfo`)
//! says of an upload, for the file types WordPress allows: a
//! bounded reader of file headers, never a C library or a child process.
//! It gives every type libmagic may answer for a file, or [`Unported`]
//! where it cannot tell; the caller decides only where all of them lead to
//! the same verdict. Matched to PHP by `reference/fileinfo/golden.sh`.

use crate::header::{getimagesize, image_mime};
use crate::meta::Unported;

mod media;
mod text;
mod zip;

/// How much of a file libmagic reads (`FILE_BYTES_MAX`).
pub(crate) const LIMIT: usize = 1 << 20;

/// The types libmagic may give `bytes`; [`Unported`] for a file not
/// recognised for certain.
pub fn finfo_candidates(bytes: &[u8]) -> Result<Vec<&'static str>, Unported> {
    match bytes.len() {
        0 => return Ok(vec!["application/x-empty"]),
        1 => return Ok(vec!["application/octet-stream"]),
        _ => {}
    }
    if let Some(mime) = image_mime(bytes) {
        // A header with no size may be what libmagic calls text.
        return getimagesize(bytes).map(|_| vec![mime]).ok_or(Unported);
    }
    if let Some(found) = signature(bytes) {
        return Ok(found);
    }
    if let Some(found) = media::media(bytes) {
        return Ok(found);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return zip::zip(bytes);
    }
    text::text(bytes).ok_or(Unported)
}

/// `%PDF-` within the first kilobyte, behind nothing but text.
fn pdf(bytes: &[u8]) -> bool {
    let head = bytes.get(..bytes.len().min(1024)).unwrap_or_default();
    let Some(at) = head.windows(5).position(|w| w == b"%PDF-") else {
        return false;
    };
    let before = head.get(..at).unwrap_or_default();
    before.is_empty()
        || before.starts_with(b"\xef\xbb\xbf")
        || before
            .iter()
            .all(|b| matches!(b, b'\t' | b'\n' | b'\r' | b' '..=b'~'))
}

/// A script that opens with PHP's tag, after a byte order mark at most.
fn php(b: &[u8]) -> bool {
    let b = b.strip_prefix(b"\xef\xbb\xbf").unwrap_or(b);
    b.get(..5)
        .is_some_and(|tag| tag.eq_ignore_ascii_case(b"<?php"))
}

/// An iCalendar file of version 2.0.
fn calendar(b: &[u8]) -> bool {
    let rest = b.strip_prefix(b"BEGIN:VCALENDAR");
    let rest = rest.and_then(|r| r.strip_prefix(b"\r").or(Some(r)));
    rest.and_then(|r| r.strip_prefix(b"\n"))
        .is_some_and(|r| r.starts_with(b"VERSION:2.0"))
}

/// A WebVTT file with a cue in its first kilobyte.
fn webvtt(b: &[u8]) -> bool {
    let head = b.get(..b.len().min(1024)).unwrap_or_default();
    let header = b.strip_prefix(b"WEBVTT");
    header.is_some_and(|r| r.first().is_none_or(|c| b" \t\r\n".contains(c)))
        && head.windows(3).any(|w| w == b"-->")
}

/// A `ustar` header whose checksum is right: the blocks `tar` writes.
fn tar(bytes: &[u8]) -> bool {
    let Some(block) = bytes.get(..512) else {
        return false;
    };
    if block.get(257..262) != Some(b"ustar") {
        return false;
    }
    let field = block.get(148..156).unwrap_or_default();
    let digits: Vec<u8> = field
        .iter()
        .copied()
        .skip_while(|b| *b == b' ')
        .take_while(|b| (b'0'..=b'7').contains(b))
        .collect();
    let stored = digits
        .iter()
        .fold(0u32, |sum, d| sum * 8 + u32::from(d - b'0'));
    let sum: u32 = block
        .iter()
        .enumerate()
        .map(|(at, b)| u32::from(if (148..156).contains(&at) { b' ' } else { *b }))
        .sum();
    !digits.is_empty() && stored == sum
}

/// The types of a file's first bytes alone: documents, archives and
/// executables.
fn signature(b: &[u8]) -> Option<Vec<&'static str>> {
    let starts = |magic: &[u8]| b.starts_with(magic);
    let one = |mime| Some(vec![mime]);
    if starts(b"%PDF-") || pdf(b) {
        return one("application/pdf");
    }
    match b {
        _ if starts(b"\x1f\x8b") && b.len() >= 10 => one("application/gzip"),
        _ if starts(b"7z\xbc\xaf\x27\x1c") && b.len() >= 8 => one("application/x-7z-compressed"),
        _ if starts(b"Rar!\x1a\x07\x00") || starts(b"Rar!\x1a\x07\x01\x00") => {
            one("application/x-rar")
        }
        _ if starts(b"\xca\xfe\xba\xbe\x00\x00\x00") && b.get(7).is_some_and(|v| *v > 0x1f) => {
            one("application/x-java-applet")
        }
        _ if starts(b"gimp xcf ") => one("image/x-xcf"),
        _ if tar(b) => one("application/x-tar"),
        _ if starts(b"{\\rtf") && b.get(5).is_some_and(u8::is_ascii_digit) => one("text/rtf"),
        _ if php(b) && text::is_text(b) => one("text/x-php"),
        _ if calendar(b) && text::is_text(b) => one("text/calendar"),
        _ if webvtt(b) && text::is_text(b) => one("text/vtt"),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
