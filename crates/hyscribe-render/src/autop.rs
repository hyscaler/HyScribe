// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wpautop()`: paragraphs and line breaks from double and single line
//! breaks, ported regex by regex with PCRE's semantics. Searches that look
//! ahead resume where the last one ended and a failed `\s*` match skips the
//! rest of its run, so the work stays linear.

use crate::blocks::is_pcre_space;
use crate::formatting::{Next, find};
use crate::template::replace;

/// `$allblocks`, in its alternation order.
const ALLBLOCKS: [&[u8]; 46] = [
    b"table",
    b"thead",
    b"tfoot",
    b"caption",
    b"col",
    b"colgroup",
    b"tbody",
    b"tr",
    b"td",
    b"th",
    b"div",
    b"dl",
    b"dd",
    b"dt",
    b"ul",
    b"ol",
    b"li",
    b"pre",
    b"form",
    b"map",
    b"area",
    b"blockquote",
    b"address",
    b"style",
    b"p",
    b"h1",
    b"h2",
    b"h3",
    b"h4",
    b"h5",
    b"h6",
    b"hr",
    b"fieldset",
    b"legend",
    b"section",
    b"article",
    b"aside",
    b"hgroup",
    b"header",
    b"footer",
    b"nav",
    b"figure",
    b"figcaption",
    b"details",
    b"menu",
    b"summary",
];

/// The block tags a `<br />` before them is dropped for.
const BR_BLOCKS: [&[u8]; 11] = [
    b"p", b"li", b"div", b"dl", b"dd", b"dt", b"th", b"pre", b"td", b"ul", b"ol",
];

/// The `<pre>` placeholders and the elements they stand for.
type PreTags = Vec<(Vec<u8>, Vec<u8>)>;

/// A match: where it ends and what replaces it.
type Found = Option<(usize, Vec<u8>)>;

/// `wpautop( $text, $br )`.
pub fn wpautop(text: &[u8], br: bool) -> Vec<u8> {
    if hyscribe_phpser::php_trim(text).is_empty() {
        return Vec::new();
    }
    let (mut text, pre_tags) = hide_pre(&[text, b"\n"].concat());
    text = substitute(&text, |t, i| {
        let end = br_tag(t, i)?;
        let end = br_tag(t, skip_space(t, end))?;
        Some((end, b"\n\n".to_vec()))
    });
    // `<$allblocks[\s/>]`: a double line break above.
    text = substitute(&text, |t, i| {
        let end = (t.get(i) == Some(&b'<')).then(|| block_name(t, i + 1))??;
        Some((
            end + 1,
            [&b"\n\n"[..], t.get(i..=end).unwrap_or_default()].concat(),
        ))
    });
    // `</$allblocks>`: a double line break below.
    text = substitute(&text, |t, i| {
        let end = (t.get(i..i + 2) == Some(b"</")).then(|| block_name(t, i + 2))??;
        (t.get(end) == Some(&b'>')).then(|| {
            (
                end + 1,
                [t.get(i..=end).unwrap_or_default(), b"\n\n"].concat(),
            )
        })
    });
    // `<hr\s*?/?>`
    text = substitute(&text, |t, i| {
        let after = (t.get(i..i + 3) == Some(b"<hr")).then(|| skip_space(t, i + 3))?;
        let end = after + usize::from(t.get(after) == Some(&b'/'));
        (t.get(end) == Some(&b'>')).then(|| {
            (
                end + 1,
                [t.get(i..=end).unwrap_or_default(), b"\n\n"].concat(),
            )
        })
    });
    text = replace(&replace(&text, b"\r\n", b"\n"), b"\r", b"\n");
    text = replace_in_html_tags(&text, b"\n", b" <!-- wpnl --> ");
    if find(&text, 0, b"<option").is_some() {
        text = strip_space_before(&text, b"<option");
        text = strip_space_after(&text, b"</option>");
    }
    if find(&text, 0, b"</object>").is_some() {
        text = {
            let mut gt = Next::new(&text, b">");
            substitute(&text, |t, i| {
                let end = tag_end(t, i, b"<object", &mut gt)?;
                Some((
                    skip_space(t, end),
                    t.get(i..end).unwrap_or_default().to_vec(),
                ))
            })
        };
        text = strip_space_before(&text, b"</object>");
        text = {
            let mut gt = Next::new(&text, b">");
            substitute(
                &text,
                leading_space(|t, _, start| {
                    let names: [&[u8]; 4] = [b"<param", b"</param", b"<embed", b"</embed"];
                    let name = names
                        .into_iter()
                        .find(|name| t.get(start..).is_some_and(|r| r.starts_with(name)))?;
                    let end = gt.from(start + name.len()).map_or(t.len(), |at| at + 1);
                    Some((
                        skip_space(t, end),
                        t.get(start..end).unwrap_or_default().to_vec(),
                    ))
                }),
            )
        };
    }
    if find(&text, 0, b"<source").is_some() || find(&text, 0, b"<track").is_some() {
        text = media_openers(&text);
        text = substitute(
            &text,
            leading_space(|t, _, start| {
                let tail = t.get(start..).unwrap_or_default();
                let matched = matches!(tail.first(), Some(b'<' | b'['))
                    && tail.get(1) == Some(&b'/')
                    && matches!(tail.get(2..7), Some(b"audio" | b"video"))
                    && matches!(tail.get(7), Some(b'>' | b']'));
                matched.then(|| (start + 8, tail.get(..8).unwrap_or_default().to_vec()))
            }),
        );
        text = {
            let mut gt = Next::new(&text, b">");
            substitute(
                &text,
                leading_space(|t, _, start| {
                    let name: &[u8] = if t.get(start..).is_some_and(|r| r.starts_with(b"<source")) {
                        b"<source"
                    } else {
                        b"<track"
                    };
                    let end = tag_end(t, start, name, &mut gt)?;
                    Some((
                        skip_space(t, end),
                        t.get(start..end).unwrap_or_default().to_vec(),
                    ))
                }),
            )
        };
    }
    if find(&text, 0, b"<figcaption").is_some() {
        text = {
            let mut gt = Next::new(&text, b">");
            substitute(
                &text,
                leading_space(|t, _, start| {
                    let end = tag_end(t, start, b"<figcaption", &mut gt)?;
                    Some((end, t.get(start..end).unwrap_or_default().to_vec()))
                }),
            )
        };
        text = strip_space_after(&text, b"</figcaption>");
    }
    // `\n\n+`
    text = substitute(&text, |t, i| {
        let run = t
            .get(i..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&b| b == b'\n')
            .count();
        (run >= 2).then(|| (i + run, b"\n\n".to_vec()))
    });
    let mut wrapped = Vec::with_capacity(text.len());
    for paragraph in paragraphs(&text) {
        let start = paragraph.iter().take_while(|&&b| b == b'\n').count();
        let end = paragraph
            .iter()
            .rposition(|&b| b != b'\n')
            .map_or(start, |p| p + 1);
        wrapped.extend_from_slice(b"<p>");
        wrapped.extend_from_slice(paragraph.get(start..end).unwrap_or_default());
        wrapped.extend_from_slice(b"</p>\n");
    }
    text = wrapped;
    // `<p>\s*</p>`
    text = substitute(&text, |t, i| {
        let after = (t.get(i..i + 3) == Some(b"<p>")).then(|| skip_space(t, i + 3))?;
        (t.get(after..after + 4) == Some(b"</p>")).then(|| (after + 4, Vec::new()))
    });
    text = unclosed_p(&text);
    // `<p>\s*(</?$allblocks[^>]*>)\s*</p>`
    text = {
        let mut gt = Next::new(&text, b">");
        substitute(&text, |t, i| {
            let start = (t.get(i..i + 3) == Some(b"<p>")).then(|| skip_space(t, i + 3))?;
            let end = block_tag(t, start, &mut gt)?;
            let after = skip_space(t, end);
            (t.get(after..after + 4) == Some(b"</p>"))
                .then(|| (after + 4, t.get(start..end).unwrap_or_default().to_vec()))
        })
    };
    text = li_in_p(&text);
    text = blockquote_in_p(&text);
    text = replace(&text, b"</blockquote></p>", b"</p></blockquote>");
    // `<p>\s*(</?$allblocks[^>]*>)`
    text = {
        let mut gt = Next::new(&text, b">");
        substitute(&text, |t, i| {
            let start = (t.get(i..i + 3) == Some(b"<p>")).then(|| skip_space(t, i + 3))?;
            let end = block_tag(t, start, &mut gt)?;
            Some((end, t.get(start..end).unwrap_or_default().to_vec()))
        })
    };
    // `(</?$allblocks[^>]*>)\s*</p>`
    text = before_space(&text, b"</p>");
    if br {
        text = preserve_newlines(&text);
        text = replace(&replace(&text, b"<br>", b"<br />"), b"<br/>", b"<br />");
        text = line_breaks(&text);
        text = replace(&text, b"<WPPreserveNewline />", b"\n");
    }
    // `(</?$allblocks[^>]*>)\s*<br />`
    text = before_space(&text, b"<br />");
    text = br_before_blocks(&text);
    // `\n</p>$`, where `$` also matches before a final line feed.
    for tail in [&b"\n</p>"[..], b"\n</p>\n"] {
        if text.ends_with(tail) {
            let at = text.len() - tail.len();
            text = [
                text.get(..at).unwrap_or_default(),
                b"</p>",
                tail.get(5..).unwrap_or_default(),
            ]
            .concat();
            break;
        }
    }
    if !pre_tags.is_empty() {
        text = restore_pre(&text, &pre_tags);
    }
    if find(&text, 0, b"<!-- wpnl -->").is_some() {
        text = replace(
            &replace(&text, b" <!-- wpnl --> ", b"\n"),
            b"<!-- wpnl -->",
            b"\n",
        );
    }
    text
}

/// `<pre>` elements swapped for numbered placeholders, and the elements.
fn hide_pre(text: &[u8]) -> (Vec<u8>, PreTags) {
    let mut pre_tags = Vec::new();
    if find(text, 0, b"<pre").is_none() {
        return (text.to_vec(), pre_tags);
    }
    let mut parts = split(text, b"</pre>");
    let last = parts.pop().unwrap_or_default();
    let mut rebuilt = Vec::with_capacity(text.len());
    for part in parts {
        match find(&part, 0, b"<pre") {
            None => rebuilt.extend(part),
            Some(start) => {
                let name = format!("<pre wp-pre-tag-{}></pre>", pre_tags.len()).into_bytes();
                let tag = [part.get(start..).unwrap_or_default(), b"</pre>"].concat();
                rebuilt.extend_from_slice(part.get(..start).unwrap_or_default());
                rebuilt.extend_from_slice(&name);
                pre_tags.push((name, tag));
            }
        }
    }
    rebuilt.extend(last);
    (rebuilt, pre_tags)
}

/// `str_replace( array_keys( $pre_tags ), array_values( $pre_tags ), $text )`
/// in one pass. The sequential replacement puts element `i` where its
/// placeholder is, and later rounds replace only placeholders numbered past
/// `i` inside it; a placeholder cannot span an element, which starts with
/// `<pre` and ends with its only `</pre>`.
fn restore_pre(text: &[u8], pre_tags: &PreTags) -> Vec<u8> {
    const PREFIX: &[u8] = b"<pre wp-pre-tag-";
    let mut out = Vec::with_capacity(text.len());
    // What is left to copy, and the first placeholder number it may hold.
    let mut stack: Vec<(&[u8], usize)> = vec![(text, 0)];
    while let Some((rest, first)) = stack.pop() {
        let Some(at) = find(rest, 0, PREFIX) else {
            out.extend_from_slice(rest);
            continue;
        };
        let digits = rest.get(at + PREFIX.len()..).unwrap_or_default();
        let len = digits.iter().take_while(|b| b.is_ascii_digit()).count();
        let number = std::str::from_utf8(digits.get(..len).unwrap_or_default())
            .ok()
            .and_then(|n| n.parse::<usize>().ok())
            .filter(|n| n.to_string().len() == len && *n >= first);
        let placeholder = number.and_then(|n| pre_tags.get(n).map(|tag| (n, tag)));
        match placeholder
            .filter(|(_, (name, _))| rest.get(at..).is_some_and(|r| r.starts_with(name)))
        {
            Some((n, (name, tag))) => {
                out.extend_from_slice(rest.get(..at).unwrap_or_default());
                stack.push((rest.get(at + name.len()..).unwrap_or_default(), first));
                stack.push((tag, n + 1));
            }
            None => {
                out.extend_from_slice(rest.get(..=at).unwrap_or_default());
                stack.push((rest.get(at + 1..).unwrap_or_default(), first));
            }
        }
    }
    out
}

/// `preg_replace()` with a pattern matched by hand: `pattern( $text, $i )`
/// is the end of a match starting at `i` and its replacement.
fn substitute(text: &[u8], mut pattern: impl FnMut(&[u8], usize) -> Found) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while let Some(&byte) = text.get(at) {
        match pattern(text, at) {
            Some((end, with)) => {
                out.extend(with);
                at = end;
            }
            None => {
                out.push(byte);
                at += 1;
            }
        }
    }
    out
}

/// A pattern that starts with `\s*`: `pattern( $text, $i, $start )` is
/// tried with `start` past the run. Where it fails, it fails for the rest of
/// the run too, which is skipped.
fn leading_space(
    mut pattern: impl FnMut(&[u8], usize, usize) -> Found,
) -> impl FnMut(&[u8], usize) -> Found {
    let mut failed_until = 0;
    move |t, i| {
        if i < failed_until {
            return None;
        }
        let start = skip_space(t, i);
        let found = pattern(t, i, start);
        if found.is_none() {
            failed_until = start;
        }
        found
    }
}

/// The end of the run of PCRE `\s` at `i`.
fn skip_space(text: &[u8], i: usize) -> usize {
    i + text
        .get(i..)
        .unwrap_or_default()
        .iter()
        .take_while(|&&b| is_pcre_space(b))
        .count()
}

/// `<br\s*/?>` at `i`: its end.
fn br_tag(text: &[u8], i: usize) -> Option<usize> {
    let after = (text.get(i..i + 3) == Some(b"<br")).then(|| skip_space(text, i + 3))?;
    let end = after + usize::from(text.get(after) == Some(&b'/'));
    (text.get(end) == Some(&b'>')).then_some(end + 1)
}

/// `$allblocks[\s/>]` at `i`: the end of the first alternative that a
/// space, `/` or `>` follows.
fn block_name(text: &[u8], i: usize) -> Option<usize> {
    let rest = text.get(i..).unwrap_or_default();
    ALLBLOCKS
        .iter()
        .filter(|name| rest.starts_with(name))
        .map(|name| i + name.len())
        .find(|&end| {
            text.get(end)
                .is_some_and(|&b| is_pcre_space(b) || b == b'/' || b == b'>')
        })
}

/// `</?$allblocks[^>]*>` at `i`: its end. Any alternative will do as a
/// prefix, since `[^>]*` takes what follows it.
fn block_tag(text: &[u8], i: usize, gt: &mut Next) -> Option<usize> {
    let name = (text.get(i) == Some(&b'<'))
        .then(|| i + 1 + usize::from(text.get(i + 1) == Some(&b'/')))?;
    let rest = text.get(name..).unwrap_or_default();
    if !ALLBLOCKS.iter().any(|block| rest.starts_with(block)) {
        return None;
    }
    Some(gt.from(name).map_or(text.len(), |at| at + 1))
}

/// `<name[^>]*>` at `i`: its end, once a line feed placeholder has given
/// every tag a `>` after it.
fn tag_end(text: &[u8], i: usize, name: &[u8], gt: &mut Next) -> Option<usize> {
    text.get(i..).filter(|rest| rest.starts_with(name))?;
    Some(gt.from(i + name.len()).map_or(text.len(), |at| at + 1))
}

/// `\s*needle` replaced by `needle`.
fn strip_space_before(text: &[u8], needle: &[u8]) -> Vec<u8> {
    substitute(
        text,
        leading_space(|t, _, start| {
            t.get(start..)
                .filter(|rest| rest.starts_with(needle))
                .map(|_| (start + needle.len(), needle.to_vec()))
        }),
    )
}

/// `needle\s*` replaced by `needle`.
fn strip_space_after(text: &[u8], needle: &[u8]) -> Vec<u8> {
    substitute(text, |t, i| {
        t.get(i..)
            .filter(|rest| rest.starts_with(needle))
            .map(|_| (skip_space(t, i + needle.len()), needle.to_vec()))
    })
}

/// `(</?$allblocks[^>]*>)\s*after` replaced by the tag.
fn before_space(text: &[u8], after: &[u8]) -> Vec<u8> {
    let mut gt = Next::new(text, b">");
    substitute(text, |t, i| {
        let end = block_tag(t, i, &mut gt)?;
        let next = skip_space(t, end);
        t.get(next..)
            .filter(|rest| rest.starts_with(after))
            .map(|_| {
                (
                    next + after.len(),
                    t.get(i..end).unwrap_or_default().to_vec(),
                )
            })
    })
}

/// `([<\[](?:audio|video)[^>\]]*[>\]])\s*` replaced by its group.
fn media_openers(text: &[u8]) -> Vec<u8> {
    // Each match ends at the closer it found, so only a failed search, which
    // means there is no closer at all, is worth remembering.
    let mut no_closer = false;
    substitute(text, |t, i| {
        let named = matches!(t.get(i), Some(b'<' | b'['))
            && matches!(t.get(i + 1..i + 6), Some(b"audio" | b"video"));
        if !named || no_closer {
            return None;
        }
        let rest = t.get(i + 6..).unwrap_or_default();
        let Some(close) = rest.iter().position(|&b| b == b'>' || b == b']') else {
            no_closer = true;
            return None;
        };
        let close = i + 6 + close;
        Some((
            skip_space(t, close + 1),
            t.get(i..=close).unwrap_or_default().to_vec(),
        ))
    })
}

/// `<br />(\s*</?(?:p|li|div|dl|dd|dt|th|pre|td|ul|ol)[^>]*>)` replaced by
/// its group.
fn br_before_blocks(text: &[u8]) -> Vec<u8> {
    let mut gt = Next::new(text, b">");
    substitute(text, |t, i| {
        let start = (t.get(i..i + 6) == Some(b"<br />")).then(|| skip_space(t, i + 6))?;
        let name = (t.get(start) == Some(&b'<'))
            .then(|| start + 1 + usize::from(t.get(start + 1) == Some(&b'/')))?;
        let rest = t.get(name..).unwrap_or_default();
        if !BR_BLOCKS.iter().any(|block| rest.starts_with(block)) {
            return None;
        }
        let close = gt.from(name).unwrap_or(t.len());
        Some((close + 1, t.get(i + 6..=close).unwrap_or_default().to_vec()))
    })
}

/// `<p>([^<]+)</(div|address|form)>` replaced by `<p>$1</p></$2>`.
fn unclosed_p(text: &[u8]) -> Vec<u8> {
    substitute(text, |t, i| {
        let body = t.get(i + 3..).filter(|_| t.get(i..i + 3) == Some(b"<p>"))?;
        let run = body.iter().take_while(|&&b| b != b'<').count();
        let close = body.get(run..).filter(|_| run > 0)?.strip_prefix(b"</")?;
        let names: [&[u8]; 3] = [b"div", b"address", b"form"];
        let name = names
            .into_iter()
            .find(|name| close.starts_with(name) && close.get(name.len()) == Some(&b'>'))?;
        let replaced = [
            &b"<p>"[..],
            body.get(..run).unwrap_or_default(),
            b"</p></",
            name,
            b">",
        ]
        .concat();
        Some((i + 3 + run + 3 + name.len(), replaced))
    })
}

/// `explode()`.
fn split(text: &[u8], separator: &[u8]) -> Vec<Vec<u8>> {
    let mut parts = Vec::new();
    let mut from = 0;
    while let Some(at) = find(text, from, separator) {
        parts.push(text.get(from..at).unwrap_or_default().to_vec());
        from = at + separator.len();
    }
    parts.push(text.get(from..).unwrap_or_default().to_vec());
    parts
}

/// `preg_split( '/\n\s*\n/', $text, -1, PREG_SPLIT_NO_EMPTY )`: a separator
/// runs from a line feed to the last line feed of the whitespace after it.
fn paragraphs(text: &[u8]) -> Vec<&[u8]> {
    let mut parts = Vec::new();
    let mut from = 0;
    let mut at = 0;
    while let Some(&byte) = text.get(at) {
        let last = (byte == b'\n')
            .then(|| text.get(at + 1..skip_space(text, at)))
            .flatten()
            .and_then(|run| run.iter().rposition(|&b| b == b'\n'));
        match last {
            Some(last) => {
                parts.push(text.get(from..at).unwrap_or_default());
                at += last + 2;
                from = at;
            }
            None => at += 1,
        }
    }
    parts.push(text.get(from..).unwrap_or_default());
    parts.into_iter().filter(|part| !part.is_empty()).collect()
}

/// `wp_html_split( $text )`: the end of each element, the text between
/// them left out, as `(start, end)` pairs of every element, which runs from
/// its `<` to the `>` that closes it, or the end of the text.
pub(crate) fn html_elements(text: &[u8]) -> Vec<(usize, usize)> {
    let mut elements = Vec::new();
    let mut comments = Next::new(text, b"-->");
    let mut cdata = Next::new(text, b"]]>");
    let mut gt = Next::new(text, b">");
    let mut at = 0;
    while let Some(start) = find(text, at, b"<") {
        let rest = text.get(start..).unwrap_or_default();
        // The last byte of the element.
        let last = if rest.starts_with(b"<!--") {
            comments.from(start + 2).map(|p| p + 2)
        } else if rest.starts_with(b"<![CDATA[") {
            cdata.from(start + 9).map(|p| p + 2)
        } else {
            gt.from(start)
        };
        let last = last.unwrap_or(text.len() - 1);
        at = last + 1;
        elements.push((start, at));
    }
    elements
}

/// `wp_replace_in_html_tags( $text, array( $needle => $with ) )`: the
/// replacement inside the elements `wp_html_split()` finds.
pub(crate) fn replace_in_html_tags(text: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    for (start, end) in html_elements(text) {
        out.extend_from_slice(text.get(at..start).unwrap_or_default());
        out.extend(replace(
            text.get(start..end).unwrap_or_default(),
            needle,
            with,
        ));
        at = end;
    }
    out.extend_from_slice(text.get(at..).unwrap_or_default());
    out
}

/// `<p>(<li.+?)</p>` replaced by its group: `.` stops at a line feed.
fn li_in_p(text: &[u8]) -> Vec<u8> {
    let mut closers = Next::new(text, b"</p>");
    let mut newlines = Next::new(text, b"\n");
    substitute(text, |t, i| {
        if t.get(i..i + 6) != Some(b"<p><li") {
            return None;
        }
        let line_end = newlines.from(i + 6).unwrap_or(t.len());
        let close = closers.from(i + 7).filter(|&close| close + 4 <= line_end)?;
        Some((close + 4, t.get(i + 3..close).unwrap_or_default().to_vec()))
    })
}

/// `<p><blockquote((?:[^>"']|"[^"]*"|'[^']*')*)>`, case-insensitively,
/// replaced by `<blockquote$1><p>`. Where attributes starting at each byte
/// end is worked out backwards once, so every start is answered at once.
fn blockquote_in_p(text: &[u8]) -> Vec<u8> {
    const OPENER: &[u8] = b"<p><blockquote";
    if !text
        .windows(OPENER.len())
        .any(|w| w.eq_ignore_ascii_case(OPENER))
    {
        return text.to_vec();
    }
    // Built from the end: `reversed[k]` is for byte `len - 1 - k`.
    let len = text.len();
    let mut reversed: Vec<Option<usize>> = Vec::with_capacity(len);
    let (mut next_double, mut next_single) = (None, None);
    for (i, &byte) in text.iter().enumerate().rev() {
        let at = |reversed: &[Option<usize>], index: usize| {
            len.checked_sub(index + 1)
                .and_then(|k| reversed.get(k).copied().flatten())
        };
        let end = match byte {
            b'>' => Some(i),
            b'"' => next_double.and_then(|close: usize| at(&reversed, close + 1)),
            b'\'' => next_single.and_then(|close: usize| at(&reversed, close + 1)),
            _ => at(&reversed, i + 1),
        };
        reversed.push(end);
        match byte {
            b'"' => next_double = Some(i),
            b'\'' => next_single = Some(i),
            _ => {}
        }
    }
    reversed.reverse();
    let ends = reversed;
    substitute(text, |t, i| {
        if !t
            .get(i..i + OPENER.len())
            .is_some_and(|w| w.eq_ignore_ascii_case(OPENER))
        {
            return None;
        }
        let attributes = i + OPENER.len();
        let end = ends.get(attributes).copied().flatten()?;
        Some((
            end + 1,
            [
                &b"<blockquote"[..],
                t.get(attributes..end).unwrap_or_default(),
                b"><p>",
            ]
            .concat(),
        ))
    })
}

/// `<(script|style|svg|math).*?</\1>` with `/s`: their line feeds kept as
/// `<WPPreserveNewline />`.
fn preserve_newlines(text: &[u8]) -> Vec<u8> {
    let names: [(&[u8], &[u8]); 4] = [
        (b"script", b"</script>"),
        (b"style", b"</style>"),
        (b"svg", b"</svg>"),
        (b"math", b"</math>"),
    ];
    let mut closers: Vec<Next> = names
        .iter()
        .map(|(_, closer)| Next::new(text, closer))
        .collect();
    substitute(text, |t, i| {
        let rest = t.get(i + 1..).filter(|_| t.get(i) == Some(&b'<'))?;
        let ((_, closer), next) = names
            .iter()
            .zip(closers.iter_mut())
            .find(|((name, _), _)| rest.starts_with(name))?;
        // The closer cannot start inside the opening name.
        let end = next.from(i)? + closer.len();
        Some((
            end,
            replace(
                t.get(i..end).unwrap_or_default(),
                b"\n",
                b"<WPPreserveNewline />",
            ),
        ))
    })
}

/// `(?<!<br />)\s*\n` replaced by `<br />\n`: in each whitespace run, a
/// match runs from its first byte not right after a `<br />` to its last
/// line feed.
fn line_breaks(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while let Some(&byte) = text.get(at) {
        if !is_pcre_space(byte) {
            out.push(byte);
            at += 1;
            continue;
        }
        let end = skip_space(text, at);
        let after_br = text
            .get(..at)
            .is_some_and(|before| before.ends_with(b"<br />"));
        let start = at + usize::from(after_br);
        let run = text.get(at..end).unwrap_or_default();
        match run
            .iter()
            .rposition(|&b| b == b'\n')
            .map(|p| at + p)
            .filter(|&last| last >= start)
        {
            Some(last) => {
                out.extend_from_slice(text.get(at..start).unwrap_or_default());
                out.extend_from_slice(b"<br />\n");
                out.extend_from_slice(text.get(last + 1..end).unwrap_or_default());
            }
            None => out.extend_from_slice(run),
        }
        at = end;
    }
    out
}

#[cfg(test)]
mod tests;
