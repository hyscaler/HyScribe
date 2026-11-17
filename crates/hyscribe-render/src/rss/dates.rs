// SPDX-License-Identifier: AGPL-3.0-or-later

//! SimplePie's `Parse\Date` for the forms feeds use: W3C dates (RFC 3339
//! and ISO 8601's profile), RFC 2822, then `strtotime()` as ported.

use crate::date::{days_from_civil, strtotime};

/// `gmmktime()`.
fn gmmktime(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64 {
    days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second
}

/// The digits at the start of `text`, at most `max`.
fn digits(text: &[u8], max: usize) -> (&[u8], &[u8]) {
    let length = text
        .iter()
        .take(max)
        .take_while(|b| b.is_ascii_digit())
        .count();
    text.split_at(length)
}

/// The value of ASCII digits.
fn number(digits: &[u8]) -> i64 {
    digits.iter().fold(0_i64, |n, d| {
        n.saturating_mul(10).saturating_add(i64::from(d - b'0'))
    })
}

/// `date_w3cdtf()`: `YYYY(-MM(-DD(Thh(:mm(:ss(.s)?)?)?(Z|±hh(:?mm)?)?)?)?)?`.
fn w3cdtf(date: &[u8]) -> Option<i64> {
    let (year, rest) = digits(date, 4);
    if year.len() != 4 {
        return None;
    }
    let mut fields = [1_i64, 1, 0, 0, 0];
    let mut fraction = (0_i64, 0_u32);
    let mut zone = 0_i64;
    let mut rest = rest;
    // Month and day, each after an optional dash; a time needs both.
    let mut read = 0;
    for field in fields.iter_mut().take(2) {
        let after = rest.strip_prefix(b"-").unwrap_or(rest);
        let (two, tail) = digits(after, 2);
        if two.len() != 2 {
            break;
        }
        *field = number(two);
        rest = tail;
        read += 1;
    }
    if let Some(time) = rest.strip_prefix(b"T").filter(|_| read == 2) {
        let (hour, tail) = digits(time, 2);
        if hour.len() != 2 {
            return None;
        }
        fields[2] = number(hour);
        rest = tail;
        for field in fields.iter_mut().skip(3) {
            let after = rest.strip_prefix(b":").unwrap_or(rest);
            let (two, tail) = digits(after, 2);
            if two.len() != 2 {
                break;
            }
            *field = number(two);
            rest = tail;
        }
        if let Some(after) = rest.strip_prefix(b".").or_else(|| rest.strip_prefix(b",")) {
            let (decimals, tail) = digits(after, usize::MAX);
            if !decimals.is_empty() {
                let length = u32::try_from(decimals.len().min(18)).unwrap_or(18);
                fraction = (
                    number(decimals.get(..length as usize).unwrap_or_default()),
                    length,
                );
                rest = tail;
            }
        }
        if let Some(tail) = rest.strip_prefix(b"Z") {
            rest = tail;
        } else if let Some(sign) = rest.first().filter(|s| matches!(s, b'+' | b'-')).copied() {
            let (hours, tail) = digits(rest.get(1..)?, 2);
            if hours.len() != 2 {
                return None;
            }
            let tail = tail.strip_prefix(b":").unwrap_or(tail);
            let (minutes, tail) = digits(tail, 2);
            zone = number(hours) * 3600 + number(minutes) * 60;
            if sign == b'-' {
                zone = -zone;
            }
            rest = tail;
        }
    }
    if !rest.is_empty() {
        return None;
    }
    let (value, length) = fraction;
    let seconds = fields[4] as f64 + value as f64 / 10_f64.powi(length as i32);
    let second = seconds.round() as i64;
    Some(
        gmmktime(
            number(year),
            fields[0],
            fields[1],
            fields[2],
            fields[3],
            second,
        ) - zone,
    )
}

/// `date_rfc2822()`: `[Day,] DD Mon YYYY hh:mm[:ss] zone`.
fn rfc2822(date: &[u8]) -> Option<i64> {
    let text = String::from_utf8_lossy(date);
    let mut words: Vec<&str> = text
        .split(|c: char| c.is_ascii_whitespace())
        .filter(|w| !w.is_empty())
        .collect();
    if words
        .first()
        .is_some_and(|w| w.ends_with(',') || w.chars().all(char::is_alphabetic))
    {
        words.remove(0);
    }
    let [day, month, year, time, rest @ ..] = words.as_slice() else {
        return None;
    };
    let day: i64 = day.parse().ok()?;
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let month = MONTHS
        .iter()
        .position(|m| month.get(..3).is_some_and(|w| w.eq_ignore_ascii_case(m)))?;
    let mut year: i64 = year.parse().ok()?;
    if year < 50 {
        year += 2000;
    } else if year < 1000 {
        year += 1900;
    }
    let parts: Vec<i64> = time
        .split(':')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let (hour, minute, second) = match parts.as_slice() {
        [h, m] => (*h, *m, 0),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    let zone = match rest.first().map(|z| z.to_ascii_uppercase()) {
        None => 0,
        Some(zone) if zone.starts_with(['+', '-']) && zone.len() == 5 => {
            let hours: i64 = zone.get(1..3)?.parse().ok()?;
            let minutes: i64 = zone.get(3..5)?.parse().ok()?;
            let offset = hours * 3600 + minutes * 60;
            if zone.starts_with('-') {
                -offset
            } else {
                offset
            }
        }
        Some(zone) => match zone.as_str() {
            "UT" | "UTC" | "GMT" | "Z" => 0,
            "EDT" => -4 * 3600,
            "EST" | "CDT" => -5 * 3600,
            "CST" | "MDT" => -6 * 3600,
            "MST" | "PDT" => -7 * 3600,
            "PST" => -8 * 3600,
            _ => return None,
        },
    };
    let month = i64::try_from(month).ok()? + 1;
    Some(gmmktime(year, month, day, hour, minute, second) - zone)
}

/// `Parse\Date::parse()`: the Unix time of a feed's date, if read.
pub(crate) fn parse(date: &[u8]) -> Option<i64> {
    let date = hyscribe_phpser::php_trim(date);
    w3cdtf(date)
        .or_else(|| rfc2822(date))
        .or_else(|| strtotime(std::str::from_utf8(date).ok()?))
}

#[cfg(test)]
mod tests;
