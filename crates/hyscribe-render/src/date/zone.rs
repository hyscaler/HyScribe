// SPDX-License-Identifier: AGPL-3.0-or-later

//! Named time zones: TZif data (RFC 8536) from `jiff-tzdb`, its POSIX TZ
//! footer for times after the last transition, and the instant PHP's
//! timelib gives a local time in them.

use std::collections::BTreeMap;
use std::sync::{Mutex, PoisonError};

use super::{civil_from_days, days_from_civil, is_leap, weekday};

/// A local time type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LocalType {
    /// Seconds east of UTC.
    pub offset: i64,
    /// Whether it is daylight saving time.
    pub dst: bool,
    /// Its abbreviation, such as `CEST` or `-03`.
    pub abbreviation: String,
}

/// The day of the year a POSIX rule changes on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Day {
    /// `Mm.w.d`: weekday `d` of week `w` (5 is the last) of month `m`.
    Month(i64, i64, i64),
    /// `Jn`: day 1 to 365, never counting February 29.
    Julian(i64),
    /// `n`: day 0 to 365, counting February 29.
    Zero(i64),
}

/// When a POSIX rule changes: the day, then seconds after local midnight.
type Change = (Day, i64);

/// A POSIX TZ string's zone: standard time, and daylight saving time
/// with its start and end.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Rule {
    standard: LocalType,
    daylight: Option<(LocalType, Change, Change)>,
}

/// A zone's TZif data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tz {
    /// Transition instants and the index of the type each starts.
    transitions: Vec<(i64, usize)>,
    types: Vec<LocalType>,
    rule: Option<Rule>,
}

/// A local time type and the instant its period started, if known.
pub type Period<'a> = (&'a LocalType, Option<i64>);

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let (taken, rest) = self.0.split_at_checked(len)?;
        self.0 = rest;
        Some(taken)
    }

    fn byte(&mut self) -> Option<u8> {
        let (first, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(*first)
    }

    /// A big-endian unsigned number of `len` bytes.
    fn number(&mut self, len: usize) -> Option<u64> {
        (0..len).try_fold(0u64, |n, _| Some((n << 8) + u64::from(self.byte()?)))
    }

    /// The six counts of a TZif header, after its magic and version.
    fn header(&mut self) -> Option<[usize; 6]> {
        let start = self.take(20)?;
        if start.get(..4) != Some(b"TZif") || start.get(4) == Some(&0) {
            return None;
        }
        let mut counts = [0; 6];
        for count in &mut counts {
            *count = self.number(4)? as usize;
        }
        Some(counts)
    }
}

impl Tz {
    /// Reads version 2 or later TZif data, ignoring leap seconds.
    pub fn parse(data: &[u8]) -> Option<Tz> {
        let mut reader = Reader(data);
        // The version 1 block, with four byte times, comes first.
        let [isut, isstd, leap, time, kinds, chars] = reader.header()?;
        let skip = [time.saturating_mul(5), kinds.saturating_mul(6), chars]
            .into_iter()
            .chain([leap.saturating_mul(8), isstd, isut])
            .fold(0usize, usize::saturating_add);
        reader.take(skip)?;
        let [isut, isstd, leap, time, kinds, chars] = reader.header()?;
        let mut instants = Vec::new();
        for _ in 0..time {
            instants.push(reader.number(8)? as i64);
        }
        let indexes = reader.take(time)?;
        let mut records = Vec::new();
        for _ in 0..kinds {
            // Seconds east of UTC as a signed 32 bit number.
            let offset = i64::from(reader.number(4)? as u32 as i32);
            records.push((offset, reader.byte()?, reader.byte()?));
        }
        let names = reader.take(chars)?;
        reader.take(
            leap.saturating_mul(12)
                .saturating_add(isstd)
                .saturating_add(isut),
        )?;
        let types = records
            .into_iter()
            .map(|(offset, dst, at)| {
                let name = names.get(usize::from(at)..)?;
                let name = name.split(|b| *b == 0).next().unwrap_or_default();
                Some(LocalType {
                    offset,
                    dst: dst != 0,
                    abbreviation: String::from_utf8_lossy(name).into_owned(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let transitions = instants
            .into_iter()
            .zip(indexes.iter().map(|i| usize::from(*i)))
            .map(|(at, index)| (index < types.len()).then_some((at, index)))
            .collect::<Option<Vec<_>>>()?;
        types.first()?;
        let footer = reader.0.strip_prefix(b"\n")?.strip_suffix(b"\n")?;
        let rule = match footer {
            [] => None,
            footer => Some(posix(footer)?),
        };
        Some(Tz {
            transitions,
            types,
            rule,
        })
    }

    /// The local time type at `unix`, and when its period started.
    pub fn period(&self, unix: i64) -> Period<'_> {
        let index = self.transitions.partition_point(|(at, _)| *at <= unix);
        let last = index.checked_sub(1).and_then(|i| self.transitions.get(i));
        match (&self.rule, last) {
            (Some(rule), _) if index == self.transitions.len() => {
                rule.period(unix, last.map(|(at, _)| *at))
            }
            (_, Some((at, kind))) => (self.kind(*kind), Some(*at)),
            (_, None) => (self.kind(0), None),
        }
    }

    fn kind(&self, index: usize) -> &LocalType {
        // `parse` checked every index and that a first type exists.
        self.types.get(index).unwrap_or(&FALLBACK)
    }

    /// The instant of local time `local` (seconds since 1970-01-01 as if
    /// it were UTC), as timelib's `do_adjust_timezone()` resolves it: a
    /// time in a gap moves on by the gap, and one in an overlap takes the
    /// offset the zone had at `local` read as UTC.
    pub fn resolve(&self, local: i64) -> i64 {
        let current = self.period(local).0.offset;
        let (after, start) = self.period(local - current);
        let shifted = local - after.offset;
        // timelib also checks `shifted >= at + current - after.offset`,
        // which always holds, as `at` is at or before `local - current`.
        let in_transition = start.is_some_and(|at| shifted < at);
        if current != after.offset && !in_transition {
            shifted
        } else {
            local - current
        }
    }
}

static FALLBACK: LocalType = LocalType {
    offset: 0,
    dst: false,
    abbreviation: String::new(),
};

impl Rule {
    /// The type at `unix`, after the TZif data's last transition `last`.
    fn period(&self, unix: i64, last: Option<i64>) -> Period<'_> {
        let Some((daylight, start, end)) = &self.daylight else {
            return (&self.standard, last);
        };
        let (year, _, _) = civil_from_days(unix.div_euclid(86_400));
        let change =
            |year: i64, (day, time): Change, before: i64| day.days(year) * 86_400 + time - before;
        let initial = (last.unwrap_or(i64::MIN), &self.standard);
        // No zone changes within a day of New Year, so the next year never
        // has a change before `unix`.
        let (at, kind) = (year - 1..=year)
            .flat_map(|year| {
                [
                    (change(year, *start, self.standard.offset), daylight),
                    (change(year, *end, daylight.offset), &self.standard),
                ]
            })
            .filter(|(at, _)| *at <= unix)
            .fold(
                initial,
                |best, next| if next.0 >= best.0 { next } else { best },
            );
        (kind, (at != i64::MIN).then_some(at))
    }
}

impl Day {
    /// Days since 1970-01-01 of this day in `year`.
    fn days(self, year: i64) -> i64 {
        let january = days_from_civil(year, 1, 1);
        match self {
            Day::Month(month, week, day) => {
                let first = days_from_civil(year, month, 1);
                let found = first + (day - weekday(first)).rem_euclid(7) + (week - 1) * 7;
                // Week 5 is the last such weekday, sometimes the fourth.
                if found >= days_from_civil(year, month + 1, 1) {
                    found - 7
                } else {
                    found
                }
            }
            Day::Julian(n) => january + n - 1 + i64::from(is_leap(year) && n >= 60),
            Day::Zero(n) => january + n,
        }
    }
}

/// A POSIX TZ string as TZif footers write it (RFC 8536 section 3.3).
fn posix(text: &[u8]) -> Option<Rule> {
    let mut text = text;
    let standard = local_type(&mut text, None)?;
    if text.is_empty() {
        return Some(Rule {
            standard,
            daylight: None,
        });
    }
    let daylight = local_type(&mut text, Some(standard.offset + 3600))?;
    text = text.strip_prefix(b",")?;
    let start = change(&mut text)?;
    text = text.strip_prefix(b",")?;
    let end = change(&mut text)?;
    text.is_empty().then_some(Rule {
        standard,
        daylight: Some((daylight, start, end)),
    })
}

/// A zone's name and offset; `default` makes the offset optional.
fn local_type(text: &mut &[u8], default: Option<i64>) -> Option<LocalType> {
    let (name, rest) = match text.strip_prefix(b"<") {
        Some(quoted) => {
            let (name, rest) = quoted.split_at(quoted.iter().position(|b| *b == b'>')?);
            (name, rest.get(1..).unwrap_or_default())
        }
        None => text.split_at(
            text.iter()
                .position(|b| !b.is_ascii_alphabetic())
                .unwrap_or(text.len()),
        ),
    };
    if name.len() < 3 {
        return None;
    }
    *text = rest;
    let offset = match (default, text.first()) {
        (Some(offset), None | Some(b',')) => offset,
        // POSIX offsets count west of UTC.
        _ => -time(text)?,
    };
    Some(LocalType {
        offset,
        dst: default.is_some(),
        abbreviation: String::from_utf8_lossy(name).into_owned(),
    })
}

/// `[+-]hh[:mm[:ss]]`, hours up to 167.
fn time(text: &mut &[u8]) -> Option<i64> {
    let sign = match text.first() {
        Some(b'-') => -1,
        _ => 1,
    };
    if let Some(rest) = text.strip_prefix(b"-").or_else(|| text.strip_prefix(b"+")) {
        *text = rest;
    }
    let hours = digits(text, 167)?;
    let mut seconds = hours * 3600;
    for unit in [60, 1] {
        let Some(rest) = text.strip_prefix(b":") else {
            break;
        };
        *text = rest;
        seconds += digits(text, 59)? * unit;
    }
    Some(sign * seconds)
}

/// One to three digits, at most `max`.
fn digits(text: &mut &[u8], max: i64) -> Option<i64> {
    let len = text
        .iter()
        .take(3)
        .take_while(|b| b.is_ascii_digit())
        .count();
    let (number, rest) = text.split_at(len);
    let value = number.iter().fold(0, |n, b| n * 10 + i64::from(b - b'0'));
    *text = rest;
    (len > 0 && value <= max).then_some(value)
}

/// A change rule `Mm.w.d`, `Jn` or `n`, then an optional `/time` (02:00
/// by default).
fn change(text: &mut &[u8]) -> Option<Change> {
    let day = if let Some(after) = text.strip_prefix(b"M") {
        *text = after;
        let month = digits(text, 12)?;
        *text = text.strip_prefix(b".")?;
        let week = digits(text, 5)?;
        *text = text.strip_prefix(b".")?;
        let day = digits(text, 6)?;
        (month > 0 && week > 0).then_some(Day::Month(month, week, day))?
    } else if let Some(after) = text.strip_prefix(b"J") {
        *text = after;
        let n = digits(text, 365)?;
        (n > 0).then_some(Day::Julian(n))?
    } else {
        Day::Zero(digits(text, 365)?)
    };
    let time = match text.strip_prefix(b"/") {
        Some(after) => {
            *text = after;
            time(text)?
        }
        None => 7200,
    };
    Some((day, time))
}

/// A named zone: `jiff-tzdb`'s canonical name and its parsed data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Named {
    /// The zone's name as the database spells it.
    pub name: &'static str,
    /// Its data.
    pub tz: &'static Tz,
}

/// The zone named `name`, in any ASCII case. Each zone is parsed once and
/// kept, so at most every zone in the database is held.
pub(super) fn named(name: &[u8]) -> Option<Named> {
    static ZONES: Mutex<BTreeMap<&'static str, Option<&'static Tz>>> = Mutex::new(BTreeMap::new());
    let (name, data) = jiff_tzdb::get(std::str::from_utf8(name).ok()?)?;
    let mut zones = ZONES.lock().unwrap_or_else(PoisonError::into_inner);
    let tz = *zones
        .entry(name)
        .or_insert_with(|| Tz::parse(data).map(|tz| &*Box::leak(Box::new(tz))));
    tz.map(|tz| Named { name, tz })
}

#[cfg(test)]
mod tests;
