// SPDX-License-Identifier: AGPL-3.0-or-later

//! The media, revisions and autosaves routes' goldens, `media.tsv` and
//! `media-ttd.tsv`, replayed for a visitor and each role over the dumps
//! with `reference/rest/media.sql`, and what the goldens cannot show.

use hyscribe_phpser::parse_str;

use super::posts::{Oracle, replay};
use super::{role, site};
use crate::{Request, Served, serve};

/// `target`, a route and its query, as `who` without the capabilities
/// `deny` names, over the default install with the media rows.
fn served(who: &'static str, target: &str, deny: &'static [&'static str]) -> Served {
    let (route, query) = target.split_once('&').unwrap_or((target, ""));
    let get = parse_str(query.as_bytes());
    let (_, user) = role(who);
    let oracle = Oracle {
        role: who,
        fixture: site::Fixture {
            ttd: false,
            pretty: false,
            extra: false,
            media: true,
        },
        deny,
    };
    let request = Request {
        method: "GET",
        route: route.as_bytes(),
        uri: b"/",
        get: get.as_array().unwrap_or_default(),
        post: &[],
        cookie_user: user,
        origin: None,
        nonce: (user != 0).then_some(b"%nonce%"),
        method_override: None,
    };
    serve(&oracle, &request)
}

/// A user who may delete a post but not edit it may delete none of its
/// revisions, which `Allow` leaves out.
#[test]
fn deleting_revisions_needs_editing() {
    let target = "/wp/v2/posts/1/revisions/9112";
    let served = served("administrator", target, &["edit_post"]);
    assert_eq!(served.status, 403);
    assert!(!served.headers.iter().any(|(name, _)| name == "Allow"));
    let all = self::served("administrator", target, &[]);
    assert_eq!(super::posts::allow(&all), "GET");
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
const UNPORTED: [(&str, &str); 8] = [
    (
        "administrator",
        "/?rest_route=/wp/v2/media/9139&context=edit",
    ),
    ("administrator", "/?rest_route=/wp/v2/media/9140"),
    ("administrator", "/?rest_route=/wp/v2/media/9141"),
    ("administrator", "/?rest_route=/wp/v2/posts/9106/revisions"),
    ("editor", "/?rest_route=/wp/v2/media/9139&context=edit"),
    ("editor", "/?rest_route=/wp/v2/media/9140"),
    ("editor", "/?rest_route=/wp/v2/media/9141"),
    ("editor", "/?rest_route=/wp/v2/posts/9106/revisions"),
];

/// The routes over the default install: `reference/rest/media.tsv`.
#[test]
fn media_match_php() {
    replay(
        include_str!("../../tests/golden/media.tsv"),
        (false, true),
        6 * 133,
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
        6 * 14,
        &[],
    );
}
