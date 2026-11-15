// SPDX-License-Identifier: AGPL-3.0-or-later

//! `_default_wp_die_handler()`'s page, the one `wp_die()` shows.

const PAGE: &str = include_str!("die.html");

/// `wp_robots()`'s directives on the login screen, a sensitive page, or
/// on `wp_die()`'s, which asks for no robots; `public` is `blog_public`.
pub(crate) fn robots(public: bool, login: bool) -> String {
    let mut directives = if public {
        vec!["max-image-preview:large"]
    } else {
        vec!["noindex", "nofollow"]
    };
    match (login, public) {
        (true, true) => directives.extend(["noindex", "noarchive"]),
        (true, false) => directives.push("noarchive"),
        (false, true) => directives.extend(["noindex", "follow"]),
        (false, false) => {}
    }
    directives.join(", ")
}

/// The page for `wp_die( $message, $title )` on a site that is `public` or
/// not; `message` is HTML.
pub fn page(public: bool, title: &str, message: &str) -> Vec<u8> {
    die(public, title, message, "")
}

/// [`page`] with its `back_link`, a link back in the browser's history.
pub fn page_with_back_link(public: bool, title: &str, message: &str) -> Vec<u8> {
    let back = "\n<p><a href='javascript:history.back()'>&laquo; Back</a></p>";
    die(public, title, message, back)
}

/// The page with `after` past the message's `div`.
fn die(public: bool, title: &str, message: &str, after: &str) -> Vec<u8> {
    let (head, rest) = PAGE.split_once("%robots%").unwrap_or_default();
    let (before, rest) = rest.split_once("%title%").unwrap_or_default();
    let (middle, tail) = rest.split_once("%message%").unwrap_or_default();
    let (div, tail) = tail.split_at_checked("</div>".len()).unwrap_or_default();
    let robots = robots(public, false);
    [
        head, &robots, before, title, middle, message, div, after, tail,
    ]
    .concat()
    .into_bytes()
}
