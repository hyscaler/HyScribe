// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::json_decode;
use hyscribe_testkit::unhex;
use sha2::{Digest, Sha256};

use super::*;
use crate::fixture::{ThemeTestData, attachment, meta, site};

/// Every case of `reference/attachment-image/golden.sh`, by hash: the
/// fixture's content is not ours to commit.
#[test]
fn matches_php() {
    let site = ThemeTestData {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../tests/golden/attachment-image.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let id: u64 = fields[0].parse().unwrap();
        let attrs = json_decode(&unhex(fields[2])).unwrap();
        let attrs: Vec<(String, Vec<u8>)> = attrs
            .as_array()
            .unwrap()
            .iter()
            .map(|(k, v)| {
                (
                    String::from_utf8(k.to_php_string()).unwrap(),
                    v.to_php_string(),
                )
            })
            .collect();
        let attrs: Vec<(&str, Vec<u8>)> =
            attrs.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
        let ours = wp_get_attachment_image(
            &site,
            &Loading::default(),
            id,
            fields[1],
            &attrs,
            &ImageOptions::default(),
        );
        let hash: String = Sha256::digest(&ours)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if hash != fields[3] {
            // Only names and hashes: the fixture's content must not reach logs; `RAW=1`
            // on the golden script shows PHP's HTML locally.
            failures.push(format!("{id} {} {}: ours {hash}", fields[1], fields[2]));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(10)].join("\n")
    );
    assert_eq!(golden.lines().count(), 1053);
}

#[test]
fn upload_urls_follow_the_options() {
    let base = |options| String::from_utf8(upload_baseurl(&site_options(&site(options)))).unwrap();
    assert_eq!(base(vec![]), "http://s.test/wp-content/uploads");
    assert_eq!(base(vec![("upload_path", "media")]), "http://s.test/media");
    assert_eq!(
        base(vec![("upload_path", "/srv/up")]),
        "http://s.test/wp-content/uploads"
    );
    assert_eq!(
        base(vec![("upload_url_path", "http://cdn.test/u")]),
        "http://cdn.test/u"
    );
}

#[test]
fn attachment_urls() {
    let mut site = site(vec![]);
    site.posts = vec![
        attachment(5, "image/jpeg", "http://g.test/x.jpg"),
        attachment(6, "image/jpeg", ""),
    ];
    site.posts.extend([
        attachment(7, "image/jpeg", ""),
        attachment(8, "image/jpeg", ""),
    ]);
    site.meta = vec![
        (
            7,
            "_wp_attached_file",
            b"/var/www/wp-content/uploads/2020/01/a.jpg".to_vec(),
        ),
        (
            8,
            "_wp_attached_file",
            b"/x/wp-content/uploads/a.jpg".to_vec(),
        ),
    ];
    let url = |id| attachment_url(&site, id).map(|u| String::from_utf8(u).unwrap());
    assert_eq!(url(5).as_deref(), Some("http://g.test/x.jpg"));
    assert_eq!(url(6), None);
    assert_eq!(
        url(7).as_deref(),
        Some("http://s.test/wp-content/uploads/2020/01/a.jpg")
    );
    assert_eq!(
        url(8).as_deref(),
        Some("http://s.test/wp-content/uploads/a.jpg")
    );
    assert_eq!(relative_path(b"a.jpg"), b"");
    assert_eq!(replace(b"x/a.jpg/a.jpg", b"a.jpg", b"b"), b"x/b/b");
    assert_eq!(find(b"", 0, b""), Some(0));
    assert_eq!(image_downsize(&site, 6, "full"), None);
}

#[test]
fn downsizes_files_that_are_not_images() {
    let mut site = site(vec![
        ("thumbnail_size_w", "150"),
        ("thumbnail_size_h", "150"),
        ("medium_size_w", "300"),
        ("medium_size_h", "300"),
    ]);
    site.posts = vec![
        hyscribe_model::Post {
            id: 9,
            post_type: "post".into(),
            ..Default::default()
        },
        attachment(10, "application/pdf", ""),
        attachment(11, "application/pdf", ""),
        attachment(12, "import", ""),
        attachment(13, "import", ""),
        attachment(14, "image/jpeg", ""),
    ];
    let full = r#"{"sizes":{"full":{"file":"doc-pdf.jpg","width":100,"height":200},"thumbnail":{"file":"doc-pdf-150x150.jpg","width":150,"height":150}}}"#;
    site.meta = vec![
        (10, "_wp_attached_file", b"2020/doc.pdf".to_vec()),
        (10, "_wp_attachment_metadata", meta(full)),
        (11, "_wp_attached_file", b"2020/doc.pdf".to_vec()),
        (12, "_wp_attached_file", b"2020/p.png".to_vec()),
        (
            12,
            "_wp_attachment_metadata",
            meta(r#"{"width":10,"height":20,"file":"2020/p.png","sizes":{}}"#),
        ),
        (13, "_wp_attached_file", b"2020/p.txt".to_vec()),
        (14, "_wp_attached_file", b"a.jpg".to_vec()),
        (14, "_wp_attachment_metadata", b"not serialized".to_vec()),
    ];
    let down = |id, size| {
        image_downsize(&site, id, size).map(|(u, w, h)| (String::from_utf8(u).unwrap(), w, h))
    };
    let uploads = "http://s.test/wp-content/uploads";
    assert_eq!(down(9, "full"), None);
    assert_eq!(
        down(10, "full"),
        Some((format!("{uploads}/2020/doc-pdf.jpg"), 100, 200))
    );
    assert_eq!(
        down(10, "thumbnail"),
        Some((format!("{uploads}/2020/doc-pdf-150x150.jpg"), 150, 150))
    );
    assert_eq!(down(11, "full"), None);
    assert_eq!(
        down(12, "medium"),
        Some((format!("{uploads}/2020/p.png"), 10, 20))
    );
    assert_eq!(down(13, "full"), None);
    let html = wp_get_attachment_image(
        &site,
        &Loading::default(),
        14,
        "full",
        &[],
        &ImageOptions::default(),
    );
    assert_eq!(
        String::from_utf8(html).unwrap(),
        format!(
            "<img src=\"{uploads}/a.jpg\" class=\"attachment-full size-full\" alt=\"\" decoding=\"async\" loading=\"lazy\" />"
        )
    );
}

#[test]
fn constrains_dimensions() {
    assert_eq!(
        constrain_for_editor(&site(vec![]), 1000, 1000, "thumbnail"),
        (96, 96)
    );
    let unlimited = site(vec![("medium_size_w", "0"), ("medium_size_h", "0")]);
    assert_eq!(
        constrain_for_editor(&unlimited, 500, 400, "medium"),
        (500, 400)
    );
    // One pixel short of the limit rounds up to it.
    assert_eq!(constrain_dimensions(600, 401, 300, 200), (300, 200));
    assert_eq!(constrain_dimensions(401, 600, 200, 300), (200, 300));
    // A side already at its limit is not rounded up.
    assert_eq!(constrain_dimensions(1000, 1000, 1000, 999), (999, 999));
    // The larger ratio when it fits both sides, by rounding.
    assert_eq!(constrain_dimensions(10, 1000, 4, 420), (4, 420));
    assert_eq!(constrain_dimensions(1000, 10, 420, 4), (420, 4));
    // A side one short of a limit it was not scaled to stays.
    assert_eq!(constrain_dimensions(100, 299, 300, 300), (100, 299));
    assert_eq!(constrain_dimensions(299, 100, 300, 300), (299, 100));
}

#[test]
fn srcsets() {
    let made = |size, src: &str, json: &str| {
        srcset(
            size,
            src.as_bytes(),
            &json_decode(json.as_bytes()).unwrap(),
            b"http://s.test/u/",
        )
    };
    let sized = r#"{"file":"a.jpg","width":1000,"height":500,"sizes":{"bad":"x","m":{"file":"a-300x150.jpg","width":300,"height":150},"m2":{"file":"a-300x150b.jpg","width":300,"height":150},"big":{"file":"a-4000x2000.jpg","width":4000,"height":2000}}}"#;
    assert_eq!(made((0, 0), "http://s.test/u/a.jpg", sized), None);
    // Same widths replace each other; the source comes first.
    assert_eq!(
        made((1000, 500), "http://s.test/u/a.jpg", sized).as_deref(),
        Some("http://s.test/u/a.jpg 1000w, http://s.test/u/a-300x150b.jpg 300w")
    );
    let edited = r#"{"file":"2020/a-e1234567890123.jpg","width":1000,"height":500,"sizes":{"m":{"file":"a-300x150.jpg","width":300,"height":150},"e":{"file":"a-e1234567890123-300x150.jpg","width":300,"height":150},"z":{"file":"-e1234567890123-z.jpg","width":300,"height":150}}}"#;
    assert_eq!(
        made(
            (1000, 500),
            "http://s.test/u/2020/a-e1234567890123.jpg",
            edited
        )
        .as_deref(),
        Some(
            "http://s.test/u/2020/a-e1234567890123.jpg 1000w, http://s.test/u/2020/a-e1234567890123-300x150.jpg 300w"
        )
    );
    let gif = r#"{"file":"g.gif","width":200,"height":200,"sizes":{"thumbnail":{"file":"g-150x150.gif","width":150,"height":150,"mime-type":"image/gif"},"medium":{"file":"g-100x100.gif","width":100,"height":100,"mime-type":"image/gif"}}}"#;
    assert_eq!(made((200, 200), "http://s.test/u/g.gif", gif), None);
    assert_eq!(
        made((150, 150), "http://s.test/u/g-150x150.gif", gif).as_deref(),
        Some("http://s.test/u/g-150x150.gif 150w, http://s.test/u/g-100x100.gif 100w")
    );
    let square = r#"{"file":"q.jpg","width":2,"height":2,"sizes":{"t":{"file":"q-1x1.jpg","width":1,"height":1}}}"#;
    assert_eq!(
        made((1, 1), "http://s.test/u/q-1x1.jpg", square).as_deref(),
        Some("http://s.test/u/q-1x1.jpg 1w, http://s.test/u/q.jpg 2w")
    );
    let four = r#"{"file":"q.jp","width":2,"height":2,"sizes":{"t":{"file":"q-1x1.jp","width":1,"height":1}}}"#;
    assert_eq!(
        made((1, 1), "http://s.test/u/q-1x1.jp", four).as_deref(),
        Some("http://s.test/u/q-1x1.jp 1w, http://s.test/u/q.jp 2w")
    );
    // Only the first file the source contains is the source.
    let later = r#"{"file":"a.jpg","width":1000,"height":500,"sizes":{"m":{"file":"a-300x150.jpg","width":300,"height":150},"big":{"file":"x150.jpg","width":600,"height":300},"wide":{"file":"a-2048.jpg","width":2048,"height":1024}}}"#;
    assert_eq!(
        made((300, 150), "http://s.test/u/a-300x150.jpg", later).as_deref(),
        Some(
            "http://s.test/u/a-300x150.jpg 300w, http://s.test/u/x150.jpg 600w, http://s.test/u/a-2048.jpg 2048w, http://s.test/u/a.jpg 1000w"
        )
    );
    // Thirteen digits without `-e` do not mark an edited image.
    let digits = r#"{"file":"a-x1234567890123.jpg","width":1000,"height":500,"sizes":{"m":{"file":"a-300x150.jpg","width":300,"height":150}}}"#;
    assert_eq!(
        made((1000, 500), "http://s.test/u/a-x1234567890123.jpg", digits).as_deref(),
        Some("http://s.test/u/a-x1234567890123.jpg 1000w, http://s.test/u/a-300x150.jpg 300w")
    );
    assert_eq!(
        made((10, 10), "a", r#"{"file":"a","sizes":{"m":{}}}"#),
        None
    );
    assert_eq!(
        made((10, 10), "a", r#"{"file":"abcd.jpg","sizes":{}}"#),
        None
    );
}

#[test]
fn given_attributes() {
    let mut site = site(vec![("medium_size_w", "300"), ("medium_size_h", "300")]);
    site.posts = vec![attachment(20, "image/jpeg", "")];
    let stored = r#"{"file":"2020/a.jpg","width":1000,"height":500,"sizes":{"medium":{"file":"a-300x150.jpg","width":300,"height":150,"mime-type":"image/jpeg"}}}"#;
    site.meta = vec![
        (20, "_wp_attached_file", b"2020/a.jpg".to_vec()),
        (20, "_wp_attachment_metadata", meta(stored)),
        (20, "_wp_attachment_image_alt", b" <b>Alt</b> ".to_vec()),
    ];
    let image = |attr: &[(&str, Vec<u8>)]| {
        String::from_utf8(wp_get_attachment_image(
            &site,
            &Loading::default(),
            20,
            "medium",
            attr,
            &ImageOptions::default(),
        ))
        .unwrap()
    };
    let u = "http://s.test/wp-content/uploads/2020";
    let srcset = format!("{u}/a-300x150.jpg 300w, {u}/a.jpg 1000w");
    assert_eq!(
        image(&[("sizes", b"AUTO, 50vw".to_vec())]),
        format!(
            "<img width=\"300\" height=\"150\" src=\"{u}/a-300x150.jpg\" class=\"attachment-medium size-medium\" alt=\"Alt\" sizes=\"AUTO, 50vw\" decoding=\"async\" loading=\"lazy\" srcset=\"{srcset}\" />"
        )
    );
}

/// The site logo's size filter leaves an image with a side of 0 alone, as
/// a stored size with no height and no size limits gives.
#[test]
fn resize_needs_both_sides() {
    let mut site = site(vec![]);
    site.posts = vec![attachment(30, "image/jpeg", "")];
    let stored = r#"{"file":"b.jpg","sizes":{"medium":{"file":"b-w.jpg","width":80}}}"#;
    site.meta = vec![
        (30, "_wp_attached_file", b"b.jpg".to_vec()),
        (30, "_wp_attachment_metadata", meta(stored)),
    ];
    assert_eq!(
        image_downsize(&site, 30, "medium").map(|(_, w, h)| (w, h)),
        Some((80, 0))
    );
    let resize = |_, _| (7, 7);
    let options = ImageOptions {
        resize: Some(&resize),
        ..ImageOptions::default()
    };
    let html = wp_get_attachment_image(&site, &Loading::default(), 30, "medium", &[], &options);
    assert!(
        String::from_utf8(html)
            .unwrap()
            .starts_with("<img width=\"80\" src=")
    );
}

#[test]
fn stored_sizes_over_their_limits_shrink() {
    let mut site = site(vec![("medium_size_w", "300"), ("medium_size_h", "300")]);
    site.posts = vec![attachment(30, "image/jpeg", "")];
    let stored = r#"{"file":"b.jpg","width":100,"height":100,"sizes":{"medium":{"file":"b-m.jpg","width":400,"height":200},"1536x1536":{"file":"b-l.jpg","width":2000,"height":1000},"2048x2048":{"file":"b-x.jpg","width":3000,"height":1500},"odd":{"file":"b-o.jpg","height":50}}}"#;
    site.meta = vec![
        (30, "_wp_attached_file", b"b.jpg".to_vec()),
        (30, "_wp_attachment_metadata", meta(stored)),
    ];
    let size = |name| image_downsize(&site, 30, name).map(|(_, w, h)| (w, h));
    assert_eq!(size("medium"), Some((300, 150)));
    assert_eq!(size("1536x1536"), Some((1536, 768)));
    assert_eq!(size("2048x2048"), Some((2048, 1024)));
    // A size without a width keeps its height, not the original's; no
    // side shrinks below 1.
    assert_eq!(size("odd"), Some((1, 50)));
}

/// Negative stored sizes print as their `absint()`; `"0"` options and files
/// are PHP's empty.
#[test]
fn falsy_and_negative_values() {
    let mut site = site(vec![
        ("medium_size_w", "0"),
        ("medium_size_h", "0"),
        ("upload_url_path", "0"),
    ]);
    site.posts = vec![
        attachment(50, "image/jpeg", "http://g.test/n.jpg"),
        attachment(51, "image/jpeg", "http://g.test/zero.jpg"),
    ];
    site.meta = vec![
        (50, "_wp_attached_file", b"n.jpg".to_vec()),
        (
            50,
            "_wp_attachment_metadata",
            meta(r#"{"file":"n.jpg","width":-5,"height":-7}"#),
        ),
        (51, "_wp_attached_file", b"0".to_vec()),
    ];
    let html = String::from_utf8(wp_get_attachment_image(
        &site,
        &Loading::default(),
        50,
        "medium",
        &[],
        &ImageOptions::default(),
    ))
    .unwrap();
    assert!(
        html.starts_with(
            r#"<img width="5" height="7" src="http://s.test/wp-content/uploads/n.jpg""#
        ),
        "{html}"
    );
    assert_eq!(
        attachment_url(&site, 51).as_deref(),
        Some(&b"http://g.test/zero.jpg"[..])
    );
}

#[test]
fn php_paths() {
    // `dirname()` keeps a root and squeezes doubled slashes.
    assert_eq!(relative_path(b"/a.jpg"), b"/");
    assert_eq!(relative_path(b"2020//a.jpg"), b"2020");
    // An imported file is an image by a lower case extension after a dot.
    assert!(image_extension(b"a.png"));
    assert!(!image_extension(b"a.PNG"));
    assert!(!image_extension(b"png"));
    let base = |options| String::from_utf8(upload_baseurl(&site_options(&site(options)))).unwrap();
    let mut slashed = vec![("upload_path", " files ")];
    slashed.push(("siteurl", "http://t.test/"));
    // The fixture site's own siteurl comes later, so this one is found first.
    assert_eq!(base(slashed), "http://t.test/files");
}
