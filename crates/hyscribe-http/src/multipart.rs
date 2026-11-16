// SPDX-License-Identifier: AGPL-3.0-or-later

//! `multipart/form-data` bodies as PHP's RFC 1867 reader takes them into
//! `$_POST` and `$_FILES`: the boundary of the `Content-Type`, each part's
//! `Content-Disposition` name and file name (its base name), and its
//! `Content-Type`. It reads untrusted input, so the fuzz target
//! `multipart_form` covers it; a body it cannot read is `None`.

use hyscribe_phpser::{Value, is_empty_string, parse_str, php_trim, rawurlencode};

/// `max_file_uploads`: files past this many are dropped.
const MAX_FILES: usize = 20;

/// A form's fields and files.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Form<'a> {
    /// The fields, by name, in order.
    pub fields: Vec<(Vec<u8>, &'a [u8])>,
    /// The files.
    pub files: Vec<File<'a>>,
}

/// A file of a form, as `$_FILES` describes it.
#[derive(Debug, PartialEq, Eq)]
pub struct File<'a> {
    /// The field's name.
    pub field: Vec<u8>,
    /// `name`: the file name sent, its base name.
    pub name: Vec<u8>,
    /// `type`: the type sent.
    pub kind: Vec<u8>,
    /// The file's bytes.
    pub bytes: &'a [u8],
}

impl Form<'_> {
    /// `$_POST`.
    pub fn post(&self) -> Value {
        let pairs: Vec<Vec<u8>> = self
            .fields
            .iter()
            .map(|(name, value)| [rawurlencode(name), b"=".to_vec(), rawurlencode(value)].concat())
            .collect();
        parse_str(&pairs.join(&b'&'))
    }

    /// The file of the field `field`, if one was sent.
    pub fn file(&self, field: &str) -> Option<&File<'_>> {
        self.files
            .iter()
            .find(|file| file.field == field.as_bytes())
    }
}

/// The boundary of a `multipart/form-data` `Content-Type`, quoted or not.
pub fn boundary(content_type: &[u8]) -> Option<&[u8]> {
    let kind = content_type.split(|&b| b == b';').next()?.trim_ascii();
    if !kind.eq_ignore_ascii_case(b"multipart/form-data") {
        return None;
    }
    let (_, value) = params(content_type)
        .into_iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(b"boundary"))?;
    (!value.is_empty() && value.len() <= 70).then_some(value)
}

/// What comes before and after the first `needle` in `haystack`, if there.
fn split<'a>(haystack: &'a [u8], needle: &[u8]) -> Option<(&'a [u8], &'a [u8])> {
    let at = haystack.windows(needle.len()).position(|w| w == needle)?;
    let (before, after) = haystack.split_at_checked(at)?;
    Some((before, after.get(needle.len()..)?))
}

/// The `name=value` parameters after a header value's first `;`, each
/// value quoted or not.
fn params(value: &[u8]) -> Vec<(&[u8], &[u8])> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some((_, after)) = split(rest, b";") {
        let Some((key, after)) = split(after, b"=") else {
            break;
        };
        let after = after.trim_ascii_start();
        let (value, next) = match after.strip_prefix(b"\"") {
            Some(quoted) => split(quoted, b"\"").unwrap_or((quoted, b"")),
            None => {
                let end = after.iter().position(|&b| b == b';').unwrap_or(after.len());
                let (value, next) = after.split_at_checked(end).unwrap_or((after, b""));
                (value.trim_ascii_end(), next)
            }
        };
        out.push((key.trim_ascii(), value));
        rest = next;
    }
    out
}

/// `php_ap_basename()`: the part after the last `/` or `\`.
fn basename(name: &[u8]) -> &[u8] {
    let mut parts = name.rsplit(|&b| b == b'/' || b == b'\\');
    parts.next().unwrap_or(name)
}

/// A part's field name, file name and type, and its body.
type Part<'a> = (Vec<u8>, Option<Vec<u8>>, Vec<u8>, &'a [u8]);

/// One part, its headers read.
fn part(raw: &[u8]) -> Option<Part<'_>> {
    let (head, body) = split(raw, b"\r\n\r\n")?;
    let (mut name, mut file, mut kind) = (None, None, Vec::new());
    for line in head.split(|&b| b == b'\n') {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        let Some((header, value)) = split(line, b":") else {
            continue;
        };
        let (header, value) = (header.trim_ascii(), value.trim_ascii());
        if header.eq_ignore_ascii_case(b"content-disposition") {
            for (key, value) in params(value) {
                if key.eq_ignore_ascii_case(b"name") {
                    name = Some(value.to_vec());
                } else if key.eq_ignore_ascii_case(b"filename") {
                    file = Some(basename(value).to_vec());
                }
            }
        } else if header.eq_ignore_ascii_case(b"content-type") {
            kind = value.to_vec();
        }
    }
    Some((name?, file, kind, body))
}

/// `get_filename_from_disposition()` of one `Content-Disposition` value:
/// its `filename` parameter, trimmed and unquoted, `None` for none or one
/// PHP takes as empty before or after unquoting.
pub fn disposition_filename(value: &[u8]) -> Option<Vec<u8>> {
    let (_, attributes) = split(php_trim(value), b";")?;
    // A later `filename` wins.
    let found = attributes
        .split(|&b| b == b';')
        .filter_map(|part| {
            let (key, value) = split(part, b"=")?;
            (php_trim(key) == b"filename").then(|| php_trim(value))
        })
        .next_back()?;
    let unquoted = found
        .strip_prefix(b"\"")
        .and_then(|inner| inner.strip_suffix(b"\""))
        .unwrap_or(found);
    // An unquoted value is the value itself, so one empty check covers both.
    (!is_empty_string(unquoted)).then(|| unquoted.to_vec())
}

/// The form a body with `boundary` sends.
pub fn read<'a>(body: &'a [u8], boundary: &[u8]) -> Option<Form<'a>> {
    let (_, mut rest) = split(body, &[b"--", boundary].concat())?;
    let separator = [b"\r\n--", boundary].concat();
    let mut form = Form::default();
    while !rest.starts_with(b"--") {
        let (raw, after) = split(rest.strip_prefix(b"\r\n")?, &separator)?;
        let (name, file, kind, bytes) = part(raw)?;
        match file {
            Some(name_sent) if form.files.len() < MAX_FILES => form.files.push(File {
                field: name,
                name: name_sent,
                kind,
                bytes,
            }),
            Some(_) => {}
            None => form.fields.push((name, bytes)),
        }
        rest = after;
    }
    Some(form)
}

#[cfg(test)]
mod tests;
