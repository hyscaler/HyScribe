// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::encode;
use hyscribe_testkit::unhex;

use super::*;
use crate::tests::corpus;
use crate::upload::wp_check_filetype;

const GOLDEN: &str = include_str!("../../tests/golden/getid3.tsv");

/// An MP3 sample of `reference/getid3/mp3-samples.py`.
fn mp3_sample(name: &str) -> Vec<u8> {
    let path = format!("{}/tests/mp3/{name}", env!("CARGO_MANIFEST_DIR"));
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
                "mp3" => (f[1].to_string(), mp3_sample(f[1]), unhex(f[2])),
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
    assert!(!left.is_empty());
}

/// How many golden files are read.
const READ: usize = 433;

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
