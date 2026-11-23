// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP's natural order comparison, `strnatcmp_ex()` from
//! `ext/standard/strnatcmp.c`: runs of digits compare as numbers, leading
//! zeros and whitespace are skipped. Past the end a string reads as NUL,
//! as PHP's NUL terminated strings do.

use std::cmp::Ordering;

/// `strnatcasecmp( $a, $b )`, as an ordering: `strnatcmp_ex()` ignoring
/// ASCII case.
pub fn strnatcasecmp(a: &[u8], b: &[u8]) -> Ordering {
    if a.is_empty() || b.is_empty() {
        return a.len().cmp(&b.len());
    }
    let (mut ai, mut bi) = (0, 0);
    let mut leading = true;
    loop {
        let (mut ca, mut cb) = (at(a, ai), at(b, bi));
        if leading {
            while ca == b'0' && digit(a, ai + 1) {
                ai += 1;
                ca = at(a, ai);
            }
            while cb == b'0' && digit(b, bi + 1) {
                bi += 1;
                cb = at(b, bi);
            }
            leading = false;
        }
        while is_space(ca) {
            ai += 1;
            ca = at(a, ai);
        }
        while is_space(cb) {
            bi += 1;
            cb = at(b, bi);
        }
        if ca.is_ascii_digit() && cb.is_ascii_digit() {
            let result = if ca == b'0' || cb == b'0' {
                compare_left(a, &mut ai, b, &mut bi)
            } else {
                compare_right(a, &mut ai, b, &mut bi)
            };
            match (result, ai == a.len(), bi == b.len()) {
                (Ordering::Equal, true, true) => return Ordering::Equal,
                (Ordering::Equal, true, false) => return Ordering::Less,
                (Ordering::Equal, false, true) => return Ordering::Greater,
                (Ordering::Equal, false, false) => {}
                (unequal, ..) => return unequal,
            }
            (ca, cb) = (at(a, ai), at(b, bi));
        }
        match ca.to_ascii_uppercase().cmp(&cb.to_ascii_uppercase()) {
            Ordering::Equal => {}
            unequal => return unequal,
        }
        ai += 1;
        bi += 1;
        match (ai >= a.len(), bi >= b.len()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Less,
            (false, true) => return Ordering::Greater,
            (false, false) => {}
        }
    }
}

/// C's `isspace()` in the C locale.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// The byte at `i`, NUL past the end.
fn at(s: &[u8], i: usize) -> u8 {
    s.get(i).copied().unwrap_or(0)
}

/// Whether `s` has a digit at `i`.
fn digit(s: &[u8], i: usize) -> bool {
    s.get(i).is_some_and(u8::is_ascii_digit)
}

/// Two numbers with a leading zero, as fractions: the first different
/// digit decides.
fn compare_left(a: &[u8], ai: &mut usize, b: &[u8], bi: &mut usize) -> Ordering {
    loop {
        match (digit(a, *ai), digit(b, *bi)) {
            (false, false) => return Ordering::Equal,
            (false, true) => return Ordering::Less,
            (true, false) => return Ordering::Greater,
            (true, true) => match at(a, *ai).cmp(&at(b, *bi)) {
                Ordering::Equal => {}
                unequal => return unequal,
            },
        }
        *ai += 1;
        *bi += 1;
    }
}

/// Two whole numbers: the longer run of digits wins, else the first
/// different digit.
fn compare_right(a: &[u8], ai: &mut usize, b: &[u8], bi: &mut usize) -> Ordering {
    let mut bias = Ordering::Equal;
    loop {
        match (digit(a, *ai), digit(b, *bi)) {
            (false, false) => return bias,
            (false, true) => return Ordering::Less,
            (true, false) => return Ordering::Greater,
            (true, true) => {
                if bias == Ordering::Equal {
                    bias = at(a, *ai).cmp(&at(b, *bi));
                }
            }
        }
        *ai += 1;
        *bi += 1;
    }
}
