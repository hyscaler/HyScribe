// SPDX-License-Identifier: AGPL-3.0-or-later

//! Text libmagic calls `text/plain`, or `text/csv` where its comma
//! counting does. libmagic has a type for source code, markup, mail and
//! configuration files; text with a marker of any of them is not decided
//! here.

use super::LIMIT;

/// The words a line may not start with, as libmagic has a type for text
/// that does (calendar entries, diffs, Python).
const KEYWORDS: &[&str] = &["begin", "end", "diff", "def"];

/// The headers that start a message or a patch libmagic names, with their
/// colon.
const HEADERS: &[&str] = &[
    "from",
    "to",
    "cc",
    "subject",
    "date",
    "received",
    "return-path",
    "message-id",
    "mime-version",
    "reply-to",
    "delivered-to",
    "index",
    "path",
    "newsgroups",
    "xref",
];

/// What a line may not start with: the markers of comments, markup,
/// headers, sections, scripts, diffs and rules.
const MARKERS: &[&str] = &[
    "#", "[", "{", "<", ";", "@", "%", "\\", "!", "$", "`", "|", "/", "=", "+", "--",
];

/// libmagic's `csv_parse()`: every one of the first ten lines holds as
/// many commas as the first, which holds one at least, outside quotes.
fn csv(bytes: &[u8]) -> bool {
    let (mut fields, mut total, mut lines) = (0usize, 0usize, 0usize);
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        at += 1;
        match byte {
            b'"' => {
                while let Some(&inner) = bytes.get(at) {
                    at += 1;
                    if inner != b'"' {
                        continue;
                    }
                    if bytes.get(at) == Some(&b'"') {
                        at += 1;
                        continue;
                    }
                    break;
                }
            }
            b',' => fields += 1,
            b'\n' => {
                lines += 1;
                if lines == 10 {
                    return total != 0 && total == fields;
                }
                if total == 0 {
                    if fields == 0 {
                        return false;
                    }
                    total = fields;
                } else if total != fields {
                    return false;
                }
                fields = 0;
            }
            _ => {}
        }
    }
    total != 0 && lines >= 2
}

/// Whether a line starts with one of the keywords as a whole word, or
/// with the name of a header and a colon.
fn keyword(line: &str) -> bool {
    let word = line
        .split(|c: char| !(c.is_ascii_alphabetic() || c == '-'))
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    let header = line
        .get(word.len()..)
        .is_some_and(|rest| rest.starts_with(':'));
    KEYWORDS.contains(&word.as_str()) || (header && HEADERS.contains(&word.as_str()))
}

/// Whether `text` holds no control character but the tab and the line
/// ends.
fn clean(text: &str) -> bool {
    text.chars()
        .all(|c| !c.is_control() || matches!(c, '\t' | '\n' | '\r'))
}

/// Whether libmagic sees `bytes` as text at all: UTF-8 that is [`clean`].
pub(super) fn is_text(bytes: &[u8]) -> bool {
    std::str::from_utf8(bytes).is_ok_and(clean)
}

/// Whether a line holds a tag, other than an address in angle brackets.
fn tagged(line: &str) -> bool {
    line.match_indices('<').any(|(at, _)| {
        let after = line.get(at + 1..).unwrap_or_default();
        let named = after.starts_with(|c: char| c.is_ascii_alphabetic() || "!?/".contains(c));
        named && !after.split('>').next().unwrap_or_default().contains('@')
    })
}

/// Whether a line is one libmagic may take for source, markup, a header
/// or a configuration file.
fn marked(line: &str) -> bool {
    let line = line.trim_start();
    MARKERS.iter().any(|marker| line.starts_with(marker))
        || tagged(line)
        || line.contains(['\\', '{', '}'])
        || keyword(line)
}

/// The types libmagic gives `bytes` as plain text; `None` for text this
/// does not decide, or a file that is not text.
pub(super) fn text(bytes: &[u8]) -> Option<Vec<&'static str>> {
    if bytes.len() > LIMIT {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok().filter(|text| clean(text))?;
    if text.trim().is_empty() || text.split(['\n', '\r']).any(marked) {
        return None;
    }
    Some(vec![if csv(bytes) { "text/csv" } else { "text/plain" }])
}
