// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::fixture;

/// The slugs PHP's `get_slug_from_attribute()` found in the cases of
/// `reference/duotone/golden.sh`.
#[test]
fn slug_of_an_attribute() {
    for (attribute, slug) in [
        ("var:preset|duotone|grayscale", "grayscale"),
        (
            "var(--wp--preset--duotone--dark-grayscale)",
            "dark-grayscale",
        ),
        ("a var:preset|duotone|grayscale b", "grayscale"),
        ("var:preset|duotone|0", ""),
        ("var:preset|duotone| x", ""),
        ("var(--wp--preset--duotone--", ""),
        ("var(--wp--preset--duotone--)", ""),
        (
            "var(--wp--preset--duotone--grayscale)extra)",
            "grayscale)extra",
        ),
        (
            "var(--wp--preset--duotone--x) var:preset|duotone|grayscale",
            "x",
        ),
        (
            "var:preset|duotone|grayscale var(--wp--preset--duotone--x)",
            "grayscale",
        ),
        ("var:preset|duotone|a var(--wp--preset--duotone--b)", "a"),
        // A form with no slug leaves the other its turn.
        ("var:preset|duotone| var(--wp--preset--duotone--b)", "b"),
        ("var(--wp--preset--duotone--) var:preset|duotone|a", "a"),
        ("unset", ""),
        ("", ""),
    ] {
        assert_eq!(duotone_slug(attribute.as_bytes()), slug, "{attribute}");
    }
}

/// A block's duotone selector is its `block.json`'s; the blocks that name a
/// preset in the global styles are those with a selector and a slug.
#[test]
fn blocks_with_a_duotone_filter() {
    let fixture = fixture();
    let plain = GlobalStyles::resolve(&fixture.sources());
    assert_eq!(
        plain.duotone_selector("core/image"),
        Some(".wp-block-image img, .wp-block-image .components-placeholder")
    );
    assert_eq!(plain.duotone_selector("core/paragraph"), None);
    assert_eq!(plain.duotone_selector("hyscribe/none"), None);
    assert_eq!(plain.duotone_blocks(), []);
    let user = br##"{"version":3,"isGlobalStylesUserThemeJSON":true,"styles":{"blocks":{
        "core/cover":{"filter":{"duotone":"var:preset|duotone|grayscale"}},
        "core/image":{"filter":{"duotone":"var(--wp--preset--duotone--blue-red)"}},
        "core/avatar":{"filter":{"duotone":"#ff0000"}},
        "core/post-author":{"filter":{"duotone":["#000","#fff"]}},
        "core/paragraph":{"filter":{"duotone":"var:preset|duotone|x"}}}}}"##;
    let styled = GlobalStyles::resolve(&Sources {
        user: Some(user),
        ..fixture.sources()
    });
    let mut blocks = styled.duotone_blocks();
    blocks.sort();
    assert_eq!(
        blocks,
        [
            ("core/cover".to_owned(), "grayscale".to_owned()),
            ("core/image".to_owned(), "blue-red".to_owned()),
        ]
    );
}
