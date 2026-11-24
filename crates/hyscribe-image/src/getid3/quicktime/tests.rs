// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::getid3::tests::sample;

/// An atom of `name` holding `body`.
fn atom(name: &[u8], body: &[u8]) -> Vec<u8> {
    let size = u32::try_from(8 + body.len()).unwrap();
    [&size.to_be_bytes()[..], name, body].concat()
}

/// A sample of `reference/getid3/mp4-samples.py`.
fn mp4(name: &str) -> Vec<u8> {
    sample("mp4", name)
}

/// The movie of `mp4-video-first.mp4` with `more` after it.
fn with(more: &[u8]) -> Vec<u8> {
    [mp4("mp4-video-first.mp4"), more.to_vec()].concat()
}

/// The bytes besides media data PHP would hold are bounded: an atom past
/// them, atoms that add up past them, or a nesting holding them over and
/// over, are left.
#[test]
fn leaves_files_past_the_bytes_held() {
    let big = vec![0; HELD];
    assert_eq!(video(&with(&atom(b"free", &big))), Err(Unported));
    let half = vec![0; HELD / 2];
    let twice = [atom(b"free", &half), atom(b"skip", &half)].concat();
    assert_eq!(video(&with(&twice)), Err(Unported));
    let mut nested = atom(b"free", &vec![0; HELD / 4]);
    for _ in 0..8 {
        nested = atom(b"udta", &nested);
    }
    assert_eq!(video(&atom(b"moov", &nested)), Err(Unported));
    let shallow = atom(b"udta", &atom(b"free", &vec![0; HELD / 4]));
    assert!(video(&atom(b"moov", &shallow)).is_ok());
}

/// Media data past getID3's read buffer is read up to it; an MPEG audio
/// frame where any of the buffers stops it is left.
#[test]
fn leaves_media_data_cut_where_a_frame_starts() {
    let file = mp4("mp4-video.mp4");
    let movie_at = file.windows(4).position(|w| w == b"moov").unwrap() - 4;
    let (head, movie) = file.split_at(movie_at);
    let head = &head[..head.len() - 908];
    for (at, frame) in [(BUFFERS[0], false), (BUFFERS[1], true), (BUFFERS[0], true)] {
        let mut data = vec![0; BUFFERS[1] + 16];
        if frame {
            data[at..at + 2].copy_from_slice(&[0xff, 0xfb]);
        }
        let big = [head, &atom(b"mdat", &data), movie].concat();
        let read = video(&big);
        assert_eq!(read.is_err(), frame, "{at} {frame}");
    }
}

/// More empty sample descriptions past the table than PHP is let read are
/// left; a few are read as one of no colour.
#[test]
fn leaves_many_empty_descriptions() {
    let file = mp4("codec-entries-overclaim.mp4");
    let at = file.windows(4).position(|w| w == b"stsd").unwrap() + 8;
    let mut many = file.clone();
    many[at..at + 4].copy_from_slice(&65u32.to_be_bytes());
    assert!(video(&many).is_ok());
    many[at..at + 4].copy_from_slice(&66u32.to_be_bytes());
    assert_eq!(video(&many), Err(Unported));
}

/// The bytes PHP reads as numbers, wherever they stop.
#[test]
fn reads_numbers_as_php_does() {
    assert_eq!(be(b"\x01\x02\x03", 0, 4), Some(0x010203));
    assert_eq!(be(b"\x01\x02\x03", 1, 1), Some(2));
    assert_eq!(be(b"\x01\x02\x03", 3, 4), None);
    assert_eq!(be(b"\x01\x02\x03", 9, 4), None);
    assert_eq!(fixed(b"\x00\xa0\x80\x00", 0), 160.5);
    assert_eq!(four(b"abcdef", 4), b"ef");
    let mut at = 0;
    assert_eq!(descr_length(b"\x81\x80\x80\x80\x80\x80", &mut at), 1 << 28);
    assert_eq!(at, 5);
    let mut at = 1;
    assert_eq!(descr_length(b"\0\x83\x01", &mut at), 0x181);
    assert_eq!(at, 3);
}
