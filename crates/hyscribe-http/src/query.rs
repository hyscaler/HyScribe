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

/// `redirect_canonical()`'s last clean up of a query string: spaces and end
/// punctuation dropped after a final `p`, `page_id`, `cat` or `tag`, then
/// those arguments dropped where empty.
pub fn clean_query(query: &str) -> String {
    const PUNCTUATION: [&str; 24] = [
        " ",
        "%20",
        "!",
        "%21",
        "\"",
        "%22",
        "'",
        "%27",
        "(",
        "%28",
        ")",
        "%29",
        ",",
        "%2C",
        ".",
        "%2E",
        ";",
        "%3B",
        "{",
        "%7B",
        "}",
        "%7D",
        "%E2%80%9C",
        "%E2%80%9D",
    ];
    let names = ["p", "page_id", "cat", "tag"];
    let mut query = query.to_string();
    let last = query.rfind('&').map_or(0, |at| at + 1);
    let named = query
        .get(last..)
        .and_then(|arg| arg.split_once('='))
        .is_some_and(|(name, _)| names.contains(&name));
    // Neither `=` nor the names hold punctuation, so this stops at the value.
    while let Some(p) = PUNCTUATION.iter().find(|p| named && query.ends_with(*p)) {
        query.truncate(query.len() - p.len());
    }
    // `#(^|&)(p|page_id|cat|tag)=?(&|$)#` to `&`, matches never overlapping.
    let mut out = String::with_capacity(query.len());
    let mut rest = query.as_str();
    let mut at_start = true;
    while !rest.is_empty() {
        let lead = if rest.starts_with('&') { 1 } else { 0 };
        let empty = (at_start || lead == 1)
            .then(|| {
                names.iter().find_map(|name| {
                    let tail = rest.get(lead..).unwrap_or_default().strip_prefix(name)?;
                    let tail = tail.strip_prefix('=').unwrap_or(tail);
                    // The match takes the `&` after it too.
                    (tail.is_empty() || tail.starts_with('&'))
                        .then(|| rest.len() - tail.len() + usize::from(!tail.is_empty()))
                })
            })
            .flatten();
        match empty {
            Some(len) => {
                out.push('&');
                rest = rest.get(len..).unwrap_or_default();
            }
            None => {
                let c = rest.chars().next().unwrap_or_default();
                out.push(c);
                rest = rest.get(c.len_utf8()..).unwrap_or_default();
            }
        }
        at_start = false;
    }
    out.trim_matches('&').to_string()
}
