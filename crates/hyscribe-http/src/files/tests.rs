// SPDX-License-Identifier: AGPL-3.0-or-later

use super::http_date;

/// The forms and checks of APR's `apr_date_parse_http()`.
#[test]
fn reads_dates_as_apache_does() {
    for (text, unix) in [
        ("Sun, 06 Nov 1994 08:49:37 GMT", Some(784_111_777)),
        ("  Sun, 6 Nov 1994 08:49:37 GMT", Some(784_111_777)),
        ("Sunday, 06-Nov-94 08:49:37 GMT", Some(784_111_777)),
        ("Sun Nov  6 08:49:37 1994", Some(784_111_777)),
        ("Sun Nov 06 08:49:37 1994", Some(784_111_777)),
        ("x 01-Jan-26 00:00:00 GMT", Some(1_767_225_600)),
        ("x 01-Jan-70 00:00:00 GMT", Some(0)),
        ("x 29 Feb 2024 00:00:00 x", Some(1_709_164_800)),
        ("x 29 Feb 2000 00:00:00 x", Some(951_782_400)),
        ("x 30 Apr 2026 00:00:00 x", Some(1_777_507_200)),
        ("x 01 Jan 1970 00:00:00 x", Some(0)),
        ("x 31 Dec 2025 23:59:61 x", Some(1_767_225_601)),
        ("x 29 Feb 2100 00:00:00 x", None),
        ("x 29 Feb 2026 00:00:00 x", None),
        ("x 31 Apr 2026 00:00:00 x", None),
        ("x 00 Jan 2026 00:00:00 x", None),
        ("x 32 Jan 2026 00:00:00 x", None),
        ("x 01 Foo 2026 00:00:00 x", None),
        ("x 01 Jan 1899 00:00:00 x", None),
        ("x 31 Dec 1969 23:59:59 x", None),
        ("x 01 Jan 2026 24:00:00 x", None),
        ("x 01 Jan 2026 00:60:00 x", None),
        ("x 01 Jan 2026 00:00:62 x", None),
        ("x 01 Jan 2026 00:00:00", None),
        ("nospace", None),
        ("", None),
    ] {
        assert_eq!(http_date(text), unix, "{text}");
    }
}
