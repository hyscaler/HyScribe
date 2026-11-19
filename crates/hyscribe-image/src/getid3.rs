// SPDX-License-Identifier: AGPL-3.0-or-later

//! What getID3 makes of an audio or video upload, as
//! `wp_read_audio_metadata()` and `wp_read_video_metadata()` keep it
//!: HyScribe's own bounded readers of the container headers, each
//! for the files it reads as getID3 does and [`Unported`] for every other
//! shape. Matched to PHP by `reference/getid3/golden.sh`.

use hyscribe_phpser::{Float, Key, Value};

use crate::meta::Unported;

mod wav;

/// An entry of the metadata array.
type Entry = (Key, Value);

fn key(name: &str) -> Key {
    Key::String(name.into())
}

fn text(value: &str) -> Value {
    Value::String(value.as_bytes().to_vec())
}

/// PHP's `a / b` of two integers: an integer where it divides, else a
/// float.
fn divide(a: i64, b: i64) -> Value {
    match (a.checked_rem(b), a.checked_div(b)) {
        (Some(0), Some(whole)) => Value::Int(whole),
        _ => Value::Float(Float::from_f64(a as f64 / b as f64)),
    }
}

/// `getid3_lib::PlaytimeString()`.
fn playtime_string(seconds: f64) -> String {
    let seconds = seconds.abs().round();
    let hours = (seconds / 3600.0).floor();
    let minutes = ((seconds - 3600.0 * hours) / 60.0).floor();
    let rest = (seconds - 3600.0 * hours - 60.0 * minutes).round();
    let (hours, minutes, rest) = (hours as i64, minutes as i64, rest as i64);
    match hours {
        0 => format!("{minutes}:{rest:02}"),
        _ => format!("{hours}:{minutes:02}:{rest:02}"),
    }
}

/// The entries `wp_read_audio_metadata()` adds to what getID3 gives the
/// audio: the file's size, getID3's type for it, and its length
/// (`playtime_seconds`, as getID3 has it).
fn file_entries(entries: &mut Vec<Entry>, size: usize, mime: &str, seconds: f64) {
    entries.push((key("filesize"), Value::Int(size as i64)));
    entries.push((key("mime_type"), text(mime)));
    if seconds != 0.0 {
        entries.push((key("length"), Value::Int(seconds.round() as i64)));
        entries.push((key("length_formatted"), text(&playtime_string(seconds))));
    }
}

/// The size below which getID3 stops on a file too small to hold the tags
/// it looks for (it does at 160 bytes, and not at 170), leaving the size
/// alone: not read here.
const SMALL: usize = 512;

/// Whether the file ends in a tag getID3 reads (ID3v1, APE, Lyrics3),
/// which would add titles and the like to the metadata, or is too small.
fn tagged(bytes: &[u8]) -> bool {
    if bytes.len() < SMALL {
        return true;
    }
    let tail = bytes
        .get(bytes.len().saturating_sub(512)..)
        .unwrap_or_default();
    [&b"TAG"[..], b"APETAGEX", b"LYRICS"]
        .iter()
        .any(|tag| tail.windows(tag.len()).any(|w| w == *tag))
}

/// The type getID3 gives a file of a format whose module WordPress left
/// out of its copy (MIDI, AAC in ADTS or ADIF, MPEG video): it names the
/// type and stops, so the metadata is the size and the type alone.
fn without_module(bytes: &[u8]) -> Option<&'static str> {
    if tagged(bytes) {
        return None;
    }
    match bytes {
        [b'M', b'T', b'h', b'd', ..] => Some("audio/midi"),
        [b'A', b'D', b'I', b'F', ..] | [0xff, 0xf0 | 0xf1 | 0xf8 | 0xf9, ..] => Some("audio/aac"),
        [0, 0, 1, 0xb3 | 0xba, ..] => Some("video/mpeg"),
        _ => None,
    }
}

/// The metadata of a file whose format getID3's copy has no module for.
fn named_only(bytes: &[u8], mime: &str) -> Vec<Entry> {
    vec![
        (key("filesize"), Value::Int(bytes.len() as i64)),
        (key("mime_type"), text(mime)),
    ]
}

/// `wp_read_audio_metadata( $file )` of an audio file's `bytes`, the
/// array it stores; [`Unported`] for a file getID3 reads in ways not
/// ported here.
pub fn wp_read_audio_metadata(bytes: &[u8]) -> Result<Vec<Entry>, Unported> {
    if bytes.starts_with(b"RIFF") && !tagged(bytes) {
        return wav::wav(bytes);
    }
    match without_module(bytes) {
        Some(mime) if mime.starts_with("audio/") => Ok(named_only(bytes, mime)),
        _ => Err(Unported),
    }
}

/// `wp_read_video_metadata( $file )` of a video file's `bytes`, the array
/// it stores; [`Unported`] for a file getID3 reads in ways not ported here.
pub fn wp_read_video_metadata(bytes: &[u8]) -> Result<Vec<Entry>, Unported> {
    match without_module(bytes) {
        Some(mime) if mime.starts_with("video/") => Ok(named_only(bytes, mime)),
        _ => Err(Unported),
    }
}

#[cfg(test)]
mod tests;
