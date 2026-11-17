// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::encode;

use super::*;
use crate::header::getimagesize;
use crate::tests::{corpus, golden, hostile};

#[test]
fn matches_wp_read_image_metadata() {
    let mut read = 0;
    for (name, php) in golden("read") {
        let bytes = corpus(&name);
        // HyScribe reads no TIFF's size: WordPress never makes sizes of one.
        let Some(size) = getimagesize(&bytes) else {
            assert!(name.ends_with(".tif"), "{name}");
            continue;
        };
        let ours =
            wp_read_image_metadata(&bytes, &size, b"").map(|meta| encode(&Value::Array(meta)));
        assert_eq!(ours.as_deref(), Ok(php.as_slice()), "{name}");
        read += 1;
    }
    assert_eq!(read, 44);
}

/// A big endian TIFF block with one IFD of `entries`: tag, type, count and
/// value (at most four bytes inline, else after the IFD).
fn tiff(entries: &[(u16, u16, u32, &[u8])]) -> Vec<u8> {
    let mut head = b"MM\0*\0\0\0\x08".to_vec();
    head.extend((entries.len() as u16).to_be_bytes());
    let mut data: Vec<u8> = Vec::new();
    let data_at = 8 + 2 + 12 * entries.len() + 4;
    for &(tag, kind, count, value) in entries {
        head.extend(tag.to_be_bytes());
        head.extend(kind.to_be_bytes());
        head.extend(count.to_be_bytes());
        if value.len() <= 4 {
            let mut inline = value.to_vec();
            inline.resize(4, 0);
            head.extend(inline);
        } else {
            head.extend(((data_at + data.len()) as u32).to_be_bytes());
            data.extend(value);
        }
    }
    head.extend([0; 4]);
    head.extend(data);
    head
}

/// The small corpus JPEG with only `segment` as its EXIF.
fn with_exif(segment: &[u8]) -> Vec<u8> {
    let photo = corpus("small.jpg");
    let mut body = b"Exif\0\0".to_vec();
    body.extend(segment);
    let mut out = vec![0xFF, 0xD8, 0xFF, 0xE1];
    out.extend(((body.len() + 2) as u16).to_be_bytes());
    out.extend(body);
    out.extend(&photo[2..]);
    out
}

fn read(bytes: &[u8]) -> Result<Vec<(Key, Value)>, Unported> {
    wp_read_image_metadata(bytes, &getimagesize(bytes).unwrap(), b"")
}

fn field<'a>(meta: &'a [(Key, Value)], name: &str) -> &'a Value {
    &meta
        .iter()
        .find(|(k, _)| *k == Key::String(name.into()))
        .unwrap()
        .1
}

#[test]
fn leaves_what_exif_read_data_reads_otherwise_unported() {
    let short = 3u16.to_be_bytes();
    let unported = [
        // A text field of another type, or of two strings.
        tiff(&[(0x0110, 3, 1, &short)]),
        tiff(&[(0x0110, 2, 4, b"a\0b\0")]),
        // A rational field of another type, or with two values.
        tiff(&[(0x829D, 3, 1, &short)]),
        tiff(&[(
            0x829D,
            5,
            2,
            &[0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4],
        )]),
        // A number of another type, or two of them.
        tiff(&[(0x0112, 2, 2, b"6\0")]),
        tiff(&[(0x0112, 3, 2, &[0, 6, 0, 1])]),
        // Comments in Unicode, too short, or of another type.
        tiff(&[(0x9286, 7, 12, b"UNICODE\0\0a\0b")]),
        tiff(&[(0x9286, 7, 4, b"ASCI")]),
        tiff(&[(0x9286, 2, 4, b"abc\0")]),
        // Each field read, of another type.
        tiff(&[(0x010E, 3, 1, &short)]),
        tiff(&[(0x013B, 3, 1, &short)]),
        tiff(&[(0x8298, 3, 1, &short)]),
        tiff(&[(0x9004, 3, 1, &short)]),
        tiff(&[(0x920A, 3, 1, &short)]),
        tiff(&[(0x829A, 3, 1, &short)]),
        tiff(&[(0x8827, 2, 2, b"9\0")]),
        // Windows XP's comment and author.
        tiff(&[(0x9C9C, 1, 4, b"a\0b\0")]),
        tiff(&[(0x9C9D, 1, 4, b"a\0b\0")]),
        // A block the reader rejects, and a date strtotime() is not ported for.
        b"MM\0*\0\0\0\xFF".to_vec(),
        tiff(&[(0x9004, 2, 9, b"tomorrow\0")]),
    ];
    for segment in unported {
        assert_eq!(read(&with_exif(&segment)), Err(Unported), "{segment:02x?}");
    }
    // No EXIF block, and a JPEG whose segments end before one.
    assert!(read(&corpus("small.jpg")).is_ok());
    assert_eq!(exif_segment(b"\xFF\xD8\xFF\xE0\x00\x10ab"), None);
    assert_eq!(exif_segment(b"\xFF\xD8\x00"), None);
    assert_eq!(exif_segment(b"\xFF\xD8\x00\x00"), None);
    assert_eq!(exif_segment(b"\xFF\xD8\xFF\xE1"), None);
    assert_eq!(exif_segment(b"\xFF\xD8\xFF\xE1\x00"), None);
    assert_eq!(exif_segment(b"\xFF\xD8\xFF\xE1\x00\x06abcd\xFF\xDA"), None);
    // An IPTC time strtotime() is not ported for.
    let iptc = b"\xFF\xD8\xFF\xED\x00\x14\x1C\x02\x37\x00\x04abcd\x1C\x02\x3C\x00\x04efgh\xFF\xC0\x00\x11\x08\x00\x01\x00\x01\x01\0\0\0\0\0\0\0\0\0\xFF\xD9";
    assert_eq!(read(iptc), Err(Unported));
    // XMP that names an alternative text.
    let mut xmp = corpus("small.jpg");
    xmp.extend(b"<x:xmpmeta><Iptc4xmpCore:AltTextAccessibility/></x:xmpmeta>");
    assert_eq!(read(&xmp), Err(Unported));
    let mut other = corpus("small.jpg");
    other.extend(b"</x:xmpmeta> AltTextAccessibility <x:xmpmeta>");
    assert!(read(&other).is_ok());
}

/// The small JPEG with `iptc` datasets in an `APP13` and `segment` as its
/// EXIF.
fn with_both(iptc: &[(u8, &[u8])], segment: &[u8]) -> Vec<u8> {
    let mut iim = Vec::new();
    for &(dataset, value) in iptc {
        iim.extend([0x1C, 2, dataset]);
        iim.extend((value.len() as u16).to_be_bytes());
        iim.extend(value);
    }
    let jpeg = with_exif(segment);
    let mut app13 = vec![0xFF, 0xED];
    app13.extend(((iim.len() + 2) as u16).to_be_bytes());
    app13.extend(iim);
    [&jpeg[..2], &app13, &jpeg[2..]].concat()
}

fn text(value: &str) -> Value {
    Value::String(value.as_bytes().to_vec())
}

#[test]
fn takes_text_as_wp_read_image_metadata_does() {
    let ascii = |text: &str| [b"ASCII\0\0\0".as_slice(), text.as_bytes()].concat();
    let eighty = "e".repeat(80);
    let seventy_nine = "s".repeat(79);
    // A caption of 80 is too long for a title, of 79 not.
    for (caption, title) in [(&eighty, ""), (&seventy_nine, seventy_nine.as_str())] {
        let iptc = read(&with_both(&[(120, caption.as_bytes())], &tiff(&[]))).unwrap();
        assert_eq!(field(&iptc, "title"), &text(title));
        let description = read(&with_exif(&tiff(&[(
            0x010E,
            2,
            caption.len() as u32 + 1,
            format!("{caption}\0").as_bytes(),
        )])))
        .unwrap();
        assert_eq!(field(&description, "title"), &text(title));
        let comment = ascii(caption);
        let comment = read(&with_exif(&tiff(&[(
            0x9286,
            7,
            comment.len() as u32,
            &comment,
        )])))
        .unwrap();
        assert_eq!(field(&comment, "title"), &text(title));
    }
    // A date without a time is none; IPTC's credit and copyright win over
    // EXIF's.
    let artist = tiff(&[(0x013B, 2, 7, b"Artist\0"), (0x8298, 2, 5, b"Copy\0")]);
    let meta = read(&with_both(
        &[(55, b"20240506"), (110, b"Credit"), (116, b"Owner")],
        &artist,
    ))
    .unwrap();
    assert_eq!(field(&meta, "created_timestamp"), &text("0"));
    assert_eq!(
        (field(&meta, "credit"), field(&meta, "copyright")),
        (&text("Credit"), &text("Owner"))
    );
    // An IPTC caption is kept over an EXIF comment.
    let comment = ascii("Comment");
    let comment = tiff(&[(0x9286, 7, comment.len() as u32, &comment)]);
    let meta = read(&with_both(&[(120, b"Caption")], &comment)).unwrap();
    assert_eq!(field(&meta, "caption"), &text("Caption"));
    let meta = read(&with_both(&[(60, b"070809")], &tiff(&[]))).unwrap();
    assert_eq!(field(&meta, "created_timestamp"), &text("0"));
    // XMP without an alternative text reads as none.
    let mut xmp = corpus("small.jpg");
    xmp.extend(b"<x:xmpmeta>a description without the alternative</x:xmpmeta>");
    assert_eq!(field(&read(&xmp).unwrap(), "alt"), &text(""));
}

#[test]
fn reads_exif_as_wp_read_image_metadata_does() {
    // An orientation of 0, an ISO of 0 and no date read as not set; the
    // date's colons become dashes.
    let zeros = tiff(&[
        (0x0112, 3, 1, &[0, 0]),
        (0x8827, 3, 1, &[0, 0]),
        (0x9004, 2, 20, b"2024:05:06 07:08:09\0"),
    ]);
    let meta = read(&with_exif(&zeros)).unwrap();
    assert_eq!(field(&meta, "orientation"), &Value::String(b"0".to_vec()));
    assert_eq!(field(&meta, "iso"), &Value::String(b"0".to_vec()));
    assert_eq!(
        field(&meta, "created_timestamp"),
        &Value::String(b"1714979289".to_vec())
    );
    // A long comment alone is the caption, not the title.
    let long = [b"ASCII\0\0\0".as_slice(), &[b'c'; 90]].concat();
    let meta = read(&with_exif(&tiff(&[(0x9286, 7, long.len() as u32, &long)]))).unwrap();
    assert_eq!(field(&meta, "title"), &Value::String(Vec::new()));
    assert_eq!(field(&meta, "caption"), &Value::String(vec![b'c'; 90]));
    assert_eq!(wp_exif_date2ts(b" 2024:05:06 07:08 "), b"2024-05-06 07:08");
    assert_eq!(wp_exif_date2ts(b"2024"), b"2024-- ");
    // Text past a comment's NUL is not read.
    let comment = tiff(&[(0x9286, 7, 14, b"ASCII\0\0\0ab\0cd\0")]);
    assert_eq!(
        field(&read(&with_exif(&comment)).unwrap(), "caption"),
        &Value::String(b"ab".to_vec())
    );
    assert_eq!(utf8(b"Caf\xE9".to_vec()), "Caf\u{e9}".as_bytes());
    assert_eq!(timestamp(b"\xFF"), Err(Unported));
}

#[test]
fn reads_exif_only_within_its_segment() {
    // The description's offset points past the APP1, where PHP reads nothing.
    assert_eq!(read(&hostile("exif-past-app1.jpg")), Err(Unported));
}
