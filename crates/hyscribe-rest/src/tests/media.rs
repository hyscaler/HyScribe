// SPDX-License-Identifier: AGPL-3.0-or-later

//! The media, revisions and autosaves routes' goldens, `media.tsv` and
//! `media-ttd.tsv`, replayed for a visitor and each role over the dumps
//! with `reference/rest/media.sql`, and what the goldens cannot show.

use hyscribe_image::subsizes::Crop;
use hyscribe_model::Post;

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

/// Embedding a featured image that is not ported, a video player, answers
/// 501, enveloped or not, and a list with the post too; a visitor, who may
/// not see the trashed video, gets the post.
#[test]
fn embedding_what_is_not_ported() {
    let oracle = oracle(false);
    let get = |who: &str, route: &str, query: &str| {
        let get = hyscribe_phpser::parse_str(query.as_bytes());
        let user = u64::from(who == "administrator");
        let request = crate::Request {
            method: "GET",
            route: route.as_bytes(),
            uri: b"/",
            get: get.as_array().unwrap_or_default(),
            post: &[],
            cookie_user: user,
            origin: None,
            nonce: (user != 0).then_some(b"%nonce%"),
            remote_addr: b"",
            postpass: false,
            user_agent: None,
            method_override: None,
            content_type: None,
            body: b"",
            host: None,
            now: 0,
        };
        crate::serve(&oracle, &request).status
    };
    let requests = [
        ("/wp/v2/posts/9101", "_embed"),
        ("/wp/v2/posts/9101", "_embed=wp:featuredmedia&_envelope"),
        ("/wp/v2/posts", "author=702&_embed"),
    ];
    for (route, query) in requests {
        assert_eq!(get("administrator", route, query), 501, "{route} {query}");
    }
    assert_eq!(get("visitor", "/wp/v2/posts/9101", "_embed"), 200);
}

/// Only the post routes' own regexes are theirs.
#[test]
fn revisions_routes_are_their_own() {
    assert!(crate::revisions::endpoints("/wp/v2/posts/(?P<id>[\\d]+)").is_none());
    assert!(crate::revisions::endpoints("/wp/v2/users").is_none());
}

/// What only those who may edit them see and is not ported: a scaled
/// image's missing sizes, which PHP reads from its original file, a
/// video player, a file size missing from the metadata, and a revision
/// with a gallery shortcode.
const UNPORTED: [(&str, &str); 17] = [
    (
        "administrator",
        "/?rest_route=/wp/v2/media/9139&context=edit",
    ),
    ("administrator", "/?rest_route=/wp/v2/media/9140"),
    (
        "administrator",
        "/?rest_route=/wp/v2/media/9140&_fields=caption",
    ),
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
    ("editor", "/?rest_route=/wp/v2/media/9139&context=edit"),
    ("editor", "/?rest_route=/wp/v2/media/9140"),
    ("editor", "/?rest_route=/wp/v2/media/9140&_fields=caption"),
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
        6 * 148,
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
