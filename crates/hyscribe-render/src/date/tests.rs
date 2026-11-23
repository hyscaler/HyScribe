// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;

fn zone(name: &str) -> Zone {
    if name == "UTC" {
        return Zone::Utc;
    }
    let sign = if name.starts_with('-') { -1 } else { 1 };
    let hours: i32 = name.get(1..3).unwrap().parse().unwrap();
    let minutes: i32 = name.get(4..6).unwrap().parse().unwrap();
    Zone::Offset(sign * (hours * 3600 + minutes * 60))
}

/// Every case of `reference/date/golden.php`.
#[test]
fn matches_php() {
    let golden = include_str!("../../tests/golden/date.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let mut fields = line
            .split('\t')
            .map(|f| String::from_utf8(unhex(f)).unwrap());
        let mut field = || fields.next().unwrap();
        let (stored, zone_name, format_text, php) = (field(), field(), field(), field());
        let ours = format(&format_text, &stored, zone(&zone_name));
        if ours != php {
            failures.push(format!(
                "{stored} {zone_name} {format_text:?}: php {php:?}, ours {ours:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(12)].join("\n")
    );
    assert!(golden.lines().count() > 20_000);
}

#[test]
fn the_empty_date_carries_over() {
    assert_eq!(
        format("Y-m-d H:i:s", "0000-00-00 00:00:00", Zone::Utc),
        "-0001-11-30 00:00:00"
    );
    assert_eq!(format("y X", "0000-00-00 00:00:00", Zone::Utc), "-1 -0001");
}

#[test]
fn site_zones_like_wp_timezone() {
    // Offsets as PHP builds them from `wp_timezone_string()`.
    for (gmt_offset, seconds) in [
        ("0", 0),
        ("5.5", 19_800),
        ("-0.5", -1_800),
        ("-3", -10_800),
        ("5.75", 20_700),
        ("-9.5", -34_200),
        ("", 0),
        ("abc", 0),
    ] {
        assert_eq!(
            site_zone(b"", gmt_offset.as_bytes()),
            Zone::Offset(seconds),
            "{gmt_offset}"
        );
    }
    let name = |zone| match zone {
        Zone::Named(named) => named.name,
        other => panic!("{other:?}"),
    };
    assert_eq!(name(site_zone(b"UTC", b"5")), "UTC");
    assert_eq!(name(site_zone(b"europe/BERLIN", b"1")), "Europe/Berlin");
    // PHP throws on these; WordPress never saves them.
    assert_eq!(site_zone(b"Bogus/Zone", b"1"), Zone::Utc);
    assert_eq!(site_zone(b"Europe/\xff", b"1"), Zone::Utc);
}

/// `wp_timezone_override_offset()`: a named zone's offset at the time, in
/// hours, and none without a zone that `timezone_open()` opens.
#[test]
fn named_zones_override_the_stored_offset() {
    // 2026-07-01 and 2026-01-01 in New York, in and out of daylight time.
    assert_eq!(
        override_offset(b"America/New_York", 1_782_878_400),
        Some(-4.0)
    );
    assert_eq!(
        override_offset(b"America/New_York", 1_767_243_600),
        Some(-5.0)
    );
    assert_eq!(override_offset(b"Asia/Kathmandu", 0), Some(5.5));
    assert_eq!(
        override_offset(b"Asia/Kathmandu", 1_767_243_600),
        Some(5.75)
    );
    assert_eq!(override_offset(b"", 0), None);
    assert_eq!(override_offset(b"Bogus/Zone", 0), None);
}

#[test]
fn strtotime_reads_iso_dates_like_php() {
    // PHP's strtotime() in UTC, for the forms both read.
    for (text, unix) in [
        ("2024-05-06T07:08:09+02:00", Some(1_714_972_089)),
        ("2024-05-06T07:08:09-0130", Some(1_714_984_689)),
        ("2024-05-06 07:08", Some(1_714_979_280)),
        ("2024-05-06T07:08:09Z", Some(1_714_979_289)),
        ("2024-05-06", Some(1_714_953_600)),
        ("1969-12-31 23:59:59", Some(-1)),
        ("2024-02-30 10:00:00", Some(1_709_287_200)),
        ("2024-05-06 24:00:00", Some(1_715_040_000)),
        ("2024-05-06T07:08:09+10:45", Some(1_714_940_589)),
        ("2024-05-06 07:08:60", Some(1_714_979_340)),
        ("2024-13-01 00:00:00", None),
        ("2024-05-06 07:60:00", None),
        ("2024-05-06T07:08:09 extra", None),
        ("", None),
    ] {
        assert_eq!(strtotime(text), unix, "{text}");
    }
    // Forms outside the subset, which PHP would read differently.
    for text in [
        "2024/05/06 07:08",
        "2024-05-06X07:08",
        "x024-05-06 07:08",
        "2024-x5-06",
        "2024-05-06 07:08:5",
        "2024-05-06 24:00:01",
        "2024-05-06 07:08:61",
        "2024-05-06T07:08+0x:00",
        "2024-05-06T07:08+02:0x",
        "2024-05-00",
        "2024-05-06 07x08",
        "2024-05-06 24:01:00",
        "2024-05-0x",
        "2024-05-06 0x:08",
        "2024-05-06 07:x8",
        "2024-05-06T07:08+x2:00",
        "2024-05-06T07:08+02:x0",
        "2024-05-32",
    ] {
        assert_eq!(strtotime(text), None, "{text}");
    }
}

#[test]
fn unix_times_show_in_the_zone() {
    assert_eq!(
        format_unix("c", 0, Zone::Offset(3600)),
        "1970-01-01T01:00:00+01:00"
    );
}

/// Every case of `reference/date/zones-golden.php`, apart from where PHP's
/// time zone database or name parsing differs from `jiff-tzdb`'s.
#[test]
fn named_zones_match_php() {
    let golden = include_str!("../../tests/golden/date-zones.tsv");
    let format_text = "e|T|I|O|P|p|Z|U|Y-m-d H:i:s|B";
    // Zones whose data `jiff-tzdb` shares with another name: PHP keeps
    // their own history before 1970 (IANA's backzone file).
    let mut shared = std::collections::HashMap::new();
    for name in jiff_tzdb::available() {
        *shared
            .entry(jiff_tzdb::get(name).unwrap().1.as_ptr())
            .or_insert(0) += 1;
    }
    let merged = |name: &str| shared[&jiff_tzdb::get(name).unwrap().1.as_ptr()] > 1;
    let mut failures = Vec::new();
    let mut known = 0;
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [name, kind, input, php] = fields[..] else {
            panic!("{line}");
        };
        let zone = site_zone(name.as_bytes(), b"");
        let ours = match kind {
            "local" => format(format_text, input, zone),
            _ => format_unix(format_text, input.parse().unwrap(), zone),
        };
        if ours == php {
            continue;
        }
        let theirs: Vec<&str> = php.split('|').collect();
        let mut mine: Vec<&str> = ours.split('|').collect();
        // PHP prints the name as given.
        mine[0] = theirs[0];
        let before_1970 = theirs[7].starts_with('-');
        let explained = mine == theirs
            // `jiff-tzdb` is built from IANA's rearguard data, which
            // flips the daylight saving flag where it is negative.
            || ["Europe/Dublin", "Eire", "Africa/Casablanca", "Africa/El_Aaiun", "Africa/Windhoek"]
                .contains(&name)
                && mine.iter().zip(&theirs).filter(|(a, b)| a != b).count() == 1
                && mine[2] != theirs[2]
            // PHP reads these as abbreviations or offsets, not zones.
            || ["CET", "EET", "MET", "WET", "UCT", "GMT+0", "GMT-0"].contains(&name)
            || before_1970 && merged(name);
        if explained {
            known += 1;
        } else {
            failures.push(format!("{name} {kind} {input}: php {php}, ours {ours}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(30)].join("\n")
    );
    assert!(known < 1_000, "{known}");
    assert!(golden.lines().count() > 30_000);
}
