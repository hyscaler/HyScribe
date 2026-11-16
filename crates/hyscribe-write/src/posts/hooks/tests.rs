// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn days_are_those_gmdate_prints() {
    assert_eq!(day("2026-01-02 03:04:05"), "2026-01-02");
    assert_eq!(day(super::super::ZERO_DATE), "-0001-11-30");
}
