// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::corpus;

fn pixels(width: u32, height: u32, channels: usize) -> Pixels {
    let data = (0..width as usize * height as usize * channels)
        .map(|i| (i * 7 % 256) as u8)
        .collect();
    Pixels {
        width,
        height,
        channels,
        data,
    }
}

#[test]
fn limits_default_to_the_oracle_policy() {
    let limits = Limits::default();
    assert_eq!(
        (
            limits.bytes,
            limits.width,
            limits.height,
            limits.area,
            limits.memory
        ),
        (64 << 20, 32_000, 32_000, 256_000_000, 1 << 30)
    );
    // A 24 megapixel photo fits; the area or a side over the policy does not.
    assert!(limits.allows(6000, 4000, 3));
    assert!(!limits.allows(32_001, 10, 3));
    assert!(!limits.allows(10, 32_001, 3));
    let wide = Limits {
        memory: u64::MAX,
        ..limits
    };
    assert!(wide.allows(16_000, 16_000, 3));
    assert!(!wide.allows(16_001, 16_000, 3));
    // The memory limit counts the source's depth and the working copies.
    let tight = Limits {
        memory: 100 * (3 + WORKING),
        ..limits
    };
    assert!(tight.allows(10, 10, 3));
    assert!(!tight.allows(10, 10, 4));
    assert!(!tight.allows(11, 10, 3));
}

#[test]
fn holds_each_format_to_its_depth_and_the_file_size() {
    let area = 400 * 300;
    for (name, depth) in [
        ("rgb.png", 8),
        ("still.gif", 1),
        ("lossy.webp", 4),
        ("photo.bmp", 4),
    ] {
        let bytes = corpus(name);
        let kind = crate::header::image_type(&bytes).unwrap();
        let size = crate::header::getimagesize(&bytes).unwrap();
        let area = u64::from(size.width) * u64::from(size.height);
        let fits = Limits {
            memory: area * (depth + WORKING),
            ..Limits::default()
        };
        assert_ne!(decode(&bytes, kind, &fits), Err(Error::TooLarge), "{name}");
        let short = Limits {
            memory: fits.memory - 1,
            ..fits
        };
        assert_eq!(decode(&bytes, kind, &short), Err(Error::TooLarge), "{name}");
    }
    assert!(area > 0);
    let photo = corpus("small.jpg");
    let exact = Limits {
        bytes: photo.len() as u64,
        ..Limits::default()
    };
    assert!(decode(&photo, ImageType::Jpeg, &exact).is_ok());
    let under = Limits {
        bytes: photo.len() as u64 - 1,
        ..exact
    };
    assert_eq!(
        decode(&photo, ImageType::Jpeg, &under),
        Err(Error::TooLarge)
    );
}

#[test]
fn holds_a_gif_frame_larger_than_its_screen_to_the_limits() {
    let mut gif = Vec::new();
    let mut encoder = gif::Encoder::new(&mut gif, 1, 1, &[0, 0, 0, 255, 255, 255]).unwrap();
    let frame = gif::Frame {
        width: 40,
        height: 1,
        buffer: vec![1; 40].into(),
        ..gif::Frame::default()
    };
    encoder.write_frame(&frame).unwrap();
    drop(encoder);
    assert!(decode(&gif, ImageType::Gif, &Limits::default()).is_ok());
    let narrow = Limits {
        width: 20,
        ..Limits::default()
    };
    assert_eq!(decode(&gif, ImageType::Gif, &narrow), Err(Error::TooLarge));
    // Its buffer is refused by the decoder past the memory's pixels.
    let small = Limits {
        memory: 30 * (4 + WORKING),
        ..Limits::default()
    };
    assert_eq!(
        decode(&gif, ImageType::Gif, &small),
        Err(Error::Undecodable)
    );
    let mut twenty = Vec::new();
    let mut encoder = gif::Encoder::new(&mut twenty, 1, 1, &[0, 0, 0, 255, 255, 255]).unwrap();
    let frame = gif::Frame {
        width: 20,
        height: 1,
        buffer: vec![1; 20].into(),
        ..gif::Frame::default()
    };
    encoder.write_frame(&frame).unwrap();
    drop(encoder);
    assert!(decode(&twenty, ImageType::Gif, &small).is_ok());
    // The area limit bounds it too.
    let few = Limits {
        area: 10,
        ..Limits::default()
    };
    assert_eq!(
        decode(&twenty, ImageType::Gif, &few),
        Err(Error::Undecodable)
    );
}

/// The transparent index of a GIF's first frame.
fn transparent(gif: &[u8]) -> Option<u8> {
    let mut decoder = gif::DecodeOptions::new().read_info(gif).unwrap();
    decoder.read_next_frame().unwrap().unwrap().transparent
}

#[test]
fn keeps_grey_and_half_transparency_in_a_gif() {
    let grey = Pixels {
        width: 3,
        height: 1,
        channels: 1,
        data: vec![10, 128, 250],
    };
    let gif = encode(&grey, "image/gif", 82, &Kept::default()).unwrap();
    let back = decode(&gif, ImageType::Gif, &Limits::default())
        .unwrap()
        .pixels;
    for pixel in back.data.chunks(4) {
        assert!(pixel[0] == pixel[1] && pixel[1] == pixel[2], "{pixel:?}");
    }
    // Alpha under 128 is transparent, 128 and over opaque.
    let edge = Pixels {
        width: 2,
        height: 1,
        channels: 2,
        data: vec![50, 127, 200, 128],
    };
    let gif = encode(&edge, "image/gif", 82, &Kept::default()).unwrap();
    let back = decode(&gif, ImageType::Gif, &Limits::default())
        .unwrap()
        .pixels;
    assert_eq!((back.data[3], back.data[7]), (0, 255));
    assert_eq!(transparent(&gif), Some(255));
    let opaque = Pixels {
        width: 1,
        height: 1,
        channels: 2,
        data: vec![50, 128],
    };
    let gif = encode(&opaque, "image/gif", 82, &Kept::default()).unwrap();
    assert_eq!(transparent(&gif), None);
    assert_eq!(
        decode(&gif, ImageType::Gif, &Limits::default())
            .unwrap()
            .pixels
            .data[3],
        255
    );
}

#[test]
fn refuses_what_the_limits_or_decoders_refuse() {
    let small = Limits {
        bytes: 1 << 20,
        width: 100,
        height: 100,
        area: 10_000,
        memory: 1 << 30,
    };
    for name in ["photo.jpg", "rgb.png", "still.gif", "lossy.webp"] {
        let bytes = corpus(name);
        let kind = crate::header::image_type(&bytes).unwrap();
        assert_eq!(decode(&bytes, kind, &small), Err(Error::TooLarge), "{name}");
        assert!(decode(&bytes, kind, &Limits::default()).is_ok(), "{name}");
        let cut = &bytes[..bytes.len() / 3];
        assert!(
            decode(&cut[..60], kind, &Limits::default()).is_err(),
            "{name}"
        );
    }
    let photo = corpus("photo.jpg");
    assert_eq!(
        decode(&photo, ImageType::Jpeg, &Limits { bytes: 10, ..small }),
        Err(Error::TooLarge)
    );
    assert_eq!(
        decode(&photo, ImageType::Avif, &Limits::default()),
        Err(Error::Undecodable)
    );
    assert_eq!(
        decode(b"GIF89a", ImageType::Gif, &Limits::default()),
        Err(Error::Undecodable)
    );
    assert_eq!(
        decode(
            &corpus("still.gif"),
            ImageType::Gif,
            &Limits { memory: 0, ..small }
        ),
        Err(Error::TooLarge)
    );
    assert_eq!(
        decode(b"\x89PNG\r\n\x1a\n", ImageType::Png, &Limits::default()),
        Err(Error::Undecodable)
    );
    assert_eq!(
        decode(b"RIFF\0\0\0\0WEBP", ImageType::Webp, &Limits::default()),
        Err(Error::Undecodable)
    );
    // A GIF without a frame.
    assert_eq!(
        decode(
            b"GIF89a\x01\x00\x01\x00\x00\x00\x00;",
            ImageType::Gif,
            &Limits::default()
        ),
        Err(Error::Undecodable)
    );
}

/// The outcomes of decoding each prefix of a corpus file past its header.
fn cut_short(name: &str) -> Vec<Result<(), Error>> {
    let bytes = corpus(name);
    let kind = crate::header::image_type(&bytes).unwrap();
    (40..bytes.len())
        .step_by(bytes.len() / 60)
        .map(|cut| decode(&bytes[..cut], kind, &Limits::default()).map(|_| ()))
        .collect()
}

#[test]
fn refuses_files_cut_short() {
    for name in [
        "photo.jpg",
        "progressive.jpg",
        "rgb.png",
        "still.gif",
        "anim.gif",
        "lossy.webp",
        "lossless.webp",
        "anim.webp",
    ] {
        assert!(cut_short(name).contains(&Err(Error::Undecodable)), "{name}");
    }
    // Headers that pass the size check but not the decoder's.
    let mut photo = corpus("small.jpg");
    let dqt = photo.windows(2).position(|w| w == [0xFF, 0xDB]).unwrap();
    photo[dqt + 4] = 0x7F;
    assert_eq!(
        decode(&photo, ImageType::Jpeg, &Limits::default()),
        Err(Error::Undecodable)
    );
    let mut webp = corpus("lossy.webp");
    webp[12..16].copy_from_slice(b"JUNK");
    webp[29] = 1;
    assert_eq!(
        decode(&webp, ImageType::Webp, &Limits::default()),
        Err(Error::Undecodable)
    );
}

/// A PNG chunk with its CRC.
fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let crc = kind.iter().chain(data).fold(!0u32, |mut crc, &b| {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 == 1 {
                crc >> 1 ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
        crc
    });
    [
        &(data.len() as u32).to_be_bytes()[..],
        kind,
        data,
        &(!crc).to_be_bytes(),
    ]
    .concat()
}

#[test]
fn carries_a_png_profile() {
    let png = corpus("rgb.png");
    // A stored (uncompressed) zlib stream of the profile.
    let profile = b"profile";
    let mut zlib = vec![
        0x78,
        0x01,
        1,
        profile.len() as u8,
        0,
        !(profile.len() as u8),
        0xFF,
    ];
    zlib.extend(profile);
    let adler = profile.iter().fold((1u32, 0u32), |(a, b), &v| {
        ((a + u32::from(v)) % 65521, (b + a + u32::from(v)) % 65521)
    });
    zlib.extend((adler.1 << 16 | adler.0).to_be_bytes());
    let iccp = chunk(b"iCCP", &[b"icc\0\0".as_slice(), &zlib].concat());
    let with = [&png[..33], &iccp, &png[33..]].concat();
    let decoded = decode(&with, ImageType::Png, &Limits::default()).unwrap();
    assert_eq!(decoded.kept.icc.as_deref(), Some(profile.as_slice()));
}

#[test]
fn decodes_each_format_to_eight_bits() {
    let decoded = |name: &str| {
        let bytes = corpus(name);
        let d = decode(
            &bytes,
            crate::header::image_type(&bytes).unwrap(),
            &Limits::default(),
        )
        .unwrap();
        (
            d.pixels.width,
            d.pixels.height,
            d.pixels.channels,
            d.kept.indexed,
        )
    };
    assert_eq!(decoded("photo.jpg"), (640, 480, 3, false));
    assert_eq!(decoded("gray.jpg"), (400, 300, 1, false));
    assert_eq!(decoded("cmyk.jpg"), (400, 300, 3, false));
    assert_eq!(decoded("rgb16.png"), (200, 150, 3, false));
    assert_eq!(decoded("gray.png"), (400, 300, 1, false));
    assert_eq!(decoded("indexed.png"), (400, 300, 3, true));
    assert_eq!(decoded("rgba.png"), (400, 300, 4, false));
    assert_eq!(decoded("anim.gif"), (320, 240, 4, false));
    assert_eq!(decoded("lossless.webp"), (400, 300, 3, false));
    assert_eq!(decoded("alpha.webp"), (400, 300, 4, false));
    assert_eq!(decoded("anim.webp"), (320, 240, 3, false));
}

#[test]
fn encodes_each_format() {
    let kept = Kept::default();
    for (mime, channels) in [
        ("image/jpeg", 1),
        ("image/jpeg", 3),
        ("image/png", 1),
        ("image/png", 2),
        ("image/png", 3),
        ("image/png", 4),
        ("image/gif", 1),
        ("image/gif", 2),
        ("image/gif", 4),
        ("image/webp", 1),
        ("image/webp", 2),
        ("image/webp", 3),
        ("image/webp", 4),
    ] {
        let image = pixels(20, 10, channels);
        let bytes = encode(&image, mime, 82, &kept).unwrap();
        let kind = crate::header::image_type(&bytes).unwrap();
        assert_eq!(kind.mime(), mime);
        let back = decode(&bytes, kind, &Limits::default()).unwrap().pixels;
        assert_eq!((back.width, back.height), (20, 10), "{mime} {channels}");
    }
    // Indexed again, with the alpha of each palette entry; a GIF's mostly
    // transparent pixels share its transparent entry.
    let indexed = Kept {
        icc: None,
        indexed: true,
    };
    let png = encode(&pixels(20, 10, 4), "image/png", 82, &indexed).unwrap();
    assert_eq!(png.get(25), Some(&3));
    assert!(png.windows(4).any(|w| w == b"tRNS"));
    let opaque = encode(&pixels(20, 10, 3), "image/png", 82, &indexed).unwrap();
    assert!(!opaque.windows(4).any(|w| w == b"tRNS"));
    let clear = Pixels {
        width: 2,
        height: 1,
        channels: 4,
        data: vec![9, 9, 9, 0, 200, 0, 0, 255],
    };
    let gif = encode(&clear, "image/gif", 82, &Kept::default()).unwrap();
    let back = decode(&gif, ImageType::Gif, &Limits::default())
        .unwrap()
        .pixels;
    assert_eq!(back.data[3], 0);
    assert_eq!(back.data[7], 255);
    // The ICC profile is carried in JPEG and WebP.
    let icc = Kept {
        icc: Some(b"profile".to_vec()),
        indexed: false,
    };
    let jpeg = encode(&pixels(8, 8, 3), "image/jpeg", 82, &icc).unwrap();
    assert!(jpeg.windows(11).any(|w| w == b"ICC_PROFILE"));
    let webp = encode(&pixels(8, 8, 3), "image/webp", 82, &icc).unwrap();
    assert!(webp.windows(4).any(|w| w == b"ICCP"));
    // Chroma is halved below quality 90 only.
    let sampling = |quality| {
        let jpeg = encode(&pixels(16, 16, 3), "image/jpeg", quality, &Kept::default()).unwrap();
        let sof = jpeg.windows(2).position(|w| w == [0xFF, 0xC0]).unwrap();
        jpeg[sof + 11]
    };
    assert_eq!((sampling(89), sampling(90)), (0x22, 0x11));
}

#[test]
fn fails_where_a_format_cannot_hold_the_image() {
    let kept = Kept::default();
    let wide = Pixels {
        width: 70_000,
        height: 1,
        channels: 1,
        data: vec![0; 70_000],
    };
    assert_eq!(
        encode(&wide, "image/jpeg", 82, &kept),
        Err(Error::Unwritable)
    );
    assert_eq!(
        encode(&wide, "image/gif", 82, &kept),
        Err(Error::Unwritable)
    );
    assert_eq!(
        encode(&wide, "image/webp", 82, &kept),
        Err(Error::Unwritable)
    );
    let tall = Pixels {
        width: 1,
        height: 70_000,
        channels: 1,
        data: vec![0; 70_000],
    };
    assert_eq!(
        encode(&tall, "image/jpeg", 82, &kept),
        Err(Error::Unwritable)
    );
    assert_eq!(
        encode(&tall, "image/gif", 82, &kept),
        Err(Error::Unwritable)
    );
    // A profile larger than JPEG's markers can carry.
    let huge = Kept {
        icc: Some(vec![0; 17 << 20]),
        indexed: false,
    };
    assert_eq!(
        encode(&pixels(8, 8, 3), "image/jpeg", 82, &huge),
        Err(Error::Unwritable)
    );
    // PNG data of the wrong length.
    let short = Pixels {
        width: 8,
        height: 8,
        channels: 3,
        data: vec![0; 10],
    };
    assert_eq!(
        encode(&short, "image/png", 82, &kept),
        Err(Error::Unwritable)
    );
    let empty = Pixels {
        width: 0,
        height: 0,
        channels: 3,
        data: Vec::new(),
    };
    assert_eq!(
        encode(&empty, "image/png", 82, &kept),
        Err(Error::Unwritable)
    );
}
