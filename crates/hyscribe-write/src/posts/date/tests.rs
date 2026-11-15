// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn only_stored_dates_are_ported() {
    assert_eq!(
        canonical("2026-01-02 03:04:05").unwrap(),
        "2026-01-02 03:04:05"
    );
    assert_eq!(
        canonical("2026-01-02 23:59:59").unwrap(),
        "2026-01-02 23:59:59"
    );
    for date in [
        "2026-1-02 03:04:05",
        "2026-01-02T03:04:05",
        "2026-01-02 03:04:05 ",
        "2026-01-02 24:00:00",
        "2026-01-02 23:60:00",
        "2026-01-02 23:59:60",
        "2026/01-02 03:04:05",
        "2026-01/02 03:04:05",
        "2026-01-02 03-04:05",
        "2026-01-02 03:04-05",
        "x026-01-02 03:04:05",
    ] {
        assert!(matches!(canonical(date), Err(Error::Unported(_))), "{date}");
    }
}

#[test]
fn days_check_as_php() {
    assert!(checkdate(2, 29, 2024));
    assert!(checkdate(2, 29, 2000));
    assert!(!checkdate(2, 29, 2023));
    assert!(!checkdate(2, 29, 1900));
    assert!(checkdate(2, 28, 2023));
    assert!(!checkdate(4, 31, 2026));
    assert!(checkdate(4, 30, 2026));
    assert!(checkdate(12, 31, 2026));
    assert!(!checkdate(13, 1, 2026));
    assert!(!checkdate(0, 1, 2026));
    assert!(!checkdate(1, 0, 2026));
    assert!(!checkdate(1, 1, 0));
    assert!(checkdate(1, 1, 32767));
    assert!(!checkdate(1, 1, 32768));
}

#[test]
fn days_are_read_as_the_pattern_reads_them() {
    assert_eq!(ymd("2026-1-2 x"), (2026, 1, 2));
    assert_eq!(ymd("2026-01-02"), (2026, 1, 2));
    assert_eq!(ymd("2026-01-123"), (2026, 1, 12));
    assert_eq!(ymd("2026-123-1"), (0, 0, 0));
    assert_eq!(ymd("2026-01-"), (2026, 1, 0));
    assert_eq!(ymd("2026-01"), (2026, 1, 0));
    assert_eq!(ymd("2026+01-01"), (0, 0, 0));
    assert_eq!(ymd("202-01-01"), (0, 0, 0));
    assert_eq!(ymd("2o26-01-01"), (0, 0, 0));
    assert_eq!(ymd("é026-01-01"), (0, 0, 0));
    assert_eq!(ymd("2026-0x-01"), (0, 0, 0));
    assert_eq!(ymd("2026--01"), (0, 0, 0));
    assert_eq!(ymd("2026-01-x"), (2026, 1, 0));
}

#[test]
fn dates_resolve_in_the_site_zone() {
    let dates = Dates {
        zone: Zone::Offset(3600),
        now: 86_400,
    };
    assert_eq!(dates.local_now(), "1970-01-02 01:00:00");
    assert_eq!(dates.gmt_now(), "1970-01-02 00:00:00");
    assert_eq!(dates.gmt_from("2024-01-01 01:00:00"), "2024-01-01 00:00:00");
    assert_eq!(dates.unix("1970-01-01 01:00:10"), 10);
    assert_eq!(Dates::unix_gmt("1970-01-01 00:00:10"), 10);
    let resolve = |date, gmt| dates.resolve(date, gmt);
    assert_eq!(resolve("", "").unwrap(), "1970-01-02 01:00:00");
    assert_eq!(resolve("0", "0").unwrap(), "1970-01-02 01:00:00");
    assert_eq!(
        resolve(ZERO_DATE, ZERO_DATE).unwrap(),
        "1970-01-02 01:00:00"
    );
    assert_eq!(
        resolve("", "2024-05-06 07:08:09").unwrap(),
        "2024-05-06 08:08:09"
    );
    assert_eq!(
        resolve("2023-01-01 00:00:00", "x").unwrap(),
        "2023-01-01 00:00:00"
    );
    assert!(matches!(
        resolve("2026-02-30 00:00:00", ""),
        Err(Error::Refused("invalid_date"))
    ));
    assert!(matches!(
        resolve("someday", ""),
        Err(Error::Refused("invalid_date"))
    ));
    assert!(matches!(
        resolve("2026-1-1 00:00:00", ""),
        Err(Error::Unported(_))
    ));
    assert!(matches!(resolve("", "2024-05-06"), Err(Error::Unported(_))));
}
