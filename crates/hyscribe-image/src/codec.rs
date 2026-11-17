// SPDX-License-Identifier: AGPL-3.0-or-later

//! Decoding and encoding with the codec crates (ADR 0015), SIMD off, each
//! decoder's own reading of the size held to the header's and the
//! [`Limits`] before a pixel buffer is allocated: `zune-jpeg`, `png`, `gif`
//! and `image-webp` in, `jpeg-encoder`, `png`, `gif` (through `color_quant`)
//! and `image-webp`'s lossless encoder out.

use std::io::Cursor;

use color_quant::NeuQuant;
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;
use zune_jpeg::zune_core::colorspace::ColorSpace;
use zune_jpeg::zune_core::options::DecoderOptions;

use crate::header::ImageType;
use crate::pixels::Pixels;

/// The limits an image is held to (ADR 0015), the `.env` settings of the
/// same names; the defaults are the oracle's ImageMagick policy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// `IMAGE_MAX_BYTES`: the largest file decoded.
    pub bytes: u64,
    /// `IMAGE_MAX_WIDTH`.
    pub width: u32,
    /// `IMAGE_MAX_HEIGHT`.
    pub height: u32,
    /// `IMAGE_MAX_AREA`: width times height.
    pub area: u64,
    /// `IMAGE_MAX_MEMORY`: one job's pixel buffers, in bytes.
    pub memory: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            bytes: 64 << 20,
            width: 32_000,
            height: 32_000,
            area: 256_000_000,
            memory: 1 << 30,
        }
    }
}

impl Limits {
    /// Whether an image `width` by `height`, `depth` bytes a pixel at its
    /// source's bit depth, may be decoded: its source buffer and what the
    /// editor works with fit the memory limit, [`WORKING`] bytes a pixel.
    fn allows(&self, width: u32, height: u32, depth: u64) -> bool {
        let area = u64::from(width) * u64::from(height);
        width <= self.width
            && height <= self.height
            && area <= self.area
            && area
                .checked_mul(depth + WORKING)
                .is_some_and(|bytes| bytes <= self.memory)
    }
}

/// The bytes a source pixel costs the editor at most: its eight bit copy
/// and the copy kept while a size is made (8), a point sampled reduction
/// (under 2.8 times the area, 12), and a filter pass in floats (16).
pub const WORKING: u64 = 36;

/// What a PNG decoder may allocate itself, its metadata (`iCCP`, `zTXt`,
/// `iTXt`) inflated and a row, apart from the pixels' memory: 16 MiB.
pub const METADATA: usize = 16 << 20;

/// Why an image is not edited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Over a limit.
    TooLarge,
    /// No codec for its type here (AVIF, BMP, HEIC), or its decoder
    /// rejects it.
    Undecodable,
    /// Its encoder fails, as on a JPEG over 65,535 pixels a side.
    Unwritable,
}

/// A decoded image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    /// The pixels of the first frame.
    pub pixels: Pixels,
    /// What its sizes keep of it.
    pub kept: Kept,
}

/// What an image's sizes keep of the source.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Kept {
    /// The ICC profile, which is carried, not applied.
    pub icc: Option<Vec<u8>>,
    /// A palette PNG, whose sizes are indexed again.
    pub indexed: bool,
}

/// Decodes the first frame of `bytes` of `kind` within `limits`, checked
/// against the size in the header before the decoder starts.
pub fn decode(bytes: &[u8], kind: ImageType, limits: &Limits) -> Result<Decoded, Error> {
    if bytes.len() as u64 > limits.bytes {
        return Err(Error::TooLarge);
    }
    let size = crate::header::getimagesize(bytes).ok_or(Error::Undecodable)?;
    // The decoded source's bytes a pixel: 16 bit PNGs' twice, a GIF's RGBA.
    let depth = match kind {
        ImageType::Png => 8,
        _ => 4,
    };
    check(limits, size.width, size.height, depth)?;
    let header = Header {
        limits,
        size: (size.width, size.height),
        depth,
    };
    match kind {
        ImageType::Jpeg => jpeg(bytes, &header),
        ImageType::Png => png(bytes, &header),
        ImageType::Gif => gif(bytes, &header),
        ImageType::Webp => webp(bytes, &header),
        _ => Err(Error::Undecodable),
    }
}

fn check(limits: &Limits, width: u32, height: u32, depth: u64) -> Result<(), Error> {
    limits
        .allows(width, height, depth)
        .then_some(())
        .ok_or(Error::TooLarge)
}

/// The header's size, checked, which a decoder's reading must match.
struct Header<'a> {
    limits: &'a Limits,
    size: (u32, u32),
    depth: u64,
}

impl Header<'_> {
    /// The size a decoder reads, held to the limits, then refused unless
    /// it is the header's: two parsers that disagree are not trusted.
    fn agrees(&self, (width, height): (u32, u32)) -> Result<(), Error> {
        check(self.limits, width, height, self.depth)?;
        (self.size == (width, height))
            .then_some(())
            .ok_or(Error::Undecodable)
    }
}

fn jpeg(bytes: &[u8], header: &Header) -> Result<Decoded, Error> {
    let options = DecoderOptions::default()
        .set_max_width(header.limits.width as usize)
        .set_max_height(header.limits.height as usize);
    let mut decoder = JpegDecoder::new_with_options(ZCursor::new(bytes), options);
    decoder.set_options(options.jpeg_set_out_colorspace(ColorSpace::RGB));
    let data = decoder
        .decode_headers()
        .map_err(|_| Error::Undecodable)
        .and_then(|()| {
            // Each side is a u16 in the frame header.
            let (width, height) = decoder.dimensions().unwrap_or_default();
            header.agrees((width as u32, height as u32))
        })
        .and_then(|()| decoder.decode().ok().ok_or(Error::Undecodable))?;
    let (width, height) = header.size;
    let icc = decoder.icc_profile();
    // A grey JPEG stays grey: one of the three equal samples.
    let (channels, data) = match decoder.input_colorspace() {
        Some(ColorSpace::Luma) => (1, data.iter().step_by(3).copied().collect()),
        _ => (3, data),
    };
    Ok(Decoded {
        pixels: Pixels {
            width,
            height,
            channels,
            data,
        },
        kept: Kept {
            icc,
            indexed: false,
        },
    })
}

fn png(bytes: &[u8], header: &Header) -> Result<Decoded, Error> {
    // The pixels' buffer is allocated here, once its size agrees.
    let png_limits = png::Limits { bytes: METADATA };
    let mut decoder = png::Decoder::new_with_limits(Cursor::new(bytes), png_limits);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder
        .read_info()
        .map_err(|_| Error::Undecodable)
        .and_then(|reader| {
            let size = (reader.info().width, reader.info().height);
            header.agrees(size).map(|()| reader)
        })?;
    let indexed = reader.info().color_type == png::ColorType::Indexed;
    let icc = reader.info().icc_profile.as_ref().map(|icc| icc.to_vec());
    let mut data = vec![0; reader.output_buffer_size().unwrap_or(0)];
    let out = reader
        .next_frame(&mut data)
        .map_err(|_| Error::Undecodable)?;
    data.truncate(out.buffer_size());
    let channels = out.color_type.samples();
    Ok(Decoded {
        pixels: Pixels {
            width: out.width,
            height: out.height,
            channels,
            data,
        },
        kept: Kept { icc, indexed },
    })
}

fn gif(bytes: &[u8], header: &Header) -> Result<Decoded, Error> {
    let mut options = gif::DecodeOptions::new();
    options.set_color_output(gif::ColorOutput::RGBA);
    let mut decoder = options
        .read_info(Cursor::new(bytes))
        .map_err(|_| Error::Undecodable)
        .and_then(|decoder| {
            let screen = (u32::from(decoder.width()), u32::from(decoder.height()));
            header.agrees(screen).map(|()| decoder)
        })?;
    // A frame may be larger than the screen: its RGBA buffer is checked
    // before it is allocated.
    let (width, height) = decoder
        .next_frame_info()
        .ok()
        .flatten()
        .map(|frame| (u32::from(frame.width), u32::from(frame.height)))
        .ok_or(Error::Undecodable)
        .and_then(|(width, height)| {
            check(header.limits, width, height, header.depth).map(|()| (width, height))
        })?;
    let mut data = vec![0; decoder.buffer_size()];
    decoder
        .read_into_buffer(&mut data)
        .map_err(|_| Error::Undecodable)?;
    Ok(Decoded {
        pixels: Pixels {
            width,
            height,
            channels: 4,
            data,
        },
        kept: Kept::default(),
    })
}

fn webp(bytes: &[u8], header: &Header) -> Result<Decoded, Error> {
    let mut decoder = image_webp::WebPDecoder::new(Cursor::new(bytes))
        .map_err(|_| Error::Undecodable)
        .and_then(|decoder| header.agrees(decoder.dimensions()).map(|()| decoder))?;
    decoder.set_memory_limit(usize::try_from(header.limits.memory).unwrap_or(usize::MAX));
    let (width, height) = decoder.dimensions();
    let icc = decoder.icc_profile().ok().flatten();
    let mut data = vec![0; decoder.output_buffer_size().unwrap_or(0)];
    let read = match decoder.is_animated() {
        true => decoder.read_frame(&mut data).map(|_| ()),
        false => decoder.read_image(&mut data),
    };
    read.map_err(|_| Error::Undecodable)?;
    let channels = if decoder.has_alpha() { 4 } else { 3 };
    Ok(Decoded {
        pixels: Pixels {
            width,
            height,
            channels,
            data,
        },
        kept: Kept {
            icc,
            indexed: false,
        },
    })
}

/// Encodes `pixels` as `mime` at `quality`, with what is `kept`.
pub fn encode(pixels: &Pixels, mime: &str, quality: u8, kept: &Kept) -> Result<Vec<u8>, Error> {
    let mut out = Vec::new();
    let written = match mime {
        "image/jpeg" => encode_jpeg(&mut out, pixels, quality, kept.icc.as_deref()),
        "image/png" => encode_png(&mut out, pixels, kept.indexed),
        "image/gif" => encode_gif(&mut out, pixels),
        _ => encode_webp(&mut out, pixels, kept.icc.as_deref()),
    };
    written.map(|()| out).ok_or(Error::Unwritable)
}

/// JPEG at `quality`, chroma halved both ways below 90 as ImageMagick
/// does, baseline, standard Huffman tables (`zune-jpeg` misreads optimised ones).
fn encode_jpeg(out: &mut Vec<u8>, pixels: &Pixels, quality: u8, icc: Option<&[u8]>) -> Option<()> {
    let mut encoder = jpeg_encoder::Encoder::new(out, quality);
    let sampling = if quality < 90 {
        jpeg_encoder::SamplingFactor::F_2_2
    } else {
        jpeg_encoder::SamplingFactor::F_1_1
    };
    encoder.set_sampling_factor(sampling);
    encoder.set_optimized_huffman_tables(false);
    if let Some(icc) = icc {
        encoder.add_icc_profile(icc).ok()?;
    }
    let colour = match pixels.channels {
        1 => jpeg_encoder::ColorType::Luma,
        _ => jpeg_encoder::ColorType::Rgb,
    };
    let (width, height) = (
        u16::try_from(pixels.width).ok()?,
        u16::try_from(pixels.height).ok()?,
    );
    encoder.encode(&pixels.data, width, height, colour).ok()
}

/// `pixels` as RGBA.
fn rgba(pixels: &Pixels) -> Vec<u8> {
    let at = |i: usize| pixels.data.get(i).copied().unwrap_or(0);
    let channels = pixels.channels;
    let alpha = |i: usize| {
        if pixels.has_alpha() {
            at(i + channels - 1)
        } else {
            255
        }
    };
    (0..pixels.data.len() / channels)
        .flat_map(|p| {
            let i = p * channels;
            match channels {
                1 | 2 => [at(i), at(i), at(i), alpha(i)],
                _ => [at(i), at(i + 1), at(i + 2), alpha(i)],
            }
        })
        .collect()
}

/// A PNG's palette, its alphas (`tRNS`, without the opaque entries that
/// end it) and its indexes: at most 256 colours with their alpha.
fn quantise_png(pixels: &Pixels) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let rgba = rgba(pixels);
    let quant = NeuQuant::new(1, 256, &rgba);
    let indexes = rgba.chunks(4).map(|p| quant.index_of(p) as u8).collect();
    let map = quant.color_map_rgba();
    let palette = map
        .chunks(4)
        .flat_map(|c| c.iter().take(3).copied())
        .collect();
    let mut alphas: Vec<u8> = map.chunks(4).filter_map(|c| c.get(3).copied()).collect();
    while alphas.last() == Some(&255) {
        alphas.pop();
    }
    (palette, alphas, indexes)
}

/// A GIF's palette, indexes and transparent index: at most 255 colours and
/// one transparent entry for the mostly transparent pixels.
fn quantise_gif(pixels: &Pixels) -> (Vec<u8>, Vec<u8>, Option<u8>) {
    let rgba = rgba(pixels);
    let transparent = rgba.chunks(4).any(|p| p.get(3).is_some_and(|&a| a < 128));
    let colours = if transparent { 255 } else { 256 };
    let quant = NeuQuant::new(1, colours, &rgba);
    let clear = transparent.then_some(255);
    let indexes = rgba
        .chunks(4)
        .map(|p| match p.get(3) {
            Some(&a) if transparent && a < 128 => 255,
            _ => quant.index_of(p) as u8,
        })
        .collect();
    let mut palette = quant.color_map_rgb();
    if transparent {
        palette.extend([0, 0, 0]);
    }
    (palette, indexes, clear)
}

fn encode_png(out: &mut Vec<u8>, pixels: &Pixels, indexed: bool) -> Option<()> {
    let mut encoder = png::Encoder::new(out, pixels.width, pixels.height);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let data = if indexed {
        let (palette, alphas, indexes) = quantise_png(pixels);
        encoder.set_color(png::ColorType::Indexed);
        if pixels.has_alpha() && !alphas.is_empty() {
            encoder.set_trns(alphas);
        }
        encoder.set_palette(palette);
        indexes
    } else {
        encoder.set_color(match pixels.channels {
            1 => png::ColorType::Grayscale,
            2 => png::ColorType::GrayscaleAlpha,
            3 => png::ColorType::Rgb,
            _ => png::ColorType::Rgba,
        });
        pixels.data.clone()
    };
    let mut writer = encoder.write_header().ok()?;
    writer.write_image_data(&data).ok()?;
    writer.finish().ok()
}

fn encode_gif(out: &mut Vec<u8>, pixels: &Pixels) -> Option<()> {
    let (width, height) = (
        u16::try_from(pixels.width).ok()?,
        u16::try_from(pixels.height).ok()?,
    );
    let (palette, indexes, transparent) = quantise_gif(pixels);
    let frame = gif::Frame {
        width,
        height,
        transparent,
        buffer: indexes.into(),
        ..gif::Frame::default()
    };
    let encoder = gif::Encoder::new(out, width, height, &palette);
    encoder
        .and_then(|mut encoder| encoder.write_frame(&frame))
        .ok()
}

fn encode_webp(out: &mut Vec<u8>, pixels: &Pixels, icc: Option<&[u8]>) -> Option<()> {
    let mut encoder = image_webp::WebPEncoder::new(out);
    if let Some(icc) = icc {
        encoder.set_icc_profile(icc.to_vec());
    }
    let colour = match pixels.channels {
        1 => image_webp::ColorType::L8,
        2 => image_webp::ColorType::La8,
        3 => image_webp::ColorType::Rgb8,
        _ => image_webp::ColorType::Rgba8,
    };
    encoder
        .encode(&pixels.data, pixels.width, pixels.height, colour)
        .ok()
}

#[cfg(test)]
mod tests;
