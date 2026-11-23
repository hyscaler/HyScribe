// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;
use crate::tests::corpus;

const GOLDEN: &str = include_str!("../../tests/golden/uploads.tsv");

/// The type checks of the golden this does not read (see the test).
const UNPORTED: usize = 26;

fn lines(kind: &str) -> Vec<Vec<&'static str>> {
    GOLDEN
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>())
        .filter(|f| f[0] == kind)
        .collect()
}

#[test]
fn names_uploads_as_wordpress_does() {
    let files: Vec<String> = lines("files")[0][1].split(',').map(String::from).collect();
    let names = lines("name");
    assert_eq!(names.len(), 53);
    for fields in names {
        let name = unhex(fields[1]);
        let label = String::from_utf8_lossy(&name).into_owned();
        assert_eq!(
            sanitize_file_name(&name, false).as_bytes(),
            unhex(fields[2]),
            "{label}"
        );
        assert_eq!(
            wp_unique_filename(&name, &[], false),
            fields[3],
            "{label} in an empty folder"
        );
        assert_eq!(
            wp_unique_filename(&name, &files, false),
            fields[4],
            "{label}"
        );
    }
}

#[test]
fn checks_types_as_wordpress_does() {
    let php = |v: &str| v.trim_matches('\'').to_string();
    let checks = lines("check");
    assert_eq!(checks.len(), 222);
    let mut unported = Vec::new();
    for fields in checks {
        let [_, corpus_name, upload, ext, kind, proper] = fields[..] else {
            unreachable!()
        };
        let Ok(checked) = wp_check_filetype_and_ext(&corpus(corpus_name), upload, false) else {
            unported.push(format!("{corpus_name} as {upload}"));
            continue;
        };
        let ours = checked
            .ext_type
            .map_or(("false".into(), "false".into()), |(e, t)| {
                (e, t.to_string())
            });
        assert_eq!(ours, (php(ext), php(kind)), "{upload}");
        assert_eq!(
            checked.proper_filename.unwrap_or("false".into()),
            php(proper),
            "{upload}"
        );
    }
    // What fileinfo may name two ways, or a type this does not read.
    assert_eq!(unported.len(), UNPORTED, "{unported:?}");
}

#[test]
fn allows_html_and_scripts_only_with_unfiltered_html() {
    assert_eq!(wp_check_filetype("a.html", false), None);
    assert_eq!(
        wp_check_filetype("a.HTML", true),
        Some(("HTML".into(), "text/html"))
    );
    assert_eq!(
        wp_check_filetype("a.js", true).map(|(_, t)| t),
        Some("application/javascript")
    );
    assert_eq!(wp_check_filetype("a.exe", true), None);
    // Not an image by its extension, or not one by its header: fileinfo.
    let pdf = wp_check_filetype_and_ext(b"x", "a.pdf", false).unwrap();
    assert_eq!(pdf.ext_type, Some(("pdf".into(), "application/pdf")));
    let jpg = wp_check_filetype_and_ext(b"x", "a.jpg", false).unwrap();
    assert_eq!(jpg.ext_type, None);
    assert_eq!(sanitize_file_name(b"x.html.jpg", true), "x.html.jpg");
    assert_eq!(sanitize_file_name(b"x.html.jpg", false), "x.html_.jpg");
    // A HEIC named as one renames to .heic.
    let heic = b"\0\0\0\x14ftypheic\0\0\0\0mif1";
    let checked = wp_check_filetype_and_ext(heic, "a.heif", false).unwrap();
    assert_eq!(
        (checked.ext_type, checked.proper_filename.as_deref()),
        (Some(("heic".into(), "image/heic")), Some("a.heic"))
    );
    assert_eq!(default_extension("image/x-unknown"), None);
    // A name "0" is PHP's empty(): its copies are not looked for.
    assert_eq!(
        wp_unique_filename(b"0.jpg", &["0-150x150.jpg".into()], false),
        "0.jpg"
    );
    assert_eq!(
        wp_unique_filename(b"a.jpg", &["a-150x150.jpg".into()], false),
        "a-1.jpg"
    );
    let copies = ["a-150x150.jpg".into(), "a-1-150x150.jpg".into()];
    assert_eq!(wp_unique_filename(b"a.jpg", &copies, false), "a-2.jpg");
    let kin = ["h.heic".into(), "h-1.heif".into()];
    assert_eq!(wp_unique_filename(b"h.jpg", &kin, false), "h-2.jpg");
    // Only a JPEG and the HEIC family have kin.
    assert_eq!(
        wp_unique_filename(b"p.png", &["p.jpg".into(), "p.heic".into()], false),
        "p.png"
    );
    // A size is digits on both sides of the x.
    assert_eq!(
        wp_unique_filename(
            b"a.jpg",
            &["a-150xab.jpg".into(), "a-x150.jpg".into()],
            false
        ),
        "a.jpg"
    );
    // Without an extension, no copies are looked for.
    assert_eq!(
        wp_unique_filename(b"noext", &["noext-150x150".into()], false),
        "noext"
    );
    // Found by fuzzing: a name ending in a character of two bytes.
    assert_eq!(sanitize_file_name("\u{7ce}\n".as_bytes(), false), "\u{7ce}");
    assert_eq!(wp_check_filetype("\u{7ce}", false), None);
    assert_eq!(wp_check_filetype("\u{e9}a", false), None);
}

/// The vetting by what `fileinfo` says: each of its special cases, as
/// `wp_check_filetype_and_ext()` makes them.
#[test]
fn vets_a_type_by_the_real_one() {
    // Nonspecific types fit application, video and audio types only.
    for real in [
        "application/octet-stream",
        "application/encrypted",
        "application/CDFV2-encrypted",
        "application/zip",
    ] {
        assert!(stands("application/pdf", real), "{real}");
        assert!(stands("video/mp4", real), "{real}");
        assert!(stands("audio/mpeg", real), "{real}");
        assert!(!stands("text/plain", real), "{real}");
        assert!(!stands("image/jpeg", real), "{real}");
    }
    // Audio and video fit their own major type only.
    assert!(stands("audio/mpeg", "audio/x-wav"));
    assert!(stands("video/mp4", "video/quicktime"));
    assert!(!stands("audio/mpeg", "video/mp4"));
    assert!(!stands("application/pdf", "audio/x-wav"));
    // Text, CSV and RTF fit the types WordPress lists.
    for kind in [
        "text/plain",
        "text/csv",
        "application/csv",
        "text/richtext",
        "text/tsv",
        "text/vtt",
    ] {
        assert!(stands(kind, "text/plain"), "{kind}");
    }
    assert!(!stands("text/calendar", "text/plain"));
    assert!(!stands("application/rtf", "text/plain"));
    for kind in ["text/csv", "text/plain", "application/csv"] {
        assert!(stands(kind, "application/csv"), "{kind}");
    }
    assert!(!stands("text/vtt", "application/csv"));
    for kind in ["text/rtf", "text/plain", "application/rtf"] {
        assert!(stands(kind, "text/rtf"), "{kind}");
    }
    assert!(!stands("text/csv", "text/rtf"));
    // Everything else must match exactly.
    assert!(stands("application/pdf", "application/pdf"));
    assert!(!stands("application/pdf", "application/msword"));
    assert!(!stands("text/plain", "text/x-php"));
}
