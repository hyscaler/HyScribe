// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;
use sha2::{Digest, Sha256};

use super::*;
use crate::fixture::ThemeTestData;

/// Every case of `reference/content-tags/golden.sh`, by hash: the
/// fixture's content is not ours to commit.
#[test]
fn matches_php() {
    let site = ThemeTestData {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../tests/golden/content-tags.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let loading = Loading::in_the_loop(fields[1] == "1");
        // A post ID, or our own content in hex.
        let content = match fields[0].parse::<u64>() {
            Ok(id) => site.post(id).unwrap().content,
            Err(_) => unhex(fields[0]),
        };
        let ours = wp_filter_content_tags(&site, &loading, &content);
        let hash: String = Sha256::digest(&ours)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if hash != fields[2] {
            let case = String::from_utf8_lossy(&content)
                .chars()
                .take(200)
                .collect::<String>();
            failures.push(format!(
                "{case} {}:\n{}",
                fields[1],
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(2)].join("\n")
    );
    assert_eq!(golden.lines().count(), 66);
}

#[test]
fn edge_cases() {
    use crate::fixture::{attachment, meta, site};
    let mut site = site(vec![]);
    site.posts = (40..=42)
        .map(|id| attachment(id, "image/jpeg", ""))
        .collect();
    site.meta = vec![
        (
            40,
            "_wp_attachment_metadata",
            meta(r#"{"file":"2020/z.jpg","width":10,"height":20}"#),
        ),
        (
            41,
            "_wp_attachment_metadata",
            meta(
                r#"{"file":"2020/e-e1234567890123.jpg","width":100,"height":50,"sizes":{"m":{"file":"e-e1234567890123-50x25.jpg","width":50,"height":25}}}"#,
            ),
        ),
        (
            42,
            "_wp_attachment_metadata",
            meta(
                r#"{"file":"t.jpg","width":100,"height":50,"sizes":{"m":{"file":"t-50x25.jpg","width":50,"height":25}}}"#,
            ),
        ),
    ];
    let filtered = |content: &str| {
        String::from_utf8(wp_filter_content_tags(
            &site,
            &Loading::default(),
            content.as_bytes(),
        ))
        .unwrap()
    };
    let u = "http://s.test/wp-content/uploads";
    // No sizes and another file: no dimensions.
    assert_eq!(
        filtered(&format!(
            r#"<img src="{u}/2020/other.jpg" class="wp-image-40">"#
        )),
        format!(r#"<img decoding="async" src="{u}/2020/other.jpg" class="wp-image-40">"#)
    );
    // A style width that is not in pixels leaves the size alone.
    assert_eq!(
        filtered(&format!(
            r#"<img src="{u}/2020/z.jpg" class="wp-image-40" style="width: auto;">"#
        )),
        format!(
            r#"<img loading="lazy" decoding="async" width="10" height="20" src="{u}/2020/z.jpg" class="wp-image-40" style="width: auto;">"#
        )
    );
    // Edited since it was inserted: no srcset.
    assert_eq!(
        filtered(&format!(
            r#"<img src="{u}/2020/e.jpg" class="wp-image-41" width="100" height="50">"#
        )),
        format!(
            r#"<img loading="lazy" decoding="async" src="{u}/2020/e.jpg" class="wp-image-41" width="100" height="50">"#
        )
    );
    // A tab after the name: none of the replacements find `<img `.
    let tab = format!("<img\tsrc=\"{u}/t.jpg\" class=\"wp-image-42\" width=\"100\" height=\"50\">");
    assert_eq!(filtered(&tab), tab);
    // A vertical tab ends the name for the regular expression, not for HTML.
    let vertical = "<img\x0bsrc=\"a.jpg\" width=\"1\" height=\"1\">";
    assert_eq!(filtered(vertical), vertical);
}

/// A width or height PHP's patterns do not read, empty or with a unit,
/// leaves an image out of the content's media count; quotes may differ.
#[test]
fn unread_sizes_do_not_count() {
    let site = crate::fixture::site(vec![]);
    let big = r#"<img src="b.jpg" width="300" height="300">"#;
    let filtered = |first: &str| {
        // Distinct tags: each is filtered once however often it repeats.
        let content = [1, 2, 3]
            .map(|n| first.replace("a.jpg", &format!("a{n}.jpg")))
            .concat()
            + big;
        let ours = wp_filter_content_tags(&site, &Loading::in_the_loop(true), content.as_bytes());
        String::from_utf8(ours).unwrap()
    };
    for unread in [
        r#"<img src="a.jpg" width="" height="">"#,
        r#"<img src="a.jpg" width="12px" height="12px">"#,
    ] {
        assert!(filtered(unread).ends_with(r#"<img fetchpriority="high" decoding="async" src="b.jpg" width="300" height="300">"#), "{unread}");
    }
    let mixed = filtered(r#"<img src="a.jpg" width="12' height='12">"#);
    assert!(
        mixed.ends_with(
            r#"<img loading="lazy" decoding="async" src="b.jpg" width="300" height="300">"#
        ),
        "{mixed}"
    );
}

/// Sizes PHP sets aside: a zero dimension, an empty source, a style
/// width found after one it cannot read, and a given width of 0.
#[test]
fn sizes_set_aside() {
    use crate::fixture::{attachment, meta, site};
    let mut site = site(vec![]);
    site.posts = (43..=46)
        .map(|id| attachment(id, "image/jpeg", ""))
        .collect();
    site.meta = vec![
        (
            43,
            "_wp_attachment_metadata",
            meta(r#"{"file":"2020/w.jpg","width":0,"height":20}"#),
        ),
        (
            44,
            "_wp_attachment_metadata",
            meta(
                r#"{"file":"y.jpg","width":10,"height":10,"sizes":{"e":{"file":"","width":5,"height":5}}}"#,
            ),
        ),
        (
            45,
            "_wp_attachment_metadata",
            meta(
                r#"{"file":"2020/s.jpg","width":100,"height":50,"sizes":{"m":{"file":"s-50x25.jpg","width":50,"height":25}}}"#,
            ),
        ),
        (
            46,
            "_wp_attachment_metadata",
            meta(r#"{"file":"2020/z.jpg","width":10,"height":20}"#),
        ),
    ];
    let filtered = |content: &str| {
        String::from_utf8(wp_filter_content_tags(
            &site,
            &Loading::default(),
            content.as_bytes(),
        ))
        .unwrap()
    };
    let u = "http://s.test/wp-content/uploads";
    assert_eq!(
        filtered(&format!(
            r#"<img src="{u}/2020/w.jpg" class="wp-image-43">"#
        )),
        format!(r#"<img decoding="async" src="{u}/2020/w.jpg" class="wp-image-43">"#)
    );
    let empty = r#"<img src="" class="wp-image-44" width="5" height="5">"#;
    assert_eq!(
        filtered(empty),
        format!(
            r#"<img loading="lazy" decoding="async"{}"#,
            empty.trim_start_matches("<img")
        )
    );
    assert_eq!(
        filtered(&format!(
            r#"<img src="{u}/2020/z.jpg" class="wp-image-46" style="width:px;" style="width: 5px;">"#
        )),
        format!(
            r#"<img loading="lazy" decoding="async" width="5" height="10" src="{u}/2020/z.jpg" class="wp-image-46" style="width:px;" style="width: 5px;">"#
        )
    );
    assert_eq!(
        filtered(&format!(
            r#"<img src="{u}/2020/s.jpg" class="wp-image-45" width="0" height="5">"#
        )),
        format!(
            r#"<img loading="lazy" decoding="async" src="{u}/2020/s.jpg" class="wp-image-45" width="0" height="5" srcset="{u}/2020/s.jpg 100w, {u}/2020/s-50x25.jpg 50w" sizes="auto, (max-width: 100px) 100vw, 100px" />"#
        )
    );
}

#[test]
fn filters_on_its_own() {
    let site = crate::fixture::site(vec![]);
    let image = br#"<img src="a.jpg" width="9" height="9">"#;
    assert_eq!(
        filter(&site, image, false),
        br#"<img loading="lazy" decoding="async" src="a.jpg" width="9" height="9">"#
    );
    assert_eq!(
        filter(&site, image, true),
        br#"<img decoding="async" src="a.jpg" width="9" height="9">"#
    );
    // Without both sizes there is no loading attribute.
    assert_eq!(
        filter(&site, br#"<img src="a.jpg" width="9">"#, false),
        br#"<img decoding="async" src="a.jpg" width="9">"#
    );
}

/// Given priorities and mixed quotes, seen through the next image.
#[test]
fn given_priorities() {
    let site = crate::fixture::site(vec![]);
    let big = r#"<img src="b.jpg" width="300" height="300">"#;
    let last = |first: &str, copies: usize| {
        let content: String = (1..=copies)
            .map(|n| first.replace("a.jpg", &format!("a{n}.jpg")))
            .collect::<String>()
            + big;
        let ours = wp_filter_content_tags(&site, &Loading::in_the_loop(true), content.as_bytes());
        let ours = String::from_utf8(ours).unwrap();
        ours.rsplit("<img")
            .next()
            .map(|s| format!("<img{s}"))
            .unwrap()
    };
    let high =
        r#"<img fetchpriority="high" decoding="async" src="b.jpg" width="300" height="300">"#;
    let plain = r#"<img decoding="async" src="b.jpg" width="300" height="300">"#;
    // Low priority images do not count.
    assert_eq!(
        last(
            r#"<img src="a.jpg" width="300" height="300" fetchpriority="low">"#,
            3
        ),
        high
    );
    // A lazy high priority image leaves the flag.
    assert_eq!(
        last(
            r#"<img src="a.jpg" width="300" height="300" fetchpriority="high" loading="lazy">"#,
            1
        ),
        high
    );
    // One in the viewport takes it.
    assert_eq!(
        last(
            r#"<img src="a.jpg" width="300" height="300" fetchpriority="high">"#,
            1
        ),
        plain
    );
    assert_eq!(
        filter(&site, br#"<img src="a.jpg" width="9" height='9'>"#, false),
        br#"<img decoding="async" src="a.jpg" width="9" height='9'>"#
    );
}
