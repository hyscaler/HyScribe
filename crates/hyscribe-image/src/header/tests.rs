// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::md5;
use hyscribe_testkit::unhex;

use super::*;
use crate::tests::corpus;

/// Each case of `header.tsv`: its bytes and PHP's answers.
fn cases() -> Vec<(String, Vec<u8>, Vec<String>)> {
    include_str!("../../tests/golden/header.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let [name, recipe, rest @ ..] = fields.as_slice() else {
                unreachable!()
            };
            let bytes = match name.strip_prefix("hex:") {
                Some(_) => unhex(recipe),
                None => {
                    let mut bytes = corpus(name);
                    let parts: Vec<&str> = recipe.split(':').collect();
                    match parts.as_slice() {
                        ["cut", n] => bytes.truncate(n.parse().unwrap()),
                        ["set", at, byte] => {
                            bytes[at.parse::<usize>().unwrap()] = byte.parse().unwrap()
                        }
                        _ => {}
                    }
                    bytes
                }
            };
            let label = format!("{name} {recipe:.40}");
            (label, bytes, rest.iter().map(|s| s.to_string()).collect())
        })
        .collect()
}

#[test]
fn matches_exif_imagetype_and_getimagesize() {
    let cases = cases();
    assert!(cases.len() > 1600);
    let mut wrong = Vec::new();
    for (label, bytes, php) in cases {
        let [mime, width, height, _, app13] = php.as_slice() else {
            unreachable!()
        };
        let ported = [
            "image/gif",
            "image/jpeg",
            "image/png",
            "image/bmp",
            "image/tiff",
            "image/webp",
            "image/avif",
        ];
        let mime = ported.contains(&mime.as_str()).then_some(mime.as_str());
        // `wp_getimagesize()` is false for 0 by 0, and HyScribe never reads
        // a TIFF's size, which WordPress does not use.
        let sized = mime.is_some_and(|m| m != "image/tiff")
            && width != "-"
            && (width != "0" || height != "0");
        let php = (
            mime,
            sized.then(|| (width.clone(), height.clone(), app13.clone())),
        );
        let size = getimagesize(&bytes).map(|s| {
            let app13 = s.app13.map_or("-".to_string(), md5);
            (s.width.to_string(), s.height.to_string(), app13)
        });
        let ours = (image_type(&bytes).map(ImageType::mime), size);
        // A HEIC's size is the editor's (the test below).
        let ours = (ours.0, ours.1.filter(|_| ours.0.is_some()));
        if ours != php {
            wrong.push(format!("{label}: {ours:?} != {php:?}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong[..wrong.len().min(30)].join("\n")
    );
}

#[test]
fn reads_a_large_box_size_in_full() {
    // A box of 2^32 + 16 bytes, past the end of the file.
    let mut file = b"\0\0\0\x10ftypavif\0\0\0\0".to_vec();
    file.extend(b"\0\0\0\x01free\0\0\0\x01\0\0\0\x10");
    assert_eq!(boxes(&file).count(), 1);
    file[27] = 0;
    assert_eq!(boxes(&file).count(), 2);
}

#[test]
fn reads_associations_as_far_as_the_box_holds_them() {
    // Version 0, one item with two narrow indexes; then cut short.
    let narrow = [0, 0, 0, 0, 0, 0, 0, 1, 0, 5, 2, 0x81, 3];
    assert_eq!(ipma(&narrow), [(5, 1), (5, 3)]);
    assert_eq!(ipma(&narrow[..12]), [(5, 1)]);
    assert_eq!(ipma(&narrow[..10]), []);
    // Version 1 and wide indexes.
    let wide = [1, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 9, 1, 0x80, 0x07];
    assert_eq!(ipma(&wide), [(9, 7)]);
    assert_eq!(ipma(&[]), []);
}

#[test]
fn reads_a_heic_as_its_editor_does() {
    let cases = cases();
    let case = |name: &str| {
        &cases
            .iter()
            .find(|(label, ..)| label.starts_with(name))
            .unwrap()
            .1
    };
    let size = |name| getimagesize(case(name)).map(|s| (s.width, s.height, s.kind, s.kind.mime()));
    assert_eq!(
        size("hex:heic "),
        Some((80, 40, ImageType::Heif, "image/heic"))
    );
    // A quarter turn swaps the sides.
    assert_eq!(
        size("hex:heic-irot"),
        Some((40, 80, ImageType::Heif, "image/heic"))
    );
    assert_eq!(image_mime(case("hex:heif ")), Some("image/heif"));
    assert_eq!(
        size("hex:heif "),
        Some((8, 4, ImageType::Heif, "image/heic"))
    );
    assert_eq!(image_mime(b"GIF89a"), Some("image/gif"));
    // Other brands are left to fileinfo, which HyScribe does not port.
    assert_eq!(image_mime(case("hex:heif-mif1")), None);
}
