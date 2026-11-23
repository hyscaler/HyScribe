// SPDX-License-Identifier: AGPL-3.0-or-later

//! A WAVE file of uncompressed PCM: getID3's `riff` module for the files
//! whose chunks are a format, the samples and the chunks that add nothing
//! to the metadata.

use hyscribe_phpser::Value;

use super::{Entry, divide, file_entries, key, text};
use crate::fileinfo::{le16, le32};
use crate::meta::Unported;

/// The `fmt ` chunk's body and the length of the `data` chunk of a WAVE
/// file whose chunks all lie inside it and are ones that add nothing.
fn chunks(b: &[u8]) -> Option<(&[u8], usize)> {
    let size = le32(b, 4).map(|size| size.saturating_add(8));
    if b.get(8..12) != Some(b"WAVE") || size != Some(b.len()) {
        return None;
    }
    let (mut format, mut data) = (None, None);
    let mut at = 12;
    while at < b.len() {
        // What the file cuts off reads as nothing, and fails as a chunk.
        let id = b.get(at..at + 4).unwrap_or_default();
        let len = le32(b, at + 4).unwrap_or(0);
        let body = b.get(at + 8..at.saturating_add(8).saturating_add(len))?;
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
    let number = |value: Option<usize>| value.unwrap_or(0) as i64;
    let (tag, channels) = (number(le16(format, 0)), number(le16(format, 2)));
    let (rate, bytes_per_second) = (number(le32(format, 4)), number(le32(format, 8)));
    let bits = number(le16(format, 14));
    // PCM, one or two channels, samples, and nothing a file leaves out (an
    // empty `data` chunk makes getID3 take the rest of the file for it).
    if data == 0
        || tag != 1
        || !(1..=2).contains(&channels)
        || rate == 0
        || bytes_per_second == 0
        || bits == 0
    {
        return Err(Unported);
    }
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
