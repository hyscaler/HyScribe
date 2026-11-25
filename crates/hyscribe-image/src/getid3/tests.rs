// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::encode;
use hyscribe_testkit::unhex;

use super::*;
use crate::tests::corpus;
use crate::upload::wp_check_filetype;

const GOLDEN: &str = include_str!("../../tests/golden/getid3.tsv");

/// A sample of `reference/getid3/mp3-samples.py` or `mp4-samples.py`,
/// in the folder `kind`.
pub(super) fn sample(kind: &str, name: &str) -> Vec<u8> {
    let path = format!("{}/tests/{kind}/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(path).unwrap()
}

/// Every golden file: its name, bytes and PHP's serialized metadata.
fn files() -> Vec<(String, Vec<u8>, Vec<u8>)> {
    GOLDEN
        .lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            match f[0] {
                "corpus" => (f[1].to_string(), corpus(f[1]), unhex(f[2])),
                "mp3" | "mp4" => (f[1].to_string(), sample(f[0], f[1]), unhex(f[2])),
                _ => (f[1].to_string(), unhex(f[2]), unhex(f[3])),
            }
        })
        .collect()
}

/// What `wp_generate_attachment_metadata()` reads a file with, by its name.
fn read(name: &str, bytes: &[u8]) -> Result<Vec<Entry>, Unported> {
    let video = wp_check_filetype(name, false).is_some_and(|(_, kind)| kind.starts_with("video/"));
    match video {
        true => wp_read_video_metadata(bytes),
        false => wp_read_audio_metadata(bytes, b""),
    }
}

/// Whatever it reads, it reads as getID3 does; the rest is left.
#[test]
fn matches_getid3() {
    let (mut read_files, mut left, mut wrong) = (Vec::new(), Vec::new(), Vec::new());
    for (name, bytes, php) in files() {
        match read(&name, &bytes) {
            Ok(meta) => {
                let ours = encode(&Value::Array(meta));
                if ours != php {
                    wrong.push(format!(
                        "{name}: {} PHP {}",
                        String::from_utf8_lossy(&ours),
                        String::from_utf8_lossy(&php)
                    ));
                }
                read_files.push(name);
            }
            Err(Unported) => left.push(name),
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert_eq!(read_files.len(), READ);
    let movies = [".mp4", ".m4a", ".m4v", ".mov", ".3gp", ".3g2"];
    let left_movies: Vec<&str> = left
        .iter()
        .map(String::as_str)
        .filter(|name| movies.iter().any(|ext| name.ends_with(ext)))
        .collect();
    assert_eq!(left_movies, LEFT_MOVIES);
}

/// The QuickTime and MP4 files left, each for a reason of the ADR 0020
/// amendment.
const LEFT_MOVIES: [&str; 47] = [
    "codec-short-entry.mp4",
    "codec-short-zero-vendor.mp4",
    "count-free-top.mp4",
    "count-free.mp4",
    "cover-bmp.m4a",
    "cover-box-small.m4a",
    "cover-unknown-zero.m4a",
    "cover-unknown.m4a",
    "cover-unknown.mp4",
    "esds-gps-inside.m4a",
    "mdat-chapter-newline.mp4",
    "mdat-chapters.mp4",
    "mdat-mpeg-after.mp4",
    "mdat-nested.mp4",
    "mdat-wide-chapters.mp4",
    "mdhd-short.mp4",
    "mdhd-zero-scale.mp4",
    "mov-audio-vendor.mov",
    "mvhd-short.mp4",
    "mvhd-v1.mp4",
    "mvhd-zero-scale.mp4",
    "nest-40.mp4",
    "nest-70.mp4",
    "other-cmov.mov",
    "other-dref-overclaim.mp4",
    "other-dss.mp4",
    "other-elst-overclaim.mp4",
    "other-frea.mov",
    "other-gps.mp4",
    "other-heic.mp4",
    "other-id32.mp4",
    "other-iods-overclaim.mp4",
    "other-mk.mp4",
    "other-moof.mp4",
    "other-small.mp4",
    "other-tagged.mp4",
    "other-thumbnail.mov",
    "other-tref.mov",
    "other-uuid-360fly.mp4",
    "size-64-small.mp4",
    "size-small-top.mp4",
    "size-two-top.mp4",
    "stts-half-milli.mp4",
    "stts-huge-count.mp4",
    "tags-keys.mov",
    "tags-meta-numeric.mov",
    "tkhd-short.mp4",
];

/// How many golden files are read.
const READ: usize = 691;

/// A WAVE file of `size` bytes of 8 bit mono samples.
fn wave(size: usize) -> Vec<u8> {
    let data = size - 44;
    let mut file = b"RIFF".to_vec();
    file.extend(u32::try_from(size - 8).unwrap().to_le_bytes());
    file.extend(b"WAVEfmt \x10\0\0\0\x01\0\x01\0\x40\x1f\0\0\x40\x1f\0\0\x01\0\x08\0data");
    file.extend(u32::try_from(data).unwrap().to_le_bytes());
    file.resize(size, 0x55);
    file
}

/// The files too small for getID3 to read as it does are left, and what
/// carries a tag; a format is read as audio or as video, not as both.
#[test]
fn leaves_small_and_tagged_files_and_the_wrong_kind() {
    assert_eq!(wp_read_audio_metadata(&wave(511), b""), Err(Unported));
    assert!(wp_read_audio_metadata(&wave(512), b"").is_ok());
    let mut tagged = wave(600);
    for tag in [&b"TAG"[..], b"APETAGEX", b"LYRICS"] {
        let at = tagged.len() - 40;
        tagged[at..at + tag.len()].copy_from_slice(tag);
        assert_eq!(
            wp_read_audio_metadata(&tagged, b""),
            Err(Unported),
            "{tag:?}"
        );
        tagged[at..at + tag.len()].fill(0x55);
    }
    // Tags farther back than the last 512 bytes are no tag at the end.
    let mut far = wave(1200);
    far[100..103].copy_from_slice(b"TAG");
    assert!(wp_read_audio_metadata(&far, b"").is_ok());
    let padded = |head: &[u8]| [head, &[0; 600]].concat();
    let midi = padded(b"MThd");
    let mpeg = padded(&[0, 0, 1, 0xba]);
    assert!(wp_read_audio_metadata(&midi, b"").is_ok());
    assert_eq!(wp_read_video_metadata(&midi), Err(Unported));
    assert!(wp_read_video_metadata(&mpeg).is_ok());
    assert_eq!(wp_read_audio_metadata(&mpeg, b""), Err(Unported));
    for head in [
        &b"ADIF"[..],
        &[0xff, 0xf1],
        &[0xff, 0xf9],
        &[0xff, 0xf0],
        &[0xff, 0xf8],
    ] {
        assert!(
            wp_read_audio_metadata(&padded(head), b"").is_ok(),
            "{head:?}"
        );
        assert_eq!(wp_read_video_metadata(&padded(head)), Err(Unported));
    }
    assert_eq!(wp_read_audio_metadata(&padded(b"zzzz"), b""), Err(Unported));
}
