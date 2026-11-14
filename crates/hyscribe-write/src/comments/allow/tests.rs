// SPDX-License-Identifier: AGPL-3.0-or-later

//! The expected values are PHP 8's (PCRE 10.42) for the same input.

use super::*;

#[test]
fn links_count_as_preg_match_all() {
    for (text, count) in [
        ("", 0),
        ("<a href=x>", 1),
        ("<A HREF=x><a href=y>", 2),
        ("<a <a href>", 1),
        ("<a title=x>", 0),
        ("<a title=x> <a href=y", 1),
        ("<ahref>", 0),
        ("<a\nhref=x>", 0),
        ("<a x href y href>", 1),
        ("<a title=x>href", 0),
    ] {
        assert_eq!(links(text.as_bytes()), count, "{text:?}");
    }
}

#[test]
fn words_match_in_any_case() {
    for (word, subject, found) in [
        ("Wörd", "WÖRD", true),
        ("straße", "STRASSE", false),
        ("ſ", "S", true),
        ("K", "\u{212a}", true),
        ("σ", "ς", true),
        ("ϐ", "Β", true),
        ("ẞ", "ß", true),
        ("ǅ", "ǆ", true),
        ("ι", "\u{345}", true),
        ("ﬀ", "FF", false),
        ("İ", "i", false),
        ("ı", "i", false),
        ("ı", "I", false),
        ("ab", "a", false),
    ] {
        let words = format!("\n {word} \n");
        assert_eq!(
            has_word(words.as_bytes(), &[subject.as_bytes()]),
            found,
            "{word} {subject}"
        );
    }
    // PCRE refuses what is not UTF-8, in the subject or the word.
    assert!(!has_word(b"x", &[b"\xff x"]));
    assert!(!has_word(b"\xff", &[b"\xff"]));
    assert!(has_word(b"\xff\nx", &[b"x"]));
}
