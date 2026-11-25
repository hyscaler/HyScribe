// SPDX-License-Identifier: AGPL-3.0-or-later

//! The media, revisions and autosaves routes' goldens, `media.tsv` and
//! `media-ttd.tsv`, replayed for a visitor and each role over the dumps
//! with `reference/rest/media.sql`, and what the goldens cannot show.

use hyscribe_image::subsizes::Crop;
use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value};

use super::posts::{Oracle, replay};
use super::site;
use crate::{Context, Site};

/// The administrator over the default install with the media rows.
fn oracle(pretty: bool) -> Oracle {
    Oracle {
        role: "administrator",
        fixture: site::Fixture {
            ttd: false,
            pretty,
            extra: false,
            state: Some(site::with_media(false)),
        },
        deny: &[],
    }
}

fn context(oracle: &Oracle) -> Context<'_> {
    Context {
        site: oracle,
        user: 1,
        head: false,
        uri: b"/",
        parameters: false,
        remote_addr: b"",
        postpass: false,
        user_agent: None,
        host: b"",
        now: 0,
    }
}

/// `get_post_status()` of attachments: a draft's image is a draft, an
/// image in a trashed post has its status before, one without a parent
/// or whose parent is gone is published, a private one private, and one
/// of another status published.
#[test]
fn attachments_inherit_their_status() {
    let oracle = oracle(false);
    let statuses = [
        (9133, "draft"),
        (9145, "draft"),
        (9146, "publish"),
        (9151, "publish"),
        (9135, "private"),
        (9149, "publish"),
    ];
    for (id, status) in statuses {
        let post = oracle.post(id).unwrap();
        let found = crate::media::post_status(&context(&oracle), &post);
        assert_eq!(found, status, "{id}");
    }
    let pending = Post {
        status: "pending".into(),
        ..oracle.post(9132).unwrap()
    };
    let found = crate::media::post_status(&context(&oracle), &pending);
    assert_eq!(found, "publish");
}

/// A trashed attachment and one that is its own parent keep their plain
/// links under pretty permalinks.
#[test]
fn attachment_links_stay_plain() {
    let oracle = oracle(true);
    let plain = |id| format!("http://127.0.0.1:8081/?attachment_id={id}").into_bytes();
    let trashed = oracle.post(9136).unwrap();
    let link = crate::media::attachment_link(&context(&oracle), &trashed, false);
    assert_eq!(link, plain(9136));
    let own = Post {
        id: 1,
        parent: 1,
        ..oracle.post(9131).unwrap()
    };
    let link = crate::media::attachment_link(&context(&oracle), &own, false);
    assert_eq!(link, plain(1));
}

/// The image sizes the default install registers, from its options.
#[test]
fn image_sizes_are_registered() {
    let oracle = oracle(false);
    let sizes = crate::media::registered_sizes(&context(&oracle));
    let sizes: Vec<_> = sizes
        .iter()
        .map(|size| {
            (
                size.name.as_str(),
                size.width,
                size.height,
                size.crop != Crop::No,
            )
        })
        .collect();
    assert_eq!(
        sizes,
        [
            ("thumbnail", 150, 150, true),
            ("medium", 300, 300, false),
            ("medium_large", 768, 0, false),
            ("large", 1024, 1024, false),
            ("1536x1536", 1536, 1536, false),
            ("2048x2048", 2048, 2048, false),
        ]
    );
}

/// Only the post routes' own regexes are theirs.
#[test]
fn revisions_routes_are_their_own() {
    assert!(crate::revisions::endpoints("/wp/v2/posts/(?P<id>[\\d]+)").is_none());
    assert!(crate::revisions::endpoints("/wp/v2/users").is_none());
}

/// What only those who may edit them see and is not ported: a file size
/// missing from the metadata, and a revision with a gallery shortcode.
const UNPORTED: [(&str, &str); 11] = [
    ("administrator", "/?rest_route=/wp/v2/media/9141"),
    (
        "administrator",
        "/?rest_route=/wp/v2/media/9152&_fields=filesize",
    ),
    (
        "administrator",
        "/?rest_route=/wp/v2/media/9153&_fields=filesize",
    ),
    ("administrator", "/?rest_route=/wp/v2/media/9157"),
    ("administrator", "/?rest_route=/wp/v2/posts/9106/revisions"),
    ("editor", "/?rest_route=/wp/v2/media/9141"),
    ("editor", "/?rest_route=/wp/v2/media/9152&_fields=filesize"),
    ("editor", "/?rest_route=/wp/v2/media/9153&_fields=filesize"),
    ("editor", "/?rest_route=/wp/v2/media/9157"),
    ("editor", "/?rest_route=/wp/v2/posts/9106/revisions"),
    ("author", "/?rest_route=/wp/v2/media/9157"),
];

/// The routes over the default install: `reference/rest/media.tsv`.
#[test]
fn media_match_php() {
    replay(
        include_str!("../../tests/golden/media.tsv"),
        (false, true),
        6 * 152,
        &UNPORTED,
    );
}

/// The media routes over the Theme Test Data fixture:
/// `reference/rest/media-ttd.tsv`.
#[test]
fn theme_test_data_media_match_php() {
    replay(
        include_str!("../../tests/golden/media-ttd.tsv"),
        (true, true),
        6 * 16,
        &[],
    );
}

/// A site that does not say it reads its uploads has no image sizes.
#[test]
fn a_site_without_uploads_reads_no_image_sizes() {
    let oracle = crate::tests::Oracle::over(super::content::default_install(), false);
    assert_eq!(Site::image_size(&oracle, "2026/10/big.jpg"), None);
}

/// The scaled image 9139's `missing_image_sizes` as the administrator
/// gets them over an uploads folder as `deny` names it.
fn missing_of_scaled(deny: &'static [&'static str]) -> (u16, String) {
    let oracle = Oracle {
        deny,
        ..oracle(false)
    };
    let get = hyscribe_phpser::parse_str(b"context=edit&_fields=missing_image_sizes");
    let request = crate::Request {
        method: "GET",
        route: b"/wp/v2/media/9139",
        uri: b"/",
        get: get.as_array().unwrap_or_default(),
        post: &[],
        cookie_user: 1,
        origin: None,
        nonce: Some(b"%nonce%"),
        remote_addr: b"",
        postpass: false,
        user_agent: None,
        method_override: None,
        content_type: None,
        body: b"",
        host: None,
        now: 0,
    };
    let served = crate::serve(&oracle, &request);
    (served.status, String::from_utf8(served.body).unwrap())
}

/// The original image's size, as PHP reads it from the file, is the full
/// size the missing sizes are made of: the scaled copy's metadata says
/// 2560 by 1600, an original of 1000 by 600 leaves out the larger sizes.
/// A site that does not read its uploads answers 501, and so does a name
/// that leaves the attachment's folder.
#[test]
fn missing_sizes_follow_the_original_image() {
    let (status, body) = missing_of_scaled(&[]);
    assert_eq!(status, 200);
    assert!(body.contains("\"large\""), "{body}");
    let (status, body) = missing_of_scaled(&["small_originals"]);
    assert_eq!(status, 200);
    assert!(!body.contains("\"large\""), "{body}");
    assert!(body.contains("\"medium_large\""), "{body}");
    assert_eq!(missing_of_scaled(&["no_uploads"]).0, 501);
    let oracle = Oracle {
        deny: &["small_originals"],
        ..oracle(false)
    };
    let join = crate::media::original_path;
    assert_eq!(
        join("2026/10/a-scaled.jpg", "a.jpg").unwrap(),
        "2026/10/a.jpg"
    );
    assert_eq!(join("a-scaled.jpg", "a.jpg").unwrap(), "a.jpg");
    for (attached, original) in [
        ("2026/10/a-scaled.jpg", "../a.jpg"),
        ("2026/10/a-scaled.jpg", "x/../../a.jpg"),
        ("2026/10/a-scaled.jpg", "/etc/a.jpg"),
        ("/etc/a-scaled.jpg", "a.jpg"),
        ("a-scaled.jpg", "../a.jpg"),
    ] {
        assert_eq!(join(attached, original), None, "{attached} {original}");
    }
    let escaping = Value::Array(vec![(
        Key::String(b"original_image".to_vec()),
        Value::String(b"../big.jpg".to_vec()),
    )]);
    let found = crate::media::original_image_size(&context(&oracle), 9139, &escaping);
    assert!(found.is_err());
    // A name beside the attached file, or none, is read or skipped.
    let beside = Value::Array(vec![(
        Key::String(b"original_image".to_vec()),
        Value::String(b"big.jpg".to_vec()),
    )]);
    let found = crate::media::original_image_size(&context(&oracle), 9139, &beside);
    assert_eq!(found.unwrap(), Some((1000, 600)));
    let none = Value::Array(Vec::new());
    let found = crate::media::original_image_size(&context(&oracle), 9139, &none);
    assert_eq!(found.unwrap(), None);
}
