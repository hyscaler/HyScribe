// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP's `base64_encode()`, and `base64_decode()` in its default,
//! non-strict mode.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `base64_encode( $data )`, padded with `=`.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let bytes = [0, 1, 2].map(|i| u32::from(chunk.get(i).copied().unwrap_or_default()));
        let bits = (bytes[0] << 16) | (bytes[1] << 8) | bytes[2];
        for i in 0..4 {
            let sextet = usize::try_from((bits >> (18 - 6 * i)) & 63).unwrap_or_default();
            let digit = ALPHABET.get(sextet).filter(|_| i <= chunk.len());
            out.push(char::from(digit.copied().unwrap_or(b'=')));
        }
    }
    out
}

/// `base64_decode( $data )`: padding and characters outside the alphabet
/// are skipped, and bits short of a whole byte at the end are dropped.
pub fn base64_decode(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut bits: u32 = 0;
    let mut count = 0;
    for &byte in data {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        bits = ((bits << 6) + u32::from(value)) & 0xfff;
        count += 6;
        if count >= 8 {
            count -= 8;
            out.extend((bits >> count).to_le_bytes().first());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{base64_decode, base64_encode};

    /// As PHP encodes them.
    #[test]
    fn encodes_like_php() {
        for (data, encoded) in [
            (&b""[..], ""),
            (b"h", "aA=="),
            (b"he", "aGU="),
            (b"hello", "aGVsbG8="),
            (b"\xfb\xff\xbf", "+/+/"),
        ] {
            assert_eq!(base64_encode(data), encoded, "{data:?}");
            assert_eq!(base64_decode(encoded.as_bytes()), data);
        }
    }

    /// As PHP 8.4 decodes them.
    #[test]
    fn decodes_like_php() {
        for (data, decoded) in [
            (&b""[..], &b""[..]),
            (b"aGVsbG8=", b"hello"),
            (b"aGVsbG8", b"hello"),
            (b"aGV sbG8=", b"hello"),
            (b"aG=VsbG8", b"hello"),
            (b"a", b""),
            (b"ab", b"\x69"),
            (b"abc", b"\x69\xb7"),
            (b"!@#$%", b""),
            (b"YQ==YQ==", b"\x61\x06\x10"),
            (
                b"4pyTIMOgIGxhIG1vZGU=",
                "\u{2713} \u{e0} la mode".as_bytes(),
            ),
            (b"-_*.", b""),
            (b"QUJD\nREVG", b"ABCDEF"),
            (b"+/+/", b"\xfb\xff\xbf"),
            (b"a+b/c=", b"\x6b\xe6\xff"),
        ] {
            assert_eq!(base64_decode(data), decoded, "{data:?}");
        }
    }
}
