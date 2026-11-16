// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_fast_hash()`: libsodium's `crypto_generichash()`, keyed BLAKE2b
//! (RFC 7693), as WordPress hashes password reset keys.

const IV: [u64; 8] = [
    0x6a09_e667_f3bc_c908,
    0xbb67_ae85_84ca_a73b,
    0x3c6e_f372_fe94_f82b,
    0xa54f_f53a_5f1d_36f1,
    0x510e_527f_ade6_82d1,
    0x9b05_688c_2b3e_6c1f,
    0x1f83_d9ab_fb41_bd6b,
    0x5be0_cd19_137e_2179,
];

const SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

/// The key `wp_fast_hash()` hashes with.
const KEY: &[u8] = b"wp_fast_hash_6.8+";
/// The parameter block's first word: fanout and depth 1, the key's 17
/// bytes and the hash's 30.
const PARAMETERS: u64 = 0x0101_111e;
/// The bytes of a hash.
const OUT: usize = 30;

/// The mixing function `G` of four words and two message words.
fn g([mut a, mut b, mut c, mut d]: [u64; 4], x: u64, y: u64) -> [u64; 4] {
    a = a.wrapping_add(b).wrapping_add(x);
    d = (d ^ a).rotate_right(32);
    c = c.wrapping_add(d);
    b = (b ^ c).rotate_right(24);
    a = a.wrapping_add(b).wrapping_add(y);
    d = (d ^ a).rotate_right(16);
    c = c.wrapping_add(d);
    b = (b ^ c).rotate_right(63);
    [a, b, c, d]
}

/// The compression function `F` of `block`, `counter` bytes in, `last`
/// for the final block.
fn compress(h: &mut [u64; 8], block: &[u8; 128], counter: u64, last: bool) {
    let (words, _) = block.as_chunks::<8>();
    let mut m = [0u64; 16];
    for (word, bytes) in m.iter_mut().zip(words) {
        *word = u64::from_le_bytes(*bytes);
    }
    let [h0, h1, h2, h3, h4, h5, h6, h7] = *h;
    let [i0, i1, i2, i3, i4, i5, i6, i7] = IV;
    // The counter's high word stays 0: no message is 2^64 bytes long.

    let i6 = if last { !i6 } else { i6 };
    let mut v = [
        h0,
        h1,
        h2,
        h3,
        h4,
        h5,
        h6,
        h7,
        i0,
        i1,
        i2,
        i3,
        i4 ^ counter,
        i5,
        i6,
        i7,
    ];
    // Twelve rounds, the last two with the first two permutations again.
    for s in SIGMA.iter().chain(SIGMA.iter().take(2)) {
        let x = |n: usize| {
            s.get(n)
                .and_then(|&i| m.get(i))
                .copied()
                .unwrap_or_default()
        };
        let [
            v0,
            v1,
            v2,
            v3,
            v4,
            v5,
            v6,
            v7,
            v8,
            v9,
            v10,
            v11,
            v12,
            v13,
            v14,
            v15,
        ] = v;
        let [v0, v4, v8, v12] = g([v0, v4, v8, v12], x(0), x(1));
        let [v1, v5, v9, v13] = g([v1, v5, v9, v13], x(2), x(3));
        let [v2, v6, v10, v14] = g([v2, v6, v10, v14], x(4), x(5));
        let [v3, v7, v11, v15] = g([v3, v7, v11, v15], x(6), x(7));
        let [v0, v5, v10, v15] = g([v0, v5, v10, v15], x(8), x(9));
        let [v1, v6, v11, v12] = g([v1, v6, v11, v12], x(10), x(11));
        let [v2, v7, v8, v13] = g([v2, v7, v8, v13], x(12), x(13));
        let [v3, v4, v9, v14] = g([v3, v4, v9, v14], x(14), x(15));
        v = [
            v0, v1, v2, v3, v4, v5, v6, v7, v8, v9, v10, v11, v12, v13, v14, v15,
        ];
    }
    let (low, high) = v.split_at(8);
    for ((word, low), high) in h.iter_mut().zip(low).zip(high) {
        *word ^= low ^ high;
    }
}

/// BLAKE2b-240 of `message` under `KEY`.
fn blake2b(message: &[u8]) -> [u8; OUT] {
    let mut h = IV;
    for word in h.iter_mut().take(1) {
        *word ^= PARAMETERS;
    }
    // The key is the first block, padded with zeros.
    let mut data = [KEY, &[0; 128 - KEY.len()], message].concat();
    let blocks = data.len().div_ceil(128);
    data.resize(blocks * 128, 0);
    let mut counter = 0;
    for (n, block) in data.as_chunks::<128>().0.iter().enumerate() {
        let last = n + 1 == blocks;
        counter = if last {
            128 + message.len() as u64
        } else {
            counter + 128
        };
        compress(&mut h, block, counter, last);
    }
    let mut out = [0; OUT];
    let bytes = h.iter().flat_map(|word| word.to_le_bytes());
    for (byte, value) in out.iter_mut().zip(bytes) {
        *byte = value;
    }
    out
}

/// `wp_fast_hash( $message )`: `$generic$` and the URL safe, unpadded
/// base64 of the hash.
pub fn wp_fast_hash(message: &[u8]) -> String {
    const URL_SAFE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    format!(
        "$generic${}",
        crate::encode(&blake2b(message), URL_SAFE, false)
    )
}

#[cfg(test)]
mod tests {
    use super::wp_fast_hash;

    /// Every case of `reference/auth/fast-hash-golden.php`.
    #[test]
    fn fast_hashes_are_php_s() {
        let golden = include_str!("../tests/golden/fast-hash.tsv");
        for line in golden.lines() {
            let (message, hash) = line.split_once('\t').unwrap();
            let message = hyscribe_testkit::unhex(message);
            assert_eq!(wp_fast_hash(&message), hash, "{line}");
        }
    }
}
