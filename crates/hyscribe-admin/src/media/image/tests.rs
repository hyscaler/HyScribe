// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{Maybe, maybe_unserialize};

use super::*;
use crate::tests::Role;

/// Metadata as `wp_generate_attachment_metadata()` stores it.
fn meta(serialized: &str) -> Value {
    match maybe_unserialize(serialized.as_bytes()) {
        Maybe::Value(value) => value,
        _ => unreachable!("{serialized}"),
    }
}

/// A size entry of `width` by `height` in `file`.
fn size(name: &str, file: &str, width: u32, height: u32) -> String {
    format!(
        "s:{}:\"{name}\";a:3:{{s:4:\"file\";s:{}:\"{file}\";s:5:\"width\";i:{width};s:6:\"height\";i:{height};}}",
        name.len(),
        file.len()
    )
}

fn sizes(entries: &[String]) -> Value {
    meta(&format!(
        "a:4:{{s:4:\"file\";s:10:\"2026/a.jpg\";s:5:\"width\";i:600;s:6:\"height\";i:600;s:5:\"sizes\";a:{}:{{{}}}}}",
        entries.len(),
        entries.concat()
    ))
}

/// The box's own size wins at once; else the smallest of its ratio that
/// covers it; else the thumbnail when it is wide enough.
#[test]
fn the_box_picks_a_size() {
    let exact = sizes(&[
        size("tiny", "a-60x60.jpg", 60, 60),
        size("big", "a-90x90.jpg", 90, 90),
    ]);
    assert_eq!(
        intermediate(&exact),
        Some((b"a-60x60.jpg".to_vec(), 60, 60))
    );
    let square = sizes(&[
        size("big", "a-300x300.jpg", 300, 300),
        size("small", "a-90x90.jpg", 90, 90),
    ]);
    assert_eq!(
        intermediate(&square),
        Some((b"a-90x90.jpg".to_vec(), 60, 60))
    );
    let wide = sizes(&[size("thumbnail", "a-150x100.jpg", 150, 100)]);
    assert_eq!(
        intermediate(&wide),
        Some((b"a-150x100.jpg".to_vec(), 60, 40))
    );
    // A size the box's width or height fits, but not its ratio, is no match.
    let tall = sizes(&[
        size("tall", "a-60x90.jpg", 60, 90),
        size("mid", "a-75x75.jpg", 75, 75),
    ]);
    assert_eq!(intermediate(&tall), Some((b"a-75x75.jpg".to_vec(), 60, 60)));
    let wide = sizes(&[
        size("wide", "a-90x60.jpg", 90, 60),
        size("mid", "a-75x75.jpg", 75, 75),
    ]);
    assert_eq!(intermediate(&wide), Some((b"a-75x75.jpg".to_vec(), 60, 60)));
    let low = sizes(&[
        size("low", "a-70x50.jpg", 70, 50),
        size("mid", "a-75x75.jpg", 75, 75),
    ]);
    assert_eq!(intermediate(&low), Some((b"a-75x75.jpg".to_vec(), 60, 60)));
    // The smaller wins in either order; of two alike, the later.
    let rising = sizes(&[
        size("mid", "a-75x75.jpg", 75, 75),
        size("big", "a-90x90.jpg", 90, 90),
    ]);
    assert_eq!(
        intermediate(&rising),
        Some((b"a-75x75.jpg".to_vec(), 60, 60))
    );
    let twins = sizes(&[size("one", "u.jpg", 75, 75), size("two", "v.jpg", 75, 75)]);
    assert_eq!(intermediate(&twins), Some((b"v.jpg".to_vec(), 60, 60)));
    let narrow = sizes(&[size("thumbnail", "a-50x50.jpg", 50, 50)]);
    assert_eq!(intermediate(&narrow), None);
    assert_eq!(intermediate(&meta("a:0:{}")), None);
}

/// A file that is not an image shows its preview's full size, and nothing
/// without one; an image without sizes its own.
#[test]
fn files_downsize_to_their_preview() {
    let url = b"http://x.test/2026/doc.pdf".to_vec();
    let preview = sizes(&[size("full", "doc-pdf.jpg", 1088, 1408)]);
    assert_eq!(
        downsize(url.clone(), &preview, false),
        Some((b"http://x.test/2026/doc-pdf.jpg".to_vec(), 46, 60))
    );
    assert_eq!(downsize(url.clone(), &meta("a:0:{}"), false), None);
    // A preview without a width keeps its own size, not the file's.
    let flat = meta(&format!(
        "a:3:{{s:5:\"width\";i:120;s:6:\"height\";i:30;s:5:\"sizes\";a:1:{{{}}}}}",
        size("full", "doc-pdf.jpg", 0, 50)
    ));
    let pdf = b"http://x.test/2026/doc.pdf".to_vec();
    assert_eq!(
        downsize(pdf, &flat, false).map(|(_, w, h)| (w, h)),
        Some((1, 50))
    );
    let plain = meta("a:2:{s:5:\"width\";i:120;s:6:\"height\";i:30;}");
    assert_eq!(
        downsize(url, &plain, true).map(|(_, w, h)| (w, h)),
        Some((60, 15))
    );
}

/// A square image with square sizes lists them in a `srcset`.
#[test]
fn square_sizes_make_a_srcset() {
    let square = sizes(&[
        size("thumbnail", "a-150x150.jpg", 150, 150),
        size("medium", "a-300x300.jpg", 300, 300),
    ]);
    let base = b"http://x.test/wp-content/uploads";
    let html = img(
        &[base.as_slice(), b"/2026/a-150x150.jpg"].concat(),
        (60, 60),
        &square,
        base,
    );
    assert!(html.contains(" srcset=\"http://x.test/wp-content/uploads/2026/a-150x150.jpg 150w, http://x.test/wp-content/uploads/2026/a-300x300.jpg 300w, http://x.test/wp-content/uploads/2026/a.jpg 600w\" sizes=\"auto, (max-width: 60px) 100vw, 60px\" />"), "{html}");
}

/// The icon of the file's type, by extension, then by MIME type.
#[test]
fn files_show_their_type_s_icon() {
    assert_eq!(icon_name(b"2026/a.MP3", "audio/mpeg"), "audio");
    assert_eq!(icon_name(b"2026/a.pdf", "application/pdf"), "document");
    assert_eq!(icon_name(b"2026/notes", "text/plain"), "text");
    assert_eq!(icon_name(b"2026/mp3", ""), "default");
    assert_eq!(icon_name(b"2026/a.jpg", "image/jpeg"), "default");
    // An attachment without a file is at its GUID.
    let site = Role("m4ttd:administrator");
    let bare = Post {
        id: 999_999,
        post_type: "attachment".into(),
        guid: b"http://x.test/a.pdf".to_vec(),
        ..Post::default()
    };
    assert_eq!(attachment_url(&site, &bare), Some(bare.guid.clone()));
    // A post that is not an attachment has the default icon.
    let html = thumbnail(&Role("m4ttd:administrator"), 999_999);
    assert!(
        html.contains("/wp-includes/images/media/default.svg"),
        "{html}"
    );
}
