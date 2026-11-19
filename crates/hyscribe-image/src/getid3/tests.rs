// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::encode;
use hyscribe_testkit::unhex;

use super::*;
use crate::tests::corpus;
use crate::upload::wp_check_filetype;

const GOLDEN: &str = include_str!("../../tests/golden/getid3.tsv");

/// Every golden file: its name, bytes and PHP's serialized metadata.
fn files() -> Vec<(String, Vec<u8>, Vec<u8>)> {
    GOLDEN
        .lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            match f[0] {
                "corpus" => (f[1].to_string(), corpus(f[1]), unhex(f[2])),
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
        false => wp_read_audio_metadata(bytes),
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
    assert_eq!(read_files.len(), READ, "{read_files:?}");
    assert!(!left.is_empty());
}

/// How many golden files are read.
const READ: usize = 20;
