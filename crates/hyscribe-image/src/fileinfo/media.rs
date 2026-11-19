// SPDX-License-Identifier: AGPL-3.0-or-later

//! The audio and video containers libmagic names by their first bytes.

/// An MPEG audio frame header libmagic takes: its second byte names the
/// version and layer, and for version 1 layer III (`FA` and `FB`) the
/// bitrate is neither free nor reserved.
fn mpeg_frame(b: &[u8]) -> bool {
    let (Some(&0xff), Some(second), Some(third)) = (b.first(), b.get(1), b.get(2)) else {
        return false;
    };
    match second {
        0xe2 | 0xe3 | 0xf2..=0xf7 | 0xfc | 0xfd => true,
        0xfa | 0xfb => (1..=14).contains(&(third >> 4)),
        _ => false,
    }
}

/// An ID3v2 tag, 2.2 to 2.4, with an MPEG audio frame right after it.
fn id3(b: &[u8]) -> bool {
    let Some(head) = b.get(..10).filter(|h| h.starts_with(b"ID3")) else {
        return false;
    };
    let (major, size) = (head.get(3), head.get(6..10).unwrap_or_default());
    if !matches!(major, Some(2..=4)) || size.iter().any(|s| s & 0x80 != 0) {
        return false;
    }
    let size = size.iter().fold(0usize, |n, s| n * 128 + usize::from(*s));
    // A tag with a footer is not one libmagic skips.
    let footer = head.get(5).is_some_and(|f| f & 0x10 != 0);
    !footer && b.get(10 + size..).is_some_and(mpeg_frame)
}

/// The brand of an ISO base media file: `ftyp`, or a `moov`, `wide` or
/// `mdat` atom first, which libmagic takes for QuickTime.
fn iso_media(b: &[u8]) -> Option<&'static str> {
    let size = u32::from_be_bytes(b.get(..4)?.try_into().ok()?);
    let atom: &[u8; 4] = b.get(4..8)?.try_into().ok()?;
    if size < 8 {
        return None;
    }
    if [b"moov", b"wide", b"mdat"].contains(&atom) {
        return Some("video/quicktime");
    }
    if atom != b"ftyp" {
        return None;
    }
    Some(match b.get(8..12)? {
        b"isom" | b"iso2" | b"mp41" | b"mp42" | b"avc1" | b"dash" | b"M4P " => "video/mp4",
        b"M4A " => "audio/x-m4a",
        b"M4B " | b"MSNV" => "audio/mp4",
        b"M4V " => "video/x-m4v",
        b"qt  " => "video/quicktime",
        b"3gp4" | b"3gp5" | b"3gp6" => "video/3gpp",
        b"3g2a" => "video/3gpp2",
        _ => return None,
    })
}

/// The first page of an Ogg stream names its codec.
fn ogg(b: &[u8]) -> Option<&'static str> {
    // The first page: version 0, the start of the stream, one segment.
    if b.get(4..6)? != b"\x00\x02" || *b.get(26)? != 1 {
        return None;
    }
    let payload = b.get(28..)?;
    let is = |magic: &[u8]| payload.starts_with(magic);
    if is(b"\x80theora") {
        return Some("video/ogg");
    }
    let audio = is(b"\x01vorbis") || is(b"OpusHead") || is(b"\x7fFLAC") || is(b"Speex   ");
    audio.then_some("audio/ogg")
}

/// The `DocType` an EBML header names.
fn ebml(b: &[u8]) -> Option<&'static str> {
    let head = b.get(..b.len().min(64))?;
    let has = |name: &[u8]| head.windows(name.len()).any(|w| w == name);
    if has(b"\x42\x82\x84webm") {
        Some("video/webm")
    } else if has(b"\x42\x82\x88matroska") {
        Some("video/x-matroska")
    } else {
        None
    }
}

/// The RIFF form type.
fn riff(b: &[u8]) -> Option<&'static str> {
    match b.get(8..12)? {
        b"WAVE" => Some("audio/x-wav"),
        b"AVI " => Some("video/x-msvideo"),
        b"RMID" => Some("audio/mid"),
        _ => None,
    }
}

/// The audio or video type libmagic gives `b`, if it is one it is certain
/// of.
pub(super) fn media(b: &[u8]) -> Option<Vec<&'static str>> {
    let starts = |magic: &[u8]| b.starts_with(magic);
    let mime = match b {
        _ if id3(b) || mpeg_frame(b) => "audio/mpeg",
        _ if starts(b"RIFF") => riff(b)?,
        _ if starts(b"OggS") => ogg(b)?,
        _ if starts(b"fLaC") => "audio/flac",
        _ if starts(b"MThd") => "audio/midi",
        _ if starts(b"FLV\x01") => "video/x-flv",
        _ if starts(b"\x1a\x45\xdf\xa3") => ebml(b)?,
        _ if starts(b"\x30\x26\xb2\x75\x8e\x66\xcf\x11\xa6\xd9\x00\xaa\x00\x62\xce\x6c") => {
            "video/x-ms-asf"
        }
        _ if starts(b"\xff\xf1") || starts(b"\xff\xf9") => "audio/x-hx-aac-adts",
        _ if starts(b"\x00\x00\x01\xba") || starts(b"\x00\x00\x01\xb3") => "video/mpeg",
        _ => iso_media(b)?,
    };
    Some(vec![mime])
}
