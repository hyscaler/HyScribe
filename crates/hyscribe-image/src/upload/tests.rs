// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;
use crate::tests::corpus;

const GOLDEN: &str = include_str!("../../tests/golden/uploads.tsv");

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
    assert_eq!(checks.len(), 25);
    for fields in checks {
        let [_, corpus_name, upload, ext, kind, proper] = fields[..] else {
            unreachable!()
        };
        let checked = wp_check_filetype_and_ext(&corpus(corpus_name), upload, false).unwrap();
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
    // Not an image by its extension: PHP asks fileinfo.
    assert_eq!(
        wp_check_filetype_and_ext(b"x", "a.pdf", false),
        Err(Unported)
    );
    assert_eq!(
        wp_check_filetype_and_ext(b"x", "a.jpg", false),
        Err(Unported)
    );
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
    // Found by fuzzing: a name ending in a character of two bytes.
    assert_eq!(sanitize_file_name("\u{7ce}\n".as_bytes(), false), "\u{7ce}");
    assert_eq!(wp_check_filetype("\u{7ce}", false), None);
    assert_eq!(wp_check_filetype("\u{e9}a", false), None);
}
