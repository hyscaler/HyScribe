// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's core write functions, ported from 7.1.3 over `hyscribe-db`'s
//! `Writer` (ADRs 0010 and 0012), each checked row for row by the write oracle.

/// `EMPTY_TRASH_DAYS`: WordPress's default, until it is a `.env` key.
/// Posts and comments go to the trash before they are deleted unless it is 0.
pub const EMPTY_TRASH_DAYS: u64 = 30;

pub mod comments;
pub mod options;
pub mod posts;
mod steps;
pub mod terms;
pub mod users;
