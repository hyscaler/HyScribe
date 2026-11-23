// SPDX-License-Identifier: AGPL-3.0-or-later

//! The selector helpers of `WP_Theme_JSON`: splitting a selector list at its
//! top-level commas, appending, prepending, scoping, and the block style
//! variation class.

/// CSS whitespace as `WP_Theme_JSON` trims it.
const CSS_WS: [char; 3] = [' ', '\t', '\n'];

fn trim_css(text: &str) -> &str {
    text.trim_matches(CSS_WS)
}

/// True when no comment, string, function, escape or CDO can hide a comma.
fn plain(selector: &str) -> bool {
    !selector.contains(['/', '\'', '"', '(', '<', '\\'])
}

/// The selectors of a list with a comma: split on every comma when nothing
/// can hide one, else at top-level commas only.
fn parts(selector: &str) -> Vec<String> {
    if plain(selector) {
        trim_css(selector)
            .split(',')
            .map(|s| trim_css(s).to_owned())
            .collect()
    } else {
        split(selector)
    }
}

/// `append_to_selector( $selector, $to_append )`.
pub(crate) fn append(selector: &str, to_append: &str) -> String {
    if !selector.contains(',') {
        return format!("{}{to_append}", trim_css(selector));
    }
    let parts = parts(selector);
    let mut out = parts.join(&format!("{to_append}, "));
    out.push_str(to_append);
    out
}

/// `prepend_to_selector( $selector, $to_prepend )`.
pub(crate) fn prepend(selector: &str, to_prepend: &str) -> String {
    if !selector.contains(',') {
        return format!("{to_prepend}{}", trim_css(selector));
    }
    let parts = parts(selector);
    format!("{to_prepend}{}", parts.join(&format!(", {to_prepend}")))
}

/// `split_selector_list( $selector )`: the top-level selectors, trimmed.
pub(crate) fn split(selector: &str) -> Vec<String> {
    if !selector.contains(',') {
        return vec![trim_css(selector).to_owned()];
    }
    let bytes = selector.as_bytes();
    let len = bytes.len();
    let byte = |i: usize| bytes.get(i).copied().unwrap_or(0);
    let piece =
        |from: usize, to: usize| trim_css(selector.get(from..to).unwrap_or_default()).to_owned();
    let mut selectors = Vec::new();
    let mut depth = 0i64;
    let mut at = 0;
    let mut was_at = 0;
    // Every pass moves `at` forward; the bound only rules out a hang.
    for _ in 0..=len {
        let Some(next_at) = (at..len).find(|&i| b"/,'\"()\\".contains(&byte(i))) else {
            break;
        };
        match byte(next_at) {
            b'\\' => at = (next_at + 2).min(len),
            b'(' => {
                depth += 1;
                at = next_at + 1;
            }
            b')' => {
                depth -= 1;
                at = next_at + 1;
            }
            quote @ (b'\'' | b'"') => {
                let mut end = next_at + 1;
                for _ in 0..len {
                    match (end..len).find(|&i| byte(i) == quote || byte(i) == b'\\') {
                        None => {
                            end = len;
                            break;
                        }
                        Some(i) if byte(i) == b'\\' => end = i + 2,
                        Some(i) => {
                            end = i + 1;
                            break;
                        }
                    }
                }
                at = end;
            }
            b'/' if byte(next_at + 1) == b'*' => {
                at = selector
                    .get(next_at + 1..)
                    .and_then(|rest| rest.find("*/"))
                    .map_or(len, |i| next_at + 1 + i + 2);
            }
            b',' if depth == 0 => {
                selectors.push(piece(was_at, next_at));
                at = next_at + 1;
                was_at = at;
            }
            _ => at = next_at + 1,
        }
    }
    if was_at < len {
        selectors.push(piece(was_at, len));
    }
    selectors
}

/// PHP's `empty()` of a string.
fn falsy(text: &str) -> bool {
    text.is_empty() || text == "0"
}

/// `scope_selector( $scope, $selector )`.
pub(crate) fn scope(scope: &str, selector: &str) -> String {
    if falsy(scope) || falsy(selector) {
        return selector.to_owned();
    }
    let inners = split(selector);
    let mut scoped = Vec::new();
    for outer in split(scope) {
        for inner in &inners {
            match (falsy(&outer), falsy(inner)) {
                (false, false) => scoped.push(format!("{outer} {inner}")),
                (true, _) => scoped.push(inner.clone()),
                (false, true) => scoped.push(outer.clone()),
            }
        }
    }
    scoped.join(", ")
}

/// `get_block_style_variation_selector( $name, $block_selector )`: the
/// variation class after the first run of characters that are neither
/// whitespace nor `:`, in every selector of the list.
pub(crate) fn variation(name: &str, block_selector: &str) -> String {
    let class = format!(".is-style-{name}");
    if falsy(block_selector) {
        return class;
    }
    let stop = |c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r' | ':');
    split(block_selector)
        .iter()
        .map(|part| match part.find(|c: char| !stop(c)) {
            Some(start) => {
                let end = part
                    .get(start..)
                    .and_then(|r| r.find(stop))
                    .map_or(part.len(), |i| start + i);
                let (head, tail) = (
                    part.get(..end).unwrap_or_default(),
                    part.get(end..).unwrap_or_default(),
                );
                format!("{head}{class}{tail}")
            }
            None => part.clone(),
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests;
