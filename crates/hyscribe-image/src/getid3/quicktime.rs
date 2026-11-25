// SPDX-License-Identifier: AGPL-3.0-or-later
// Ported from getID3() by James Heinrich (https://www.getid3.org), which offers its
// code under the GNU GPL version 3 among other licences; taken under that one.

//! A QuickTime or MP4 file as getID3's `audio-video.quicktime` module reads
//! it, with the sums `getID3::analyze()` makes after it: the movie and track
//! headers, the sample descriptions and time tables, the AAC bitrate and the
//! first cover picture. A file whose reading would depend on PHP's memory
//! limit, reach another getID3 module or read in ways not ported is
//! [`Unported`]. Every read is a slice of the upload; the time is linear in
//! its size, with fixed limits on nesting, atoms and the bytes held.

use hyscribe_phpser::{Float, Value, is_empty_string};

use super::mp3::{Audio, Num};
use super::{Entry, key, playtime_string, text};
use crate::meta::Unported;

/// getID3's read buffer of an atom, a quarter of PHP's memory limit, for
/// the limits of 128M and above: where an atom is cut, the byte after the
/// cut is the one getID3 tests for an MPEG audio frame.
const BUFFERS: [usize; 4] = [32 << 20, 64 << 20, 128 << 20, 256 << 20];

/// The bytes of the atoms other than media data a file may have, which PHP
/// keeps in memory: past them its memory limit could stop it.
const HELD: usize = 16 << 20;

/// The deepest nesting read.
const DEPTH: usize = 32;

/// The most atoms read, each an array PHP keeps.
const ATOMS: usize = 20_000;

/// The time table entries getID3 reads under a memory limit of 128M: a
/// longer table is read to a length that depends on the limit.
const TIMES: u64 = 13_421;

/// The sample descriptions past the end of their table PHP is let read as
/// empty ones; more would take its memory.
const PHANTOMS: u64 = 64;

/// The seconds from 1904, QuickTime's epoch, to 1970.
const MAC_EPOCH: i64 = 2_082_844_800;

/// `BigEndian2Int( substr( $data, $at, $n ) )`: the number of the bytes
/// there, up to `n` (at most 8); `None`, PHP's `false`, for none.
fn be(data: &[u8], at: usize, n: usize) -> Option<u64> {
    let rest = data.get(at..).unwrap_or_default();
    let part = rest.get(..n).unwrap_or(rest);
    (!part.is_empty()).then(|| part.iter().fold(0, |v, &b| v * 256 + u64::from(b)))
}

/// `FixedPoint16_16( substr( $data, $at, 4 ) )` of four bytes there.
fn fixed(data: &[u8], at: usize) -> f64 {
    let whole = be(data, at, 2).unwrap_or(0) as f64;
    whole + be(data, at + 2, 2).unwrap_or(0) as f64 / 65536.0
}

/// The four bytes, or fewer at the end, at `at`.
fn four(data: &[u8], at: usize) -> &[u8] {
    let rest = data.get(at..).unwrap_or_default();
    rest.get(..4).unwrap_or(rest)
}

/// `quicktime_read_mp4_descr_length()`: a length of up to five bytes of
/// seven bits each.
fn descr_length(data: &[u8], at: &mut usize) -> u64 {
    let mut length = 0;
    for _ in 0..5 {
        let byte = data.get(*at).copied().unwrap_or(0);
        *at += 1;
        length = length * 128 + u64::from(byte & 0x7f);
        if byte & 0x80 == 0 {
            break;
        }
    }
    length
}

/// Whether PHP's `empty()` holds of a number.
fn zero(n: Option<Num>) -> bool {
    n.is_none_or(|n| n.float() == 0.0)
}

/// A number of the audio array, which holds no other values where read.
fn number(value: &Value) -> Num {
    match value {
        Value::Float(f) => Num::Float(f.to_f64()),
        value => Num::Int(value.to_int()),
    }
}

/// The formats getID3 reads a video sample description of as MP4's.
const MP4_VIDEO: [&[u8]; 27] = [
    b"2vuY", b"avc1", b"cvid", b"dvc ", b"dvcp", b"gif ", b"h263", b"hvc1", b"jpeg", b"kpcd",
    b"mjpa", b"mjpb", b"mp4v", b"png ", b"raw ", b"rle ", b"rpza", b"smc ", b"SVQ1", b"SVQ3",
    b"tiff", b"v210", b"v216", b"v308", b"v408", b"v410", b"yuv2",
];

/// `QuicktimeVideoCodecLookup()` of those formats, then the RIFF module's
/// `fourccLookup()` of the rest (empty but for one).
fn codec_name(format: &[u8]) -> &'static str {
    match format {
        b"avc1" => "H.264/MPEG-4 AVC",
        b"cvid" => "Cinepak",
        b"dvc " => "DVC-NTSC",
        b"dvcp" => "DVC-PAL",
        b"gif " => "GIF",
        b"h263" => "H263",
        b"hvc1" => "H.265/HEVC",
        b"jpeg" => "JPEG",
        b"kpcd" => "PhotoCD",
        b"mjpa" => "Motion JPEG-A",
        b"mjpb" => "Motion JPEG-B",
        b"png " => "PNG",
        b"raw " => "RAW",
        b"rpza" => "Video",
        b"smc " => "Graphics",
        b"SVQ1" => "Sorenson Video 3",
        b"tiff" => "TIFF",
        b"yuv2" => "ComponentVideo",
        b"SVQ3" => "Sorenson Video 3 (Apple Quicktime 5)",
        _ => "",
    }
}

/// Whether the QuickTime lookup names the format, so that the RIFF one is
/// not asked.
fn quicktime_codec(format: &[u8]) -> bool {
    !matches!(format, b"SVQ3") && !codec_name(format).is_empty()
}

/// The pixel depths `QuicktimeColorNameLookup()` names.
const DEPTHS: [u64; 11] = [1, 2, 4, 8, 16, 24, 32, 33, 34, 36, 40];

/// The brands getID3 takes for HEIF images, which it reads otherwise.
const HEIF: [&[u8]; 8] = [
    b"heic", b"heix", b"hevc", b"hevx", b"heim", b"heis", b"hevm", b"hevs",
];

/// The atoms getID3 reads with effects not ported: compressed, fragmented
/// and image movies, GPS tables and Nikon and Kodak pictures it reads from
/// elsewhere in the file, an ID3 tag, metadata keys, and the track lists
/// it keeps a number of per four bytes.
const LEFT: [&[u8]; 31] = [
    b"moof", b"traf", b"cmov", b"cmvd", b"gps ", b"ID32", b"NCDT", b"NCTH", b"NCVW", b"NCM1",
    b"NCM2", b"NCTG", b"frea", b"thma", b"scra", b"keys", b"pitm", b"iloc", b"iinf", b"iref",
    b"iprp", b"tfdt", b"tfhd", b"mfhd", b"trun", b"tmcd", b"chap", b"sync", b"scpt", b"ssrc",
    b"imgt",
];

/// The atoms getID3 reads as lists of others.
const CONTAINERS: [&[u8]; 16] = [
    b"moov", b"trak", b"edts", b"tref", b"mdia", b"minf", b"dinf", b"udta", b"clip", b"matt",
    b"nmhd", b"rmra", b"rmda", b"gmhd", b"ilst", b"meta",
];

/// The `uuid` of 360fly sensor data, which getID3 splits into an array per
/// 32 bytes.
const FLY: [u8; 16] = [
    0xef, 0xe1, 0x58, 0x9a, 0xbb, 0x77, 0x49, 0xef, 0x80, 0x95, 0x27, 0x75, 0x9e, 0xb1, 0xdc, 0x6f,
];

/// `$info['video']`, for what `wp_read_video_metadata()` keeps of it.
#[derive(Default)]
struct Video {
    dataformat: Option<&'static str>,
    fourcc: Option<Vec<u8>>,
    codec: Option<Vec<u8>>,
    /// `resolution_x` and `resolution_y`, always set together.
    size: Option<(f64, f64)>,
    /// `lossless` is set, always to `false`.
    lossless: bool,
    bitrate: Option<Num>,
    /// `frame_rate` is set and not 0.
    moving: bool,
}

/// `resolution_x` of `video`.
fn width(video: &Video) -> Option<Num> {
    video.size.map(|(width, _)| Num::Float(width))
}

/// What a sample table atom holds, for its `stbl`.
#[derive(Debug, PartialEq)]
enum Table {
    None,
    /// A sample description table, and whether an entry is AVC or MPEG-4
    /// video.
    Descriptions(bool),
    /// A time to sample table's counts and durations.
    Times(Vec<(Option<u64>, Option<u64>)>),
}

/// What an atom gives the atom holding it.
struct Parsed {
    /// `creation_time_unix` of a movie, track or media header.
    created: Option<i64>,
    /// A `moov`'s: that of its first atom.
    first: Option<i64>,
    table: Table,
}

/// Where a top level atom lies, for the media data's checks.
#[derive(Clone, Copy)]
struct Top {
    /// Its offset and size.
    at: usize,
    size: usize,
    /// Where its data starts, past the 64 bit size if any.
    start: usize,
    /// The bytes getID3 reads of it: its size less the 64 bit size, eight
    /// bytes past its end.
    read: usize,
}

/// getID3's `$info` as the module and `analyze()` leave it.
struct Movie<'a> {
    file: &'a [u8],
    format: &'static str,
    mime: &'static str,
    seconds: Option<Num>,
    bitrate: Option<Num>,
    data_at: usize,
    /// `None` once unset.
    video: Option<Video>,
    audio: Audio,
    /// `$info['quicktime']['video']` and `['audio']` are not empty.
    quicktime_video: bool,
    quicktime_audio: bool,
    time_scale: Option<u64>,
    created: Option<i64>,
    /// The first picture, with its type where it is one of the three the
    /// cover attachment takes.
    picture: Option<(Vec<u8>, Option<&'static str>)>,
    atoms: usize,
    held: usize,
}

impl<'a> Movie<'a> {
    /// `$info` as `openfile()` and the format test leave it for `file`.
    fn new(file: &'a [u8]) -> Self {
        let mut audio = Audio::default();
        audio.set("dataformat", text(""));
        Movie {
            file,
            format: "quicktime",
            mime: "video/quicktime",
            seconds: None,
            bitrate: None,
            data_at: 0,
            video: Some(Video::default()),
            audio,
            quicktime_video: false,
            quicktime_audio: false,
            time_scale: None,
            created: None,
            picture: None,
            atoms: 0,
            held: 0,
        }
    }

    fn video(&mut self) -> &mut Video {
        self.video.get_or_insert_with(Video::default)
    }

    fn audio_number(&self, name: &str) -> Option<Num> {
        self.audio
            .0
            .iter()
            .find(|(k, _)| *k == key(name))
            .map(|(_, v)| number(v))
    }

    fn audio_empty(&self, name: &str) -> bool {
        let found = self.audio.0.iter().find(|(k, _)| *k == key(name));
        found.is_none_or(|(_, value)| match value {
            Value::String(s) => is_empty_string(s),
            value => zero(Some(number(value))),
        })
    }

    /// The bytes from the media data to the end, in bits.
    fn data_bits(&self) -> Num {
        Num::Int((self.file.len() - self.data_at) as i64 * 8)
    }

    /// `QuicktimeParseContainerAtom()`: the atoms of `data`, inside
    /// `parent`.
    fn container(
        &mut self,
        data: &[u8],
        parent: &[u8],
        depth: usize,
        held: usize,
    ) -> Result<Vec<Parsed>, Unported> {
        let mut found = Vec::new();
        let mut at = 0;
        while let Some(size) = be(data, at, 4) {
            let size = size as usize;
            if size == 0 {
                // A list may end in four zero bytes, which are skipped.
                if data.len() > 12 {
                    at += 4;
                    continue;
                }
                break;
            }
            let rest = data.get(at + 8..).unwrap_or_default();
            // A size below the header's takes `substr()`'s negative length:
            // the rest but its last bytes, and the next atom starts inside it.
            let short = rest.get(..rest.len().saturating_sub(8 - size.min(8)));
            let Some(body) = (if size < 8 {
                short
            } else {
                rest.get(..size - 8)
            }) else {
                break;
            };
            let name = four(data, at + 4);
            found.push(self.atom(name, body, parent, depth + 1, held, None)?);
            at += size;
        }
        Ok(found)
    }

    /// `QuicktimeParseAtom()` of the atom `name` holding `data`, inside
    /// `parent`, `top` where it is at the top level.
    fn atom(
        &mut self,
        name: &[u8],
        data: &[u8],
        parent: &[u8],
        depth: usize,
        held: usize,
        top: Option<Top>,
    ) -> Result<Parsed, Unported> {
        self.atoms += 1;
        let held = held + data.len();
        if self.atoms > ATOMS || depth > DEPTH || held > 2 * HELD {
            return Err(Unported);
        }
        // A name of three zero bytes and a number is read as a list whose
        // compaction PHP may stop on with a fatal error.
        if name.starts_with(&[0, 0, 0]) || LEFT.contains(&name) {
            return Err(Unported);
        }
        let mut parsed = Parsed {
            created: None,
            first: None,
            table: Table::None,
        };
        match name {
            _ if CONTAINERS.contains(&name) => {
                let found = self.container(data, name, depth, held)?;
                parsed.first = found.first().and_then(|atom| atom.created);
            }
            b"stbl" => self.stbl(data, depth, held)?,
            b"stsd" => parsed.table = self.stsd(data, depth, held)?,
            b"stts" => parsed.table = stts(data)?,
            b"mvhd" | b"mdhd" | b"tkhd" => parsed.created = Some(self.header(name, data)?),
            b"esds" => self.esds(data),
            // Media data inside another atom, of no size, adds nothing.
            b"mdat" if top.is_none() && data.is_empty() => {}
            b"mdat" => self.mdat(data, top.ok_or(Unported)?)?,
            b"hdlr" if data.get(8..16) == Some(b"STpnzzzz") => {
                self.video().dataformat = Some("quicktimevr");
            }
            b"ctyp" if data.get(..4) == Some(b"qtvr") => {
                self.video().dataformat = Some("quicktimevr");
            }
            b"ftyp" if top.is_some() && HEIF.contains(&four(data, 0)) => return Err(Unported),
            b"covr" if parent != b"udta" => self.cover(data)?,
            b"uuid" if data.starts_with(&FLY) => return Err(Unported),
            b"dref" => dref(data)?,
            b"elst" if 8 + 12 * be(data, 4, 4).unwrap_or(0) > data.len() as u64 => {
                return Err(Unported);
            }
            b"iods" => {
                let mut at = 5;
                if descr_length(data, &mut at) > 7 + 6 * data.len() as u64 {
                    return Err(Unported);
                }
            }
            // Every other atom adds nothing WordPress keeps.
            _ => {}
        }
        Ok(parsed)
    }

    /// A movie, track or media header: the time scale, the movie's length
    /// and a track's size; its `creation_time_unix`.
    fn header(&mut self, name: &[u8], data: &[u8]) -> Result<i64, Unported> {
        let created = be(data, 4, 4).unwrap_or(0) as i64 - MAC_EPOCH;
        if name == b"tkhd" {
            // The track header's size and matrix, read as their version 0.
            if data.len() < 84 {
                return Err(Unported);
            }
            // Its rotation is always set, if only to 0.
            self.quicktime_video = true;
            if be(data, 1, 3).unwrap_or(0) & 1 == 1 {
                let (width, height) = (fixed(data, 76), fixed(data, 80));
                let video = self.video();
                let (have_width, have_height) = video.size.unwrap_or((width, height));
                video.size = Some((have_width.max(width), have_height.max(height)));
            }
            return Ok(created);
        }
        // getID3 stops on a scale of 0 in ways not ported.
        let scale = be(data, 12, 4).unwrap_or(0);
        if scale == 0 || data.len() < 20 {
            return Err(Unported);
        }
        self.time_scale = Some(match self.time_scale {
            Some(have) if have < 1000 => have.max(scale),
            _ => scale,
        });
        if name == b"mvhd" {
            let duration = be(data, 16, 4).unwrap_or(0) as i64;
            self.seconds = Some(Num::Int(duration).div(Num::Int(scale as i64)));
        }
        Ok(created)
    }

    /// A sample table: the frame rate of a video's.
    fn stbl(&mut self, data: &[u8], depth: usize, held: usize) -> Result<(), Unported> {
        let found = self.container(data, b"stbl", depth, held)?;
        let video = found
            .iter()
            .any(|atom| matches!(atom.table, Table::Descriptions(true)));
        let (mut rate, mut count) = (false, None);
        let scale = self.time_scale.unwrap_or(0);
        for atom in &found {
            let Table::Times(times) = &atom.table else {
                continue;
            };
            for &(samples, duration) in times {
                let duration = duration.unwrap_or(0);
                if duration == 0 || scale == 0 {
                    continue;
                }
                // round( scale / duration, 3 ) is 0 below half a
                // thousandth; at it, PHP's rounding is not ported.
                rate = match (scale * 2000).cmp(&duration) {
                    std::cmp::Ordering::Equal => return Err(Unported),
                    order => order.is_gt(),
                };
                count = samples;
            }
        }
        if video && rate {
            self.video().moving = true;
            self.quicktime_video = true;
        }
        if video && count.is_some_and(|count| count > 0) {
            self.quicktime_video = true;
        }
        Ok(())
    }

    /// A sample description table: its entries, and as many past the end
    /// as PHP reads as empty video descriptions.
    fn stsd(&mut self, data: &[u8], depth: usize, held: usize) -> Result<Table, Unported> {
        // PHP masks off the upper byte of a count past it.
        let count = be(data, 4, 4).unwrap_or(0) & 0x00ff_ffff;
        let (mut at, mut read, mut video) = (8, 0, false);
        while read < count && at < data.len() {
            let size = be(data, at, 4).unwrap_or(0) as usize;
            let rest = data.get(at..).unwrap_or_default();
            let entry = rest.get(..size).filter(|_| size >= 16).ok_or(Unported)?;
            let format = four(entry, 4);
            video |= format == b"avc1" || format == b"mp4v";
            self.entry(format, entry.get(16..).unwrap_or_default(), depth, held)?;
            (at, read) = (at + size, read + 1);
        }
        if read < count {
            if at != data.len() || count - read > PHANTOMS {
                return Err(Unported);
            }
            self.video().lossless = true;
        }
        Ok(Table::Descriptions(video))
    }

    /// One sample description of `format`, its `data` after the reference
    /// index.
    fn entry(
        &mut self,
        format: &[u8],
        data: &[u8],
        depth: usize,
        held: usize,
    ) -> Result<(), Unported> {
        // A Parrot drone's metadata, which getID3 skips.
        if data.get(1..55) == Some(b"application/octet-stream;type=com.parrot.videometadata") {
            return Ok(());
        }
        match four(data, 4) {
            [0, 0, 0, 0] => {
                if data.len() < 20 {
                    return Err(Unported);
                }
                if MP4_VIDEO.contains(&format) {
                    self.format = "mp4";
                    let video = self.video();
                    video.fourcc = Some(format.to_vec());
                    if quicktime_codec(format) {
                        video.codec = Some(codec_name(format).into());
                    }
                    let (width, height) = (be(data, 16, 2), be(data, 18, 2));
                    if let (Some(w @ 1..), Some(h @ 1..)) = (width, height) {
                        video.size = Some((w as f64, h as f64));
                        self.quicktime_video = true;
                    }
                } else if format == b"qtvr" {
                    self.video().dataformat = Some("quicktimevr");
                } else if format == b"mp4a" {
                    self.container(data.get(20..).unwrap_or_default(), b"stsd", depth, held)?;
                    self.quicktime_audio = true;
                    let channels = be(data, 8, 2).unwrap_or(0) as i64;
                    let bits = be(data, 10, 2).unwrap_or(0) as i64;
                    self.audio.set("codec", text("ISO/IEC 14496-3 AAC"));
                    let rate = Float::from_f64(fixed(data, 16));
                    self.audio.set("sample_rate", Value::Float(rate));
                    self.audio.set("channels", Value::Int(channels));
                    self.audio.set("bits_per_sample", Value::Int(bits));
                    self.audio.set("lossless", Value::Bool(false));
                }
            }
            b"keys" => {}
            _ if format == b"mp4s" => self.format = "mp4",
            _ => {
                // A video description of a vendor's: its compressor's name.
                if data.len() < 70 {
                    return Err(Unported);
                }
                if be(data, 66, 2).is_some_and(|depth| DEPTHS.contains(&depth)) {
                    let length = usize::from(data.get(34).copied().unwrap_or(0));
                    let name = data.get(35..).unwrap_or_default();
                    let name = name.get(..length).unwrap_or(name);
                    let codec = if length > 0 { name } else { format };
                    self.quicktime_video = true;
                    self.video().codec = Some(codec.to_vec());
                }
                self.video().lossless = true;
            }
        }
        match format.to_ascii_lowercase().as_slice() {
            b"mp4a" => {
                self.audio.set("dataformat", text("mp4"));
                self.quicktime_audio = true;
            }
            b"3ivx" | b"3iv1" | b"3iv2" => self.video().dataformat = Some("3ivx"),
            b"xvid" => self.video().dataformat = Some("xvid"),
            b"mp4v" => self.video().dataformat = Some("mpeg4"),
            b"divx" | b"div1" | b"div2" | b"div3" | b"div4" | b"div5" | b"div6" => {
                self.video().dataformat = Some("divx");
            }
            _ => {}
        }
        Ok(())
    }

    /// An elementary stream descriptor: the average bitrate of its audio.
    fn esds(&mut self, data: &[u8]) {
        let mut at = 5;
        if be(data, 4, 1) != Some(3) {
            return;
        }
        descr_length(data, &mut at);
        let flags = be(data, at + 2, 1).unwrap_or(0);
        at += 3;
        if flags & 0x40 != 0 {
            return;
        }
        at += if flags & 0x80 != 0 { 2 } else { 0 };
        at += if flags & 0x20 != 0 { 2 } else { 0 };
        if be(data, at, 1) != Some(4) {
            return;
        }
        at += 1;
        descr_length(data, &mut at);
        let average = be(data, at + 9, 4).unwrap_or(0);
        if average != 0 {
            self.quicktime_audio = true;
            self.audio.set("bitrate", Value::Int(average as i64));
        }
    }

    /// The media data, which only marks where the media starts; [`Unported`]
    /// where getID3 would read chapter titles from it or an MPEG audio
    /// stream from where its read stops.
    fn mdat(&mut self, data: &[u8], top: Top) -> Result<(), Unported> {
        let mut at = 0;
        while matches!(
            data.get(at..at + 8),
            Some(b"\0\0\0\x08wide" | b"\0\0\0\0mdat")
        ) {
            at += 8;
        }
        let length = be(data, at, 2).unwrap_or(0) as usize;
        let title = data.get(at + 2..at + 2 + length).unwrap_or_default();
        let printable = title.iter().all(|&b| b >= 0x20 || b == b'\n');
        if at + 8 < data.len() && (1..1000).contains(&length) && title.len() == length && printable
        {
            return Err(Unported);
        }
        if top.size > 8 {
            self.data_at = top.at + 8;
            for buffer in BUFFERS {
                let stop = (top.start + top.read.min(buffer)).min(self.file.len());
                if matches!(self.file.get(stop..), Some([0xff, b, ..]) if b & 0xe0 == 0xe0) {
                    return Err(Unported);
                }
            }
        }
        Ok(())
    }

    /// A cover picture's data boxes, the first picture of the file kept.
    fn cover(&mut self, data: &[u8]) -> Result<(), Unported> {
        // getID3 reads boxes of a 16 bit size here, and no picture.
        if data.get(2..4) == Some(&[0x10, 0xb5]) {
            return Ok(());
        }
        let mut at = 0;
        while let Some(size) = be(data, at, 4) {
            let size = size as usize;
            if size <= 1 {
                break;
            }
            if size < 8 {
                return Err(Unported);
            }
            let kind = four(data, at + 4);
            let rest = data.get(at + 8..).unwrap_or_default();
            let body = rest.get(..size - 8).unwrap_or(rest);
            at += size;
            if kind != b"data" {
                continue;
            }
            let picture = body.get(8..).unwrap_or_default();
            let flagged = matches!(be(body, 1, 3), None | Some(0 | 21));
            if (flagged || !is_empty_string(picture)) && self.picture.is_none() {
                self.picture = Some((picture.to_vec(), picture_type(picture)));
            }
        }
        Ok(())
    }

    /// The end of the module's `Analyze()`, for a file read to its end.
    fn finish(&mut self) {
        // No bitrate is set before: only getID3's MP3 reading in the media
        // data, which is not ported, would.
        if let Some(seconds) = self.seconds.filter(|seconds| !zero(Some(*seconds))) {
            self.bitrate = Some(self.data_bits().div(seconds));
        }
        let audio_bitrate = self.audio_number("bitrate");
        if let (Some(bitrate), None, false) = (self.bitrate, audio_bitrate, self.quicktime_video) {
            self.audio.set("bitrate", bitrate.value());
        }
        let audio_bitrate = self.audio_number("bitrate");
        let bitrate = self.bitrate;
        // An audio bitrate is never 0, and no video bitrate is set before.
        if let (Some(video), Some(total), Some(audio)) = (&mut self.video, bitrate, audio_bitrate) {
            let more = total.sub(audio);
            let gained = more.float().partial_cmp(&0.0) == Some(std::cmp::Ordering::Greater);
            if gained && video.moving && !zero(width(video)) {
                video.bitrate = Some(more);
            }
        }
        let mp4 = (key("dataformat"), text("mp4"));
        if self.audio.0.contains(&mp4) {
            self.format = "mp4";
            let video = self.video();
            if zero(width(video)) {
                video.dataformat = None;
                self.mime = "audio/mp4";
            } else {
                self.mime = "video/mp4";
            }
        }
        if self.audio_empty("dataformat") && self.quicktime_audio {
            self.audio.set("dataformat", text("quicktime"));
        }
        let quicktime_video = self.quicktime_video;
        let video = self.video();
        if video.dataformat.is_none_or(str::is_empty) && quicktime_video {
            video.dataformat = Some("quicktime");
        }
        let height = video.size.map(|(_, height)| Num::Float(height));
        let flat = zero(width(video)) && zero(height);
        if self.mime == "audio/mp4" && flat {
            self.video = None;
        }
    }

    /// `ChannelsBitratePlaytimeCalculations()`,
    /// `CalculateCompressionRatioAudio()` and `CleanUp()` of the audio.
    fn sums(&mut self) {
        // No channel mode is set before.
        match self.audio_number("channels") {
            Some(Num::Int(1)) => self.audio.set("channelmode", text("mono")),
            Some(Num::Int(2)) => self.audio.set("channelmode", text("stereo")),
            _ => {}
        }
        let audio_bitrate = self.audio_number("bitrate");
        let video_bitrate = self.video.as_ref().and_then(|video| video.bitrate);
        let combined = [audio_bitrate, video_bitrate]
            .into_iter()
            .flatten()
            .fold(Num::Int(0), Num::add);
        if combined.float() > 0.0 && zero(self.bitrate) {
            self.bitrate = Some(combined);
        }
        let bits = self.data_bits();
        let (bitrate, seconds) = (self.bitrate, self.seconds);
        if let Some(video) = &mut self.video {
            let undetermined = video.dataformat.is_some_and(|format| !format.is_empty());
            // An audio bitrate is never 0.
            let same = audio_bitrate
                .zip(bitrate)
                .filter(|(audio, total)| audio.float() == total.float());
            let timed = seconds.filter(|seconds| seconds.float() > 0.0);
            if let (true, true, Some((audio, _)), Some(seconds)) =
                (undetermined, zero(video.bitrate), same, timed)
            {
                let total = Num::Float(bits.div(seconds).float().round());
                self.bitrate = Some(total);
                video.bitrate = Some(total.sub(audio));
            }
        }
        if self.seconds.is_none_or(|seconds| seconds.float() <= 0.0) && !zero(self.bitrate) {
            self.seconds = self.bitrate.map(|bitrate| bits.div(bitrate));
        }
        let seconds = self.seconds.filter(|seconds| !zero(Some(*seconds)));
        if let (None, Some(seconds)) = (self.bitrate, seconds) {
            self.bitrate = Some(bits.div(seconds));
        }
        let video_bitrate = self.video.as_ref().and_then(|video| video.bitrate);
        if let (Some(bitrate), true, true) = (
            self.bitrate,
            self.audio_empty("bitrate"),
            zero(video_bitrate),
        ) {
            let wide = self.video.as_ref().is_some_and(|video| !zero(width(video)));
            let silent = self.audio_empty("dataformat");
            if !wide {
                self.audio.set("bitrate", bitrate.value());
            } else if let (Some(video), true) = (&mut self.video, silent) {
                video.bitrate = Some(bitrate);
            }
        }
        if let Some(video) = &mut self.video
            && let (Some(fourcc), None) = (&video.fourcc, &video.codec)
        {
            video.codec = Some(codec_name(fourcc).into());
        }
        let rate = self.audio_number("sample_rate");
        let channels = self.audio_number("channels");
        let audio_bitrate = self.audio_number("bitrate");
        if let (Some(bitrate), Some(channels), Some(rate)) = (audio_bitrate, channels, rate)
            && !zero(Some(bitrate))
            && !zero(Some(channels))
            && !zero(Some(rate))
        {
            let bits = self
                .audio_number("bits_per_sample")
                .filter(|bits| !zero(Some(*bits)));
            let divisor = channels.mul(rate).mul(bits.unwrap_or(Num::Int(16)));
            self.audio
                .set("compression_ratio", bitrate.div(divisor).value());
        }
        for name in ["dataformat", "bits_per_sample", "bitrate"] {
            if self.audio_empty(name) {
                self.audio.0.retain(|(k, _)| *k != key(name));
            }
        }
    }
}

/// A time to sample table: its counts and durations, those past the end of
/// the atom read as PHP's `false`.
fn stts(data: &[u8]) -> Result<Table, Unported> {
    let count = be(data, 4, 4).unwrap_or(0);
    if count > TIMES {
        return Err(Unported);
    }
    let times: Vec<_> = (0..count as usize)
        .map(|n| (be(data, 8 + 8 * n, 4), be(data, 12 + 8 * n, 4)))
        .collect();
    Ok(if times.is_empty() {
        Table::None
    } else {
        Table::Times(times)
    })
}

/// A data reference table, whose entries must each lie in it: PHP walks
/// back and forth over the bytes of entries past its end.
fn dref(data: &[u8]) -> Result<(), Unported> {
    let mut at = 8;
    for _ in 0..be(data, 4, 4).unwrap_or(0) {
        let size = be(data, at, 4).unwrap_or(0) as usize;
        if size < 12 || data.len().saturating_sub(at) < size {
            return Err(Unported);
        }
        at += size;
    }
    Ok(())
}

/// The type of a cover picture, where it is one of the three the cover
/// attachment takes: by its first bytes, which PHP's `getimagesize()`
/// and getID3's own test agree on.
fn picture_type(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if data.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if data.starts_with(b"GIF") {
        Some("image/gif")
    } else {
        None
    }
}

/// Whether getID3 reads `b` with this module: one of its atoms first, and
/// none of the formats it tests before (a first atom of 32 MiB or more
/// could start like one, and is left).
pub(super) fn quicktime(b: &[u8]) -> bool {
    let atom = matches!(
        b.get(4..8),
        Some(b"cmov" | b"free" | b"ftyp" | b"mdat" | b"moov" | b"pnot" | b"skip" | b"wide")
    );
    atom && b.first().is_some_and(|&first| first <= 1)
        && !matches!(b, [0, 0, 1, 0xb3 | 0xba, ..])
        && b.get(44..48) != Some(b"SCRM")
        && b.get(1080..1084) != Some(b"M.K.")
}

/// The file `b` read: the module, then `analyze()`'s sums.
fn read(b: &[u8]) -> Result<Movie<'_>, Unported> {
    let mut movie = Movie::new(b);
    let mut at = 0;
    let whole = loop {
        if at >= b.len() {
            break true;
        }
        let mut size = u128::from(be(b, at, 4).unwrap_or(0));
        let name = four(b, at + 4);
        let mut extended = 0;
        if size == 1 {
            extended = 8;
            size = u128::from(be(b, at + 8, 8).unwrap_or(0));
        }
        // An atom past the end stops the module before its end.
        if at as u128 + size > b.len() as u128 {
            break false;
        }
        let size = size as usize;
        if size == 0 {
            break true;
        }
        // getID3 reads a size of less than its header in ways not ported.
        if size < 8 + extended {
            return Err(Unported);
        }
        let start = at + 8 + extended;
        let read = size - extended;
        let data = b.get(start..b.len().min(start + read)).unwrap_or_default();
        let top = Top {
            at,
            size,
            start,
            read,
        };
        if name == b"mdat" {
            movie.atom(
                name,
                data.get(..BUFFERS[0]).unwrap_or(data),
                b"",
                0,
                0,
                Some(top),
            )?;
        } else {
            movie.held += data.len();
            if movie.held > HELD {
                return Err(Unported);
            }
            let parsed = movie.atom(name, data, b"", 0, 0, Some(top))?;
            if name == b"moov" {
                movie.created = parsed.first;
            }
        }
        at += size;
    };
    if whole {
        movie.finish();
    }
    movie.sums();
    Ok(movie)
}

/// The entries both readings end with: the size, type and length, the date
/// and the cover picture.
fn file_entries(
    movie: &Movie<'_>,
    entries: &mut Vec<Entry>,
    middle: Vec<Entry>,
) -> Result<(), Unported> {
    entries.push((key("filesize"), Value::Int(movie.file.len() as i64)));
    entries.push((key("mime_type"), text(movie.mime)));
    if let Some(seconds) = movie.seconds.filter(|seconds| !zero(Some(*seconds))) {
        let seconds = seconds.float();
        entries.push((key("length"), Value::Int(seconds.round() as i64)));
        entries.push((key("length_formatted"), text(&playtime_string(seconds))));
    }
    entries.extend(middle);
    if let Some(created) = movie.created {
        entries.push((key("created_timestamp"), Value::Int(created)));
    }
    if let Some((data, mime)) = movie
        .picture
        .as_ref()
        .filter(|(data, _)| !is_empty_string(data))
    {
        let mime = mime.ok_or(Unported)?;
        let image = vec![
            (key("data"), Value::String(data.clone())),
            (key("mime"), text(mime)),
        ];
        entries.push((key("image"), Value::Array(image)));
    }
    Ok(())
}

/// `wp_read_video_metadata()` of a QuickTime or MP4 file.
pub(super) fn video(b: &[u8]) -> Result<Vec<Entry>, Unported> {
    let movie = read(b)?;
    let video = movie.video.as_ref();
    let mut entries = Vec::new();
    if video.is_some_and(|video| video.lossless) {
        entries.push((key("lossless"), Value::Bool(false)));
    }
    if let Some(bitrate) = video
        .and_then(|video| video.bitrate)
        .filter(|b| !zero(Some(*b)))
    {
        let bitrate = match bitrate {
            Num::Int(n) => n,
            Num::Float(f) => f as i64,
        };
        entries.push((key("bitrate"), Value::Int(bitrate)));
    }
    let mut middle = Vec::new();
    for (name, side) in [
        ("width", video.and_then(|v| v.size).map(|(width, _)| width)),
        (
            "height",
            video.and_then(|v| v.size).map(|(_, height)| height),
        ),
    ] {
        if let Some(side) = side.filter(|side| *side != 0.0) {
            middle.push((key(name), Value::Int(side as i64)));
        }
    }
    middle.push((key("fileformat"), text(movie.format)));
    if let Some(format) = video.and_then(|v| v.dataformat).filter(|f| !f.is_empty()) {
        middle.push((key("dataformat"), text(format)));
    }
    if let Some(codec) = video
        .and_then(|v| v.codec.clone())
        .filter(|c| !is_empty_string(c))
    {
        middle.push((key("codec"), Value::String(codec)));
    }
    if !movie.audio.0.is_empty() {
        middle.push((key("audio"), Value::Array(movie.audio.0.clone())));
    }
    file_entries(&movie, &mut entries, middle)?;
    Ok(entries)
}

/// `wp_read_audio_metadata()` of a QuickTime or MP4 file.
pub(super) fn audio(b: &[u8]) -> Result<Vec<Entry>, Unported> {
    let movie = read(b)?;
    let mut entries = movie.audio.0.clone();
    entries.push((key("fileformat"), text(movie.format)));
    file_entries(&movie, &mut entries, Vec::new())?;
    Ok(entries)
}

#[cfg(test)]
mod tests;
