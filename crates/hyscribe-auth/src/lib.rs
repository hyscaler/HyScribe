// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress 7's authentication: password hashing and checks (bcrypt over
//! an HMAC-SHA384, and the phpass, bcrypt and MD5 hashes older sites hold),
//! salts, nonces, auth cookies, and roles and capabilities.

pub mod caps;
pub mod cookie;
mod fast_hash;
pub mod sessions;

pub use fast_hash::{wp_fast_hash, wp_verify_fast_hash};

use blowfish::Blowfish;
use hyscribe_phpser::{hash_equals, hash_hmac_md5, md5, md5_raw, php_trim};
use sha2::{Digest, Sha384};
use std::io::{self, Read};

/// Longer passwords are refused, as WordPress refuses them.
const MAX_PASSWORD: usize = 4096;
/// `password_hash()`'s bcrypt cost in PHP 8.4.
const COST: u32 = 12;
const BCRYPT_ALPHABET: &[u8; 64] =
    b"./ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const PHPASS_ALPHABET: &[u8; 64] =
    b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// `wp_hash_password()`: a `$wp` bcrypt hash of the trimmed password with a
/// random salt, or `*` for a password over 4096 bytes.
pub fn hash_password(password: &[u8]) -> io::Result<String> {
    let mut salt = [0; 16];
    let salt = random_bytes(&mut salt).map(|()| salt);
    hash_from(password, salt)
}

/// `wp_generate_password( $length, $special_chars )`, drawn from the
/// kernel: letters, digits and, when asked, the special characters.
pub fn generate_password(length: usize, special_chars: bool) -> io::Result<String> {
    let chars = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()";
    let chars = if special_chars {
        &chars[..]
    } else {
        &chars[..62]
    };
    sessions::generated(random_bytes, length, chars)
}

/// Fills `buf` from the kernel's random source.
pub(crate) fn random_bytes(buf: &mut [u8]) -> io::Result<()> {
    std::fs::File::open("/dev/urandom").and_then(|mut random| random.read_exact(buf))
}

fn hash_from(password: &[u8], salt: io::Result<[u8; 16]>) -> io::Result<String> {
    if password.len() > MAX_PASSWORD {
        return Ok("*".into());
    }
    Ok(hash_with_salt(password, salt?))
}

fn hash_with_salt(password: &[u8], salt: [u8; 16]) -> String {
    format!(
        "$wp{}",
        bcrypt(b'y', COST, salt, &prehash(php_trim(password)))
    )
}

/// `wp_check_password()`. Stored hashes set their own cost, up to 2^31
/// bcrypt or 2^30 MD5 rounds, so `hash` must never come from a request.
pub fn check_password(password: &[u8], hash: &[u8]) -> bool {
    if hash.len() <= 32 {
        return hash_equals(hash, md5(password).as_bytes());
    }
    if password.len() > MAX_PASSWORD {
        return false;
    }
    if let Some(bcrypt) = hash.strip_prefix(b"$wp") {
        password_verify(&prehash(password), bcrypt)
    } else if hash.starts_with(b"$P$") {
        phpass(password, hash)
    } else {
        password_verify(password, hash)
    }
}

/// `wp_password_needs_rehash()`: anything but a `$wp` bcrypt hash of
/// `$2y$` at cost 12.
pub fn password_needs_rehash(hash: &[u8]) -> bool {
    let Some(bcrypt) = hash.strip_prefix(b"$wp") else {
        return true;
    };
    let Some(cost) = bcrypt.strip_prefix(b"$2y$").filter(|_| bcrypt.len() == 60) else {
        return true;
    };
    // `sscanf( "$2y$%ld$" )`. When it reads no number PHP compares an
    // uninitialized cost, which matched in every recorded run.
    let mut rest = cost
        .iter()
        .skip_while(|b| b" \t\n\x0b\x0c\r".contains(b))
        .peekable();
    let negative = rest.next_if_eq(&&b'-').is_some();
    if !negative {
        rest.next_if_eq(&&b'+');
    }
    let (mut value, mut digits) = (0u32, 0);
    for b in rest.take_while(|b| b.is_ascii_digit()) {
        value = value.saturating_mul(10).saturating_add(u32::from(b - b'0'));
        digits += 1;
    }
    digits > 0 && (negative || value != COST)
}

/// `base64_encode( hash_hmac( 'sha384', $password, 'wp-sha384', true ) )`.
fn prehash(password: &[u8]) -> Vec<u8> {
    let mac = hmac::<Sha384>(password, b"wp-sha384", 128);
    encode(&mac, BASE64_ALPHABET, false).into_bytes()
}

/// RFC 2104 HMAC over a SHA-2 digest whose blocks are `block` bytes.
pub(crate) fn hmac<D: Digest>(data: &[u8], key: &[u8], block: usize) -> Vec<u8> {
    let key = if key.len() > block {
        D::digest(key).to_vec()
    } else {
        key.to_vec()
    };
    let pad = |byte: u8| -> Vec<u8> {
        (0..block)
            .map(|i| key.get(i).copied().unwrap_or(0) ^ byte)
            .collect()
    };
    let inner = D::new()
        .chain_update(pad(0x36))
        .chain_update(data)
        .finalize();
    D::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .to_vec()
}

/// `password_verify()` for bcrypt hashes. Other formats PHP's `crypt()`
/// knows (MD5, SHA-256 and SHA-512 crypt, DES) and Argon2 are not ported.
fn password_verify(password: &[u8], hash: &[u8]) -> bool {
    crypt(password, hash).is_some_and(|out| hash_equals(out.as_bytes(), hash))
}

/// PHP's `crypt()` with a bcrypt setting (`crypt_blowfish`), or `None`
/// where it fails.
fn crypt(key: &[u8], setting: &[u8]) -> Option<String> {
    let [b'$', b'2', variant, b'$', tens, ones, b'$', salt @ ..] = setting else {
        return None;
    };
    if !b"abxy".contains(variant) || !tens.is_ascii_digit() || !ones.is_ascii_digit() {
        return None;
    }
    let cost = u32::from(tens - b'0') * 10 + u32::from(ones - b'0');
    if !(4..=31).contains(&cost) {
        return None;
    }
    Some(bcrypt(*variant, cost, decode_salt(salt.get(..22)?)?, key))
}

/// The 22 salt characters as 16 bytes; the last character gives only its
/// top two bits.
fn decode_salt(chars: &[u8]) -> Option<[u8; 16]> {
    let (mut bits, mut count, mut salt) = (0u32, 0, Vec::new());
    for c in chars {
        let value = BCRYPT_ALPHABET.iter().position(|a| a == c)?;
        bits = (bits << 6) + value as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            salt.push((bits >> count) as u8);
        }
    }
    salt.truncate(16);
    salt.try_into().ok()
}

/// `$2<variant>$<cost>$` followed by the salt and the hash.
fn bcrypt(variant: u8, cost: u32, salt: [u8; 16], key: &[u8]) -> String {
    let (initial, expanded) = key_words(variant, key);
    let mut state = Blowfish::bc_init_state();
    state.salted_expand_key(&salt, &initial);
    for _ in 0..1u32 << cost {
        state.bc_expand_key(&expanded);
        state.bc_expand_key(&salt);
    }
    // "OrpheanBeholderScryDoubt"
    let mut text = [
        [0x4f72_7068, 0x6561_6e42],
        [0x6568_6f6c, 0x6465_7253],
        [0x6372_7944, 0x6f75_6274],
    ];
    for pair in &mut text {
        for _ in 0..64 {
            *pair = state.bc_encrypt(*pair);
        }
    }
    let mut hash: Vec<u8> = text
        .iter()
        .flatten()
        .flat_map(|w| w.to_be_bytes())
        .collect();
    hash.truncate(23);
    format!(
        "$2{}${cost:02}${}{}",
        char::from(variant),
        encode(&salt, BCRYPT_ALPHABET, false),
        encode(&hash, BCRYPT_ALPHABET, false)
    )
}

/// `BF_set_key()`: the 18 key words, as bytes, that start the salted key
/// schedule and that the expensive loop expands. The key ends at its
/// first NUL, which is part of it, and repeats. `$2x$` keeps the old sign
/// extension bug; `$2a$` marks the one case where the bug left no trace.
fn key_words(variant: u8, key: &[u8]) -> ([u8; 72], [u8; 72]) {
    let end = key.iter().position(|b| *b == 0).unwrap_or(key.len());
    let key: Vec<u8> = key.iter().take(end).chain([&0]).take(72).copied().collect();
    let mut bytes = key.iter().cycle();
    let (mut words, mut diff, mut sign) = ([0u32; 18], 0u32, 0u32);
    for word in &mut words {
        let (mut correct, mut bug) = (0u32, 0u32);
        for j in 0..4 {
            let byte = bytes.next().copied().unwrap_or_default();
            correct = (correct << 8) + u32::from(byte);
            bug = (bug << 8) | (i32::from(byte as i8) as u32);
            if j > 0 {
                sign |= bug & 0x80;
            }
        }
        diff |= correct ^ bug;
        *word = if variant == b'x' { bug } else { correct };
    }
    let expanded = to_bytes(&words);
    if let Some(first) = words.first_mut().filter(|_| variant == b'a' && diff == 0) {
        *first ^= sign << 9;
    }
    (to_bytes(&words), expanded)
}

fn to_bytes(words: &[u32; 18]) -> [u8; 72] {
    let mut bytes = [0; 72];
    for (chunk, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(words) {
        *chunk = word.to_be_bytes();
    }
    bytes
}

/// Base64 without padding, over the given alphabet; phpass's `encode64()`
/// packs each group of three bytes little endian.
fn encode(bytes: &[u8], alphabet: &[u8; 64], little_endian: bool) -> String {
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let mut group = [0; 3];
        for (g, b) in group.iter_mut().zip(chunk) {
            *g = *b;
        }
        if little_endian {
            group.reverse();
        }
        let n = group.iter().fold(0u32, |n, b| (n << 8) + u32::from(*b));
        for i in 0..=chunk.len() {
            let shift = if little_endian { 6 * i } else { 18 - 6 * i };
            out.extend(
                alphabet
                    .get(((n >> shift) & 0x3f) as usize)
                    .map(|c| char::from(*c)),
            );
        }
    }
    out
}

/// phpass's `CheckPassword()` for a portable `$P$` hash: `count` rounds of
/// MD5 over the salt and the password.
fn phpass(password: &[u8], hash: &[u8]) -> bool {
    let Some(rounds) = hash
        .get(3)
        .and_then(|c| PHPASS_ALPHABET.iter().position(|a| a == c))
        .filter(|log2| (7..=30).contains(log2))
    else {
        return false;
    };
    // Only hashes over 32 bytes get here.
    let (setting, salt) = (
        hash.get(..12).unwrap_or_default(),
        hash.get(4..12).unwrap_or_default(),
    );
    let mut digest = md5_raw(&[salt, password].concat());
    for _ in 0..1u32 << rounds {
        digest = md5_raw(&[&digest[..], password].concat());
    }
    let mut out = setting.to_vec();
    out.extend(encode(&digest, PHPASS_ALPHABET, true).bytes());
    hash_equals(&out, hash)
}

/// `wp_salt( $scheme )` for `auth`, `secure_auth`, `logged_in` and `nonce`:
/// its key and salt constants unless empty, shared or the placeholder, else
/// their options; `None` where WordPress would store new ones.
pub fn wp_salt(
    scheme: &str,
    constants: &[(&str, &[u8])],
    option: impl Fn(&str) -> Option<Vec<u8>>,
) -> Option<Vec<u8>> {
    let truthy = |value: &[u8]| !value.is_empty() && value != b"0";
    let usable = |value: &[u8]| {
        truthy(value)
            && value != b"put your unique phrase here"
            && constants.iter().filter(|(_, v)| *v == value).count() < 2
    };
    let part = |name: String| {
        let constant = constants
            .iter()
            .find(|(n, v)| *n == name && usable(v))
            .map(|(_, v)| v.to_vec());
        constant.or_else(|| option(&name.to_ascii_lowercase()).filter(|v| truthy(v)))
    };
    let scheme = scheme.to_ascii_uppercase();
    Some(
        [
            part(format!("{scheme}_KEY"))?,
            part(format!("{scheme}_SALT"))?,
        ]
        .concat(),
    )
}

/// `wp_nonce_tick()`: `ceil( time() / ( DAY_IN_SECONDS / 2 ) )`.
pub fn nonce_tick(now: i64) -> i64 {
    now.div_euclid(43_200) + i64::from(now.rem_euclid(43_200) != 0)
}

/// `wp_create_nonce( $action )` for the tick, signed with the `nonce` salt.
pub fn create_nonce(salt: &[u8], tick: i64, action: &str, user: u64, token: &[u8]) -> String {
    let data = [format!("{tick}|{action}|{user}|").as_bytes(), token].concat();
    hash_hmac_md5(&data, salt)
        .get(20..30)
        .unwrap_or_default()
        .to_string()
}

/// `wp_verify_nonce()`: 1 for the tick's nonce, 2 for the tick before's,
/// else 0.
pub fn verify_nonce(
    nonce: &[u8],
    salt: &[u8],
    tick: i64,
    action: &str,
    user: u64,
    token: &[u8],
) -> u8 {
    // An empty nonce, which PHP turns down first, never matches either.
    let matches = |tick| {
        hash_equals(
            create_nonce(salt, tick, action, user, token).as_bytes(),
            nonce,
        )
    };
    if matches(tick) {
        1
    } else if matches(tick - 1) {
        2
    } else {
        0
    }
}

#[cfg(test)]
mod tests;
