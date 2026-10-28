// SPDX-License-Identifier: AGPL-3.0-or-later

//! PHP's `md5()` (RFC 1321), for the identifiers WordPress derives from
//! content, such as `wp_unique_id_from_values()`. Not for security.

const SHIFTS: [u32; 64] = [
    7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5, 9,
    14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10, 15,
    21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
];

/// `floor( abs( sin( i + 1 ) ) * 2^32 )`.
const SINES: [u32; 64] = [
    0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613, 0xfd469501,
    0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193, 0xa679438e, 0x49b40821,
    0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d, 0x02441453, 0xd8a1e681, 0xe7d3fbc8,
    0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed, 0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a,
    0xfffa3942, 0x8771f681, 0x6d9d6122, 0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70,
    0x289b7ec6, 0xeaa127fa, 0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665,
    0xf4292244, 0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
    0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb, 0xeb86d391,
];

/// `md5( $data )`: the digest as 32 lower case hex digits.
pub fn md5(data: &[u8]) -> String {
    let mut message = data.to_vec();
    message.push(0x80);
    // Zeros up to 8 bytes short of a 64 byte block.
    message.resize(message.len() + (119 - data.len() % 64) % 64, 0);
    message.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_le_bytes());
    let mut state: [u32; 4] = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476];
    for chunk in message.as_chunks::<64>().0 {
        let mut words = [0u32; 16];
        for (word, bytes) in words.iter_mut().zip(chunk.as_chunks::<4>().0) {
            *word = u32::from_le_bytes(*bytes);
        }
        let [mut a, mut b, mut c, mut d] = state;
        for (i, (shift, sine)) in SHIFTS.iter().zip(SINES).enumerate() {
            let (f, g) = match i / 16 {
                // RFC 1321's selector functions F and G, in the equivalent
                // forms `d ^ (b & (c ^ d))` and `c ^ (d & (b ^ c))`.
                0 => (d ^ (b & (c ^ d)), i),
                1 => (c ^ (d & (b ^ c)), (5 * i + 1) % 16),
                2 => (b ^ c ^ d, (3 * i + 5) % 16),
                _ => (c ^ (b | !d), (7 * i) % 16),
            };
            let rotated = a
                .wrapping_add(f)
                .wrapping_add(sine)
                .wrapping_add(words.get(g).copied().unwrap_or(0))
                .rotate_left(*shift);
            (a, d, c, b) = (d, c, b, b.wrapping_add(rotated));
        }
        for (value, added) in state.iter_mut().zip([a, b, c, d]) {
            *value = value.wrapping_add(added);
        }
    }
    let digest: Vec<u8> = state.iter().flat_map(|word| word.to_le_bytes()).collect();
    bin2hex(&digest)
}

/// `bin2hex( $bytes )`.
pub fn bin2hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc_1321_test_suite() {
        for (input, digest) in [
            ("", "d41d8cd98f00b204e9800998ecf8427e"),
            ("a", "0cc175b9c0f1b6a831c399e269772661"),
            ("abc", "900150983cd24fb0d6963f7d28e17f72"),
            ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
            (
                "abcdefghijklmnopqrstuvwxyz",
                "c3fcd3d76192e4007dfb496cca67e13b",
            ),
            (
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
                "d174ab98d277d9f5a5611c2c9f419d9f",
            ),
            (
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
                "57edf4a22be3c955ac49da2e2107b67a",
            ),
        ] {
            assert_eq!(md5(input.as_bytes()), digest, "{input:?}");
        }
    }

    /// PHP's `md5()` at the block boundaries, on multibyte text.
    #[test]
    fn block_boundaries_like_php() {
        let text = "HyScribe \u{e9} ".repeat(100);
        for (len, digest) in [
            (55, "e4ec0b64c4562a8ab4a38eadae4f9c92"),
            (56, "9b4adf3ff8d74da4268b5db71de732d9"),
            (63, "456c86d9f7c95114f00fd2d4bb6a4534"),
            (64, "aa4dcbb6d812451498972fa1a4588319"),
            (119, "fb70528382784e86513e59129456ec5d"),
            (1000, "cf99013ad280afe9f95b676ad1badfc6"),
        ] {
            assert_eq!(md5(&text.as_bytes()[..len]), digest, "{len}");
        }
    }
}
