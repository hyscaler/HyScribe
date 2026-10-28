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

/// WordPress's `absint()` on request input.
pub fn absint(value: &str) -> u64 {
    hyscribe_phpser::absint(&hyscribe_phpser::Value::String(value.as_bytes().to_vec()))
}
