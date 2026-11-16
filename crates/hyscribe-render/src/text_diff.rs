// SPDX-License-Identifier: AGPL-3.0-or-later
//
// Ported from Text_Diff (wp-includes/Text/Diff.php, Diff/Engine/native.php,
// Diff/Renderer.php and Diff/Renderer/inline.php):
//   Copyright 2004 Geoffrey T. Dairiki <dairiki@dairiki.org>
//   Copyright 2004-2010 The Horde Project (http://www.horde.org/)
// Licensed under the LGPL-2.1, taken under its section 3 as the GPL version 2
// or any later version (docs/adr/0004-licensing.md).

//! `wp_text_diff()` as the revisions screen calls it: Text_Diff's native
//! engine and WordPress's table and inline renderers, over bytes as PHP
//! compares them.

use std::collections::{HashMap, HashSet};

use hyscribe_phpser::Value;

use crate::formatting::normalize_whitespace;

/// What Text_Diff found: lines copied, deleted, added or changed.
#[derive(Debug, PartialEq)]
enum Op<'a> {
    Copy(Vec<&'a [u8]>),
    Delete(Vec<&'a [u8]>),
    Add(Vec<&'a [u8]>),
    Change(Vec<&'a [u8]>, Vec<&'a [u8]>),
}

/// `_compareseq()`'s chunks of `_diag()`, which only change the speed:
/// no input tells a mutant apart (searched).
fn chunks((xoff, xlim): (i64, i64), (yoff, ylim): (i64, i64)) -> i64 {
    7.min(xlim - xoff).min(ylim - yoff) + 1
}

/// `$corresponding = $j < $other_len ? $i : $len`, whose two ends move the
/// run alike when `$j` meets `$other_len`: no input tells a mutant apart.
fn corresponding_end((j, other_len): (i64, i64), i: usize, len: usize) -> usize {
    if j < other_len { i } else { len }
}

/// The blank final rows before an original row whose match is up a ways:
/// past the first row, never reached (searched).
fn up(orig_pos: usize, pos: usize) -> Vec<i64> {
    (1..=(orig_pos - pos) as i64).map(|n| -n).collect()
}

/// `$list[$i]`, the default past its end.
fn get<T: Copy + Default>(list: &[T], i: usize) -> T {
    list.get(i).copied().unwrap_or_default()
}

/// `$list[$i] = $value`, within the list.
fn put<T>(list: &mut [T], i: usize, value: T) {
    let _ = list.get_mut(i).map(|slot| *slot = value);
}

/// An engine index as a list's; it is never negative where read.
fn ix(i: i64) -> usize {
    usize::try_from(i).unwrap_or_default()
}

/// PHP 8's `==` of two strings: numerically when both are numeric.
fn loose_eq(a: &[u8], b: &[u8]) -> bool {
    let (x, y) = (Value::String(a.to_vec()), Value::String(b.to_vec()));
    if !x.is_numeric() || !y.is_numeric() {
        return a == b;
    }
    // Integers compare as integers; a pair that both overflow, by bytes.
    let int = |text: &[u8]| {
        let text = String::from_utf8_lossy(text);
        let text = text.trim_matches([' ', '\t', '\n', '\r', '\x0b', '\x0c']);
        let digits = text.strip_prefix(['+', '-']).unwrap_or(text);
        let integral = !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit());
        integral.then(|| text.parse::<i64>().ok())
    };
    match (int(a), int(b)) {
        (Some(Some(m)), Some(Some(n))) => m == n,
        (Some(None), Some(None)) => a == b,
        _ => x.to_float() == y.to_float(),
    }
}

/// `Text_Diff_Engine_native`'s state over one pair of line lists.
struct Engine<'a> {
    xchanged: Vec<bool>,
    ychanged: Vec<bool>,
    xv: Vec<&'a [u8]>,
    yv: Vec<&'a [u8]>,
    xind: Vec<usize>,
    yind: Vec<usize>,
    seq: Vec<i64>,
    in_seq: HashSet<i64>,
    lcs: usize,
}

/// `Text_Diff_Engine_native::diff()` of lines without newlines.
fn diff<'a>(from: &[&'a [u8]], to: &[&'a [u8]]) -> Vec<Op<'a>> {
    let (n_from, n_to) = (from.len(), to.len());
    let mut e = Engine {
        xchanged: vec![false; n_from],
        ychanged: vec![false; n_to],
        xv: Vec::new(),
        yv: Vec::new(),
        xind: Vec::new(),
        yind: Vec::new(),
        seq: Vec::new(),
        in_seq: HashSet::new(),
        lcs: 0,
    };
    let skip = from.iter().zip(to).take_while(|(x, y)| x == y).count();
    // `--$xi > $skip && --$yi > $skip`, which leaves the skipped lines.
    let tails = [from, to].map(|lines| {
        let past = lines.get(skip..).and_then(|rest| rest.get(1..));
        past.unwrap_or_default()
    });
    let endskip = (tails[0].iter().rev().zip(tails[1].iter().rev()))
        .take_while(|(x, y)| x == y)
        .count();
    let middle = |lines: &[&'a [u8]], n: usize| {
        let lines = lines.iter().copied().enumerate().take(n - endskip);
        lines.skip(skip).collect::<Vec<_>>()
    };
    let xhash: HashSet<&[u8]> = middle(from, n_from).into_iter().map(|(_, l)| l).collect();
    let mut yhash = HashSet::new();
    for (yi, line) in middle(to, n_to) {
        let changed = !xhash.contains(line);
        put(&mut e.ychanged, yi, changed);
        if !changed {
            yhash.insert(line);
            e.yv.push(line);
            e.yind.push(yi);
        }
    }
    for (xi, line) in middle(from, n_from) {
        let changed = !yhash.contains(line);
        put(&mut e.xchanged, xi, changed);
        if !changed {
            e.xv.push(line);
            e.xind.push(xi);
        }
    }
    e.compareseq((0, e.xv.len() as i64), (0, e.yv.len() as i64));
    shift_boundaries(from, &mut e.xchanged, &e.ychanged);
    shift_boundaries(to, &mut e.ychanged, &e.xchanged);
    let mut edits = Vec::new();
    let mut x = from.iter().copied().zip(e.xchanged).peekable();
    let mut y = to.iter().copied().zip(e.ychanged).peekable();
    loop {
        let mut copy = Vec::new();
        while let (Some((line, false)), Some((_, false))) = (x.peek(), y.peek()) {
            copy.push(*line);
            x.next();
            y.next();
        }
        let changed = |lines: &mut std::iter::Peekable<_>| {
            std::iter::from_fn(|| lines.next_if(|(_, changed)| *changed))
                .map(|(line, _)| line)
                .collect::<Vec<&[u8]>>()
        };
        let (delete, add) = (changed(&mut x), changed(&mut y));
        if !copy.is_empty() {
            edits.push(Op::Copy(copy));
        }
        match (delete.is_empty(), add.is_empty()) {
            (false, false) => edits.push(Op::Change(delete, add)),
            (false, true) => edits.push(Op::Delete(delete)),
            (true, false) => edits.push(Op::Add(add)),
            // Both lists are done: PHP asserts nothing else is left.
            (true, true) => break,
        }
    }
    edits
}

/// `$ymids[$k] = $ymids[$k - 1]`.
fn grow(ymids: &mut Vec<Vec<i64>>, k: usize) {
    if ymids.len() <= k {
        ymids.resize(k + 1, Vec::new());
    }
    let before = ymids.get(k.wrapping_sub(1)).cloned().unwrap_or_default();
    put(ymids, k, before);
}

impl<'a> Engine<'a> {
    /// `$this->seq[$k]`.
    fn seq(&self, k: usize) -> i64 {
        get(&self.seq, k)
    }

    /// `_diag()`: the longest common subsequence's length and the points
    /// that split the problem into `nchunks` parts.
    fn diag(&mut self, x: (i64, i64), y: (i64, i64), nchunks: i64) -> (usize, Vec<(i64, i64)>) {
        let flip = x.1 - x.0 > y.1 - y.0;
        let ((xoff, xlim), (yoff, ylim)) = if flip { (y, x) } else { (x, y) };
        let (xs, ys) = if flip {
            (self.yv.clone(), &self.xv)
        } else {
            (self.xv.clone(), &self.yv)
        };
        let mut ymatches: HashMap<&'a [u8], Vec<i64>> = HashMap::new();
        for i in (yoff..ylim).rev() {
            ymatches.entry(get(ys, ix(i))).or_default().push(i);
        }
        self.lcs = 0;
        self.seq = vec![yoff - 1];
        self.in_seq.clear();
        let mut ymids: Vec<Vec<i64>> = vec![Vec::new()];
        let numer = xlim - xoff + nchunks - 1;
        let mut xi = xoff;
        for chunk in 0..nchunks {
            let x1 = xoff + (numer + (xlim - xoff) * chunk) / nchunks;
            while xi < x1 {
                let line = get(&xs, ix(xi));
                xi += 1;
                let Some(matches) = ymatches.get(line) else {
                    continue;
                };
                // `while ($y = current($matches))`, which stops at a 0 too.
                let mut found = matches.iter().copied().take_while(|&y| y != 0).peekable();
                let mut k = 0;
                while let Some(&y) = found.peek() {
                    if !self.in_seq.contains(&y) {
                        k = self.lcs_pos(y);
                        grow(&mut ymids, k);
                        break;
                    }
                    found.next();
                }
                for y in found {
                    if y > self.seq(k.wrapping_sub(1)) {
                        self.in_seq.remove(&self.seq(k));
                        put(&mut self.seq, k, y);
                        self.in_seq.insert(y);
                    } else if !self.in_seq.contains(&y) {
                        k = self.lcs_pos(y);
                        grow(&mut ymids, k);
                    }
                }
            }
            // `$ymids[$i][$chunk]` of the next chunk; the last is never read.
            for (i, mids) in ymids.iter_mut().enumerate().take(self.lcs + 1) {
                mids.resize(ix(chunk), 0);
                mids.push(get(&self.seq, i));
            }
        }
        let point = |x: i64, y: i64| if flip { (y, x) } else { (x, y) };
        let mut seps = vec![point(xoff, yoff)];
        let ymid = ymids.get(self.lcs).cloned().unwrap_or_default();
        for n in 0..nchunks - 1 {
            let x1 = xoff + (numer + (xlim - xoff) * n) / nchunks;
            seps.push(point(x1, get(&ymid, ix(n)) + 1));
        }
        seps.push(point(xlim, ylim));
        (self.lcs, seps)
    }

    /// `_lcsPos()`: where `ypos` goes in the increasing `seq`, a new
    /// longest subsequence past its end.
    fn lcs_pos(&mut self, ypos: i64) -> usize {
        let seq = self.seq.get(1..=self.lcs).unwrap_or_default();
        let end = seq.binary_search(&ypos).unwrap_or_else(|at| at) + 1;
        if end > self.lcs {
            self.lcs = end;
            self.seq.resize(end, 0);
            self.seq.push(ypos);
        } else {
            self.in_seq.remove(&self.seq(end));
            put(&mut self.seq, end, ypos);
        }
        self.in_seq.insert(ypos);
        end
    }

    /// `_compareseq()`: marks the lines outside the common subsequence.
    fn compareseq(&mut self, (mut xoff, mut xlim): (i64, i64), (mut yoff, mut ylim): (i64, i64)) {
        let same = |e: &Engine, x: i64, y: i64| loose_eq(get(&e.xv, ix(x)), get(&e.yv, ix(y)));
        while xoff < xlim && yoff < ylim && same(self, xoff, yoff) {
            xoff += 1;
            yoff += 1;
        }
        while xlim > xoff && ylim > yoff && same(self, xlim - 1, ylim - 1) {
            xlim -= 1;
            ylim -= 1;
        }
        // An empty side has no common subsequence, as `_diag()` finds.
        let nchunks = chunks((xoff, xlim), (yoff, ylim));
        let (lcs, seps) = self.diag((xoff, xlim), (yoff, ylim), nchunks);
        if lcs == 0 {
            for y in yoff..ylim {
                put(&mut self.ychanged, get(&self.yind, ix(y)), true);
            }
            for x in xoff..xlim {
                put(&mut self.xchanged, get(&self.xind, ix(x)), true);
            }
            return;
        }
        for (from, to) in seps.iter().zip(seps.iter().skip(1)) {
            self.compareseq((from.0, to.0), (from.1, to.1));
        }
    }
}

/// `_shiftBoundaries()`: slides each run of changes to where it groups
/// with others, as GNU diff does.
fn shift_boundaries(lines: &[&[u8]], changed: &mut [bool], other: &[bool]) {
    let (len, other_len) = (lines.len(), other.len() as i64);
    let other_at = |j: i64| usize::try_from(j).is_ok_and(|j| get(other, j));
    let same = |a: usize, b: usize| loose_eq(get(lines, a), get(lines, b));
    // `while ($other_changed[--$j])`, which stops before index 0 too.
    let back = |j: &mut i64| {
        *j -= 1;
        while other_at(*j) {
            *j -= 1;
        }
    };
    let (mut i, mut j) = (0, 0i64);
    loop {
        while other_at(j) {
            j += 1;
        }
        while changed.get(i) == Some(&false) {
            i += 1;
            j += 1;
            while other_at(j) {
                j += 1;
            }
        }
        if i == len {
            break;
        }
        // The run from `$start`, which is changed.
        let mut start = i;
        while get(changed, i) {
            i += 1;
        }
        let mut corresponding;
        loop {
            let runlength = i - start;
            while start > 0 && same(start - 1, i - 1) {
                start -= 1;
                put(changed, start, true);
                i -= 1;
                put(changed, i, false);
                while start > 0 && get(changed, start - 1) {
                    start -= 1;
                }
                back(&mut j);
            }
            corresponding = corresponding_end((j, other_len), i, len);
            while lines
                .get(i)
                .is_some_and(|&line| loose_eq(get(lines, start), line))
            {
                put(changed, start, false);
                start += 1;
                put(changed, i, true);
                while get(changed, i) {
                    i += 1;
                }
                j += 1;
                if other_at(j) {
                    corresponding = i;
                    while other_at(j) {
                        j += 1;
                    }
                }
            }
            if runlength == i - start {
                break;
            }
        }
        while corresponding < i {
            start -= 1;
            put(changed, start, true);
            i -= 1;
            put(changed, i, false);
            back(&mut j);
        }
    }
}

/// `Text_Diff_Renderer::render()`'s blocks with 10000 lines of context on
/// each side, for the renderer's `_block()`.
fn blocks(ops: Vec<Op<'_>>) -> Vec<Vec<Op<'_>>> {
    const CONTEXT: usize = 10_000;
    let mut done = Vec::new();
    let (mut open, mut context): (Option<Vec<Op>>, Vec<&[u8]>) = (None, Vec::new());
    let last = ops.len().saturating_sub(1);
    for (i, op) in ops.into_iter().enumerate() {
        match op {
            Op::Copy(lines) => {
                if let Some(mut edits) = open.take() {
                    let keep = if i == last { CONTEXT } else { 2 * CONTEXT };
                    if lines.len() <= keep {
                        edits.push(Op::Copy(lines.clone()));
                        open = Some(edits);
                    } else {
                        edits.push(Op::Copy(lines.iter().copied().take(CONTEXT).collect()));
                        done.push(edits);
                    }
                }
                context = lines;
            }
            op => {
                let edits = open.get_or_insert_with(|| {
                    let skipped = context.len().saturating_sub(CONTEXT);
                    let lead: Vec<&[u8]> = context.iter().copied().skip(skipped).collect();
                    if lead.is_empty() {
                        Vec::new()
                    } else {
                        vec![Op::Copy(lead)]
                    }
                });
                edits.push(op);
            }
        }
    }
    done.extend(open);
    done
}

/// `htmlspecialchars( $text )`, PHP 8's defaults: quotes encoded and
/// invalid UTF-8 replaced.
fn specialchars(text: &[u8]) -> String {
    let mut out = String::with_capacity(text.len());
    for c in String::from_utf8_lossy(text).chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#039;"),
            c => out.push(c),
        }
    }
    out
}

/// `\w` of a PCRE pattern with `/u` (PCRE2 10.44): letters, numbers,
/// non-spacing marks and connector punctuation.
fn is_word(c: char) -> bool {
    use icu_properties::props::GeneralCategory as G;
    let category = icu_properties::CodePointMapData::<G>::new().get(c);
    matches!(
        category,
        G::UppercaseLetter
            | G::LowercaseLetter
            | G::TitlecaseLetter
            | G::ModifierLetter
            | G::OtherLetter
            | G::DecimalNumber
            | G::LetterNumber
            | G::OtherNumber
            | G::NonspacingMark
            | G::ConnectorPunctuation
    )
}

/// `WP_Text_Diff_Renderer_inline::_splitOnWords()`: words and each other
/// character between them; `None` for invalid UTF-8, a fatal error in PHP.
fn split_words(line: &[u8]) -> Option<Vec<&[u8]>> {
    let text = std::str::from_utf8(line).ok()?;
    let (mut words, mut start) = (Vec::new(), 0);
    let part = |from: usize, to: usize| line.get(from..to).unwrap_or_default();
    for (at, c) in text.char_indices().filter(|(_, c)| !is_word(*c)) {
        let end = at + c.len_utf8();
        words.push(part(start, at));
        words.push(part(at, end));
        start = end;
    }
    words.push(part(start, line.len()));
    Some(words)
}

/// `Text_Diff_Renderer_inline` at the words level: `<del>` and `<ins>`
/// around what changed.
fn inline_words(edits: &[Op]) -> String {
    let encoded = |lines: &[&[u8]]| lines.iter().map(|l| specialchars(l)).collect::<String>();
    let wrapped = |lines: &[&[u8]], tag: &str| format!("<{tag}>{}</{tag}>", encoded(lines));
    let mut out = String::new();
    for edit in edits {
        match edit {
            Op::Copy(lines) => out.push_str(&encoded(lines)),
            Op::Add(lines) => out.push_str(&wrapped(lines, "ins")),
            Op::Delete(lines) => out.push_str(&wrapped(lines, "del")),
            Op::Change(orig, fin) => {
                let (mut orig, mut fin) = (orig.clone(), fin.clone());
                while let (Some(o), Some(f)) = (orig.first_mut(), fin.first_mut()) {
                    let (word_o, word_f): (&[u8], &[u8]) = (o, f);
                    let (Some(rest_o), Some(rest_f)) =
                        (word_o.strip_prefix(b" "), word_f.strip_prefix(b" "))
                    else {
                        break;
                    };
                    out.push(' ');
                    (*o, *f) = (rest_o, rest_f);
                }
                out.push_str(&wrapped(&orig, "del"));
                out.push_str(&wrapped(&fin, "ins"));
            }
        }
    }
    out
}

/// `WP_Text_Diff_Renderer_inline`'s diff of one line to another, which
/// differs: words diffed, `None` when PHP fails.
fn inline(orig: &[u8], fin: &[u8]) -> Option<String> {
    let (from, to) = (split_words(orig)?, split_words(fin)?);
    let words: String = blocks(diff(&from, &to))
        .iter()
        .map(|b| inline_words(b))
        .collect();
    Some(format!("{words}\n"))
}

/// `diff` without its `tag` spans.
fn without(diff: &str, tag: &str) -> String {
    let (open, close) = (format!("<{tag}>"), format!("</{tag}>"));
    let mut out = String::new();
    let mut rest = diff;
    // The inline renderer closes each span it opens.
    while let Some((head, tail)) = rest.split_once(&open) {
        out.push_str(head);
        rest = tail.split_once(&close).map_or("", |(_, after)| after);
    }
    out.push_str(rest);
    out
}

/// `compute_string_distance()`: how differently two lines use each byte,
/// per byte of the first.
fn distance(a: &[u8], b: &[u8]) -> f64 {
    // `count_chars()` of each, byte by byte.
    let count = |text: &[u8], byte: u8| text.iter().filter(|&&b| b == byte).count() as i64;
    let difference = (0..=u8::MAX)
        .map(|byte| (count(a, byte) - count(b, byte)).abs())
        .sum::<i64>() as f64;
    if a.is_empty() || a == b"0" {
        difference
    } else {
        difference / a.len() as f64
    }
}

/// `interleave_changed_lines()`: each line's match, by distance, and the
/// rows of each side, negative for a blank.
fn interleave(orig: &[&[u8]], fin: &[&[u8]]) -> (Vec<Option<usize>>, Vec<i64>, Vec<i64>) {
    let mut pairs = Vec::new();
    for (o, a) in orig.iter().enumerate() {
        for (f, b) in fin.iter().enumerate() {
            pairs.push((o, f, distance(a, b)));
        }
    }
    pairs.sort_by(|x, y| x.2.total_cmp(&y.2));
    // `Some(None)` for an `'x'`, a line left without a match.
    let mut orig_matches: Vec<Option<Option<usize>>> = vec![None; orig.len()];
    let mut final_matches: Vec<Option<Option<usize>>> = vec![None; fin.len()];
    for (o, f, _) in pairs {
        match (get_match(&orig_matches, o), get_match(&final_matches, f)) {
            (true, true) => {}
            (false, false) => {
                put(&mut orig_matches, o, Some(Some(f)));
                put(&mut final_matches, f, Some(Some(o)));
            }
            (true, false) => put(&mut final_matches, f, Some(None)),
            (false, true) => put(&mut orig_matches, o, Some(None)),
        }
    }
    let orig_matches: Vec<Option<usize>> = orig_matches.into_iter().flatten().collect();
    let mut orig_rows: Vec<i64> = (0..orig.len() as i64).collect();
    let mut final_rows: Vec<i64> = (0..fin.len() as i64).collect();
    for (row, matched) in orig_matches.iter().enumerate() {
        let row = row as i64;
        let final_pos = matched.and_then(|f| final_rows.iter().position(|&r| r == f as i64));
        let orig_pos = orig_rows.iter().position(|&r| r == row).unwrap_or_default();
        let blanks = |n: usize| (1..=n as i64).map(|n| -n).collect::<Vec<_>>();
        let Some(pos) = final_pos else {
            splice(&mut final_rows, orig_pos, &[-1]);
            continue;
        };
        match pos.cmp(&orig_pos) {
            std::cmp::Ordering::Less => splice(&mut final_rows, orig_pos, &up(orig_pos, pos)),
            std::cmp::Ordering::Greater => {
                splice(&mut orig_rows, orig_pos, &blanks(pos - orig_pos))
            }
            std::cmp::Ordering::Equal => {}
        }
    }
    let pad = |rows: &mut Vec<i64>, to: usize| {
        let missing = to.saturating_sub(rows.len()) as i64;
        rows.extend((1..=missing).rev().map(|n| -n));
    };
    let longest = orig_rows.len().max(final_rows.len());
    pad(&mut orig_rows, longest);
    pad(&mut final_rows, longest);
    (orig_matches, orig_rows, final_rows)
}

/// `isset( $matches[ $i ] )`.
fn get_match(matches: &[Option<Option<usize>>], i: usize) -> bool {
    matches.get(i).is_some_and(Option::is_some)
}

/// `array_splice( $rows, $at, 0, $items )`, past the end appending.
fn splice(rows: &mut Vec<i64>, at: usize, items: &[i64]) {
    let at = at.min(rows.len());
    rows.splice(at..at, items.iter().copied());
}

const EMPTY: &str = "<td>&nbsp;</td>";

fn added_line(line: &str) -> String {
    format!(
        "<td class='diff-addedline'><span aria-hidden='true' class='dashicons dashicons-plus'></span><span class='screen-reader-text'>Added: </span>{line}</td>"
    )
}

fn deleted_line(line: &str) -> String {
    format!(
        "<td class='diff-deletedline'><span aria-hidden='true' class='dashicons dashicons-minus'></span><span class='screen-reader-text'>Deleted: </span>{line}</td>"
    )
}

fn context_line(line: &str) -> String {
    format!(
        "<td class='diff-context'><span class='screen-reader-text'>Unchanged: </span>{line}</td>"
    )
}

/// `WP_Text_Diff_Renderer_Table::_changed()` in the split view: lines
/// paired by distance, with their words' differences where alike enough.
fn changed(orig: &[&[u8]], fin: &[&[u8]]) -> Option<String> {
    let (matches, orig_rows, final_rows) = interleave(orig, fin);
    let mut orig_diffs = HashMap::new();
    let mut final_diffs = HashMap::new();
    for (o, f) in matches.iter().enumerate() {
        let Some(f) = *f else {
            continue;
        };
        let (left, right) = (get(orig, o), get(fin, f));
        let diff = if left == right {
            String::new()
        } else {
            inline(left, right)?
        };
        // `!(<ins>.*?</ins>|<del>.*?</del>)!`'s matches: each span, which
        // the inline renderer closes; their text, joined by spaces.
        // Without one, the ratio is negative, as PHP skips it.
        let found = (diff.matches("<ins>").count() + diff.matches("<del>").count()) as i64;
        let stripped = ["<ins>", "</ins>", "<del>", "</del>"]
            .iter()
            .fold(diff.clone(), |text, tag| text.replace(tag, ""))
            .len() as i64;
        let outside = without(&without(&diff, "ins"), "del").len() as i64;
        let matched = stripped - outside + found - 1;
        // Never 0: the diff holds more than its spans.
        let whole = stripped * 2 - matched;
        if matched as f64 / whole as f64 > 0.6 {
            continue;
        }
        orig_diffs.insert(o as i64, without(&diff, "ins"));
        final_diffs.insert(f as i64, without(&diff, "del"));
    }
    let line = |diffs: &HashMap<i64, String>, lines: &[&[u8]], row: i64| {
        diffs.get(&row).cloned().unwrap_or_else(|| {
            let line = usize::try_from(row).ok().and_then(|row| lines.get(row));
            line.map_or_else(String::new, |line| specialchars(line))
        })
    };
    let mut out = String::new();
    for (&o, &f) in orig_rows.iter().zip(&final_rows) {
        if o < 0 && f < 0 {
            continue;
        }
        let (orig_line, final_line) = (line(&orig_diffs, orig, o), line(&final_diffs, fin, f));
        let row = if o < 0 {
            format!("{EMPTY}{}", added_line(&final_line))
        } else if f < 0 {
            format!("{}{EMPTY}", deleted_line(&orig_line))
        } else {
            format!("{}{}", deleted_line(&orig_line), added_line(&final_line))
        };
        out.push_str(&format!("<tr>{row}</tr>\n"));
    }
    Some(out)
}

/// `WP_Text_Diff_Renderer_Table`'s rows of a block, split view.
fn table_block(edits: &[Op]) -> Option<String> {
    let mut out = String::new();
    let rows = |lines: &[&[u8]], row: &dyn Fn(&str) -> String| {
        lines
            .iter()
            .map(|line| format!("<tr>{}</tr>\n", row(&specialchars(line))))
            .collect::<String>()
    };
    for edit in edits {
        out.push_str(&match edit {
            Op::Copy(lines) => rows(lines, &|l| format!("{0}{0}", context_line(l))),
            Op::Add(lines) => rows(lines, &|l| format!("{EMPTY}{}", added_line(l))),
            Op::Delete(lines) => rows(lines, &|l| format!("{}{EMPTY}", deleted_line(l))),
            Op::Change(orig, fin) => changed(orig, fin)?,
        });
    }
    Some(out)
}

/// `wp_text_diff( $left, $right, $args )` with the revisions screen's
/// arguments: the split view titled Removed and Added. Empty without a
/// difference; `None` where PHP stops with a fatal error.
pub fn wp_text_diff(left: &[u8], right: &[u8]) -> Option<String> {
    let (left, right) = (normalize_whitespace(left), normalize_whitespace(right));
    let from: Vec<&[u8]> = left.split(|&b| b == b'\n').collect();
    let to: Vec<&[u8]> = right.split(|&b| b == b'\n').collect();
    let blocks = blocks(diff(&from, &to));
    let rows: String = blocks
        .iter()
        .map(|b| table_block(b))
        .collect::<Option<_>>()?;
    if rows.is_empty() {
        return Some(String::new());
    }
    Some(format!(
        "<table class='diff is-split-view'>\n<thead><tr class='diff-sub-title'>\n\t<th>Removed</th>\n\t<th>Added</th>\n</tr>\n</thead>\n<tbody>\n{rows}\n</tbody>\n</table>"
    ))
}

#[cfg(test)]
mod tests;
