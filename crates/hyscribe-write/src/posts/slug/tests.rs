// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn slugs_truncate_as_php() {
    assert_eq!(truncate(b"abc-", 10).unwrap(), b"abc");
    assert_eq!(truncate(b"abcd", 4).unwrap(), b"abcd");
    assert_eq!(truncate(b"abcdef", 3).unwrap(), b"abc");
    assert_eq!(truncate(b"ab-cd", 3).unwrap(), b"ab");
    assert!(matches!(
        truncate(b"%c3%a9%c3%a9", 6),
        Err(Error::Unported(_))
    ));
}

#[test]
fn page_numbers_are_reserved() {
    assert!(paged(b"page2"));
    assert!(paged(b"2"));
    assert!(!paged(b"page"));
    assert!(!paged(b"p2"));
    assert!(!paged(b"2a"));
}

#[test]
fn numeric_slugs_read_as_dates_where_the_structure_says() {
    assert!(reads_as_date(b"/%postname%/", 99));
    assert!(reads_as_date(b"/0/%postname%/", 99));
    assert!(reads_as_date(b"/%year%/%postname%/", 12));
    assert!(!reads_as_date(b"/%year%/%postname%/", 13));
    assert!(reads_as_date(b"/%monthnum%/%postname%/", 31));
    assert!(!reads_as_date(b"/%monthnum%/%postname%/", 32));
    assert!(!reads_as_date(b"/%category%/%postname%/", 1));
    assert!(!reads_as_date(b"", 1));
}
