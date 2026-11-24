// SPDX-License-Identifier: AGPL-3.0-or-later
// Ported from getID3() by James Heinrich (https://www.getid3.org), which offers its
// code under the GNU GPL version 3 among other licences; taken under that one.

//! The ID3 tags of an MP3 file as getID3's `id3v2` and `id3v1` modules read
//! them, for the parts `wp_add_id3_tag_data()` keeps: the comments (title,
//! artist, album and the rest) and the first attached picture. A tag shape
//! getID3 reads in ways not ported here is [`Unported`].

use super::tables::{FRAMES, GENRES};
use crate::header::{ImageType, getimagesize};
use crate::meta::Unported;

/// The comments of one tag: each key's values, in the order getID3 adds
/// them. Their list keys are left out: `wp_add_id3_tag_data()` keeps each
/// list's first value, and getID3 only ever appends.
pub(super) type Comments = Vec<(Vec<u8>, Vec<Vec<u8>>)>;

/// The first `APIC` picture of an ID3v2 tag.
pub(super) struct Picture {
    /// The picture's bytes.
    pub data: Vec<u8>,
    /// `image_mime`: `image/gif`, `image/jpeg` or `image/png`.
    pub mime: &'static str,
    /// `image_width`.
    pub width: u32,
    /// `image_height`.
    pub height: u32,
}

/// A tag read.
pub(super) struct Tag {
    /// The comments.
    pub comments: Comments,
    /// The picture of the ID3v2 tag.
    pub picture: Option<Picture>,
}

/// The ID3v2 tag at the start of a file: its total length with its header,
/// and what it holds.
pub(super) struct V2 {
    /// `headerlength`.
    pub length: usize,
    /// What it holds.
    pub tag: Tag,
}

/// `$comments[ $name ][] = $value`, the list made if missing.
fn append(comments: &mut Comments, name: &str, value: Vec<u8>) {
    match comments
        .iter_mut()
        .find(|(have, _)| have == name.as_bytes())
    {
        Some((_, list)) => list.push(value),
        None => comments.push((name.as_bytes().to_vec(), vec![value])),
    }
}

/// PHP's `empty()` of a string.
pub(super) fn empty(text: &[u8]) -> bool {
    text.is_empty() || text == b"0"
}

/// PHP's `trim()` of its default characters.
pub(super) fn trim(text: &[u8]) -> &[u8] {
    let blank = |b: &u8| b" \t\n\r\0\x0b".contains(b);
    let start = text.iter().position(|b| !blank(b)).unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|b| !blank(b))
        .map_or(start, |at| at + 1);
    text.get(start..end).unwrap_or_default()
}

/// The big endian number of up to four bytes; as `BigEndian2Int()`, a
/// synchsafe one drops the high bit of each.
fn be(bytes: &[u8], synchsafe: bool) -> usize {
    bytes.iter().fold(0, |n, &b| {
        if synchsafe {
            n * 128 + usize::from(b & 0x7f)
        } else {
            n * 256 + usize::from(b)
        }
    })
}

/// `strpos( $data, $needle, $from )`.
fn find(data: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let tail = data.get(from..).unwrap_or_default();
    tail.windows(needle.len())
        .position(|w| w == needle)
        .map(|at| at + from)
}

/// A text encoding byte of an ID3v2 frame: the terminator and the name.
#[derive(Clone, Copy, PartialEq)]
enum Encoding {
    Latin1,
    Utf16,
    Utf16Be,
    Utf8,
}

impl Encoding {
    /// The encoding of byte `byte` in a tag of `major`, [`Unported`] where
    /// getID3 warns and defaults.
    fn of(byte: u8, major: u8) -> Result<Self, Unported> {
        match (byte, major) {
            (0, _) => Ok(Self::Latin1),
            (1, _) => Ok(Self::Utf16),
            (2, 4) => Ok(Self::Utf16Be),
            (3, 4) => Ok(Self::Utf8),
            _ => Err(Unported),
        }
    }

    /// `TextEncodingTerminatorLookup()`.
    fn terminator(self) -> &'static [u8] {
        match self {
            Self::Latin1 | Self::Utf8 => b"\0",
            Self::Utf16 | Self::Utf16Be => b"\0\0",
        }
    }

    /// `getid3_lib::iconv_fallback( $encoding, 'UTF-8', $text )` as
    /// mbstring does it, [`Unported`] for text it would substitute in.
    fn utf8(self, text: &[u8]) -> Result<Vec<u8>, Unported> {
        let converted = match self {
            Self::Utf8 => return Ok(text.to_vec()),
            Self::Latin1 => text.iter().map(|&b| char::from(b)).collect::<String>(),
            Self::Utf16 | Self::Utf16Be => {
                let mut text = text.to_vec();
                if self == Self::Utf16
                    && !text.starts_with(&[0xff, 0xfe])
                    && !text.starts_with(&[0xfe, 0xff])
                {
                    text.splice(0..0, [0xff, 0xfe]);
                }
                if self == Self::Utf16 && text.len() == 2 {
                    return Ok(Vec::new());
                }
                match self.decode16(&text)? {
                    decoded if decoded.is_empty() || decoded == "0" => return Ok(text),
                    decoded => decoded,
                }
            }
        };
        // Latin-1 converts "" and "0" to themselves.
        Ok(converted.into_bytes())
    }

    /// UTF-16 text, its byte order mark taken for `UTF-16`.
    fn decode16(self, text: &[u8]) -> Result<String, Unported> {
        let (mut little, mut body) = (false, text);
        if self == Self::Utf16 {
            little = text.starts_with(&[0xff, 0xfe]);
            body = text.get(2..).unwrap_or_default();
        }
        if body.len() % 2 != 0 {
            return Err(Unported);
        }
        let units = body.as_chunks::<2>().0.iter().map(|&pair| {
            if little {
                u16::from_le_bytes(pair)
            } else {
                u16::from_be_bytes(pair)
            }
        });
        char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .map_err(|_| Unported)
    }
}

/// `RemoveStringTerminator()`.
fn unterminated<'a>(text: &'a [u8], terminator: &[u8]) -> &'a [u8] {
    text.strip_suffix(terminator).unwrap_or(text)
}

/// The text from `from` up to the encoding's terminator, and the data
/// after it, with getID3's step over a terminator found inside a
/// double byte character.
fn split_at_terminator(
    data: &[u8],
    from: usize,
    encoding: Encoding,
) -> Result<(&[u8], &[u8]), Unported> {
    let terminator = encoding.terminator();
    let mut at = find(data, terminator, from).ok_or(Unported)?;
    if data.get(at + terminator.len()) == Some(&0) {
        at += 1;
    }
    let head = data.get(from..at).unwrap_or_default();
    let rest = data.get(at + terminator.len()..).unwrap_or_default();
    Ok((head, rest))
}

/// `FrameNameShortLookup()`.
fn short_name(frame: &[u8]) -> Option<&'static str> {
    FRAMES
        .iter()
        .find_map(|(long, short)| (long.as_bytes() == frame).then_some(*short))
}

/// Whether `name` is a frame name of ID3v2.3 or 2.4.
fn valid_name(name: &[u8]) -> bool {
    matches!(name, [first, rest @ ..] if first.is_ascii_uppercase() && rest.len() == 3
        && rest.iter().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()))
}

/// `DeUnsynchronise()`.
fn deunsynchronise(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    let mut at = 0;
    while let Some(&byte) = data.get(at) {
        out.push(byte);
        at += if byte == 0xff && data.get(at + 1) == Some(&0) {
            2
        } else {
            1
        };
    }
    out
}

/// What the frames read so far add up to.
struct Frames {
    comments: Comments,
    picture: Option<Picture>,
    seen_picture: bool,
    major: u8,
}

impl Frames {
    /// One frame of `name`, its `data` after the frame header and the
    /// flags' undoing; `size` is the length the frame header gives.
    fn frame(&mut self, name: &[u8], data: &[u8], size: usize) -> Result<(), Unported> {
        let short = short_name(name);
        let first = name.first().copied().unwrap_or_default();
        match name {
            _ if size == 0 => Ok(()),
            b"TXXX" => self.user_text(short, data),
            b"COMM" => self.comment(short, data),
            b"APIC" => self.attached_picture(data, size),
            b"IPLS" | b"MCDI" | b"USLT" | b"LINK" | b"USER" => Err(Unported),
            _ if first == b'T' => self.text(short, data),
            _ if first == b'W' => Err(Unported),
            _ => Ok(()),
        }
    }

    /// A `T???` frame.
    fn text(&mut self, short: Option<&str>, data: &[u8]) -> Result<(), Unported> {
        let encoding = Encoding::of(data.first().copied().unwrap_or_default(), self.major)?;
        let text = unterminated(data.get(1..).unwrap_or_default(), encoding.terminator());
        let Some(short) = short.filter(|_| !empty(text)) else {
            return Ok(());
        };
        let word = if encoding == Encoding::Utf16 || encoding == Encoding::Utf16Be {
            2
        } else {
            1
        };
        // The values a word of zeros separates.
        let words: Vec<&[u8]> = text.chunks(word).collect();
        let nul = |unit: &&[u8]| unit.len() == word && unit.iter().all(|&b| b == 0);
        for element in words.split(nul) {
            let string = encoding.utf8(&element.concat())?;
            if !empty(&string) {
                append(&mut self.comments, short, string);
            }
        }
        Ok(())
    }

    /// A `TXXX` frame.
    fn user_text(&mut self, short: Option<&str>, data: &[u8]) -> Result<(), Unported> {
        let encoding = Encoding::of(data.first().copied().unwrap_or_default(), self.major)?;
        // The description names the value's key alone.
        let (_, text) = split_at_terminator(data, 1, encoding)?;
        let text = unterminated(text, encoding.terminator());
        let Some(short) = short.filter(|_| !empty(text)) else {
            return Ok(());
        };
        let value = encoding.utf8(text)?;
        append(&mut self.comments, short, trim(&value).to_vec());
        Ok(())
    }

    /// A `COMM` frame.
    fn comment(&mut self, short: Option<&str>, data: &[u8]) -> Result<(), Unported> {
        if data.len() < 5 {
            return Ok(());
        }
        let encoding = Encoding::of(data.first().copied().unwrap_or_default(), self.major)?;
        let (_, text) = split_at_terminator(data, 4, encoding)?;
        let text = unterminated(text, encoding.terminator());
        let Some(short) = short.filter(|_| !empty(text)) else {
            return Ok(());
        };
        let value = encoding.utf8(text)?;
        append(&mut self.comments, short, value);
        Ok(())
    }

    /// An `APIC` frame, whose first is the picture WordPress keeps.
    fn attached_picture(&mut self, data: &[u8], size: usize) -> Result<(), Unported> {
        if self.seen_picture {
            return Ok(());
        }
        self.seen_picture = true;
        let encoding = Encoding::of(data.first().copied().unwrap_or_default(), self.major)?;
        let end = find(data, b"\0", 1).ok_or(Unported)?;
        let offset = end + 2;
        if offset >= size {
            return Err(Unported);
        }
        let (_, picture) = split_at_terminator(data, offset, encoding)?;
        if empty(picture) {
            return Ok(());
        }
        // A size of 0 is left out of getID3's frame, and PHP stores null.
        let image = getimagesize(picture)
            .filter(|image| image.width > 0 && image.height > 0)
            .ok_or(Unported)?;
        let mime = match image.kind {
            ImageType::Gif => "image/gif",
            ImageType::Jpeg => "image/jpeg",
            ImageType::Png => "image/png",
            _ => return Err(Unported),
        };
        self.picture = Some(Picture {
            data: picture.to_vec(),
            mime,
            width: image.width,
            height: image.height,
        });
        Ok(())
    }
}

/// `getid3_id3v1::LookupGenreName()` of a number.
fn genre_name(number: usize) -> Option<&'static str> {
    let number = u8::try_from(number).ok()?;
    GENRES
        .iter()
        .find_map(|(id, name)| (*id == number).then_some(*name))
}

/// `ParseID3v2GenreString()`.
fn genres(string: &[u8], major: u8) -> Result<Vec<Vec<u8>>, Unported> {
    let mut string = string.to_vec();
    if major == 3 && !string.contains(&0) {
        if string.contains(&b'/') {
            const SLASHED: [&[u8]; 4] = [b"Pop/Funk", b"Cut-up/DJ", b"RnB/Swing", b"Funk / Soul"];
            let all = |from: &[u8], to: &[u8], text: &[u8]| -> Vec<u8> {
                let mut out = Vec::new();
                let mut rest = text;
                while let Some((&byte, after)) = rest.split_first() {
                    match rest.get(..from.len()) {
                        Some(head) if head.eq_ignore_ascii_case(from) => {
                            out.extend_from_slice(to);
                            rest = rest.get(from.len()..).unwrap_or_default();
                        }
                        _ => {
                            out.push(byte);
                            rest = after;
                        }
                    }
                }
                out
            };
            string = string
                .iter()
                .map(|&b| if b == b'/' { 0 } else { b })
                .collect();
            for slashed in SLASHED {
                let split: Vec<u8> = slashed
                    .iter()
                    .map(|&b| if b == b'/' { 0 } else { b })
                    .collect();
                string = all(&split, slashed, &string);
            }
        }
        string = string
            .iter()
            .map(|&b| if b == b';' { 0 } else { b })
            .collect();
    }
    if !string.contains(&0) {
        string = bracketed_numbers(&string);
    }
    let mut clean = Vec::new();
    for element in string.split(|&b| b == 0) {
        let element = trim(element);
        if empty(element) {
            continue;
        }
        let digits = element.len() <= 3 && element.iter().all(u8::is_ascii_digit);
        if digits {
            let number = be_decimal(element);
            clean.push(genre_name(number).ok_or(Unported)?.as_bytes().to_vec());
        } else {
            clean.push(replace_all(element, b"((", b"("));
        }
    }
    Ok(clean)
}

/// The value of up to three decimal digits.
fn be_decimal(digits: &[u8]) -> usize {
    digits
        .iter()
        .fold(0, |n, &b| n * 10 + usize::from(b - b'0'))
}

/// `str_replace( $from, $to, $text )`.
fn replace_all(text: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some((&byte, after)) = rest.split_first() {
        match rest.strip_prefix(from) {
            Some(tail) => {
                out.extend_from_slice(to);
                rest = tail;
            }
            None => {
                out.push(byte);
                rest = after;
            }
        }
    }
    out
}

/// `preg_replace( '#\(([0-9]{1,3})\)#', '$1' . "\0", $text )`.
fn bracketed_numbers(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some((&byte, after)) = rest.split_first() {
        let digits = after
            .iter()
            .take(3)
            .take_while(|b| b.is_ascii_digit())
            .count();
        let number = after.get(..digits).unwrap_or_default();
        if byte == b'(' && digits > 0 && after.get(digits) == Some(&b')') {
            out.extend_from_slice(number);
            out.push(0);
            rest = after.get(digits + 1..).unwrap_or_default();
        } else {
            out.push(byte);
            rest = after;
        }
    }
    out
}

/// The tag after the frames: the genre list, the track number and the year.
fn finish(comments: &mut Comments, major: u8) -> Result<(), Unported> {
    // Only each list's first value is kept, so the genres' duplicates
    // getID3 drops make no difference.
    if let Some((_, values)) = comments.iter_mut().find(|(key, _)| key == b"genre") {
        let mut all = Vec::new();
        for value in values.iter() {
            all.extend(genres(value, major)?);
        }
        *values = all;
    }
    let mut totals = Vec::new();
    if let Some((_, values)) = comments.iter_mut().find(|(key, _)| key == b"track_number") {
        for value in values.iter_mut() {
            if let Some(slash) = value.iter().position(|&b| b == b'/') {
                let rest = value.split_off(slash + 1);
                value.pop();
                totals.push(
                    rest.split(|&b| b == b'/')
                        .next()
                        .unwrap_or_default()
                        .to_vec(),
                );
            }
        }
    }
    // No frame gives `totaltracks`.
    if !totals.is_empty() {
        comments.push((b"totaltracks".to_vec(), totals));
    }
    if !comments.iter().any(|(key, _)| key == b"year") {
        let time = comments
            .iter()
            .find(|(key, _)| key == b"recording_time")
            .and_then(|(_, values)| values.first());
        let year = time
            .and_then(|value| trim(value).get(..4))
            .filter(|year| year.iter().all(u8::is_ascii_digit));
        if let Some(year) = year.map(<[u8]>::to_vec) {
            comments.push((b"year".to_vec(), vec![year]));
        }
    }
    Ok(())
}

/// The ID3v2 tag at the start of `bytes`: `None` where there is none.
pub(super) fn id3v2(bytes: &[u8]) -> Result<Option<V2>, Unported> {
    if !bytes.starts_with(b"ID3") {
        return Ok(None);
    }
    // The file holds 512 bytes or more.
    let byte = |i: usize| bytes.get(i).copied().unwrap_or_default();
    let (major, flags) = (byte(3), byte(5));
    let footer = major == 4 && flags & 0x10 != 0;
    if !(3..=4).contains(&major) || footer {
        return Err(Unported);
    }
    let length = be(bytes.get(6..10).unwrap_or_default(), true) + 10;
    let body = bytes.get(10..length).ok_or(Unported)?;
    let mut data = if flags & 0x80 != 0 && major <= 3 {
        deunsynchronise(body)
    } else {
        body.to_vec()
    };
    if flags & 0x40 != 0 {
        data = data
            .get(extended_header(&data, major)?..)
            .unwrap_or_default()
            .to_vec();
    }
    let mut frames = Frames {
        comments: Vec::new(),
        picture: None,
        seen_picture: false,
        major,
    };
    let mut rest = data.as_slice();
    // Ten bytes or fewer left are padding.
    while let Some((header, after)) = rest
        .split_first_chunk::<10>()
        .filter(|(_, after)| !after.is_empty())
    {
        let name = &header[..4];
        let raw = be(&header[4..8], false);
        let size = if major == 3 {
            raw
        } else {
            be(&header[4..8], true)
        };
        // A frame getID3 would read with another size (an ID3v2.4 tag of
        // ID3v2.3 sizes, broken MP3ext names) leaves an invalid frame name
        // after it, which is left below.
        if name == b"\0\0\0\0" {
            break;
        }
        let (Some(payload), true) = (after.get(..size), valid_name(name)) else {
            return Err(Unported);
        };
        let flag_bits = be(&header[8..10], false);
        let mut payload = payload.to_vec();
        // Compression, encryption and grouping; unsynchronisation; data
        // length.
        let (unread, unsync, indicator) = if major == 3 {
            (0xe0, 0, 0)
        } else {
            (0x4c, 0x02, 0x01)
        };
        if flag_bits & unread != 0 {
            return Err(Unported);
        }
        if flag_bits & unsync != 0 {
            payload = deunsynchronise(&payload);
        }
        if flag_bits & indicator != 0 {
            payload = payload.get(4..).unwrap_or_default().to_vec();
        }
        frames.frame(name, &payload, size)?;
        rest = after.get(size..).unwrap_or_default();
    }
    let Frames {
        mut comments,
        picture,
        ..
    } = frames;
    finish(&mut comments, major)?;
    Ok(Some(V2 {
        length,
        tag: Tag { comments, picture },
    }))
}

/// The length of the extended header at the start of `data`.
fn extended_header(data: &[u8], major: u8) -> Result<usize, Unported> {
    let byte = |at: usize| data.get(at).copied().ok_or(Unported);
    let mut at = 4;
    if major == 3 {
        let flags = be(data.get(4..6).ok_or(Unported)?, false);
        let padding = be(data.get(6..10).ok_or(Unported)?, false);
        at = 10 + padding;
        if flags & 0x8000 != 0 {
            at += 4;
        }
        return Ok(at);
    }
    // More flag bytes than PHP's integers hold are not read.
    let count = usize::from(byte(at)?);
    if count > 8 {
        return Err(Unported);
    }
    at += 1;
    let flags = be(data.get(at..at + count).ok_or(Unported)?, false);
    at += count;
    if flags & 0x40 != 0 {
        at += 1;
    }
    if flags & 0x20 != 0 {
        at += 1 + usize::from(byte(at)?);
    }
    // getID3 fails on the restrictions flag, which PHP takes as an array.
    if flags & 0x10 != 0 {
        return Err(Unported);
    }
    Ok(at)
}

/// `getid3_id3v1::cutfield()`.
fn cut_field(field: &[u8]) -> Vec<u8> {
    let end = field.split(|&b| b == 0).next().unwrap_or_default();
    trim(end).to_vec()
}

/// The ID3v1 tag in the last 256 bytes of a file, `tail`, and whether a
/// second one precedes it, which getID3 takes as a duplicate.
pub(super) fn id3v1(tail: &[u8; 256]) -> Result<Option<(Tag, bool)>, Unported> {
    let (before, tag) = tail.split_at(128);
    let duplicate = before.starts_with(b"TAG");
    if !tag.starts_with(b"TAG") {
        return if duplicate { Err(Unported) } else { Ok(None) };
    }
    let at = |i: usize| tag.get(i).copied().unwrap_or_default();
    let field = |from: usize, len: usize| cut_field(tag.get(from..from + len).unwrap_or_default());
    let genre_id = usize::from(at(127));
    let mut comment = tag.get(97..127).unwrap_or_default().to_vec();
    let mut track = None;
    if at(125) == 0 && at(126) != 0 {
        track = Some(at(126));
        comment.truncate(28);
    }
    let mut pairs: Vec<(&str, Vec<u8>)> = vec![
        ("title", field(3, 30)),
        ("artist", field(33, 30)),
        ("album", field(63, 30)),
        ("year", field(93, 4)),
        ("comment", cut_field(&comment)),
    ];
    let genre = genre_name(genre_id).filter(|name| *name != "Unknown");
    if genre_name(genre_id).is_none() {
        pairs.push(("genreid", genre_id.to_string().into_bytes()));
    }
    if let Some(track) = track {
        pairs.push(("track_number", track.to_string().into_bytes()));
    }
    if let Some(genre) = genre {
        pairs.push(("genre", genre.as_bytes().to_vec()));
    }
    let comments = pairs
        .into_iter()
        .map(|(key, value)| (key.as_bytes().to_vec(), vec![value]))
        .collect();
    Ok(Some((
        Tag {
            comments,
            picture: None,
        },
        duplicate,
    )))
}
