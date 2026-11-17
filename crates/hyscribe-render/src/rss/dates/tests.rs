// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn reads_feed_dates() {
    for (date, unix) in [
        ("2026-11-05T11:00:00.5Z", Some(1_793_876_401)),
        ("20261105T110000+0130", Some(1_793_871_000)),
        ("2026-11-05T11:00-05:00", Some(1_793_894_400)),
        ("2026-11", Some(1_793_491_200)),
        ("2026-11-05T11:00", Some(1_793_876_400)),
        ("5 Nov 2026 11:00", Some(1_793_876_400)),
        ("2026", Some(1_767_225_600)),
        ("2026-11-05T1", None),
        ("2026-11-05T11:00+5", None),
        ("2026-11-05x", None),
        ("Thu, 05 Nov 2026 11:00:00 GMT", Some(1_793_876_400)),
        ("05 nov 26 11:00 EST", Some(1_793_894_400)),
        ("5 Nov 70 11:00:00 PDT", Some(26_676_000)),
        ("Thu 5 Nov 2026 11:00 CDT", Some(1_793_894_400)),
        ("Thu 5 Nov 2026 11:00 MDT", Some(1_793_898_000)),
        ("Thu 5 Nov 2026 11:00 XYZ", None),
        ("Thu 5 Nov 2026 11", None),
        ("Thu 5 Nope 2026 11:00", None),
        ("2026/11/05", None),
        ("2026-11-05T11:00:00.", None),
        ("Thu, x Nov 2026 11:00", None),
        ("5 Nov year 11:00", None),
        ("5 Nov 2026 11:xx", None),
        ("5 Nov 2026 11:00 +00ab", None),
        ("5 Nov 2026 11:00 +ab00", None),
        ("5 Nov 2026 11:00 EDT", Some(1_793_890_800)),
        ("5 Nov 2026 11:00 PST", Some(1_793_905_200)),
    ] {
        assert_eq!(parse(date.as_bytes()), unix, "{date}");
    }
}

#[test]
fn reads_no_date_from_other_bytes() {
    assert_eq!(parse(b"\xff"), None);
}
