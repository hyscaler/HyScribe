// SPDX-License-Identifier: AGPL-3.0-or-later

//! HyScribe's own image header reader: the type and size PHP's
//! `exif_imagetype()` and `getimagesize()` give for the formats WordPress
//! uploads, and the JPEG `APP13` segment `getimagesize()`'s `$image_info`
//! hands to `iptcparse()`. Written from the formats' specifications and
//! matched to PHP by `reference/uploads/header-golden.php`. Nothing is
//! decoded and nothing is allocated beyond the size of the input.

/// An `IMAGETYPE_*` constant WordPress reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageType {
    /// `IMAGETYPE_GIF`.
    Gif,
    /// `IMAGETYPE_JPEG`.
    Jpeg,
    /// `IMAGETYPE_PNG`.
    Png,
    /// `IMAGETYPE_BMP`.
    Bmp,
    /// `IMAGETYPE_TIFF_II`.
    TiffIi,
    /// `IMAGETYPE_TIFF_MM`.
    TiffMm,
    /// `IMAGETYPE_WEBP`.
    Webp,
    /// `IMAGETYPE_AVIF`.
    Avif,
    /// `IMAGETYPE_HEIF`, which WordPress gives a HEIC its editor opens.
    Heif,
}

impl ImageType {
    /// `image_type_to_mime_type()`, `image/heic` for a HEIF as
    /// `wp_getimagesize()` says.
    pub fn mime(self) -> &'static str {
        match self {
            ImageType::Gif => "image/gif",
            ImageType::Jpeg => "image/jpeg",
            ImageType::Png => "image/png",
            ImageType::Bmp => "image/bmp",
            ImageType::TiffIi | ImageType::TiffMm => "image/tiff",
            ImageType::Webp => "image/webp",
            ImageType::Avif => "image/avif",
            ImageType::Heif => "image/heic",
        }
    }
}

/// What `wp_getimagesize( $file, $info )` gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageSize<'a> {
    /// `[0]`.
    pub width: u32,
    /// `[1]`.
    pub height: u32,
    /// `[2]`.
    pub kind: ImageType,
    /// `$info['APP13']`, a JPEG's first `APP13` segment.
    pub app13: Option<&'a [u8]>,
}

fn be16(b: &[u8], at: usize) -> Option<u32> {
    let bytes = b.get(at..).and_then(<[u8]>::first_chunk::<2>);
    bytes.map(|&bytes| u16::from_be_bytes(bytes).into())
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    let bytes = b.get(at..).and_then(<[u8]>::first_chunk::<4>);
    bytes.map(|&bytes| u32::from_be_bytes(bytes))
}

/// The little endian number in `bytes`.
fn le(bytes: &[u8]) -> u32 {
    bytes.iter().rev().fold(0, |n, &b| n * 256 + u32::from(b))
}

fn byte(b: &[u8], at: usize) -> Option<u32> {
    b.get(at).map(|&v| u32::from(v))
}

/// `exif_imagetype()` for the types WordPress maps to an extension; any
/// other (`image/psd`, `image/vnd.microsoft.icon` and the rest) is `None`,
/// which `wp_check_filetype_and_ext()` treats as PHP treats those.
pub fn image_type(b: &[u8]) -> Option<ImageType> {
    b.get(2)?;
    Some(match b {
        [b'G', b'I', b'F', ..] => ImageType::Gif,
        [0xFF, 0xD8, 0xFF, ..] => ImageType::Jpeg,
        [0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1A, b'\n', ..] => ImageType::Png,
        [b'B', b'M', ..] => ImageType::Bmp,
        [b'I', b'I', b'*', 0, ..] => ImageType::TiffIi,
        [b'M', b'M', 0, b'*', ..] => ImageType::TiffMm,
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => ImageType::Webp,
        _ if isobmff_brand(b, &[b"avif", b"avis"]) => ImageType::Avif,
        _ => return None,
    })
}

/// `wp_get_image_mime()`: `exif_imagetype()`'s type, else a HEIC or HEIF
/// by the major brand of its `ftyp` box.
pub fn image_mime(b: &[u8]) -> Option<&'static str> {
    if let Some(kind) = image_type(b) {
        return Some(kind.mime());
    }
    match (b.get(4..8), b.get(8..12)) {
        (Some(b"ftyp"), Some(b"heic")) => Some("image/heic"),
        (Some(b"ftyp"), Some(b"heif")) => Some("image/heif"),
        _ => None,
    }
}

/// `wp_getimagesize( $file, $info )`, `None` for `false`. A HEIC's size is
/// its primary image's, turned as its `irot` turns it, as the editor reads
/// it.
pub fn getimagesize(b: &[u8]) -> Option<ImageSize<'_>> {
    let mut app13 = None;
    let (kind, size) = match image_type(b) {
        Some(kind) => (
            kind,
            match kind {
                ImageType::Gif => gif(b),
                ImageType::Jpeg => jpeg(b, &mut app13),
                ImageType::Png => png(b),
                ImageType::Bmp => bmp(b),
                ImageType::Webp => webp(b),
                ImageType::Avif => isobmff_size(b, false),
                _ => None,
            },
        ),
        None => (
            ImageType::Heif,
            image_mime(b).and_then(|_| isobmff_size(b, true)),
        ),
    };
    let (width, height) = size?;
    (width != 0 || height != 0).then_some(ImageSize {
        width,
        height,
        kind,
        app13,
    })
}

// PHP reads a fixed number of bytes for each format, and fails short of them.
fn gif(b: &[u8]) -> Option<(u32, u32)> {
    let h = b.first_chunk::<11>()?;
    Some((le(&h[6..8]), le(&h[8..10])))
}

fn png(b: &[u8]) -> Option<(u32, u32)> {
    let h = b.first_chunk::<25>()?;
    Some((
        le(&[h[19], h[18], h[17], h[16]]),
        le(&[h[23], h[22], h[21], h[20]]),
    ))
}

fn bmp(b: &[u8]) -> Option<(u32, u32)> {
    let h = b.first_chunk::<30>()?;
    match le(&h[14..18]) {
        12 => Some((le(&h[18..20]), le(&h[20..22]))),
        13..=64 | 108 | 124 => Some((le(&h[18..22]), le(&h[22..26]).cast_signed().unsigned_abs())),
        _ => None,
    }
}

fn webp(b: &[u8]) -> Option<(u32, u32)> {
    let h = b.first_chunk::<30>()?;
    match &h[12..16] {
        b"VP8 " => Some((le(&h[26..28]) & 0x3FFF, le(&h[28..30]) & 0x3FFF)),
        b"VP8L" => {
            let bits = le(&h[21..25]);
            Some(((bits & 0x3FFF) + 1, (bits >> 14 & 0x3FFF) + 1))
        }
        b"VP8X" => Some((le(&h[24..27]) + 1, le(&h[27..30]) + 1)),
        _ => None,
    }
}

/// A JPEG's size from its first start of frame, reading markers up to the
/// start of scan; its first `APP13` goes to `app13`.
fn jpeg<'a>(b: &'a [u8], app13: &mut Option<&'a [u8]>) -> Option<(u32, u32)> {
    let mut at = 2;
    let mut size = None;
    loop {
        // The next marker: past anything but 0xFF, then past fill bytes.
        while b.get(at).is_some_and(|&v| v != 0xFF) {
            at += 1;
        }
        while b.get(at) == Some(&0xFF) {
            at += 1;
        }
        let Some(&marker) = b.get(at) else {
            return size;
        };
        at += 1;
        if matches!(marker, 0xD9 | 0xDA) {
            return size;
        }
        let length = be16(b, at)? as usize;
        if length < 2 {
            return None;
        }
        match marker {
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) && size.is_none() => {
                // A field cut short reads as 0.
                let field = |at| be16(b, at).unwrap_or(0);
                size = Some((field(at + 5), field(at + 3)));
            }
            0xED if app13.is_none() => match b.get(at + 2..at + length) {
                Some(segment) => *app13 = Some(segment),
                None => return size,
            },
            _ => {}
        }
        at += length;
    }
}

/// The boxes of an ISO base media file's `data`: type and contents.
fn boxes(mut data: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    std::iter::from_fn(move || {
        let size = be32(data, 0)? as usize;
        let kind = data.get(4..8)?;
        let (start, size) = match size {
            0 => (8, data.len()),
            1 => (
                16,
                usize::try_from(u64::from_be_bytes(
                    *data.get(8..).and_then(<[u8]>::first_chunk::<8>)?,
                ))
                .unwrap_or(usize::MAX),
            ),
            _ => (8, size),
        };
        let body = data.get(start..size)?;
        data = data.get(size..).unwrap_or_default();
        Some((kind, body))
    })
}

/// Whether the file's first box is an `ftyp` naming one of `brands`, in
/// what there is of it.
fn isobmff_brand(b: &[u8], brands: &[&[u8; 4]]) -> bool {
    if b.get(4..8) != Some(b"ftyp") {
        return false;
    }
    let end = be32(b, 0).map_or(0, |size| size as usize).min(b.len());
    let body = b.get(8..end).unwrap_or_default();
    let named = |brand: &[u8]| brands.iter().any(|&known| known == brand);
    named(body.get(..4).unwrap_or_default())
        || body
            .get(8..)
            .unwrap_or_default()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|b| named(b))
}

/// The size in the `ispe` property of an AVIF's or HEIF's primary item,
/// with width and height swapped by a quarter `irot` where `turn` is set;
/// `None` without an `iinf` box or a `pixi` or `av1C` property, as
/// `libavifinfo` needs them. Malformed boxes are read as far as they go.
fn isobmff_size(b: &[u8], turn: bool) -> Option<(u32, u32)> {
    let (_, meta) = boxes(b).find(|(kind, _)| *kind == b"meta")?;
    let (mut primary, mut items) = (None, false);
    let (mut properties, mut associations) = (Vec::new(), Vec::new());
    for (kind, body) in boxes(meta.get(4..).unwrap_or_default()) {
        match kind {
            b"iinf" => items = true,
            b"pitm" if body.first() == Some(&0) => primary = be16(body, 4),
            b"pitm" => primary = be32(body, 4),
            b"iprp" => {
                for (kind, body) in boxes(body) {
                    match kind {
                        b"ipco" => properties = boxes(body).collect(),
                        b"ipma" => associations = ipma(body),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    let primary = primary.filter(|_| items)?;
    let mut size = None;
    let (mut quarter, mut depth) = (false, turn);
    let primary = associations.iter().filter(|(item, _)| *item == primary);
    let found = primary.filter_map(|(_, index)| properties.get(index.checked_sub(1)?));
    for (kind, body) in found {
        match *kind {
            b"ispe" => size = be32(body, 4).zip(be32(body, 8)),
            b"irot" => quarter = byte(body, 0).is_some_and(|r| r & 1 == 1),
            b"pixi" | b"av1C" => depth = true,
            _ => {}
        }
    }
    let (width, height) = size.filter(|_| depth)?;
    Some(if turn && quarter {
        (height, width)
    } else {
        (width, height)
    })
}

/// An `ipma` box's associations, item and property index (from 1), as
/// far as the box holds them.
fn ipma(body: &[u8]) -> Vec<(u32, usize)> {
    let wide_item = byte(body, 0) != Some(0);
    let wide_index = byte(body, 3).is_some_and(|flags| flags & 1 == 1);
    let mut out = Vec::new();
    let mut at = 8;
    for _ in 0..be32(body, 4).unwrap_or(0) {
        let item = if wide_item {
            be32(body, at)
        } else {
            be16(body, at)
        };
        at += if wide_item { 4 } else { 2 };
        let (Some(item), Some(n)) = (item, byte(body, at)) else {
            break;
        };
        at += 1;
        for _ in 0..n {
            let index = if wide_index {
                be16(body, at).map(|i| i & 0x7FFF)
            } else {
                byte(body, at).map(|i| i & 0x7F)
            };
            at += if wide_index { 2 } else { 1 };
            out.extend(index.map(|index| (item, index as usize)));
        }
    }
    out
}

#[cfg(test)]
mod tests;
