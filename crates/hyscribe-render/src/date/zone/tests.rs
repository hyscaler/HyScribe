// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

/// Version 2 TZif data with an empty version 1 block.
fn tzif(
    transitions: &[(i64, u8)],
    types: &[(i32, u8, u8)],
    chars: &[u8],
    footer: &[u8],
) -> Vec<u8> {
    let header = |counts: [usize; 6]| {
        let mut out = b"TZif2".to_vec();
        out.extend([0; 15]);
        for count in counts {
            out.extend((count as u32).to_be_bytes());
        }
        out
    };
    let mut out = header([0; 6]);
    out.extend(header([
        0,
        0,
        0,
        transitions.len(),
        types.len(),
        chars.len(),
    ]));
    for (at, _) in transitions {
        out.extend(at.to_be_bytes());
    }
    out.extend(transitions.iter().map(|(_, index)| index));
    for (offset, dst, at) in types {
        out.extend(offset.to_be_bytes());
        out.extend([*dst, *at]);
    }
    out.extend(chars);
    out.push(b'\n');
    out.extend(footer);
    out.push(b'\n');
    out
}

const TYPES: [(i32, u8, u8); 2] = [(3600, 0, 0), (7200, 1, 4)];
const CHARS: &[u8] = b"CET\0CEST\0";

#[test]
fn reads_types_and_transitions() {
    let tz = Tz::parse(&tzif(&[(100, 1), (200, 0)], &TYPES, CHARS, b"")).unwrap();
    let show = |unix| {
        let (kind, start) = tz.period(unix);
        (kind.abbreviation.as_str(), kind.dst, start)
    };
    assert_eq!(show(99), ("CET", false, None));
    assert_eq!(show(100), ("CEST", true, Some(100)));
    // Past the last transition with no footer, its type stays.
    assert_eq!(show(5000), ("CET", false, Some(200)));
    let empty = Tz::parse(&tzif(&[], &TYPES, CHARS, b"")).unwrap();
    assert_eq!(empty.period(0), (&empty.types[0], None));
}

#[test]
fn rejects_malformed_data() {
    let valid = tzif(&[(100, 1)], &TYPES, CHARS, b"CET-1CEST,M3.5.0,M10.5.0/3");
    assert!(Tz::parse(&valid).is_some());
    for len in 0..valid.len() {
        assert_eq!(Tz::parse(&valid[..len]), None, "{len}");
    }
    let mut version_1 = valid.clone();
    version_1[4] = 0;
    let mut magic = valid.clone();
    magic[0] = b'X';
    for data in [
        version_1,
        magic,
        // A transition to a type that does not exist.
        tzif(&[(100, 2)], &TYPES, CHARS, b""),
        tzif(&[], &[], b"", b""),
        // An abbreviation past the characters.
        tzif(&[], &[(0, 0, 10)], CHARS, b""),
        tzif(&[], &TYPES, CHARS, b"CE"),
    ] {
        assert_eq!(Tz::parse(&data), None);
    }
    // Counts too large for the data, in the version 1 block.
    let mut huge = valid.clone();
    huge[20 + 12..20 + 16].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(Tz::parse(&huge), None);
    // More leap seconds than the version 2 block holds.
    let mut leaps = valid.clone();
    leaps[44 + 28..44 + 32].copy_from_slice(&u32::MAX.to_be_bytes());
    assert_eq!(Tz::parse(&leaps), None);
    // An abbreviation without its NUL runs to the end.
    let open = Tz::parse(&tzif(&[], &[(0, 0, 0)], b"ABC", b"")).unwrap();
    assert_eq!(open.types[0].abbreviation, "ABC");
}

#[test]
fn reads_posix_rules() {
    let rule = |text: &str| posix(text.as_bytes());
    let standard = |offset, abbreviation: &str| LocalType {
        offset,
        dst: false,
        abbreviation: abbreviation.to_string(),
    };
    assert_eq!(
        rule("<+0330>-3:30"),
        Some(Rule {
            standard: standard(12_600, "+0330"),
            daylight: None,
        })
    );
    assert_eq!(rule("<-03>+3").unwrap().standard, standard(-10_800, "-03"));
    assert_eq!(rule("XYZ-1:02:03").unwrap().standard.offset, 3723);
    let us = rule("EST5EDT,M3.2.0,M11.1.0").unwrap();
    let (daylight, start, end) = us.daylight.unwrap();
    assert_eq!((daylight.offset, daylight.dst), (-14_400, true));
    assert_eq!(
        (start, end),
        ((Day::Month(3, 2, 0), 7200), (Day::Month(11, 1, 0), 7200))
    );
    let dublin = rule("IST-1GMT0,M10.5.0,M3.5.0/1")
        .unwrap()
        .daylight
        .unwrap();
    assert_eq!(
        (dublin.0.offset, dublin.2),
        (0, (Day::Month(3, 5, 0), 3600))
    );
    let days = rule("AAA3BBB,J60/-1,300/25:30").unwrap().daylight.unwrap();
    assert_eq!(
        (days.1, days.2),
        ((Day::Julian(60), -3600), (Day::Zero(300), 91_800))
    );
    for bad in [
        "",
        "AB1",
        "<AB>1",
        "<ABC1",
        "ABC",
        "ABC168",
        "ABC1:60",
        "ABC+",
        "ABC1DEF",
        "ABC1DEF,M3.2.0",
        "ABC1DEF,M3.2.0,",
        "ABC1DEF,M3.2.0,M11.1.0x",
        "ABC1DEF,M0.2.0,M11.1.0",
        "ABC1DEF,M13.1.0,M11.1.0",
        "ABC1DEF,M3.0.0,M11.1.0",
        "ABC1DEF,M3.6.0,M11.1.0",
        "ABC1DEF,M3.2.7,M11.1.0",
        "ABC1DEF,M3,M11.1.0",
        "ABC1DEF,M3.2,M11.1.0",
        "ABC1DEF,J0,J1",
        "ABC1DEF,366,1",
        "ABC1DEF,J,1",
        "ABC1DEF,1/,2",
        "ABC1DEF2,1,2/x",
        "ABC1x",
    ] {
        assert_eq!(rule(bad), None, "{bad}");
    }
}

#[test]
fn finds_days_of_rules() {
    let date = |days| civil_from_days(days);
    // The last Sunday of March 2026 is the 29th; of May 2027, the 30th.
    assert_eq!(date(Day::Month(3, 5, 0).days(2026)), (2026, 3, 29));
    assert_eq!(date(Day::Month(5, 5, 0).days(2027)), (2027, 5, 30));
    assert_eq!(date(Day::Month(3, 2, 0).days(2026)), (2026, 3, 8));
    // `Jn` skips February 29, `n` counts it.
    assert_eq!(date(Day::Julian(60).days(2024)), (2024, 3, 1));
    assert_eq!(date(Day::Julian(59).days(2024)), (2024, 2, 28));
    assert_eq!(date(Day::Julian(60).days(2026)), (2026, 3, 1));
    assert_eq!(date(Day::Zero(59).days(2024)), (2024, 2, 29));
}

#[test]
fn rules_apply_after_the_last_transition() {
    let footer = b"CET-1CEST,M3.5.0,M10.5.0/3";
    let tz = Tz::parse(&tzif(&[(0, 0)], &TYPES, CHARS, footer)).unwrap();
    // 2026-03-29 01:00 UTC, when summer time starts.
    let (kind, start) = tz.period(1_774_746_000);
    assert_eq!((kind.offset, start), (7200, Some(1_774_746_000)));
    assert_eq!(tz.period(1_774_745_999).0.offset, 3600);
    let fixed = Tz::parse(&tzif(&[], &TYPES, CHARS, b"CET-1")).unwrap();
    assert_eq!(fixed.period(0), (&fixed.types[0], None));
}

/// The next transition is the data's, then the rule's, none for a rule
/// without daylight saving time.
#[test]
fn finds_the_next_transition() {
    let footer = b"CET-1CEST,M3.5.0,M10.5.0/3";
    let tz = Tz::parse(&tzif(&[(100, 1), (200, 0)], &TYPES, CHARS, footer)).unwrap();
    assert_eq!(tz.next_transition(99), Some((100, &tz.types[1])));
    assert_eq!(tz.next_transition(100), Some((200, &tz.types[0])));
    // Summer time from 2026-03-29 01:00 UTC to 2026-10-25 01:00 UTC, then
    // from 2027-03-28 01:00 UTC.
    let (summer, winter, next_summer) = (1_774_746_000, 1_792_890_000, 1_806_195_600);
    let next = |unix| tz.next_transition(unix).map(|(at, kind)| (at, kind.dst));
    assert_eq!(next(summer - 1), Some((summer, true)));
    assert_eq!(next(summer), Some((winter, false)));
    assert_eq!(next(winter), Some((next_summer, true)));
    let fixed = Tz::parse(&tzif(&[], &TYPES, CHARS, b"CET-1")).unwrap();
    assert_eq!(fixed.next_transition(0), None);
    let bare = Tz::parse(&tzif(&[], &TYPES, CHARS, b"")).unwrap();
    assert_eq!(bare.next_transition(0), None);
}

#[test]
fn resolves_local_times_like_timelib() {
    let paris = named(b"Europe/Paris").unwrap().tz;
    // 02:30 on 2026-03-29 is in the gap and moves on to 03:30 CEST; 02:30
    // on 2026-10-25 happens twice and takes CET.
    assert_eq!(paris.resolve(1_774_751_400), 1_774_747_800);
    assert_eq!(paris.resolve(1_792_891_800 + 3600), 1_792_891_800);
    let new_york = named(b"America/New_York").unwrap().tz;
    // 01:30 on 2026-11-01 happens twice and takes EDT.
    assert_eq!(new_york.resolve(1_793_511_000 - 14_400), 1_793_511_000);
}

#[test]
fn extreme_transitions_do_not_overflow() {
    let tz = Tz::parse(&tzif(&[(i64::MIN, 1), (100, 0)], &TYPES, CHARS, b"")).unwrap();
    assert_eq!(tz.resolve(3650), -3550);
}
