// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;
use crate::tests::corpus;

const GOLDEN: &str = include_str!("../../tests/golden/fileinfo.tsv");

/// Every golden file: its name, bytes and PHP's answer.
fn files() -> Vec<(String, Vec<u8>, &'static str)> {
    GOLDEN
        .lines()
        .map(|line| {
            let f: Vec<&str> = line.split('\t').collect();
            match f[0] {
                "corpus" => (f[1].to_string(), corpus(f[1]), f[2]),
                _ => (f[1].to_string(), unhex(f[2]), f[3]),
            }
        })
        .collect()
}

/// Whenever it answers, libmagic is among the types it gives, and it
/// answers for the families it reads.
#[test]
fn agrees_with_libmagic() {
    let (mut wrong, mut answered, mut total) = (Vec::new(), 0, 0);
    for (name, bytes, mime) in files() {
        total += 1;
        if let Ok(found) = finfo_candidates(&bytes) {
            answered += 1;
            if !found.contains(&mime) {
                wrong.push(format!("{name}: {found:?}, PHP says {mime}"));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    assert_eq!((answered, total), ANSWERED, "files answered, of all");
}

/// What the golden cannot hold: no bytes, and archives cut or altered
/// where libmagic would answer otherwise.
#[test]
fn leaves_what_it_cannot_tell() {
    assert_eq!(finfo_candidates(b""), Ok(vec!["application/x-empty"]));
    let docx = corpus("sample.docx");
    // A central directory that is not one.
    let mut broken = docx.clone();
    let end = broken.windows(4).rposition(|w| w == b"PK\x05\x06").unwrap();
    let at = usize::try_from(u32::from_le_bytes(
        broken[end + 16..end + 20].try_into().unwrap(),
    ))
    .unwrap();
    broken[at] = b'X';
    assert_eq!(finfo_candidates(&broken), Err(Unported));
    // An end more than a comment's length from the end of the file is not
    // looked for.
    let mut trailing = docx.clone();
    trailing.extend(vec![0; 0xffff + 22]);
    assert_eq!(finfo_candidates(&trailing), Err(Unported));
    let mut near = docx.clone();
    near.extend(vec![0; 0xffff]);
    assert!(finfo_candidates(&near).is_ok());
    // An archive with no end, and an OpenDocument `mimetype` compressed.
    assert_eq!(finfo_candidates(&docx[..docx.len() - 30]), Err(Unported));
    let mut compressed = corpus("sample.odt");
    compressed[8] = 8;
    assert_eq!(finfo_candidates(&compressed), Err(Unported));
    // Its text longer than the file, a name longer than the file, an end
    // with no room for its fields, and a directory with too little to read.
    let mut long = corpus("sample.odt");
    long[22..26].copy_from_slice(&0x00ff_ffffu32.to_le_bytes());
    assert_eq!(finfo_candidates(&long), Err(Unported));
    let mut named = docx.clone();
    let start = usize::try_from(u32::from_le_bytes(
        named[end + 16..end + 20].try_into().unwrap(),
    ))
    .unwrap();
    named[start + 28..start + 30].copy_from_slice(&0xffffu16.to_le_bytes());
    assert_eq!(finfo_candidates(&named), Err(Unported));
    let local = docx[..start].to_vec();
    for room in [4, 9, 11, 13, 19] {
        let cut = [
            local.as_slice(),
            b"PK\x05\x06".as_slice(),
            &[0u8; 20][..room - 4],
        ]
        .concat();
        assert_eq!(finfo_candidates(&cut), Err(Unported), "{room}");
    }
    // Entries cut at each of the three fields that follow their sizes.
    for room in [29, 31, 33] {
        let mut cut = local.clone();
        let mut ending = b"PK\x05\x06\0\0\0\0\x01\0\x01\0\0\0\0\0".to_vec();
        ending.extend(u32::try_from(local.len() + 22).unwrap().to_le_bytes());
        ending.extend([0, 0]);
        cut.extend(ending);
        cut.extend(b"PK\x01\x02");
        cut.resize(local.len() + 22 + room, 0);
        assert_eq!(finfo_candidates(&cut), Err(Unported), "{room}");
    }
}

/// An image header with a size is an image, and one without may be text.
#[test]
fn reads_images_that_have_a_size() {
    let jpeg = corpus("small.jpg");
    assert_eq!(finfo_candidates(&jpeg), Ok(vec!["image/jpeg"]));
    assert_eq!(finfo_candidates(&jpeg[..4]), Err(Unported));
}

/// How many golden files are answered, of how many: every family the
/// reader knows, so a rule that stops answering shows here.
const ANSWERED: (usize, usize) = (1068, 1791);

/// Text is read up to the limit libmagic reads, and no further.
#[test]
fn reads_text_up_to_the_limit() {
    let words = b"Plain words. ".repeat(LIMIT / 13 + 1);
    assert_eq!(finfo_candidates(&words[..LIMIT]), Ok(vec!["text/plain"]));
    assert_eq!(finfo_candidates(&words[..LIMIT + 1]), Err(Unported));
}

/// What the golden holds that this reads, by file, for the types the
/// default allowed list takes.
#[test]
fn reads_what_wordpress_uploads() {
    for (name, mime) in [
        ("plain.txt", "text/plain"),
        ("commas.txt", "text/csv"),
        ("words.csv", "text/csv"),
        ("captions.vtt", "text/vtt"),
        ("event.ics", "text/calendar"),
        ("sample.pdf", "application/pdf"),
        ("sample.rtf", "text/rtf"),
        ("sample.zip", "application/zip"),
        ("sample.tar", "application/x-tar"),
        ("sample.gz", "application/gzip"),
        ("sample.odt", "application/vnd.oasis.opendocument.text"),
        (
            "sample.ods",
            "application/vnd.oasis.opendocument.spreadsheet",
        ),
        ("cbr.mp3", "audio/mpeg"),
        ("tagged.mp3", "audio/mpeg"),
        ("tone.wav", "audio/x-wav"),
        ("tone.flac", "audio/flac"),
        ("tone.ogg", "audio/ogg"),
        ("tone.m4a", "audio/x-m4a"),
        ("tone.aac", "audio/x-hx-aac-adts"),
        ("tone.wma", "video/x-ms-asf"),
        ("tune.mid", "audio/midi"),
        ("clip.mp4", "video/mp4"),
        ("clip.mov", "video/quicktime"),
        ("clip.avi", "video/x-msvideo"),
        ("clip.webm", "video/webm"),
        ("clip.mkv", "video/x-matroska"),
        ("clip.3gp", "video/3gpp"),
        ("clip.wmv", "video/x-ms-asf"),
        ("clip.flv", "video/x-flv"),
    ] {
        let found = finfo_candidates(&corpus(name)).unwrap_or_default();
        assert!(found.contains(&mime), "{name}: {found:?}");
    }
}
