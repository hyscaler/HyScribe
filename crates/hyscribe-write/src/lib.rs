// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress's core write functions, ported from 7.1.3 over `hyscribe-db`'s
//! `Writer` (ADRs 0010 and 0012), each checked row for row by the write oracle.

pub mod comments;
pub mod options;
pub mod posts;
pub mod terms;
pub mod users;
