// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP's `base64_decode()`, in its default, non-strict mode.

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
    use super::base64_decode;

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
