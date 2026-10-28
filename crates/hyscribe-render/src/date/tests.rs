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
            Some(Zone::Offset(seconds)),
            "{gmt_offset}"
        );
    }
    assert_eq!(site_zone(b"UTC", b"5"), Some(Zone::Utc));
    assert_eq!(site_zone(b"Europe/Berlin", b"1"), None);
}
