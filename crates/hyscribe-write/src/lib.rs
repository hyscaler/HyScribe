// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's core write functions, ported from 7.1.3 over `hyscribe-db`'s
//! `Writer` (ADRs 0010 and 0012), each checked row for row by the write oracle.

use hyscribe_db::DbError;

pub mod comments;
pub mod options;
pub mod posts;
pub mod terms;
pub mod users;

/// Why a write function stopped.
#[derive(Debug)]
pub enum Error {
    /// The database failed or refused the write.
    Db(DbError),
    /// What PHP refuses: its `WP_Error` code, or why it would `wp_die()`.
    Refused(&'static str),
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

/// PHP's `time()`.
pub(crate) fn now() -> i64 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.map_or(0, |since| since.as_secs() as i64)
}

#[cfg(test)]
mod tests;
