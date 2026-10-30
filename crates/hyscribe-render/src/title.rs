// SPDX-License-Identifier: AGPL-3.0-or-later

//! `get_the_archive_title()` for the archives the default install has.

use hyscribe_kses::strip_tags;
use hyscribe_model::Term;

use crate::date::{self, Zone};
use crate::formatting::{CORE_SHORTCODE_TAGS, wptexturize};
use crate::site::{Archive, Taxonomy};

/// `get_the_archive_title()` in `archive`. `posted` is the global post's
/// date, which date archives show; `date_format` the option of that name.
/// Without `show_prefix`, the title alone, as with an empty
/// `get_the_archive_title_prefix`.
pub fn archive_title(
    archive: &Archive,
    posted: &str,
    zone: Zone,
    date_format: &[u8],
    show_prefix: bool,
) -> Vec<u8> {
    let date = |format: &str| date::format(format, posted, zone).into_bytes();
    // The term's name through the `single_cat_title` or `single_tag_title` filters.
    let name = |term: &Option<Term>| {
        term.as_ref().map_or_else(Vec::new, |term| {
            strip_tags(&wptexturize(&term.name, &CORE_SHORTCODE_TAGS))
        })
    };
    let (prefix, title): (&[u8], Vec<u8>) = match archive {
        Archive::Term(Taxonomy::Category, term) => (b"Category:", name(term)),
        Archive::Term(Taxonomy::Tag, term) => (b"Tag:", name(term)),
        Archive::Term(Taxonomy::Format, term) => {
            let slug = term.as_ref().map_or(&b""[..], |term| &term.slug);
            let title = FORMAT_TITLES.iter().find(|(s, _)| *s == slug);
            (
                b"",
                title.map_or("Archives", |(_, t)| t).as_bytes().to_vec(),
            )
        }
        Archive::Date => (b"", b"Archives".to_vec()),
        Archive::Year => (b"Year:", date("Y")),
        Archive::Month => (b"Month:", date("F Y")),
        Archive::Day => (b"Day:", date(&String::from_utf8_lossy(date_format))),
    };
    if show_prefix && !prefix.is_empty() {
        [prefix, b" <span>", &title, b"</span>"].concat()
    } else {
        title
    }
}

/// The archive titles of the post formats, by term slug; others are
/// `Archives`.
const FORMAT_TITLES: [(&[u8], &str); 9] = [
    (b"post-format-aside", "Asides"),
    (b"post-format-gallery", "Galleries"),
    (b"post-format-image", "Images"),
    (b"post-format-video", "Videos"),
    (b"post-format-quote", "Quotes"),
    (b"post-format-link", "Links"),
    (b"post-format-status", "Statuses"),
    (b"post-format-audio", "Audio"),
    (b"post-format-chat", "Chats"),
];

#[cfg(test)]
mod tests;
