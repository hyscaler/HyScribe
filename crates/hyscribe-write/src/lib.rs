// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's core write functions, ported from 7.1.3 over `hyscribe-db`'s
//! `Writer` (ADRs 0010 and 0012), each checked row for row by the write oracle.
//!
//! An action stops at its first database error and returns it, so it is
//! never committed: the terms, comments and users ports run their
//! statements through `steps::Steps`, the options, meta and posts
//! functions return it with `?`. Every area's error tests spend
//! `Write::fail_after` budgets.

use hyscribe_db::{DbError, Write};
use hyscribe_kses::wp_specialchars;
use hyscribe_phpser::{addslashes, stripslashes};
use hyscribe_render::date::{Zone, site_zone};
use hyscribe_render::formatting::sanitize_text_field;
use hyscribe_render::kses::wp_kses_data;

pub mod caps;
pub mod comments;
pub mod cron;
pub mod install;
pub mod options;
pub mod posts;
pub mod privacy;
mod steps;
pub mod terms;
pub mod users;

/// Why a write function stopped.
#[derive(Debug)]
pub enum Error {
    /// The database failed or refused the write.
    Db(DbError),
    /// What PHP refuses: its `WP_Error` code, or why it would `wp_die()`.
    Refused(&'static str),
    /// `wpdb::process_fields()` refused a row whose text is not valid
    /// UTF-8: its `$last_error`, which names the columns.
    InvalidText(String),
    /// A PHP function this write needs is not ported yet.
    Unported(&'static str),
    /// The kernel's random source failed, so no password was hashed.
    Random(std::io::Error),
}

impl From<DbError> for Error {
    fn from(err: DbError) -> Error {
        Error::Db(err)
    }
}

/// `wp_filter_kses()`: kses over the unslashed text, slashed again.
pub(crate) fn wp_filter_kses(text: &[u8], kses: fn(&[u8]) -> Vec<u8>) -> Vec<u8> {
    addslashes(&kses(&stripslashes(text)))
}

/// The default filters of a slashed name (`pre_term_name`,
/// `pre_comment_author_name`, `pre_user_*`), slashed: `sanitize_text_field()`,
/// `wp_filter_kses()` and `_wp_specialchars()`.
pub(crate) fn name_filters(name: &[u8]) -> Vec<u8> {
    wp_specialchars(&wp_filter_kses(&sanitize_text_field(name), wp_kses_data))
}

/// The date format the database stores, PHP's `'Y-m-d H:i:s'`.
pub(crate) const MYSQL: &str = "Y-m-d H:i:s";

/// `wp_timezone()`, from the site's options inside the action.
pub async fn wp_timezone(write: &mut Write<'_>) -> Result<Zone, DbError> {
    let name = write.option_value("timezone_string").await?;
    let offset = write.option_value("gmt_offset").await?;
    Ok(site_zone(
        &name.unwrap_or_default(),
        &offset.unwrap_or_default(),
    ))
}

/// PHP's `time()`.
pub(crate) fn now() -> i64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |since| since.as_secs() as i64)
}

#[cfg(test)]
mod tests;

/// `convert_invalid_entities()`: the numeric references to the C1 controls
/// `&#128;` to `&#159;` become the Windows-1252 characters they meant.
pub(crate) fn convert_invalid_entities(content: &[u8]) -> Vec<u8> {
    const WINDOWS_1252: [&[u8]; 32] = [
        b"&#8364;", b"", b"&#8218;", b"&#402;", b"&#8222;", b"&#8230;", b"&#8224;", b"&#8225;",
        b"&#710;", b"&#8240;", b"&#352;", b"&#8249;", b"&#338;", b"", b"&#381;", b"", b"",
        b"&#8216;", b"&#8217;", b"&#8220;", b"&#8221;", b"&#8226;", b"&#8211;", b"&#8212;",
        b"&#732;", b"&#8482;", b"&#353;", b"&#8250;", b"&#339;", b"", b"&#382;", b"&#376;",
    ];
    let mut out = Vec::with_capacity(content.len());
    let mut at = 0;
    while let Some(byte) = content.get(at) {
        let reference = content.get(at..at + 6).and_then(|entity| match entity {
            [
                b'&',
                b'#',
                b'1',
                tens @ b'0'..=b'9',
                ones @ b'0'..=b'9',
                b';',
            ] => {
                let n = usize::from(tens - b'0') * 10 + usize::from(ones - b'0');
                WINDOWS_1252.get(n.checked_sub(28)?)
            }
            _ => None,
        });
        match reference {
            Some(replacement) => {
                out.extend_from_slice(replacement);
                at += 6;
            }
            None => {
                out.push(*byte);
                at += 1;
            }
        }
    }
    out
}
