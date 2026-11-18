// SPDX-License-Identifier: AGPL-3.0-or-later

//! The files `wp_delete_attachment_files()` deletes, by their names.

use hyscribe_phpser::{Value, unserialize};

use super::{attachment_files, dirname};

fn files(file: &str, meta: &str, backups: &str) -> Vec<(String, String)> {
    let value = |text: &str| unserialize(text.as_bytes()).unwrap_or_default();
    let found = attachment_files(file.as_bytes(), &value(meta), &value(backups)).unwrap();
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
    found.into_iter().map(|(f, d)| (text(f), text(d))).collect()
}

fn pair(file: &str, folder: &str) -> (String, String) {
    (file.into(), folder.into())
}

/// Each size, the original, a source and a video's companions beside the
/// file, its backups beside the metadata's file, and the file itself.
#[test]
fn every_file_of_the_attachment() {
    let meta = "a:6:{s:4:\"file\";s:13:\"2026/05/a.gif\";s:5:\"sizes\";a:1:{s:5:\"thumb\";a:1:{s:4:\"file\";s:9:\"a-1x1.gif\";}}s:14:\"original_image\";s:5:\"o.gif\";s:12:\"source_image\";a:0:{}s:14:\"animated_video\";s:5:\"v.mp4\";s:21:\"animated_video_poster\";s:0:\"\";}";
    let backups = "a:1:{s:9:\"full-orig\";a:1:{s:4:\"file\";s:5:\"b.gif\";}}";
    assert_eq!(
        files("2026/05/a.gif", meta, backups),
        [
            pair("2026/05/a-1x1.gif", "2026/05"),
            pair("2026/05/o.gif", "2026/05"),
            pair("2026/05/v.mp4", "2026/05"),
            pair("2026/05/b.gif", "2026/05"),
            pair("2026/05/a.gif", ""),
        ]
    );
    assert_eq!(files("a.gif", "b:0;", "s:0:\"\";"), [pair("a.gif", "")]);
}

/// A source or video that is no string is left, an original that is none is
/// taken as PHP's `Array`.
#[test]
fn files_that_are_no_string_are_taken_as_php_takes_them() {
    let meta = "a:3:{s:4:\"file\";s:13:\"2026/05/a.gif\";s:14:\"original_image\";a:1:{i:0;s:1:\"x\";}s:12:\"source_image\";a:1:{i:0;s:1:\"y\";}}";
    assert_eq!(
        files("2026/05/a.gif", meta, "b:0;"),
        [pair("2026/05/Array", "2026/05"), pair("2026/05/a.gif", "")]
    );
}

/// A file by its absolute path, or with WordPress 2.6's thumbnail, is not
/// ported.
#[test]
fn old_files_are_not_ported() {
    let absolute = attachment_files(b"/srv/a.gif", &Value::Null, &Value::Null);
    assert!(absolute.is_err());
    let thumb = unserialize(b"a:1:{s:5:\"thumb\";s:5:\"t.gif\";}").unwrap();
    assert!(attachment_files(b"a.gif", &thumb, &Value::Null).is_err());
}

#[test]
fn dirnames_as_php_s() {
    assert_eq!(dirname(b"2026/05/a.gif"), b"2026/05");
    assert_eq!(dirname(b"a.gif"), b".");
    assert_eq!(dirname(b""), b"");
}
