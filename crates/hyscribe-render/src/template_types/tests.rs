// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

fn named(slug: &str) -> Option<(String, String)> {
    let (title, description) = default_type(slug.as_bytes())?;
    Some((
        String::from_utf8(title).unwrap(),
        String::from_utf8(description).unwrap(),
    ))
}

/// A default type's and a post format's title and description, as
/// `reference/site-editor/page-golden.sh` recorded PHP's; none for others.
#[test]
fn default_types_are_php_s() {
    let page = (
        "Pages".to_string(),
        "Displays a static page unless a custom template has been applied to that page or a dedicated template exists.".to_string(),
    );
    assert_eq!(named("page"), Some(page));
    let aside = (
        "Post Format: Aside".to_string(),
        "Displays the Aside post format archive.".to_string(),
    );
    assert_eq!(named("taxonomy-post_format-post-format-aside"), Some(aside));
    for slug in [
        "nope",
        "taxonomy-post_format-post-format-nope",
        "post-format-aside",
    ] {
        assert_eq!(named(slug), None, "{slug}");
    }
}
