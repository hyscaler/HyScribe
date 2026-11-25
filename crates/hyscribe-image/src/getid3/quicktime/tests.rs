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

/// PHP's sums of integers stay integers; of a float, floats.
#[test]
fn adds_and_subtracts_as_php_does() {
    assert_eq!(Num::Int(7).add(Num::Int(5)), Num::Int(12));
    assert_eq!(Num::Int(7).sub(Num::Int(5)), Num::Int(2));
    assert_eq!(Num::Int(7).add(Num::Float(0.5)), Num::Float(7.5));
    assert_eq!(Num::Float(7.5).sub(Num::Int(5)), Num::Float(2.5));
}

/// The QuickTime lookup names its own formats; the RIFF one gives the rest
/// later, SVQ3 among them.
#[test]
fn names_codecs_by_the_lookup_getid3_asks_first() {
    assert!(quicktime_codec(b"avc1"));
    assert!(!quicktime_codec(b"SVQ3"));
    assert!(!quicktime_codec(b"2vuY"));
    assert_eq!(codec_name(b"SVQ3"), "Sorenson Video 3 (Apple Quicktime 5)");
}

/// The fixed limits: nesting, atoms, the bytes held along a nesting and at
/// the top level, each read up to its bound and left past it.
#[test]
fn reads_up_to_the_limits() {
    let mut movie = Movie::new(b"");
    assert!(movie.atom(b"free", b"", b"", DEPTH, 0, None).is_ok());
    assert!(movie.atom(b"free", b"", b"", DEPTH + 1, 0, None).is_err());
    let mut movie = Movie::new(b"");
    movie.atoms = ATOMS - 1;
    assert!(movie.atom(b"free", b"", b"", 0, 0, None).is_ok());
    assert!(movie.atom(b"free", b"", b"", 0, 0, None).is_err());
    let mut movie = Movie::new(b"");
    let data = vec![0; 8];
    assert!(
        movie
            .atom(b"free", &data, b"", 0, 2 * HELD - 8, None)
            .is_ok()
    );
    assert!(
        movie
            .atom(b"free", &data, b"", 0, 2 * HELD - 7, None)
            .is_err()
    );
    let top = |size: usize| atom(b"free", &vec![0; size]);
    assert!(video(&top(HELD)).is_ok());
    assert_eq!(video(&top(HELD + 1)), Err(Unported));
}

/// A controller type other than QuickTime VR's changes nothing.
#[test]
fn takes_only_the_vr_controller() {
    let mut movie = Movie::new(b"");
    movie.atom(b"ctyp", b"none", b"", 1, 0, None).unwrap();
    assert_eq!(movie.video().dataformat, None);
    movie.atom(b"ctyp", b"qtvr", b"", 1, 0, None).unwrap();
    assert_eq!(movie.video().dataformat, Some("quicktimevr"));
}

/// An object descriptor whose tracks the atom could hold is read; one
/// claiming more is left.
#[test]
fn leaves_descriptors_claiming_more_tracks_than_they_hold() {
    let iods = |length: u8| {
        let mut data = vec![0, 0, 0, 0, 0x10, length];
        data.resize(16, 0);
        data
    };
    let mut movie = Movie::new(b"");
    assert!(movie.atom(b"iods", &iods(103), b"", 1, 0, None).is_ok());
    assert!(movie.atom(b"iods", &iods(104), b"", 1, 0, None).is_err());
}

/// An atom of the header's size holds nothing, whatever follows it.
#[test]
fn reads_an_atom_of_the_header_s_size_as_empty() {
    let mut movie = Movie::new(b"");
    let list = [atom(b"udta", b""), atom(b"free", b"")].concat();
    let found = movie.container(&list, b"moov", 1, 0).unwrap();
    assert_eq!((found.len(), movie.atoms), (2, 2));
}

/// A data reference table of entries one after the other is read.
#[test]
fn reads_references_one_after_the_other() {
    let entry = [&12u32.to_be_bytes()[..], b"url ", &[0, 0, 0, 1]].concat();
    let table = [&[0, 0, 0, 0, 0, 0, 0, 2][..], &entry, &entry].concat();
    assert!(dref(&table).is_ok());
    assert!(dref(&table[..table.len() - 1]).is_err());
}

/// A time table's counts and durations in their order; one longer than
/// PHP reads under 128M is left.
#[test]
fn reads_time_tables_in_order() {
    let table = [
        0, 0, 0, 0, 0, 0, 0, 2, 0, 0, 0, 5, 0, 0, 0, 7, 0, 0, 0, 9, 0, 0, 0, 11,
    ];
    let read = stts(&table).unwrap();
    let times = vec![(Some(5), Some(7)), (Some(9), Some(11))];
    assert_eq!(read, Table::Times(times));
    let long = |count: u32| [[0, 0, 0, 0], count.to_be_bytes()].concat();
    assert!(stts(&long(13_421)).is_ok());
    assert!(stts(&long(13_422)).is_err());
}

/// A cover data box of the header's size alone is a first picture of no
/// data, which leaves the file without one.
#[test]
fn reads_a_cover_box_of_its_header_alone() {
    let mut movie = Movie::new(b"");
    movie.cover(&atom(b"data", b"")).unwrap();
    assert_eq!(movie.picture, Some((Vec::new(), None)));
}
