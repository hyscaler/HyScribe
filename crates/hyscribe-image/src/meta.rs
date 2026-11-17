// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_read_image_metadata()`: the `image_meta` WordPress stores, from the
//! IPTC (HyScribe's reader), EXIF (`kamadak-exif`, for JPEG) and XMP of an
//! upload, each value a string after `wp_kses_post_deep()`. What PHP's
//! `exif_read_data()` would read in a way not ported (Unicode comments,
//! Windows XP tags, repeated values, unusual field types, an EXIF block
//! the reader rejects, XMP alternative text) is [`Unported`].

use exif::{In, Reader, Value as Exif};
use hyscribe_phpser::{Key, Value, float_to_string, php_trim};
use hyscribe_render::date::strtotime;
use hyscribe_render::kses::wp_kses_post;

use crate::header::{ImageSize, ImageType};
use crate::iptc::iptcparse;

/// What HyScribe does not read as PHP reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unported;

/// PHP's `empty()` of a string.
fn empty(text: &[u8]) -> bool {
    text.is_empty() || text == b"0"
}

/// `wp_exif_frac2dec( "$num/$denom" )`, 0 over 0. PHP's whole quotients
/// are integers, which `(string)` writes as `float_to_string()` does.
fn fraction(num: u32, denom: u32) -> f64 {
    match denom {
        0 => 0.0,
        _ => f64::from(num) / f64::from(denom),
    }
}

/// The fields `wp_read_image_metadata()` reads from `exif_read_data()`.
#[derive(Default)]
struct ExifFields {
    description: Vec<u8>,
    comment: Vec<u8>,
    artist: Vec<u8>,
    copyright: Vec<u8>,
    model: Vec<u8>,
    digitized: Vec<u8>,
    f_number: Option<f64>,
    focal_length: Option<f64>,
    exposure: Option<f64>,
    iso: Option<u32>,
    orientation: Option<u32>,
}

/// The first JPEG `APP1` segment holding EXIF, without its `Exif` header.
fn exif_segment(b: &[u8]) -> Option<&[u8]> {
    let mut at = 2;
    loop {
        let (0xFF, marker) = (*b.get(at)?, *b.get(at + 1)?) else {
            return None;
        };
        if matches!(marker, 0xD9 | 0xDA) {
            return None;
        }
        let length = usize::from(u16::from_be_bytes([*b.get(at + 2)?, *b.get(at + 3)?]));
        // The TIFF block within its segment, as PHP reads it.
        if let (0xE1, Some(tiff)) = (
            marker,
            b.get(at + 4..at + 2 + length)
                .unwrap_or_default()
                .strip_prefix(b"Exif\0\0"),
        ) {
            return Some(tiff);
        }
        at += 2 + length;
    }
}

/// What `exif_read_data()` gives of `field`: an ASCII field's text up to
/// its first NUL, a rational's value, an unsigned number.
fn ascii(value: &Exif) -> Result<Vec<u8>, Unported> {
    match value {
        Exif::Ascii(parts) => match parts.as_slice() {
            [] | [_] => Ok(parts.first().cloned().unwrap_or_default()),
            _ => Err(Unported),
        },
        _ => Err(Unported),
    }
}

fn rational(value: &Exif) -> Result<f64, Unported> {
    match value {
        Exif::Rational(r) => match r.as_slice() {
            [r] => Ok(fraction(r.num, r.denom)),
            _ => Err(Unported),
        },
        _ => Err(Unported),
    }
}

fn unsigned(value: &Exif) -> Result<u32, Unported> {
    match value {
        Exif::Short(v) => match v.as_slice() {
            [v] => Ok(u32::from(*v)),
            _ => Err(Unported),
        },
        _ => Err(Unported),
    }
}

/// `$exif['COMPUTED']['UserComment']`: an ASCII comment's text, or an
/// undefined one's with its trailing NULs and spaces dropped.
fn user_comment(value: &Exif) -> Result<Vec<u8>, Unported> {
    let Exif::Undefined(bytes, _) = value else {
        return Err(Unported);
    };
    let (code, text) = bytes.split_at_checked(8).ok_or(Unported)?;
    let text = match code {
        b"ASCII\0\0\0" => text.split(|&b| b == 0).next().unwrap_or_default(),
        // Its trailing NULs and spaces go with WordPress's trim().
        [0, 0, 0, 0, 0, 0, 0, 0] => text,
        _ => return Err(Unported),
    };
    Ok(text.to_vec())
}

fn read_exif(b: &[u8]) -> Result<ExifFields, Unported> {
    let mut out = ExifFields::default();
    let Some(tiff) = exif_segment(b) else {
        return Ok(out);
    };
    let exif = Reader::new()
        .read_raw(tiff.to_vec())
        .map_err(|_| Unported)?;
    for field in exif.fields().filter(|field| field.ifd_num == In::PRIMARY) {
        let value = &field.value;
        // By number: PHP names a tag the same in whichever IFD it is.
        match field.tag.number() {
            0x010E => out.description = ascii(value)?,
            0x013B => out.artist = ascii(value)?,
            0x8298 => out.copyright = ascii(value)?,
            0x0110 => out.model = ascii(value)?,
            0x9004 => out.digitized = ascii(value)?,
            0x9286 => out.comment = user_comment(value)?,
            0x829D => out.f_number = Some(rational(value)?),
            0x920A => out.focal_length = Some(rational(value)?),
            0x829A => out.exposure = Some(rational(value)?),
            0x8827 => out.iso = Some(unsigned(value)?),
            0x0112 => out.orientation = Some(unsigned(value)?),
            // XPComment and XPAuthor, read by PHP as `Comments` and `Author`.
            0x9C9C | 0x9C9D => return Err(Unported),
            _ => {}
        }
    }
    Ok(out)
}

/// `strtotime()` of IPTC's `CCYYMMDD HHMMSS` and EXIF's
/// `YYYY-MM-DD HH:MM:SS`, `None` for `false`.
fn timestamp(text: &[u8]) -> Result<Option<i64>, Unported> {
    let iso = match *text {
        // Any other text in this shape fails strtotime() both ways.
        [y1, y2, y3, y4, m1, m2, d1, d2, b' ', h1, h2, i1, i2, s1, s2] => {
            vec![
                y1, y2, y3, y4, b'-', m1, m2, b'-', d1, d2, b' ', h1, h2, b':', i1, i2, b':', s1,
                s2,
            ]
        }
        _ => text.to_vec(),
    };
    let iso = String::from_utf8(iso).map_err(|_| Unported)?;
    strtotime(&iso).map(Some).ok_or(Unported)
}

/// What `wp_exif_date2ts()` hands `strtotime()`: `YYYY:MM:DD HH:MM:SS`
/// with the date's colons made dashes, missing parts empty.
fn wp_exif_date2ts(text: &[u8]) -> Vec<u8> {
    let mut parts = php_trim(text).split(|&b| b == b' ');
    let (date, time) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    let mut ymd = date.split(|&b| b == b':');
    let mut part = || ymd.next().unwrap_or_default();
    [part(), b"-", part(), b"-", part(), b" ", time].concat()
}

/// `wp_get_image_alttext()`: empty unless the XMP names the alternative
/// text, which is not ported.
fn alt_text(b: &[u8]) -> Result<(), Unported> {
    let find = |needle: &[u8]| b.windows(needle.len()).position(|w| w == needle);
    match (find(b"<x:xmpmeta"), find(b"</x:xmpmeta>")) {
        (Some(start), Some(end))
            if b.get(start..end)
                .is_some_and(|xmp| xmp.windows(20).any(|w| w == b"AltTextAccessibility")) =>
        {
            Err(Unported)
        }
        _ => Ok(()),
    }
}

/// `utf8_encode()` where the text is not valid UTF-8.
fn utf8(text: Vec<u8>) -> Vec<u8> {
    match String::from_utf8(text) {
        Ok(text) => text.into_bytes(),
        Err(err) => err
            .into_bytes()
            .iter()
            .map(|&b| char::from(b))
            .collect::<String>()
            .into_bytes(),
    }
}

/// `wp_read_image_metadata( $file )` of the upload `b`, whose size and
/// type `wp_getimagesize()` gave; `upload_host` is the host of the uploads
/// URL, which `wp_kses_post()` checks embedded PDFs against.
pub fn wp_read_image_metadata(
    b: &[u8],
    size: &ImageSize<'_>,
    upload_host: &[u8],
) -> Result<Vec<(Key, Value)>, Unported> {
    let mut title = Vec::new();
    let mut caption = Vec::new();
    let mut credit = Vec::new();
    let mut copyright = Vec::new();
    let mut created: Option<i64> = Some(0);
    let mut keywords: Vec<Vec<u8>> = Vec::new();
    if let Some(iptc) = size.app13.and_then(iptcparse) {
        let first = |key: &str| {
            iptc.iter()
                .find(|(k, _)| k == key)
                .and_then(|(_, values)| values.first().copied())
                .unwrap_or_default()
        };
        if !empty(first("2#105")) {
            title = php_trim(first("2#105")).to_vec();
        } else if !empty(first("2#005")) {
            title = php_trim(first("2#005")).to_vec();
        }
        if !empty(first("2#120")) {
            caption = php_trim(first("2#120")).to_vec();
            if empty(&title) && caption.len() < 80 {
                title.clone_from(&caption);
            }
        }
        if !empty(first("2#110")) {
            credit = php_trim(first("2#110")).to_vec();
        } else if !empty(first("2#080")) {
            credit = php_trim(first("2#080")).to_vec();
        }
        if !empty(first("2#055")) && !empty(first("2#060")) {
            created = timestamp(&[first("2#055"), b" ", first("2#060")].concat())?;
        }
        if !empty(first("2#116")) {
            copyright = php_trim(first("2#116")).to_vec();
        }
        if let Some((_, values)) = iptc
            .iter()
            .find(|(k, values)| k == "2#025" && !empty(values.first().copied().unwrap_or_default()))
        {
            keywords = values.iter().map(|v| v.to_vec()).collect();
        }
    }
    alt_text(b)?;
    let (mut aperture, mut camera, mut focal, mut iso, mut shutter, mut orientation) = (
        "0".to_string(),
        Vec::new(),
        "0".to_string(),
        b"0".to_vec(),
        "0".to_string(),
        "0".to_string(),
    );
    if size.kind == ImageType::Jpeg {
        let exif = read_exif(b)?;
        let description = php_trim(&exif.description).to_vec();
        let comment = php_trim(&exif.comment).to_vec();
        if !empty(&description) {
            if empty(&title) && description.len() < 80 {
                title.clone_from(&description);
            }
            if empty(&caption) && !empty(&comment) {
                caption = if !empty(&title) && description == title {
                    comment.clone()
                } else if description == comment {
                    description.clone()
                } else {
                    php_trim(&[&description[..], b" ", &comment].concat()).to_vec()
                };
            }
            if empty(&caption) {
                caption.clone_from(&description);
            }
        } else if empty(&caption) && !empty(&comment) {
            caption.clone_from(&comment);
            if empty(&title) && comment.len() < 80 {
                title.clone_from(&comment);
            }
        }
        if empty(&credit) && !empty(&exif.artist) {
            credit = php_trim(&exif.artist).to_vec();
        }
        if empty(&copyright) && !empty(&exif.copyright) {
            copyright = php_trim(&exif.copyright).to_vec();
        }
        if let Some(f) = exif.f_number {
            aperture = float_to_string((f * 100.0).round() / 100.0);
        }
        if !empty(&exif.model) {
            camera = php_trim(&exif.model).to_vec();
        }
        if matches!(created, Some(0) | None) && !empty(&exif.digitized) {
            created = timestamp(&wp_exif_date2ts(&exif.digitized))?;
        }
        if let Some(n) = exif.focal_length {
            focal = float_to_string(n);
        }
        if let Some(n) = exif.iso.filter(|&n| n != 0) {
            iso = n.to_string().into_bytes();
        }
        if let Some(n) = exif.exposure {
            shutter = float_to_string(n);
        }
        if let Some(n) = exif.orientation.filter(|&n| n != 0) {
            orientation = n.to_string();
        }
    }
    let kses = |text: Vec<u8>| Value::String(wp_kses_post(&utf8(text), upload_host));
    let text = |text: String| kses(text.into_bytes());
    let created = created.map_or(String::new(), |t| t.to_string());
    let keywords = keywords
        .into_iter()
        .enumerate()
        .map(|(i, k)| (Key::Int(i as i64), kses(k)))
        .collect();
    let fields = [
        ("aperture", text(aperture)),
        ("credit", kses(credit)),
        ("camera", kses(camera)),
        ("caption", kses(caption)),
        ("created_timestamp", text(created)),
        ("copyright", kses(copyright)),
        ("focal_length", text(focal)),
        ("iso", kses(iso)),
        ("shutter_speed", text(shutter)),
        ("title", kses(title)),
        ("orientation", text(orientation)),
        ("keywords", Value::Array(keywords)),
        ("alt", Value::String(Vec::new())),
    ];
    Ok(fields
        .into_iter()
        .map(|(k, v)| (Key::String(k.into()), v))
        .collect())
}

#[cfg(test)]
mod tests;
