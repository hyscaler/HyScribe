// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_mail()`'s reading of a message's headers string, one header per
//! line, and the check that keeps a value from starting a header of its
//! own (ADR 0016).

use hyscribe_kses::str_replace;
use hyscribe_phpser::php_trim;

/// What `wp_mail()` takes from the headers; the transport uses only the
/// sender's name, `Reply-To` and the content type and charset.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Headers {
    /// The `From` header's name, its double quotes removed.
    pub from_name: Option<Vec<u8>>,
    /// The `From` header's address.
    pub from_email: Option<Vec<u8>>,
    /// `Content-Type`'s type.
    pub content_type: Option<Vec<u8>>,
    /// `Content-Type`'s charset.
    pub charset: Option<Vec<u8>>,
    /// The `Reply-To` addresses, each as written.
    pub reply_to: Vec<Vec<u8>>,
    /// The names of the headers dropped: `Cc`, `Bcc` and every other.
    pub dropped: Vec<Vec<u8>>,
}

/// Whether `value` holds a CR, LF or NUL, which would end a header line
/// and start another.
pub fn breaks_line(value: &[u8]) -> bool {
    value.iter().any(|b| matches!(b, b'\r' | b'\n' | 0))
}

/// The headers of `wp_mail()`'s `$headers` string, split on line ends as
/// it splits them; none when a line holds a stray CR or a NUL.
pub fn parse(headers: &[u8]) -> Option<Headers> {
    let mut parsed = Headers::default();
    let lines = str_replace(headers, b"\r\n", b"\n");
    for line in lines.split(|&b| b == b'\n') {
        if breaks_line(line) {
            return None;
        }
        let mut parts = php_trim(line).splitn(2, |&b| b == b':');
        let name = php_trim(parts.next().unwrap_or_default());
        // A line without a colon names only a multipart boundary.
        let Some(content) = parts.next() else {
            continue;
        };
        let content = php_trim(content);
        match name.to_ascii_lowercase().as_slice() {
            b"from" => from(&mut parsed, content),
            b"content-type" => content_type(&mut parsed, content),
            b"reply-to" => parsed
                .reply_to
                .extend(content.split(|&b| b == b',').map(<[u8]>::to_vec)),
            _ => parsed.dropped.push(name.to_vec()),
        }
    }
    Some(parsed)
}

/// The `From` header: `Name <address>`, or an address alone.
fn from(parsed: &mut Headers, content: &[u8]) {
    match content.iter().position(|&b| b == b'<') {
        Some(bracket) => {
            let (name, address) = content.split_at(bracket);
            if bracket > 0 {
                let name: Vec<u8> = name.iter().copied().filter(|&b| b != b'"').collect();
                parsed.from_name = Some(php_trim(&name).to_vec());
            }
            let address: Vec<u8> = address
                .iter()
                .skip(1)
                .copied()
                .filter(|&b| b != b'>')
                .collect();
            parsed.from_email = Some(php_trim(&address).to_vec());
        }
        None if !content.is_empty() => parsed.from_email = Some(content.to_vec()),
        None => {}
    }
}

/// The `Content-Type` header: a type, and a charset after a `;`.
fn content_type(parsed: &mut Headers, content: &[u8]) {
    let mut parts = content.split(|&b| b == b';');
    match (parts.next(), parts.next()) {
        (Some(kind), Some(rest)) => {
            parsed.content_type = Some(php_trim(kind).to_vec());
            if rest
                .to_ascii_lowercase()
                .windows(8)
                .any(|w| w == b"charset=")
            {
                let charset = str_replace(&str_replace(rest, b"charset=", b""), b"\"", b"");
                parsed.charset = Some(php_trim(&charset).to_vec());
            }
        }
        _ if !content.is_empty() => parsed.content_type = Some(content.to_vec()),
        _ => {}
    }
}

#[cfg(test)]
mod tests;
