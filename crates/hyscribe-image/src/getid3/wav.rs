// SPDX-License-Identifier: AGPL-3.0-or-later

//! A WAVE file of uncompressed PCM: getID3's `riff` module for the files
//! whose chunks are a format, the samples and the chunks that add nothing
//! to the metadata.

use hyscribe_phpser::Value;

use super::{Entry, divide, file_entries, key, text};
use crate::meta::Unported;

fn le16(b: &[u8], at: usize) -> Option<i64> {
    let bytes = b.get(at..at.checked_add(2)?)?;
    Some(i64::from(u16::from_le_bytes(bytes.try_into().ok()?)))
}

fn le32(b: &[u8], at: usize) -> Option<usize> {
    let bytes = b.get(at..at.checked_add(4)?)?;
    usize::try_from(u32::from_le_bytes(bytes.try_into().ok()?)).ok()
}

/// The `fmt ` chunk's body and the length of the `data` chunk of a WAVE
/// file whose chunks all lie inside it and are ones that add nothing.
fn chunks(b: &[u8]) -> Option<(&[u8], usize)> {
    if b.get(8..12)? != b"WAVE" || le32(b, 4)?.checked_add(8)? != b.len() {
        return None;
    }
    let (mut format, mut data) = (None, None);
    let mut at = 12;
    while at < b.len() {
        let id = b.get(at..at + 4)?;
        let len = le32(b, at + 4)?;
        let body = b.get(at + 8..at.checked_add(8)?.checked_add(len)?)?;
        match (id, &format, &data) {
            (b"fmt ", None, _) => format = Some(body),
            (b"data", _, None) => data = Some(len),
            (b"LIST" | b"fact" | b"JUNK" | b"PAD ", _, _) => {}
            _ => return None,
        }
        at += 8 + len + (len & 1);
    }
    (at == b.len()).then_some((format?, data?))
}

/// The metadata of a WAVE file as `wp_read_audio_metadata()` keeps it.
pub(super) fn wav(b: &[u8]) -> Result<Vec<Entry>, Unported> {
    let (format, data) = chunks(b).ok_or(Unported)?;
    let field = |at, wide: bool| match wide {
        true => le32(format, at).and_then(|n| i64::try_from(n).ok()),
        false => le16(format, at),
    };
    let (tag, channels) = (field(0, false), field(2, false));
    let (rate, bytes_per_second) = (field(4, true), field(8, true));
    let bits = field(14, false);
    // PCM, one or two channels, and nothing a file leaves out.
    let (Some(1), Some(channels @ (1 | 2)), Some(rate @ 1..), Some(bytes_per_second @ 1..)) =
        (tag, channels, rate, bytes_per_second)
    else {
        return Err(Unported);
    };
    let Some(bits @ 1..) = bits else {
        return Err(Unported);
    };
    let bitrate = bytes_per_second * 8;
    let mode = if channels == 1 { "mono" } else { "stereo" };
    let mut entries: Vec<Entry> = vec![
        (key("dataformat"), text("wav")),
        (key("bitrate_mode"), text("cbr")),
        (key("wformattag"), Value::Int(1)),
        (key("codec"), text("Pulse Code Modulation (PCM)")),
        (key("channels"), Value::Int(channels)),
        (key("sample_rate"), Value::Int(rate)),
        (key("bitrate"), Value::Int(bitrate)),
        (key("bits_per_sample"), Value::Int(bits)),
        (key("lossless"), Value::Bool(true)),
        (key("channelmode"), text(mode)),
        (
            key("compression_ratio"),
            divide(bitrate, channels * rate * bits),
        ),
        (key("fileformat"), text("wav")),
    ];
    let seconds = (data * 8) as f64 / bitrate as f64;
    file_entries(&mut entries, b.len(), "audio/wav", seconds);
    Ok(entries)
}
