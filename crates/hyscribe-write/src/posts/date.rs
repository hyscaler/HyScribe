// SPDX-License-Identifier: AGPL-3.0-or-later

//! The dates `wp_insert_post()` writes: `current_time()`,
//! `wp_resolve_post_date()`, `get_gmt_from_date()` and
//! `get_date_from_gmt()` in the site's time zone.

use hyscribe_db::Write;
use hyscribe_render::date::{Zone, format_unix, site_zone, stored_unix};

use super::{PostError, Result, ZERO_DATE, truthy};

const MYSQL: &str = "Y-m-d H:i:s";

/// The site's time zone and the time of the action.
pub(super) struct Dates {
    zone: Zone,
    now: i64,
}

/// `date` when it has the form the database stores, `YYYY-MM-DD hh:mm:ss`;
/// PHP also takes shorter forms, which HyScribe does not yet.
pub(super) fn canonical(date: &str) -> Result<&str> {
    let shaped = date.len() == 19
        && date.bytes().enumerate().all(|(i, b)| match i {
            4 | 7 => b == b'-',
            10 => b == b' ',
            13 | 16 => b == b':',
            _ => b.is_ascii_digit(),
        });
    let field = |at: usize| date.get(at..at + 2).and_then(|f| f.parse::<u8>().ok());
    let time = shaped && field(11) < Some(24) && field(14) < Some(60) && field(17) < Some(60);
    if time {
        Ok(date)
    } else {
        Err(PostError::Unported("a date not shaped YYYY-MM-DD hh:mm:ss"))
    }
}

/// PHP's `checkdate( $month, $day, $year )`.
fn checkdate(month: u32, day: u32, year: u32) -> bool {
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = match month {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    };
    (1..=32767).contains(&year) && (1..=12).contains(&month) && (1..=days).contains(&day)
}

/// `preg_match( '/^(\d{4})-(\d{1,2})-(\d{1,2})/' )`'s year, month and
/// day; zeros, which no calendar has, when it does not match.
fn ymd(date: &str) -> (u32, u32, u32) {
    let mut parts = date.splitn(3, '-');
    let mut part = || parts.next().unwrap_or_default();
    let (year, month, rest) = (part(), part(), part());
    let digits = |text: &str| text.bytes().all(|b| b.is_ascii_digit());
    let day: String = rest
        .chars()
        .take_while(char::is_ascii_digit)
        .take(2)
        .collect();
    if year.len() != 4 || !digits(year) || !(1..=2).contains(&month.len()) || !digits(month) {
        return (0, 0, 0);
    }
    let number = |text: &str| text.parse().unwrap_or_default();
    (number(year), number(month), number(&day))
}

impl Dates {
    /// The site's time zone, from its options, at `now`.
    pub(super) async fn read(write: &mut Write<'_>, now: i64) -> Result<Dates> {
        let zone = write.option("timezone_string").await?.unwrap_or_default();
        let offset = write.option("gmt_offset").await?.unwrap_or_default();
        Ok(Dates {
            zone: site_zone(&zone, &offset),
            now,
        })
    }

    /// `current_time( 'mysql' )`.
    pub(super) fn local_now(&self) -> String {
        format_unix(MYSQL, self.now, self.zone)
    }

    /// `current_time( 'mysql', true )`.
    pub(super) fn gmt_now(&self) -> String {
        format_unix(MYSQL, self.now, Zone::Utc)
    }

    /// `get_gmt_from_date( $local )`.
    pub(super) fn gmt_from(&self, local: &str) -> String {
        format_unix(MYSQL, stored_unix(local, self.zone), Zone::Utc)
    }

    /// The Unix time of a site time.
    pub(super) fn unix(&self, local: &str) -> i64 {
        stored_unix(local, self.zone)
    }

    /// `strtotime( $gmt )`.
    pub(super) fn unix_gmt(gmt: &str) -> i64 {
        stored_unix(gmt, Zone::Utc)
    }

    /// `wp_resolve_post_date( $date, $gmt )`: now when neither is given,
    /// `invalid_date` when the day does not exist.
    pub(super) fn resolve(&self, date: &str, gmt: &str) -> Result<String> {
        let date = if truthy(date.as_bytes()) && date != ZERO_DATE {
            date.to_string()
        } else if truthy(gmt.as_bytes()) && gmt != ZERO_DATE {
            format_unix(MYSQL, Self::unix_gmt(canonical(gmt)?), self.zone)
        } else {
            self.local_now()
        };
        let (year, month, day) = ymd(&date);
        if !checkdate(month, day, year) {
            return Err(PostError::Invalid("invalid_date"));
        }
        canonical(&date)?;
        Ok(date)
    }
}

#[cfg(test)]
mod tests;
