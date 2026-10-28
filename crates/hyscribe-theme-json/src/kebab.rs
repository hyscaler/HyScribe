// SPDX-License-Identifier: AGPL-3.0-or-later

//! `_wp_to_kebab_case()`, lodash's `kebabCase`: the words its regex finds,
//! joined by `-` and lower cased. The regex alternatives are matched by
//! hand in their order of preference. Character classes follow the PHP
//! pattern for ASCII and Latin-1; other characters count as "misc" (letters
//! without case), except general punctuation and spaces, which break words.

/// `_wp_to_kebab_case( $text )`.
pub fn kebab_case(text: &str) -> String {
    let chars: Vec<char> = text.chars().filter(|&c| c != '\'').collect();
    let mut words: Vec<String> = Vec::new();
    let mut at = 0;
    // Every pass moves `at` forward; the bound only rules out a hang.
    for _ in 0..=chars.len() {
        match word_at(&chars, at) {
            Some(end) => {
                words.push(chars.get(at..end).unwrap_or_default().iter().collect());
                at = end;
            }
            None => at += 1,
        }
    }
    words.join("-").to_ascii_lowercase()
}

fn lower(c: char) -> bool {
    matches!(c, 'a'..='z' | '\u{df}'..='\u{f6}' | '\u{f8}'..='\u{ff}')
}

fn upper(c: char) -> bool {
    matches!(c, 'A'..='Z' | '\u{c0}'..='\u{d6}' | '\u{d8}'..='\u{de}')
}

fn digit(c: char) -> bool {
    c.is_ascii_digit()
}

fn breaks(c: char) -> bool {
    matches!(
        c,
        '\0'..='\u{2f}'
            | '\u{3a}'..='\u{40}'
            | '\u{5b}'..='\u{60}'
            | '\u{7b}'..='\u{bf}'
            | '\u{2000}'..='\u{206f}'
            | '\u{feff}'
            | '\u{1680}'
            | '\u{180e}'
            | '\u{3000}'
    )
}

fn misc(c: char) -> bool {
    !(breaks(c) || digit(c) || lower(c) || upper(c))
}

/// `\w` as PCRE's Unicode mode sees it.
fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The ordinal suffix after the digit `n`: `1ST`, `2ND`, `3RD`, else `TH`.
fn ordinal_suffix(n: char, capital: bool) -> &'static str {
    match (n, capital) {
        ('1', true) => "ST",
        ('2', true) => "ND",
        ('3', true) => "RD",
        (_, true) => "TH",
        ('1', false) => "st",
        ('2', false) => "nd",
        ('3', false) => "rd",
        (_, false) => "th",
    }
}

/// The end of the word the regex matches at `at`, if any.
fn word_at(chars: &[char], at: usize) -> Option<usize> {
    let is = |i: usize, class: fn(char) -> bool| chars.get(i).is_some_and(|&c| class(c));
    let run = |from: usize, class: fn(char) -> bool| {
        from + chars
            .get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&c| class(c))
            .count()
    };
    let end = chars.len();
    let misc_lower = |c: char| lower(c) || misc(c);
    let misc_upper = |c: char| upper(c) || misc(c);
    let start = at + usize::from(is(at, upper));

    // `Upper? Lower+ (?= Break | Upper | $)` comes first in the regex, but
    // where it matches, `MiscUpper+` cannot and `Upper? MiscLower+` matches
    // the same characters, so it is left out.
    // MiscUpper+ (?= Break | Upper MiscLower | $), backtracking.
    let uppers = run(at, misc_upper);
    if uppers > at {
        if uppers == end || is(uppers, breaks) {
            return Some(uppers);
        }
        if let Some(e) = (at + 1..=uppers)
            .rev()
            .find(|&e| is(e, upper) && is(e + 1, misc_lower))
        {
            return Some(e);
        }
    }
    // Upper? MiscLower+
    let mixed = run(start, misc_lower);
    if mixed > start {
        return Some(mixed);
    }
    // Upper+
    let caps = run(at, upper);
    if caps > at {
        return Some(caps);
    }
    // Ordinals: \d*(?:1ST|2ND|3RD|(?![123])\dTH), then \d*(?:1st|...).
    let digits = run(at, digit);
    for capital in [true, false] {
        for p in (at..digits).rev() {
            let suffix = ordinal_suffix(chars.get(p).copied().unwrap_or('0'), capital);
            let q = p + 3;
            let matched = chars
                .get(p + 1..q)
                .is_some_and(|s| s.iter().copied().eq(suffix.chars()));
            let after = |c: char| {
                c == '_'
                    || (capital && c.is_ascii_lowercase())
                    || (!capital && c.is_ascii_uppercase())
            };
            if matched && (q == end || !is(q, word) || chars.get(q).is_some_and(|&c| after(c))) {
                return Some(q);
            }
        }
    }
    // \d+
    (digits > at).then_some(digits)
}

#[cfg(test)]
mod tests;
