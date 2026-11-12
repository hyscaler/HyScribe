// SPDX-License-Identifier: AGPL-3.0-or-later

//! Auth cookies in WordPress's format, `login|expiration|token|hmac`,
//! signed with the site's salts. HyScribe sends them under its own names,
//! so they never stand for a PHP WordPress login.

use hyscribe_phpser::{Value, bin2hex, hash_equals, hash_hmac_md5};
use sha2::{Digest, Sha256};

/// `hash_hmac( 'sha256', $data, $key )`.
fn hmac_sha256(data: &[u8], key: &[u8]) -> String {
    let key = if key.len() > 64 {
        Sha256::digest(key).to_vec()
    } else {
        key.to_vec()
    };
    let pad = |byte: u8| -> Vec<u8> {
        (0..64)
            .map(|i| key.get(i).copied().unwrap_or(0) ^ byte)
            .collect()
    };
    let inner = Sha256::digest([pad(0x36), data.to_vec()].concat());
    bin2hex(&Sha256::digest([&pad(0x5c)[..], &inner].concat()))
}

/// The four bytes of the password hash a cookie binds to: past the prefix
/// of a phpass or bcrypt hash, else its end.
fn pass_frag(user_pass: &[u8]) -> &[u8] {
    if user_pass.starts_with(b"$P$") || user_pass.starts_with(b"$2y$") {
        user_pass
            .get(8..12)
            .unwrap_or_else(|| user_pass.get(8..).unwrap_or_default())
    } else {
        user_pass
            .get(user_pass.len().saturating_sub(4)..)
            .unwrap_or_default()
    }
}

/// The cookie's signature: `hash_hmac( 'sha256' )` keyed with `wp_hash()`.
fn sign(login: &[u8], user_pass: &[u8], expiration: &[u8], token: &[u8], salt: &[u8]) -> String {
    let fields = |parts: &[&[u8]]| parts.join(&b'|');
    let key = hash_hmac_md5(
        &fields(&[login, pass_frag(user_pass), expiration, token]),
        salt,
    );
    hmac_sha256(&fields(&[login, expiration, token]), key.as_bytes())
}

/// `wp_generate_auth_cookie()` with the scheme's salt.
pub fn generate_auth_cookie(
    login: &[u8],
    user_pass: &[u8],
    expiration: i64,
    token: &[u8],
    salt: &[u8],
) -> Vec<u8> {
    let expiration = expiration.to_string();
    let hmac = sign(login, user_pass, expiration.as_bytes(), token, salt);
    [login, expiration.as_bytes(), token, hmac.as_bytes()].join(&b'|')
}

/// A cookie `wp_parse_auth_cookie()` splits into its four parts.
pub struct AuthCookie<'a> {
    /// The user's login.
    pub username: &'a [u8],
    expiration: &'a [u8],
    /// The session token.
    pub token: &'a [u8],
    hmac: &'a [u8],
}

/// `wp_parse_auth_cookie()`: exactly four `|`-separated parts.
pub fn parse_auth_cookie(cookie: &[u8]) -> Option<AuthCookie<'_>> {
    let parts: Vec<&[u8]> = cookie.split(|&b| b == b'|').collect();
    let [username, expiration, token, hmac] = parts[..] else {
        return None;
    };
    Some(AuthCookie {
        username,
        expiration,
        token,
        hmac,
    })
}

impl AuthCookie<'_> {
    /// `wp_validate_auth_cookie()` short of the session: unexpired, with an
    /// hour's grace for a POST, and signed for this user's password hash.
    pub fn is_valid(&self, user_pass: &[u8], salt: &[u8], now: i64, post: bool) -> bool {
        let grace = if post { 3_600 } else { 0 };
        let expired = Value::String(self.expiration.to_vec())
            .to_int()
            .saturating_add(grace)
            < now;
        let hmac = sign(self.username, user_pass, self.expiration, self.token, salt);
        !expired && hash_equals(hmac.as_bytes(), self.hmac)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_sha256_like_php() {
        // RFC 4231 test case 2, and keys past and at the block size.
        assert_eq!(
            hmac_sha256(b"what do ya want for nothing?", b"Jefe"),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
        assert_eq!(
            hmac_sha256(
                b"Test Using Larger Than Block-Size Key - Hash Key First",
                &[0xaa; 131]
            ),
            "60e431591ee0b67f0d8a26aacbf5b77f8e0bc6213728c5140546040f0ee37f54"
        );
        assert_eq!(
            hmac_sha256(b"data", &[b'k'; 64]),
            "93244b031118b8060c6619e5900fcf916faefb2d8d020fecb22ae0f62275db08"
        );
    }

    #[test]
    fn a_cookie_needs_four_parts() {
        assert!(parse_auth_cookie(b"a|1|t").is_none());
        assert!(parse_auth_cookie(b"a|1|t|h|x").is_none());
        let cookie = parse_auth_cookie(b"a|1|t|h").unwrap();
        assert_eq!((cookie.username, cookie.token), (&b"a"[..], &b"t"[..]));
    }
}
