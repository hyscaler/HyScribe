// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading query vars from request input, as `WP_Query` and PHP do. Every
//! function here takes untrusted input, so the fuzz target
//! `http_query` covers them all.

/// `raw_query` without the `name` argument, as `&`-joined pairs.
pub fn other_args(raw_query: Option<&str>, name: &str) -> String {
    raw_query
        .unwrap_or_default()
        .split('&')
        .filter(|pair| !pair.is_empty() && pair.split('=').next() != Some(name))
        .map(|pair| format!("&{pair}"))
        .collect()
}

/// The `m` query var's date parts, as `WP_Query` reads them: digits only,
/// `YYYY`, then `MM`, then `DD`.
pub fn date_parts(m: &str) -> (Option<u64>, Option<u64>, Option<u64>) {
    let digits: String = m.chars().filter(char::is_ascii_digit).collect();
    let part = |range: std::ops::Range<usize>| digits.get(range).and_then(|d| d.parse().ok());
    if digits.is_empty() {
        return (None, None, None);
    }
    let year = part(0..4).or_else(|| digits.parse().ok());
    (year, part(4..6), part(6..8))
}

/// Search terms as WordPress splits them: on spaces, tabs, commas and plus
/// signs. Stopword removal comes with i18n (M9.4).
pub fn search_terms(search: &str) -> Vec<String> {
    search
        .split([' ', '\t', ',', '+'])
        .filter(|term| !term.is_empty())
        .map(String::from)
        .collect()
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// PHP's `date()` for the formats archive titles use, applied to a stored
/// `YYYY-MM-DD HH:MM:SS` value. Other format characters pass through.
pub fn php_date(format: &str, stored: &str) -> String {
    let number = |range: std::ops::Range<usize>| -> u64 {
        stored.get(range).and_then(|d| d.parse().ok()).unwrap_or(0)
    };
    let (year, month, day) = (number(0..4), number(5..7), number(8..10));
    let month_name = month
        .checked_sub(1)
        .and_then(|m| usize::try_from(m).ok())
        .and_then(|m| MONTHS.get(m))
        .copied()
        .unwrap_or("");
    let mut out = String::new();
    let mut chars = format.chars();
    while let Some(c) = chars.next() {
        match c {
            'Y' => out.push_str(&format!("{year:04}")),
            'y' => out.push_str(&format!("{:02}", year % 100)),
            'F' => out.push_str(month_name),
            'M' => out.push_str(month_name.get(..3).unwrap_or("")),
            'm' => out.push_str(&format!("{month:02}")),
            'n' => out.push_str(&month.to_string()),
            'd' => out.push_str(&format!("{day:02}")),
            'j' => out.push_str(&day.to_string()),
            '\\' => out.extend(chars.next()),
            c => out.push(c),
        }
    }
    out
}

/// PHP's `absint()`: the leading digits as a number, else 0.
pub fn absint(value: &str) -> u64 {
    let digits: String = value
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    // (int) saturates at PHP_INT_MAX.
    match digits.parse() {
        Ok(n) => n,
        Err(_) if digits.is_empty() => 0,
        Err(_) => i64::MAX.unsigned_abs(),
    }
}
