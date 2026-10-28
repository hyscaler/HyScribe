// SPDX-License-Identifier: AGPL-3.0-or-later

//! `get_the_archive_title()` for the archives the default install has.

use hyscribe_kses::strip_tags;

use crate::date::{self, Zone};
use crate::formatting::{CORE_SHORTCODE_TAGS, wptexturize};
use crate::site::Archive;

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
    let (prefix, title): (&[u8], Vec<u8>) = match archive {
        // `single_cat_title()`: the `single_cat_title` filters.
        Archive::Category(name) => (
            b"Category:",
            strip_tags(&wptexturize(name, &CORE_SHORTCODE_TAGS)),
        ),
        Archive::Year => (b"Year:", date("Y")),
        Archive::Month => (b"Month:", date("F Y")),
        Archive::Day => (b"Day:", date(&String::from_utf8_lossy(date_format))),
    };
    if show_prefix {
        [prefix, b" <span>", &title, b"</span>"].concat()
    } else {
        title
    }
}
