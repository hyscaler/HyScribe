// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn whitespace_normalizes_as_php() {
    assert_eq!(normalize_whitespace(b" a\r\n\n b \t c\t"), b"a\n b c");
    assert_eq!(normalize_whitespace(b"a\n\tb"), b"a\n b");
    assert_eq!(normalize_whitespace(b"a  \n\nb"), b"a \nb");
}

#[test]
fn days_are_those_gmdate_prints() {
    assert_eq!(day("2026-01-02 03:04:05"), "2026-01-02");
    assert_eq!(day(super::super::ZERO_DATE), "-0001-11-30");
}
