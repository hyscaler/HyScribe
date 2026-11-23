// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP's `version_compare( $a, $b )`, which Hello's version write asks
//! of the stored version, written from its documented behaviour.

use alloc::vec::Vec;
use core::cmp::Ordering;

/// The special forms PHP orders words by, in the order it tries them;
/// a word starting with one takes its rank, any other ranks lowest.
const FORMS: [(&str, i8); 10] = [
    ("dev", 0),
    ("alpha", 1),
    ("a", 1),
    ("beta", 2),
    ("b", 2),
    ("RC", 3),
    ("rc", 3),
    ("#", 4),
    ("pl", 5),
    ("p", 5),
];

/// `version_compare( $a, $b )`'s answer as an ordering, over the
/// versions' bytes as PHP reads them.
pub fn version_compare(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes().to_vec(), b.as_bytes().to_vec());
    loop {
        match compare_once(&a, &b) {
            Ok(order) => return order,
            // PHP compares the rest against `#N#` again; the rest is
            // shorter each time.
            Err(again) => (a, b) = again,
        }
    }
}

type Pair = (Vec<u8>, Vec<u8>);

/// One round of PHP's comparison: its answer, or the pair it compares
/// next when one version has parts left past the other's.
fn compare_once(a: &[u8], b: &[u8]) -> Result<Ordering, Pair> {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ok(Ordering::Equal),
        (true, false) => return Ok(Ordering::Less),
        (false, true) => return Ok(Ordering::Greater),
        (false, false) => {}
    }
    let (a, b) = (prepared(a), prepared(b));
    let (mut p1, mut p2) = (&a[..], &b[..]);
    // Whether a dot followed the part last compared, as `strchr()` found.
    let (mut more1, mut more2) = (true, true);
    while !p1.is_empty() && !p2.is_empty() && more1 && more2 {
        let (x, rest1) = split(p1);
        let (y, rest2) = split(p2);
        (more1, more2) = (rest1.is_some(), rest2.is_some());
        let order = part(x, y);
        if order.is_ne() {
            return Ok(order);
        }
        p1 = rest1.unwrap_or(p1);
        p2 = rest2.unwrap_or(p2);
    }
    let next = b"#N#".to_vec();
    match (more1, more2) {
        (true, _) if digit(p1) => Ok(Ordering::Greater),
        (true, _) => Err((p1.to_vec(), next)),
        (false, true) if digit(p2) => Ok(Ordering::Less),
        (false, true) => Err((next, p2.to_vec())),
        (false, false) => Ok(Ordering::Equal),
    }
}

/// The part before the first dot, and what follows the dot if there is one.
fn split(version: &[u8]) -> (&[u8], Option<&[u8]>) {
    match version.iter().position(|&b| b == b'.') {
        Some(at) => (version.get(..at).unwrap_or_default(), version.get(at + 1..)),
        None => (version, None),
    }
}

/// A version as PHP compares it: canonical, unless it starts with `#`.
fn prepared(version: &[u8]) -> Vec<u8> {
    if version.starts_with(b"#") {
        version.to_vec()
    } else {
        canonical(version)
    }
}

fn digit(p: &[u8]) -> bool {
    p.first().is_some_and(u8::is_ascii_digit)
}

/// One part against another: numbers by value, words by their forms, and a
/// number above a word but below `pl`.
fn part(x: &[u8], y: &[u8]) -> Ordering {
    match (digit(x), digit(y)) {
        (true, true) => number(x).cmp(&number(y)),
        (false, false) => rank(x).cmp(&rank(y)),
        (true, false) => rank(b"#N#").cmp(&rank(y)),
        (false, true) => rank(x).cmp(&rank(b"#N#")),
    }
}

/// A part's leading digits as `strtol()` reads them, saturating.
fn number(p: &[u8]) -> i64 {
    p.iter()
        .take_while(|b| b.is_ascii_digit())
        .fold(0i64, |n, d| {
            n.saturating_mul(10).saturating_add(i64::from(d - b'0'))
        })
}

fn rank(word: &[u8]) -> i8 {
    FORMS
        .iter()
        .find(|(form, _)| word.starts_with(form.as_bytes()))
        .map_or(-1, |(_, rank)| *rank)
}

/// The version with `-`, `_`, `+` and other signs as dots, and a dot
/// between digits and other characters, never two dots in a row.
fn canonical(version: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(version.len() * 2);
    let mut last: Option<u8> = None;
    let not_digit = |c: u8| !c.is_ascii_digit() && c != b'.';
    for &c in version {
        // The first byte is kept as it is.
        let Some(previous) = last.replace(c) else {
            out.push(c);
            continue;
        };
        let special = matches!(c, b'-' | b'_' | b'+');
        let changes = !special
            && ((not_digit(previous) && c.is_ascii_digit())
                || (previous.is_ascii_digit() && not_digit(c)));
        if (special || changes || !c.is_ascii_alphanumeric()) && out.last() != Some(&b'.') {
            out.push(b'.');
        }
        if changes || (!special && c.is_ascii_alphanumeric()) {
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests;
