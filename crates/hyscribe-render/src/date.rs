// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP's date formatting (`DateTimeInterface::format()`), as `wp_date()`
//! applies it with the default English locale, for times stored the way
//! WordPress stores them: `YYYY-MM-DD HH:MM:SS` in the site's time zone.

use hyscribe_phpser::Value;

/// The site's time zone, as `wp_timezone()` builds it. Named zones other
/// than UTC need a time zone database; until one lands they format as UTC.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Zone {
    /// `UTC` as the `timezone_string`.
    Utc,
    /// A fixed offset in seconds east of UTC, from `gmt_offset`.
    Offset(i32),
}

const DAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

pub(crate) const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn offset(zone: Zone) -> i64 {
    match zone {
        Zone::Utc => 0,
        Zone::Offset(seconds) => i64::from(seconds),
    }
}

/// A stored time, resolved.
struct Time {
    year: i64,
    month: u32,
    day: u32,
    hour: i64,
    minute: i64,
    second: i64,
    /// Days since 1970-01-01.
    days: i64,
    offset: i64,
    zone: Zone,
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The weekday of a day count from 1970-01-01, a Thursday: 0 for Sunday.
pub(crate) fn weekday(days: i64) -> i64 {
    (days + 4).rem_euclid(7)
}

/// The date of a day count from 1970-01-01.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month as u32, day as u32)
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

impl Time {
    /// Reads `YYYY-MM-DD HH:MM:SS`. Out of range parts carry over as PHP's
    /// do, so the empty date `0000-00-00 00:00:00` is `-0001-11-30`.
    fn parse(stored: &str, zone: Zone) -> Time {
        let number = |range: std::ops::Range<usize>| -> i64 {
            stored.get(range).and_then(|d| d.parse().ok()).unwrap_or(0)
        };
        let (year, month) = (number(0..4), number(5..7) - 1);
        let days = days_from_civil(year + month.div_euclid(12), month.rem_euclid(12) + 1, 1)
            + number(8..10)
            - 1;
        let seconds = number(11..13) * 3600 + number(14..16) * 60 + number(17..19);
        Time::at(days, seconds, zone)
    }

    /// The local time `seconds` into day `days`.
    fn at(days: i64, seconds: i64, zone: Zone) -> Time {
        let (year, month, day) = civil_from_days(days);
        Time {
            year,
            month,
            day,
            hour: seconds / 3600,
            minute: seconds / 60 % 60,
            second: seconds % 60,
            days,
            offset: offset(zone),
            zone,
        }
    }

    fn from_unix(unix: i64, zone: Zone) -> Time {
        let local = unix + offset(zone);
        Time::at(local.div_euclid(86_400), local.rem_euclid(86_400), zone)
    }

    /// 0 for Sunday.
    fn weekday(&self) -> usize {
        weekday(self.days) as usize
    }

    /// The ISO 8601 year and week.
    fn iso_week(&self) -> (i64, i64) {
        let monday_based = (self.weekday() + 6) % 7;
        let thursday = self.days - monday_based as i64 + 3;
        let (year, _, _) = civil_from_days(thursday);
        (year, (thursday - days_from_civil(year, 1, 1)) / 7 + 1)
    }

    fn unix(&self) -> i64 {
        self.days * 86_400 + self.hour * 3600 + self.minute * 60 + self.second - self.offset
    }

    /// `+hhmm`, with `separator` between hours and minutes.
    fn offset_text(&self, separator: &str) -> String {
        let sign = if self.offset < 0 { '-' } else { '+' };
        let minutes = self.offset.abs() / 60;
        format!("{sign}{:02}{separator}{:02}", minutes / 60, minutes % 60)
    }

    /// `Y`: at least four digits, `-` before years BCE.
    fn year_text(&self) -> String {
        let sign = if self.year < 0 { "-" } else { "" };
        format!("{sign}{:04}", self.year.abs())
    }

    fn day_name(&self) -> &'static str {
        DAYS.get(self.weekday()).copied().unwrap_or_default()
    }

    fn month_name(&self) -> &'static str {
        MONTHS
            .get(self.month as usize - 1)
            .copied()
            .unwrap_or_default()
    }

    fn hour12(&self) -> i64 {
        (self.hour + 11) % 12 + 1
    }

    fn character(&self, c: char, out: &mut String) {
        let pushed = match c {
            'd' => format!("{:02}", self.day),
            'D' => self.day_name().get(..3).unwrap_or_default().to_string(),
            'j' => self.day.to_string(),
            'l' => self.day_name().to_string(),
            'N' => ((self.weekday() + 6) % 7 + 1).to_string(),
            'S' => match (self.day % 10, self.day) {
                (_, 11..=13) => "th",
                (1, _) => "st",
                (2, _) => "nd",
                (3, _) => "rd",
                _ => "th",
            }
            .to_string(),
            'w' => self.weekday().to_string(),
            'z' => (self.days - days_from_civil(self.year, 1, 1)).to_string(),
            'W' => format!("{:02}", self.iso_week().1),
            'F' => self.month_name().to_string(),
            'm' => format!("{:02}", self.month),
            'M' => self.month_name().get(..3).unwrap_or_default().to_string(),
            'n' => self.month.to_string(),
            't' => (days_from_civil(self.year, i64::from(self.month) + 1, 1)
                - days_from_civil(self.year, i64::from(self.month), 1))
            .to_string(),
            'L' => u8::from(is_leap(self.year)).to_string(),
            'o' => self.iso_week().0.to_string(),
            'X' => format!(
                "{}{:04}",
                if self.year < 0 { '-' } else { '+' },
                self.year.abs()
            ),
            'x' | 'Y' => self.year_text(),
            'y' => format!("{:02}", self.year % 100),
            'a' => if self.hour < 12 { "am" } else { "pm" }.to_string(),
            'A' => if self.hour < 12 { "AM" } else { "PM" }.to_string(),
            'B' => format!("{:03}", (self.unix() + 3600).rem_euclid(86_400) * 10 / 864),
            'g' => self.hour12().to_string(),
            'G' => self.hour.to_string(),
            'h' => format!("{:02}", self.hour12()),
            'H' => format!("{:02}", self.hour),
            'i' => format!("{:02}", self.minute),
            's' => format!("{:02}", self.second),
            'u' => "000000".to_string(),
            'v' => "000".to_string(),
            'e' => match self.zone {
                Zone::Utc => "UTC".to_string(),
                Zone::Offset(_) => self.offset_text(":"),
            },
            'I' => "0".to_string(),
            'O' => self.offset_text(""),
            'P' => self.offset_text(":"),
            'p' if self.offset == 0 => "Z".to_string(),
            'p' => self.offset_text(":"),
            'T' => match self.zone {
                Zone::Utc => "UTC".to_string(),
                Zone::Offset(_) => format!("GMT{}", self.offset_text("")),
            },
            'Z' => self.offset.to_string(),
            'c' => format_time("Y-m-d\\TH:i:sP", self),
            'r' => format_time("D, d M Y H:i:s O", self),
            'U' => self.unix().to_string(),
            other => {
                out.push(other);
                return;
            }
        };
        out.push_str(&pushed);
    }
}

fn format_time(format: &str, time: &Time) -> String {
    let mut out = String::new();
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        match c {
            // A backslash escapes the next character; on a final one PHP
            // reads its string's NUL terminator.
            '\\' => out.push(chars.next().unwrap_or('\0')),
            c => time.character(c, &mut out),
        }
    }
    out
}

/// `wp_timezone()` from the `timezone_string` and `gmt_offset` options.
pub fn site_zone(timezone_string: &[u8], gmt_offset: &[u8]) -> Zone {
    match timezone_string {
        b"" => {
            // `wp_timezone_string()` prints whole hours and minutes.
            let offset = Value::String(gmt_offset.to_vec()).to_float();
            let minutes = (offset.fract() * 60.0) as i32;
            Zone::Offset(offset.trunc() as i32 * 3600 + minutes * 60)
        }
        _ => Zone::Utc,
    }
}

/// `wp_date( $format, strtotime( $stored ) )` for a time stored in `zone`.
pub fn format(format: &str, stored: &str, zone: Zone) -> String {
    format_time(format, &Time::parse(stored, zone))
}

/// `wp_date( $format, $unix )` in `zone`.
pub fn format_unix(format: &str, unix: i64, zone: Zone) -> String {
    format_time(format, &Time::from_unix(unix, zone))
}

/// The Unix time of a stored local time in `zone`.
pub(crate) fn stored_unix(stored: &str, zone: Zone) -> i64 {
    Time::parse(stored, zone).unix()
}

/// `strtotime()` for the ISO 8601 forms the editor and the post data
/// binding write: `YYYY-MM-DD`, optionally followed by `T` or a space and
/// `HH:MM` with optional `:SS`, then `Z` or an offset (`+02:00`, `+0200`).
/// Without an offset the time is UTC, PHP's default zone under WordPress.
/// Other forms PHP reads (relative times, other separators) give `None`.
pub fn strtotime(text: &str) -> Option<i64> {
    let bytes = text.as_bytes();
    let two = |at: usize| -> Option<i64> {
        match bytes.get(at..at + 2)? {
            [a @ b'0'..=b'9', b @ b'0'..=b'9'] => {
                Some(i64::from(a - b'0') * 10 + i64::from(b - b'0'))
            }
            _ => None,
        }
    };
    let (year, month, day) = match bytes.get(..10)? {
        [y1, y2, y3, y4, b'-', _, _, b'-', _, _]
            if [y1, y2, y3, y4].iter().all(|b| b.is_ascii_digit()) =>
        {
            let year = [y1, y2, y3, y4]
                .iter()
                .fold(0, |n, b| n * 10 + i64::from(**b - b'0'));
            (year, two(5)?, two(8)?)
        }
        _ => return None,
    };
    let (seconds, rest) = match bytes.get(10..).unwrap_or_default() {
        [] => (0, &[][..]),
        [b'T' | b' ', ..] if bytes.get(13) == Some(&b':') => {
            let (hour, minute) = (two(11)?, two(14)?);
            let (second, rest) = match bytes.get(16) {
                Some(b':') => (two(17)?, bytes.get(19..).unwrap_or_default()),
                _ => (0, bytes.get(16..).unwrap_or_default()),
            };
            // PHP rolls a leap second over, and reads `24:00:00` as midnight.
            let valid =
                minute < 60 && second <= 60 && (hour < 24 || (hour, minute, second) == (24, 0, 0));
            (valid.then_some(hour * 3600 + minute * 60 + second)?, rest)
        }
        _ => return None,
    };
    let offset = match rest {
        [] | [b'Z'] => 0,
        [sign @ (b'+' | b'-'), h1, h2, b':', m1, m2] | [sign @ (b'+' | b'-'), h1, h2, m1, m2] => {
            let digit = |b: &u8| b.is_ascii_digit().then(|| i64::from(b - b'0'));
            let seconds =
                (digit(h1)? * 10 + digit(h2)?) * 3600 + (digit(m1)? * 10 + digit(m2)?) * 60;
            if *sign == b'-' { -seconds } else { seconds }
        }
        _ => return None,
    };
    let valid = (1..=12).contains(&month) && (1..=31).contains(&day);
    valid.then(|| days_from_civil(year, month, day) * 86_400 + seconds - offset)
}

/// `human_time_diff( $from, $to )`.
pub(crate) fn human_time_diff(from: i64, to: i64) -> String {
    const UNITS: [(i64, &str); 6] = [
        (60, "minute"),
        (3600, "hour"),
        (86_400, "day"),
        (604_800, "week"),
        (2_592_000, "month"),
        (31_536_000, "year"),
    ];
    let diff = (to - from).abs();
    // Each unit runs until the next one starts.
    let (size, unit) = UNITS
        .iter()
        .rev()
        .find(|(size, _)| diff >= *size)
        .copied()
        .unwrap_or((1, "second"));
    // `round()` halves away from zero; `diff` is never negative.
    let count = ((diff + size / 2) / size).max(1);
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {unit}{plural}")
}

#[cfg(test)]
mod tests;
