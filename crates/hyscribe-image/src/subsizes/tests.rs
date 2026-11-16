// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{decode, encode, md5};

use super::*;
use crate::codec::decode as decode_image;
use crate::header::image_type;
use crate::hooks::NoHooks;
use crate::pixels::Pixels;
use crate::tests::{corpus, golden};

/// The default install's sizes.
fn sizes() -> Vec<SubSize> {
    let option = |name: &str| {
        let value: &[u8] = match name {
            "thumbnail_size_w" | "thumbnail_size_h" => b"150",
            "thumbnail_crop" => b"1",
            "medium_size_w" | "medium_size_h" => b"300",
            "medium_large_size_w" => b"768",
            "medium_large_size_h" => b"0",
            "large_size_w" | "large_size_h" => b"1024",
            _ => b"",
        };
        value.to_vec()
    };
    wp_get_registered_image_subsizes(&option)
}

/// The metadata with each `filesize` checked against PHP's and blanked:
/// within half and double for JPEG, PNG and GIF; any for WebP (lossless
/// here, ADR 0015), and for 16 bit PNGs and CMYK JPEGs, which ImageMagick
/// writes as they are and HyScribe at 8 bits (ADR 0015) and in RGB.
fn banded(ours: &Value, php: &Value, name: &str) -> (Value, Value) {
    fn blank(value: &Value) -> Value {
        match value {
            Value::Array(entries) => Value::Array(
                entries
                    .iter()
                    .map(|(k, v)| match k {
                        Key::String(s) if s == b"filesize" => (k.clone(), Value::Null),
                        _ => (k.clone(), blank(v)),
                    })
                    .collect(),
            ),
            other => other.clone(),
        }
    }
    fn sizes(value: &Value, out: &mut Vec<i64>) {
        if let Value::Array(entries) = value {
            for (k, v) in entries {
                match (k, v) {
                    (Key::String(s), Value::Int(n)) if s == b"filesize" => out.push(*n),
                    _ => sizes(v, out),
                }
            }
        }
    }
    let (mut a, mut b) = (Vec::new(), Vec::new());
    sizes(ours, &mut a);
    sizes(php, &mut b);
    // The first is the upload's own, unless it was scaled or turned.
    let skip = usize::from(ours.get_str("original_image").is_none());
    for (ours, php) in a.iter().zip(&b).skip(skip) {
        let unbanded = [
            "lossless.webp",
            "lossy.webp",
            "alpha.webp",
            "anim.webp",
            "rgb16.png",
            "gray.png",
            "cmyk.jpg",
        ];
        if !unbanded.contains(&name) {
            assert!(
                *ours * 2 >= *php && *ours <= *php * 2,
                "{name}: filesize {ours} against {php}"
            );
        }
    }
    (blank(ours), blank(php))
}

/// PSNR in dB and the mean SSIM over 8 by 8 windows, of the grey levels.
fn similarity(a: &Pixels, b: &Pixels) -> (f64, f64) {
    // Over mid grey, where there is alpha.
    let over = |v: f64, a: u8| v * f64::from(a) / 255.0 + 128.0 * (1.0 - f64::from(a) / 255.0);
    let grey = |p: &Pixels| -> Vec<f64> {
        p.data
            .chunks(p.channels)
            .map(|px| match *px {
                [l] => f64::from(l),
                [l, a] => over(f64::from(l), a),
                [r, g, b] => 0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b),
                [r, g, b, a, ..] => over(
                    0.299 * f64::from(r) + 0.587 * f64::from(g) + 0.114 * f64::from(b),
                    a,
                ),
                [] => 0.0,
            })
            .collect()
    };
    let (x, y) = (grey(a), grey(b));
    let mse = x.iter().zip(&y).map(|(p, q)| (p - q).powi(2)).sum::<f64>() / x.len() as f64;
    let psnr = if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0 * 255.0 / mse).log10()
    };
    let (w, h) = (a.width as usize, a.height as usize);
    let (c1, c2) = ((0.01 * 255.0f64).powi(2), (0.03 * 255.0f64).powi(2));
    let mut total = (0.0, 0);
    for wy in (0..h.saturating_sub(7)).step_by(4) {
        for wx in (0..w.saturating_sub(7)).step_by(4) {
            let at = |v: &[f64], i: usize| v[(wy + i / 8) * w + wx + i % 8];
            let n = 64.0;
            let (mx, my) = (
                (0..64).map(|i| at(&x, i)).sum::<f64>() / n,
                (0..64).map(|i| at(&y, i)).sum::<f64>() / n,
            );
            let vx = (0..64).map(|i| (at(&x, i) - mx).powi(2)).sum::<f64>() / n;
            let vy = (0..64).map(|i| (at(&y, i) - my).powi(2)).sum::<f64>() / n;
            let cov = (0..64)
                .map(|i| (at(&x, i) - mx) * (at(&y, i) - my))
                .sum::<f64>()
                / n;
            total.0 += ((2.0 * mx * my + c1) * (2.0 * cov + c2))
                / ((mx * mx + my * my + c1) * (vx + vy + c2));
            total.1 += 1;
        }
    }
    (
        psnr,
        if total.1 == 0 {
            1.0
        } else {
            total.0 / f64::from(total.1)
        },
    )
}

/// What HyScribe makes of a corpus file, as the tests run it.
fn generated(name: &str) -> Generated {
    let bytes = corpus(name);
    let mime = image_type(&bytes).map_or("image/heic", |kind| kind.mime());
    let upload = Upload {
        bytes: &bytes,
        file: &format!("2026/11/{name}"),
        attachment: 0,
    };
    wp_generate_attachment_metadata(upload, mime, sizes(), &Limits::default(), &NoHooks, b"")
        .unwrap()
}

#[test]
fn makes_the_sizes_and_metadata_php_makes() {
    let pinned = include_str!("../../tests/golden/sizes.tsv");
    let mut hashes = Vec::new();
    let mut compared = 0;
    for (name, php) in golden("meta") {
        // ADR 0015: no AVIF or BMP sizes; WordPress makes no sizes of a TIFF
        // (the golden asks the editor directly).
        if [".avif", ".bmp", ".tif"]
            .iter()
            .any(|ext| name.ends_with(ext))
        {
            continue;
        }
        let made = generated(&name);
        let php = decode(&php).unwrap();
        let (ours, theirs) = banded(&made.meta, &php, &name);
        assert_eq!(
            String::from_utf8_lossy(&encode(&ours)),
            String::from_utf8_lossy(&encode(&theirs)),
            "{name}"
        );
        for saved in &made.files {
            let file = saved.path.rsplit('/').next().unwrap();
            let theirs = std::fs::read(format!(
                "{}/tests/golden/uploads/{file}",
                env!("CARGO_MANIFEST_DIR")
            ))
            .unwrap();
            let kind = image_type(&theirs).unwrap();
            let theirs = decode_image(&theirs, kind, &Limits::default())
                .unwrap()
                .pixels;
            let ours = decode_image(&saved.bytes, kind, &Limits::default())
                .unwrap()
                .pixels;
            assert_eq!(
                (ours.width, ours.height),
                (theirs.width, theirs.height),
                "{file}"
            );
            let (psnr, ssim) = similarity(&ours, &theirs);
            // Palettes are chosen apart (ImageMagick's octree, NeuQuant here):
            // their SSIM bound is 0.95 (ADR 0015's bounds, tuned in 6.3).
            let palette =
                file.ends_with(".gif") || ["indexed", "large"].iter().any(|n| file.starts_with(n));
            let bound = if palette { 0.95 } else { 0.97 };
            assert!(
                psnr >= 35.0 && ssim >= bound,
                "{file}: PSNR {psnr:.2} dB, SSIM {ssim:.4}"
            );
            hashes.push(format!(
                "{file}\t{}\t{psnr:.1}\t{ssim:.3}",
                md5(&saved.bytes)
            ));
        }
        compared += 1;
    }
    assert_eq!(compared, 42);
    assert_eq!(
        hashes.join("\n") + "\n",
        pinned,
        "HyScribe's own output changed"
    );
}

#[test]
fn resizes_dimensions_as_wordpress_does() {
    let golden = include_str!("../../tests/golden/uploads.tsv");
    let mut checked = 0;
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let int = |at: usize| fields[at].parse::<u32>().unwrap();
        match fields[0] {
            "dims" => {
                let crop = match fields[5] {
                    "false" => Crop::No,
                    "true" => Crop::centre(),
                    json => {
                        let (x, y) = json.trim_matches(['[', ']']).split_once(',').unwrap();
                        Crop::At(x.trim_matches('"').into(), y.trim_matches('"').into())
                    }
                };
                let ours = image_resize_dimensions((int(1), int(2)), (int(3), int(4)), &crop);
                let ours = ours.map_or("false".to_string(), |d| d.map(|n| n.to_string()).join(","));
                assert_eq!(ours, fields[6], "{line}");
            }
            "constrain" => {
                let int = |at: usize| i64::from(int(at));
                let (w, h) = constrain_dimensions(int(1), int(2), int(3), int(4));
                assert_eq!(format!("{w},{h}"), fields[5], "{line}");
            }
            _ => continue,
        }
        checked += 1;
    }
    assert_eq!(checked, 5 * 12 * 12 * 6);
}

#[test]
fn reads_the_registered_sizes_from_the_options() {
    let names: Vec<String> = sizes()
        .iter()
        .map(|s| format!("{} {}x{} {:?}", s.name, s.width, s.height, s.crop))
        .collect();
    assert_eq!(
        names,
        [
            "thumbnail 150x150 At(\"center\", \"center\")",
            "medium 300x300 No",
            "medium_large 768x0 No",
            "large 1024x1024 No",
            "1536x1536 1536x1536 No",
            "2048x2048 2048x2048 No"
        ]
    );
    // An unset size is left out; a crop of "0" is none; numbers are read
    // as PHP's (int) reads them.
    let option = |name: &str| match name {
        "thumbnail_size_w" => b" 12px".to_vec(),
        "thumbnail_crop" => b"0".to_vec(),
        _ => Vec::new(),
    };
    let sizes = wp_get_registered_image_subsizes(&option);
    assert_eq!(sizes.len(), 3);
    assert_eq!(
        (sizes[0].width, sizes[0].height, &sizes[0].crop),
        (12, 0, &Crop::No)
    );
}

/// Hooks that move the threshold, drop or keep the sizes, and mark the
/// metadata.
struct Moved(i64, bool);

impl Hooks for Moved {
    fn big_image_size_threshold(&self, _: i64, _: (u32, u32), _: &str, _: u64) -> i64 {
        self.0
    }

    fn intermediate_image_sizes_advanced(
        &self,
        sizes: Vec<SubSize>,
        _: &Value,
        _: u64,
    ) -> Vec<SubSize> {
        if self.1 { sizes } else { Vec::new() }
    }

    fn wp_generate_attachment_metadata(
        &self,
        meta: Value,
        attachment: u64,
        context: &str,
    ) -> Value {
        Value::Array(vec![
            (Key::Int(attachment as i64), Value::String(context.into())),
            (Key::Int(0), meta),
        ])
    }
}

fn with(name: &str, mime: &str, limits: &Limits, hooks: &dyn Hooks) -> Generated {
    let bytes = corpus(name);
    let upload = Upload {
        bytes: &bytes,
        file: &format!("2026/11/{name}"),
        attachment: 7,
    };
    wp_generate_attachment_metadata(upload, mime, sizes(), limits, hooks, b"").unwrap()
}

#[test]
fn applies_the_hooks() {
    // A lower threshold scales; a negative one gives no size to scale to.
    let scaled = with(
        "photo.jpg",
        "image/jpeg",
        &Limits::default(),
        &Moved(500, false),
    );
    assert_eq!(
        scaled.attached_file.as_deref(),
        Some("2026/11/photo-scaled.jpg")
    );
    assert_eq!((scaled.files.len(), scaled.files[0].width), (1, 500));
    assert_eq!(
        scaled.meta.get(&Key::Int(7)),
        Some(&Value::String(b"create".to_vec()))
    );
    let none = with(
        "photo.jpg",
        "image/jpeg",
        &Limits::default(),
        &Moved(-1, false),
    );
    assert_eq!((none.files.len(), none.attached_file), (0, None));
    // At the threshold an image is not scaled: a sideways one is turned.
    let at = with(
        "orient-6.jpg",
        "image/jpeg",
        &Limits::default(),
        &Moved(320, false),
    );
    assert_eq!(
        at.attached_file.as_deref(),
        Some("2026/11/orient-6-rotated.jpg")
    );
    // So for its height: a tall JPEG turned by its EXIF.
    let tall = corpus("tall.jpg");
    let exif =
        b"\xFF\xE1\x00\x22Exif\0\0MM\0*\0\0\0\x08\0\x01\x01\x12\0\x03\0\0\0\x01\0\x06\0\0\0\0\0\0";
    let turned = [&tall[..2], exif.as_slice(), &tall[2..]].concat();
    let upload = Upload {
        bytes: &turned,
        file: "tall.jpg",
        attachment: 0,
    };
    let made = wp_generate_attachment_metadata(
        upload,
        "image/jpeg",
        Vec::new(),
        &Limits::default(),
        &Moved(2700, false),
        b"",
    )
    .unwrap();
    assert_eq!(made.attached_file.as_deref(), Some("tall-rotated.jpg"));
    // Off, a big image keeps its size, and a sideways one is still turned.
    let big = with(
        "big-orient-6.jpg",
        "image/jpeg",
        &Limits::default(),
        &Moved(0, true),
    );
    assert_eq!(
        big.attached_file.as_deref(),
        Some("2026/11/big-orient-6-rotated.jpg")
    );
    assert_eq!((big.files[0].width, big.files[0].height), (260, 2600));
}

#[test]
fn keeps_what_no_editor_opens_without_sizes() {
    // ADR 0015: AVIF and BMP get no sizes; the rest of their metadata is
    // PHP's.
    for name in ["photo.avif", "photo.bmp"] {
        let made = generated(name);
        assert_eq!(
            (made.files.len(), made.unedited),
            (0, Some(Error::Undecodable)),
            "{name}"
        );
        let (_, php) = golden("meta").into_iter().find(|(n, _)| n == name).unwrap();
        let mut php = decode(&php).unwrap();
        if let Value::Array(entries) = &mut php {
            entries
                .iter_mut()
                .filter(|(k, _)| *k == Key::String(b"sizes".to_vec()))
                .for_each(|(_, v)| *v = Value::Array(Vec::new()));
        }
        assert_eq!(made.meta, php, "{name}");
    }
    // Over the limits: the size from the header and no sizes, as PHP keeps
    // huge.png; a JPEG to turn that is over them is not turned.
    assert_eq!(generated("huge.png").unedited, Some(Error::TooLarge));
    let small = Limits {
        area: 100,
        ..Limits::default()
    };
    let turned = with("orient-6.jpg", "image/jpeg", &small, &NoHooks);
    assert_eq!(
        (turned.files.len(), turned.attached_file, turned.unedited),
        (0, None, Some(Error::TooLarge))
    );
    let sized = with("photo.jpg", "image/jpeg", &small, &NoHooks);
    assert_eq!(
        (sized.files.len(), sized.unedited),
        (0, Some(Error::TooLarge))
    );
    // A TIFF, which WordPress does not display, and what is no image, have
    // their size in bytes alone; a HEIC is kept as its header says.
    for (name, mime) in [
        ("photo.tif", "image/tiff"),
        ("photo.jpg", "application/pdf"),
    ] {
        let made = with(name, mime, &Limits::default(), &NoHooks);
        let filesize = Value::Int(corpus(name).len() as i64);
        let meta = Value::Array(vec![(Key::String(b"filesize".to_vec()), filesize)]);
        assert_eq!((made.meta, made.files.len()), (meta, 0), "{name} {mime}");
    }
    // What the metadata reader does not port, the upload does not either.
    let mut xmp = corpus("small.jpg");
    xmp.extend(b"<x:xmpmeta>AltTextAccessibility</x:xmpmeta>");
    let upload = Upload {
        bytes: &xmp,
        file: "x.jpg",
        attachment: 0,
    };
    let unported = wp_generate_attachment_metadata(
        upload,
        "image/jpeg",
        sizes(),
        &Limits::default(),
        &NoHooks,
        b"",
    );
    assert_eq!(unported, Err(crate::meta::Unported));
    // Sizes named twice are made once, the last one kept.
    let twice = vec![
        SubSize {
            name: "a".into(),
            width: 100,
            height: 0,
            crop: Crop::No,
        },
        SubSize {
            name: "a".into(),
            width: 50,
            height: 0,
            crop: Crop::No,
        },
    ];
    let bytes = corpus("photo.jpg");
    let upload = Upload {
        bytes: &bytes,
        file: "photo.jpg",
        attachment: 0,
    };
    let made = wp_generate_attachment_metadata(
        upload,
        "image/jpeg",
        twice,
        &Limits::default(),
        &NoHooks,
        b"",
    )
    .unwrap();
    let sizes = made
        .meta
        .get_str("sizes")
        .and_then(Value::as_array)
        .unwrap();
    assert_eq!((sizes.len(), made.files.len()), (1, 2));
    assert_eq!(sizes[0].1.get_str("width"), Some(&Value::Int(50)));
}
