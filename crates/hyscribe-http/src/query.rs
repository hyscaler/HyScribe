// SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading query vars from request input, as `WP_Query` and PHP do. Every
//! function here takes untrusted input, so the fuzz target
//! `http_query` covers them all.

use hyscribe_model::TermFilter;
use hyscribe_phpser::{Key, Value, decode, is_empty_string};

/// `raw_query` without the `names` arguments, as `&`-joined pairs.
pub fn other_args(raw_query: Option<&str>, names: &[&str]) -> String {
    raw_query
        .unwrap_or_default()
        .split('&')
        .filter(|pair| {
            !pair.is_empty() && !names.contains(&pair.split('=').next().unwrap_or_default())
        })
        .map(|pair| format!("&{pair}"))
        .collect()
}

/// The `m` query var's date parts, as `WP_Query` reads them: digits only,
/// `YYYY`, then `MM`, then `DD`, none for `0`, which PHP takes as false.
pub fn date_parts(m: &str) -> (Option<u64>, Option<u64>, Option<u64>) {
    let digits: String = m.chars().filter(char::is_ascii_digit).collect();
    let part = |range: std::ops::Range<usize>| digits.get(range).and_then(|d| d.parse().ok());
    if digits.is_empty() || digits == "0" {
        return (None, None, None);
    }
    let year = part(0..4).or_else(|| digits.parse().ok());
    (year, part(4..6), part(6..8))
}

/// WordPress's `absint()` on request input.
pub fn absint(value: &str) -> u64 {
    hyscribe_phpser::absint(&hyscribe_phpser::Value::String(value.as_bytes().to_vec()))
}

/// The `tax_query` `WP_Query` makes of the `cat` query var: all but digits,
/// commas and dashes dropped, then IDs split on commas, positive ones in and
/// negative ones out, each with its children in `hierarchy`, the stored
/// `category_children`; none for an empty() `cat`.
pub fn category_filters(cat: &str, hierarchy: &[u8]) -> Vec<TermFilter> {
    let cat: Vec<u8> = cat
        .bytes()
        .filter(|&b| b.is_ascii_digit() || b == b',' || b == b'-')
        .collect();
    if is_empty_string(&cat) {
        return vec![];
    }
    let (mut included, excluded): (Vec<i64>, Vec<i64>) = cat
        .split(|&b| b == b',')
        .map(|id| Value::String(id.to_vec()).to_int())
        .filter(|&id| id != 0)
        .partition(|id| id.is_positive());
    // sort() of the list puts the archive's category first.
    included.sort_unstable();
    let tree = decode(hierarchy).unwrap_or(Value::Null);
    [(included, false), (excluded, true)]
        .into_iter()
        .filter(|(ids, _)| !ids.is_empty())
        .map(|(ids, exclude)| TermFilter {
            taxonomy: "category".into(),
            terms: with_children(&tree, ids.iter().map(|id| id.unsigned_abs()).collect()),
            exclude,
        })
        .collect()
}

/// `ids` and, after them, their descendants in `tree`, as
/// get_term_children() walks it, once each where a cycle would loop.
fn with_children(tree: &Value, mut ids: Vec<u64>) -> Vec<u64> {
    let mut next = 0;
    while let Some(&id) = ids.get(next) {
        let children = tree
            .get(&Key::Int(i64::try_from(id).unwrap_or(i64::MAX)))
            .and_then(Value::as_array)
            .unwrap_or_default();
        for (_, child) in children {
            let child = child.to_int().unsigned_abs();
            if !ids.contains(&child) {
                ids.push(child);
            }
        }
        next += 1;
    }
    ids
}
