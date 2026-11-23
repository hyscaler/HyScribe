// SPDX-License-Identifier: AGPL-3.0-or-later

//! `force_balance_tags()`: closes what is left open, drops closers with
//! nothing to close and closes known single tags, with its tag pattern
//! matched by hand.

use crate::blocks::is_pcre_space;
use crate::formatting::Next;
use crate::template::replace;

/// Known single-entity tags, closed with ` /`.
const SINGLE_TAGS: [&[u8]; 18] = [
    b"area",
    b"base",
    b"basefont",
    b"br",
    b"col",
    b"command",
    b"embed",
    b"frame",
    b"hr",
    b"img",
    b"input",
    b"isindex",
    b"link",
    b"meta",
    b"param",
    b"source",
    b"track",
    b"wbr",
];

/// Tags that may nest directly inside themselves.
const NESTABLE_TAGS: [&[u8]; 10] = [
    b"article",
    b"aside",
    b"blockquote",
    b"details",
    b"div",
    b"figure",
    b"object",
    b"q",
    b"section",
    b"span",
];

/// The open tags, with where each name is open, so the topmost open tag of
/// a name is found at once.
#[derive(Default)]
struct Stack {
    tags: Vec<Vec<u8>>,
    open: std::collections::HashMap<Vec<u8>, Vec<usize>>,
}

impl Stack {
    fn push(&mut self, name: &[u8]) {
        self.open
            .entry(name.to_vec())
            .or_default()
            .push(self.tags.len());
        self.tags.push(name.to_vec());
    }

    fn pop(&mut self) -> Vec<u8> {
        let name = self.tags.pop().unwrap_or_default();
        self.open.entry(name.clone()).or_default().pop();
        name
    }

    /// The topmost open `name`.
    fn find(&self, name: &[u8]) -> Option<usize> {
        self.open.get(name).and_then(|at| at.last()).copied()
    }
}

/// One match of the tag pattern.
struct Tag<'a> {
    end: usize,
    closer: bool,
    name: &'a [u8],
    /// Group 4, the whitespace before attributes, when they were matched.
    space: &'a [u8],
    /// Group 5, or group 3 (an empty tag's `/`).
    attributes: &'a [u8],
}

/// `force_balance_tags( $text )`.
pub fn force_balance_tags(text: &[u8]) -> Vec<u8> {
    let text = replace(text, b"< !--", b"<    !--");
    let text = less_than_digits(&text);
    let mut gt = Next::new(&text, b">");
    let mut stack = Stack::default();
    let mut queue: Vec<u8> = Vec::new();
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while let Some((start, tag)) = next_tag(&text, at, &mut gt) {
        out.append(&mut queue);
        let name = tag.name.to_ascii_lowercase();
        let single = SINGLE_TAGS.contains(&name.as_slice());
        let attributes = hyscribe_phpser::php_trim(tag.attributes);
        let self_closer = attributes.ends_with(b"/");
        let element = if tag.closer {
            // A closer for the top tag gives `</tag>` at once and one further down
            // queues its closers; the queue lands in the same place either way.
            if let Some(found) = stack.find(&name) {
                while stack.tags.len() > found {
                    queue.extend([&b"</"[..], &stack.pop(), b">"].concat());
                }
            }
            Vec::new()
        } else {
            let mut space = tag.space.to_vec();
            let mut attributes = attributes.to_vec();
            if self_closer {
                if !single {
                    let trimmed = hyscribe_phpser::php_trim(
                        attributes.get(..attributes.len() - 1).unwrap_or_default(),
                    );
                    attributes = [trimmed, b"></", &name].concat();
                }
            } else if single {
                space = b" ".to_vec();
                attributes.push(b'/');
            } else {
                if !NESTABLE_TAGS.contains(&name.as_slice()) && stack.tags.last() == Some(&name) {
                    stack.pop();
                    queue = [&b"</"[..], &name, b">"].concat();
                }
                stack.push(&name);
            }
            if self_closer && single {
                space = b" ".to_vec();
            }
            let element = [&b"<"[..], &name, &space, &attributes, b">"].concat();
            if queue.is_empty() {
                element
            } else {
                queue.extend(element);
                Vec::new()
            }
        };
        out.extend_from_slice(text.get(at..start).unwrap_or_default());
        out.extend(element);
        at = tag.end;
    }
    out.append(&mut queue);
    out.extend_from_slice(text.get(at..).unwrap_or_default());
    for open in stack.tags.iter().rev() {
        out.extend([&b"</"[..], open, b">"].concat());
    }
    replace(&replace(&out, b"< !--", b"<!--"), b"<    !--", b"< !--")
}

/// `#<([0-9]{1})#` replaced by `&lt;$1`.
fn less_than_digits(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    for (i, &byte) in text.iter().enumerate() {
        if byte == b'<' && text.get(i + 1).is_some_and(u8::is_ascii_digit) {
            out.extend_from_slice(b"&lt;");
        } else {
            out.push(byte);
        }
    }
    out
}

/// The first match of the tag pattern at or after `from`, with where it
/// starts.
fn next_tag<'a>(text: &'a [u8], from: usize, gt: &mut Next) -> Option<(usize, Tag<'a>)> {
    let mut at = from;
    while let Some(start) = text
        .get(at..)
        .unwrap_or_default()
        .iter()
        .position(|&b| b == b'<')
        .map(|p| at + p)
    {
        if let Some(tag) = tag_at(text, start, gt) {
            return Some((start, tag));
        }
        at = start + 1;
    }
    None
}

/// The tag pattern at `start`, a `<`.
fn tag_at<'a>(text: &'a [u8], start: usize, gt: &mut Next) -> Option<Tag<'a>> {
    let closer = text.get(start + 1) == Some(&b'/');
    let name_start = start + 1 + usize::from(closer);
    let run = |from: usize, class: fn(u8) -> bool| {
        from + text
            .get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|&&b| class(b))
            .count()
    };
    // A custom element name, `[a-z][a-z0-9._]*-[a-z0-9._-]+`, then the
    // traditional `[\w:]+`; each is only ever tried at its full length.
    let custom = text
        .get(name_start)
        .filter(|b| b.is_ascii_lowercase())
        .and_then(|_| {
            // The first letter and the dash are in the classes after them.
            let dash = run(name_start, |b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'.' || b == b'_'
            });
            let end = run(dash, |b| {
                b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
            });
            (text.get(dash) == Some(&b'-') && end > dash + 1).then_some(end)
        });
    let traditional = run(name_start, |b| {
        b.is_ascii_alphanumeric() || b == b'_' || b == b':'
    });
    let ends = custom
        .into_iter()
        .chain((traditional > name_start).then_some(traditional));
    for name_end in ends {
        let name = text.get(name_start..name_end).unwrap_or_default();
        // `\s*(/?)>`
        let after_space = run(name_end, is_pcre_space);
        let slash = usize::from(text.get(after_space) == Some(&b'/'));
        if text.get(after_space + slash) == Some(&b'>') {
            let attributes = text
                .get(after_space..after_space + slash)
                .unwrap_or_default();
            return Some(Tag {
                end: after_space + slash + 1,
                closer,
                name,
                space: b"",
                attributes,
            });
        }
        // `(\s+)([^>]*)>`
        if after_space > name_end
            && let Some(close) = gt.from(after_space)
        {
            return Some(Tag {
                end: close + 1,
                closer,
                name,
                space: text.get(name_end..after_space).unwrap_or_default(),
                attributes: text.get(after_space..close).unwrap_or_default(),
            });
        }
    }
    None
}

#[cfg(test)]
mod tests;
