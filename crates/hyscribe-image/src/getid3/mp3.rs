// SPDX-License-Identifier: AGPL-3.0-or-later
// Ported from getID3() by James Heinrich (https://www.getid3.org), which offers its
// code under the GNU GPL version 3 among other licences; taken under that one.

//! An MP3 file as getID3's `audio.mp3` module reads it: the frame header,
//! the Xing, Info and VBRI headers, the LAME tag and the length, for the
//! files whose frames follow each other from the end of the ID3v2 tag. A
//! file getID3 would rescan, warn about or read as variable bitrate without
//! a header is [`Unported`].

use hyscribe_phpser::{Float, Value};

use super::encoder::{Lame, Stream, encoder_options};
use super::id3::{self, Tag, V2};
use super::{Entry, key, text};
use crate::meta::Unported;

/// A PHP number: an integer, or a float once a division leaves a remainder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Num {
    Int(i64),
    Float(f64),
}

impl Num {
    /// The value as a float.
    pub(super) fn float(self) -> f64 {
        match self {
            Self::Int(n) => n as f64,
            Self::Float(n) => n,
        }
    }

    /// PHP's `$a * $b`.
    pub(super) fn mul(self, other: Self) -> Self {
        match (self, other) {
            // The counts multiplied stay far below 2^63: a 32 bit byte
            // count by a sample rate and a channel count.
            (Self::Int(a), Self::Int(b)) => Self::Int(a * b),
            _ => Self::Float(self.float() * other.float()),
        }
    }

    /// PHP's `$a + $b` of counts far below 2^63.
    pub(super) fn add(self, other: Self) -> Self {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Self::Int(a + b),
            _ => Self::Float(self.float() + other.float()),
        }
    }

    /// PHP's `$a - $b` of counts far below 2^63.
    pub(super) fn sub(self, other: Self) -> Self {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Self::Int(a - b),
            _ => Self::Float(self.float() - other.float()),
        }
    }

    /// PHP's `$a / $b`: an integer where both are and it divides.
    pub(super) fn div(self, other: Self) -> Self {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) if a.checked_rem(b) == Some(0) => Self::Int(a / b),
            _ => Self::Float(self.float() / other.float()),
        }
    }

    /// The PHP value.
    pub(super) fn value(self) -> Value {
        match self {
            Self::Int(n) => Value::Int(n),
            Self::Float(n) => Value::Float(Float::from_f64(n)),
        }
    }
}

/// The audio array of getID3's `$info`: keys in the order they were first
/// set.
#[derive(Default)]
pub(super) struct Audio(pub(super) Vec<Entry>);

impl Audio {
    pub(super) fn set(&mut self, name: &str, value: Value) {
        match self.0.iter_mut().find(|(have, _)| *have == key(name)) {
            Some(entry) => entry.1 = value,
            None => self.0.push((key(name), value)),
        }
    }
}

/// An MPEG version of a frame header.
#[derive(Clone, Copy, PartialEq)]
enum Version {
    One,
    Two,
    TwoFive,
}

/// A valid frame header (`MPEGaudioHeaderValid()`).
#[derive(Clone, Copy)]
struct Header {
    version: Version,
    layer: u8,
    /// `None` for free format.
    bitrate: Option<i64>,
    rate: i64,
    /// 0 stereo, 1 joint stereo, 2 dual channel, 3 mono.
    mode: u8,
    padding: bool,
}

/// The bitrates of version 1 layers 1 to 3, and of the other versions
/// layers 1 and 2 (and 3), in kbit/s, by index 1 to 14.
const RATES: [[[i64; 14]; 3]; 2] = [
    [
        [
            32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448,
        ],
        [
            32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384,
        ],
        [
            32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320,
        ],
    ],
    [
        [
            32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256,
        ],
        [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
        [8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
    ],
];

impl Header {
    /// `MPEGaudioHeaderDecode()` and `MPEGaudioHeaderValid()` of the four
    /// bytes at `at`.
    fn at(b: &[u8], at: usize) -> Option<Self> {
        let byte = |i: usize| b.get(at + i).copied().unwrap_or_default();
        let head = [byte(0), byte(1), byte(2), byte(3)];
        if head[0] != 0xff || head[1] & 0xe0 != 0xe0 {
            return None;
        }
        let version = match (head[1] >> 3) & 3 {
            0 => Version::TwoFive,
            2 => Version::Two,
            3 => Version::One,
            _ => return None,
        };
        let layer = match (head[1] >> 1) & 3 {
            0 => return None,
            raw => 4 - raw,
        };
        let index = usize::from(head[2] >> 4);
        let bitrate = match index {
            15 => return None,
            0 => None,
            index => {
                let table = RATES.get(usize::from(version != Version::One));
                let rates = table.and_then(|table| table.get(usize::from(layer) - 1));
                Some(
                    rates
                        .and_then(|rates| rates.get(index - 1))
                        .copied()
                        .unwrap_or_default()
                        * 1000,
                )
            }
        };
        let rates = match version {
            Version::One => [44100, 48000, 32000],
            Version::Two => [22050, 24000, 16000],
            Version::TwoFive => [11025, 12000, 8000],
        };
        let rate = *rates.get(usize::from((head[2] >> 2) & 3))?;
        Some(Self {
            version,
            layer,
            bitrate,
            rate,
            mode: head[3] >> 6,
            padding: head[2] & 2 != 0,
        })
    }

    /// `MPEGaudioFrameLength()`: `None` for free format.
    fn length(&self) -> Option<usize> {
        let bitrate = self.bitrate?;
        let (coefficient, slot) = match (self.version, self.layer) {
            (Version::One, 1) => (48, 4),
            (Version::One, _) | (_, 2) => (144, 1),
            (_, 1) => (24, 4),
            _ => (72, 1),
        };
        let length =
            (coefficient * bitrate / self.rate) / slot * slot + if self.padding { slot } else { 0 };
        usize::try_from(length).ok()
    }

    /// Whether getID3 takes the header (`decodeMPEGaudioHeader()` stops on
    /// layer 2 combinations of bitrate and mode that do not exist).
    fn allowed(&self) -> bool {
        match (self.layer, self.bitrate) {
            (2, Some(bitrate)) if self.mode == 3 => bitrate <= 192_000,
            (2, Some(bitrate)) => bitrate == 64_000 || bitrate >= 96_000,
            _ => true,
        }
    }

    /// The channel count.
    fn channels(&self) -> i64 {
        if self.mode == 3 { 1 } else { 2 }
    }

    /// The name of the channel mode.
    fn mode_name(&self) -> &'static str {
        match self.mode {
            0 => "stereo",
            1 => "joint stereo",
            2 => "dual channel",
            _ => "mono",
        }
    }

    /// `XingVBRidOffset()`.
    fn id_offset(&self) -> usize {
        match (self.version, self.mode == 3) {
            (Version::One, false) => 0x24,
            (Version::Two, true) => 0x0d,
            _ => 0x15,
        }
    }
}

/// The big endian number of up to four bytes.
fn be(bytes: &[u8]) -> i64 {
    bytes.iter().fold(0, |n, &b| n * 256 + i64::from(b))
}

/// `ClosestStandardMP3Bitrate()` of a frame header's bitrate: those past
/// 320 kbit/s, of six digits, rounded to two; the two the standard list
/// lacks lie at its midpoints, which go up; the rest are in it.
fn closest_standard(bitrate: i64) -> Num {
    match bitrate {
        320_001.. => Num::Float((bitrate as f64 / 10_000.0).round() * 10_000.0),
        144_000 => Num::Int(160_000),
        176_000 => Num::Int(192_000),
        _ => Num::Int(bitrate),
    }
}

/// What a Xing, Info or VBRI header says.
#[derive(Default)]
struct VariableHeader {
    /// `VBR_method` is `Fraunhofer` (VBRI) rather than `Xing`.
    fraunhofer: bool,
    frames: Option<i64>,
    bytes: Option<i64>,
    quality: i64,
    lame: Option<Lame>,
}

/// The first frame's reading: `$info['mpeg']['audio']` as far as the
/// output needs it.
struct First {
    header: Header,
    /// `bitrate_mode`.
    mode: String,
    /// `framelength`, as the Xing header may change it.
    length: usize,
    /// `$info['audio']['bitrate']`.
    bitrate: Num,
    variable: Option<VariableHeader>,
    /// The bytes the audio is said to take, 0 for none.
    expected: i64,
}

/// The first frame at `at` (`decodeMPEGaudioHeader()` without its
/// scan); `unused` is what the file leaves for the audio where a Xing
/// header gives no byte count.
fn first_frame(b: &[u8], at: usize, unused: i64) -> Result<First, Unported> {
    let header = Header::at(b, at).filter(Header::allowed).ok_or(Unported)?;
    // Free format is not read.
    let (Some(length), Some(bitrate)) = (header.length(), header.bitrate) else {
        return Err(Unported);
    };
    // The caller keeps 226 bytes from `at`, which hold every field read.
    let window = b.get(at..at + 226).unwrap_or_default();
    let field = |from: usize, len: usize| window.get(from..from + len).map_or(0, be);
    let id = header.id_offset();
    let marker = window.get(id..id + 4).unwrap_or_default();
    let mut first = First {
        header,
        mode: "cbr".into(),
        length,
        bitrate: Num::Int(bitrate),
        variable: None,
        expected: 0,
    };
    if window.get(36..40) == Some(b"VBRI") {
        let bytes = field(46, 4);
        first.mode = "vbr".into();
        first.expected = bytes;
        first.variable = Some(VariableHeader {
            fraunhofer: true,
            quality: field(44, 2),
            bytes: Some(bytes),
            frames: Some(field(50, 4)),
            lame: None,
        });
    } else if marker == b"Xing" || marker == b"Info" {
        first.mode = "vbr".into();
        let flags = field(id + 4, 4);
        let frames = (flags & 1 != 0).then(|| field(id + 8, 4));
        let bytes = (flags & 2 != 0).then(|| field(id + 12, 4));
        let mut variable = VariableHeader {
            frames,
            bytes,
            ..VariableHeader::default()
        };
        if let Some(frames) = frames.filter(|&frames| frames != 0) {
            let used = bytes.filter(|&bytes| bytes != 0).unwrap_or(unused);
            first.bitrate = xing_bitrate(&header, used, frames);
            first.length = Num::Int(used).div(Num::Int(frames)).float().floor() as usize;
        }
        if window.get(id + 120..id + 124) == Some(b"LAME") {
            let lame = Lame::parse(window, id);
            if let Some(full) = &lame.full {
                first.mode = full.method_mode().into();
                first.expected = if full.audio_bytes > 0 {
                    full.audio_bytes
                } else {
                    bytes.unwrap_or(0)
                };
                if full.method == 1 {
                    first.mode = "cbr".into();
                    first.bitrate = closest_standard(bitrate);
                }
            }
            variable.lame = Some(lame);
        }
        first.variable = Some(variable);
    }
    Ok(first)
}

/// The bitrate a Xing header's counts give (`$info['audio']['bitrate']` of
/// the header branch).
fn xing_bitrate(header: &Header, used: i64, frames: i64) -> Num {
    let length = Num::Int(used).div(Num::Int(frames));
    let channels = Num::Int(2).div(Num::Int(header.channels()));
    let rate = Num::Int(header.rate);
    if header.layer == 1 {
        length
            .div(Num::Int(4))
            .mul(rate)
            .mul(channels)
            .div(Num::Int(12))
    } else {
        length.mul(rate).mul(channels).div(Num::Int(144))
    }
}

/// `RecursiveFrameScanning()` of the frames after the first, which ends
/// at `end`: the bitrates it met, or [`Unported`] where getID3 would warn
/// or read the stream another way.
fn scan(
    b: &[u8],
    first: &First,
    at: usize,
    end: usize,
    as_cbr: bool,
) -> Result<Vec<i64>, Unported> {
    let step = first.header.length().unwrap_or_default();
    let mut next = at + step;
    let mut seen = Vec::new();
    for _ in 0..50 {
        if next + 4 >= end {
            return Ok(seen);
        }
        let Some(header) = Header::at(b, next).filter(Header::allowed) else {
            if next + first.length > end {
                return Ok(seen);
            }
            return Err(Unported);
        };
        let length = header.length().ok_or(Unported)?;
        let id = header.id_offset();
        let marker = b.get(next + id..next + id + 4);
        let header_frame = marker == Some(b"Xing") || marker == Some(b"Info");
        if header_frame || b.get(next + 36..next + 40) == Some(b"VBRI") {
            return Err(Unported);
        }
        if as_cbr && header.bitrate != first.header.bitrate {
            return Err(Unported);
        }
        seen.push(header.bitrate.unwrap_or(0));
        next += length;
    }
    Ok(seen)
}

/// Whether a variable bitrate header's `bitrate` lies within one percent
/// of the frames' one bitrate `only`.
fn within_a_percent(bitrate: f64, only: i64) -> bool {
    (bitrate - only as f64).abs() / (only as f64) < 0.01
}

/// An MP3 file's reading.
pub(super) struct Mp3 {
    /// `$data['audio']`.
    pub audio: Vec<Entry>,
    /// `fileformat`.
    pub format: &'static str,
    /// `playtime_seconds`.
    pub seconds: Num,
    /// The ID3v2 tag.
    pub v2: Option<V2>,
    /// The ID3v1 tag.
    pub v1: Option<Tag>,
}

/// Whether the file ends in a tag whose module this does not port: APE or
/// Lyrics3.
fn other_tag(b: &[u8]) -> bool {
    let tail = b.get(b.len().saturating_sub(512)..).unwrap_or_default();
    [&b"APETAGEX"[..], b"LYRICS"]
        .iter()
        .any(|tag| tail.windows(tag.len()).any(|w| w == *tag))
}

/// The layer's name as getID3 gives the data format.
fn data_format(layer: u8) -> &'static str {
    match layer {
        1 => "mp1",
        2 => "mp2",
        _ => "mp3",
    }
}

/// The encoder name as getID3 completes it from the file's end where it
/// starts with `LAME3.` (its pattern for a version's last character never
/// matches, so every such version is looked for).
fn longer_version(b: &[u8], end: usize, encoder: &mut Vec<u8>) -> Result<(), Unported> {
    if !encoder.starts_with(b"LAME3.") {
        return Ok(());
    }
    let from = end.checked_sub(1441).ok_or(Unported)?;
    let data = b.get(from..end).unwrap_or_default();
    let mut current = encoder.clone();
    if matches!(current.last(), Some(b'a' | b'b')) {
        current.pop();
    }
    let Some(at) = data
        .windows(current.len())
        .position(|w| w == current.as_slice())
    else {
        return Ok(());
    };
    const MASK: &[u8] = b"LAME0123456789., (abcdefghijklmnopqrstuvwxyzJFSOND)";
    let found = data.get(at..).unwrap_or_default();
    let span = found.iter().take_while(|byte| MASK.contains(byte)).count();
    if span > encoder.len() {
        *encoder = found.get(..span).unwrap_or_default().to_vec();
    }
    Ok(())
}

/// `rtrim( $text, $blanks )`.
fn rtrim(text: &[u8], blanks: &[u8]) -> Vec<u8> {
    let end = text
        .iter()
        .rposition(|b| !blanks.contains(b))
        .map_or(0, |at| at + 1);
    text.get(..end).unwrap_or_default().to_vec()
}

/// An MP3 file.
pub(super) fn analyze(b: &[u8]) -> Result<Mp3, Unported> {
    let (Some(tail), false) = (b.last_chunk::<256>(), b.len() < 512 || other_tag(b)) else {
        return Err(Unported);
    };
    let v2 = id3::id3v2(b)?;
    let v1 = id3::id3v1(tail)?;
    let start = v2.as_ref().map_or(0, |v2| v2.length);
    let mut end = b.len();
    if let Some((_, duplicate)) = &v1 {
        end -= 128 * (1 + usize::from(*duplicate));
    }
    // The first frame's header and the bytes after it lie in the audio.
    if start + 226 > end {
        return Err(Unported);
    }
    let unused = (b.len() - start - 128 * usize::from(v1.is_some())) as i64;
    let first = first_frame(b, start, unused)?;
    let header = first.header;
    // The audio bytes a header states, which may leave a byte over.
    let body = (end - start) as i64;
    // A header without the count states 0, never one byte short.
    if body - first.expected == 1 {
        end -= 1;
    }
    let seen = scan(b, &first, start, end, first.variable.is_none())?;

    let mut mode = first.mode.clone();
    let mut bitrate = first.bitrate;
    let mut vbr_bitrate = None;
    if let Some(variable) = &first.variable {
        let frames = variable.frames.unwrap_or(0);
        if frames != 0 && (mode == "vbr" || mode == "abr") {
            let per_frame = match (header.version, header.layer) {
                (Version::One, 1) => 384,
                (Version::Two | Version::TwoFive, 3) => 576,
                _ => 1152,
            };
            let rate = Num::Int(header.rate).div(Num::Int(per_frame));
            let computed = variable.bytes.map_or(Num::Int(0), |bytes| {
                Num::Int(bytes)
                    .div(Num::Int(frames))
                    .mul(Num::Int(8))
                    .mul(rate)
            });
            vbr_bitrate = Some(computed).filter(|computed| computed.float() != 0.0);
            if computed.float() > 0.0 {
                bitrate = computed;
            }
        }
    }
    if let (Some(vbr), Some(&only)) = (vbr_bitrate, seen.first()) {
        let single = seen.iter().all(|&rate| rate == only);
        if mode == "vbr" && single && within_a_percent(vbr.float(), only) {
            mode = "cbr".into();
        }
    }
    let mut audio = Audio::default();
    audio.set("dataformat", text(""));
    audio.set("channels", Value::Int(header.channels()));
    audio.set("sample_rate", Value::Int(header.rate));
    audio.set("bitrate", bitrate.value());
    let variable = first.variable.as_ref();
    if variable.is_some_and(|variable| variable.fraunhofer) {
        audio.set("codec", text("Fraunhofer"));
    }
    audio.set("channelmode", text(header.mode_name()));
    audio.set("bitrate_mode", text(&mode));
    let lame = variable.and_then(|variable| variable.lame.as_ref());
    let mut encoder = Vec::new();
    if let Some(lame) = lame {
        audio.set("codec", text("LAME"));
        encoder = rtrim(&lame.version, b"\0");
    }
    if !encoder.is_empty() {
        longer_version(b, end, &mut encoder)?;
        audio.set("encoder", Value::String(rtrim(&encoder, b"\0 ")));
    }
    audio.set("dataformat", text(data_format(header.layer)));
    audio.set("lossless", Value::Bool(false));
    let options = encoder_options(&Stream {
        fraunhofer: variable
            .filter(|variable| variable.fraunhofer)
            .map(|variable| variable.quality),
        lame,
        mode: &mode,
        bitrate,
        sample_rate: header.rate,
    });
    // getID3's CleanUp() drops empty options.
    if !options.is_empty() {
        audio.set("encoder_options", text(&options));
    }

    // The bitrate is never 0 here: a header's, or the positive counts of a
    // Xing or VBRI header's.
    let header_frame = variable.is_some_and(|variable| variable.frames.is_some());
    let frame = if header_frame { first.length } else { 0 };
    let audio_bytes = (end - start) as i64 - frame as i64;
    // A file of its header frame alone, which PHP gives a length that
    // could not be matched.
    if audio_bytes == 0 {
        return Err(Unported);
    }
    let bytes = Num::Int(audio_bytes).mul(Num::Int(8));
    let seconds = bytes.div(bitrate);
    let divisor = Num::Int(header.channels())
        .mul(Num::Int(header.rate))
        .mul(Num::Int(16));
    audio.set("compression_ratio", bitrate.div(divisor).value());
    Ok(Mp3 {
        audio: audio.0,
        format: data_format(header.layer),
        seconds,
        v2,
        v1: v1.map(|(tag, _)| tag),
    })
}
