// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use hyscribe_testkit::unhex;

const GOLDEN: &str = include_str!("../tests/golden/passwords.tsv");

/// Formats `crypt()` knows and this port does not.
fn unported(hash: &[u8]) -> bool {
    [&b"$1$"[..], b"$6$", b"$argon2"]
        .iter()
        .any(|p| hash.starts_with(p))
}

/// Every case of `reference/auth/golden.php`.
#[test]
fn passwords_match_php() {
    let mut counts = [0; 5];
    for line in GOLDEN.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        match fields[..] {
            ["crypt", key, setting, out] => {
                counts[0] += 1;
                let ours = crypt(&unhex(key), setting.as_bytes());
                assert_eq!(ours.as_deref().unwrap_or("*0"), out, "{line}");
            }
            ["check", password, hash, expected] => {
                let hash = unhex(hash);
                if unported(&hash) {
                    counts[4] += 1;
                    assert!(!check_password(&unhex(password), &hash), "{line}");
                    continue;
                }
                counts[1] += 1;
                assert_eq!(
                    check_password(&unhex(password), &hash),
                    expected == "1",
                    "{line}"
                );
            }
            ["hash", password, hash, expected] => {
                counts[2] += 1;
                let password = unhex(password);
                assert_eq!(
                    check_password(&password, hash.as_bytes()),
                    expected == "1",
                    "{line}"
                );
                assert!(!password_needs_rehash(hash.as_bytes()));
                // The same salt gives the same hash.
                let salt = decode_salt(&hash.as_bytes()[10..32]).unwrap();
                assert_eq!(hash_with_salt(&password, salt), hash, "{line}");
            }
            ["rehash", hash, expected] => {
                counts[3] += 1;
                assert_eq!(
                    password_needs_rehash(&unhex(hash)),
                    expected == "1",
                    "{line}"
                );
            }
            _ => panic!("{line}"),
        }
    }
    assert_eq!(counts, [1275, 413, 26, 18, 81]);
}

#[test]
fn hashes_are_salted_at_random() {
    let first = hash_password(b"secret").unwrap();
    let second = hash_password(b"secret").unwrap();
    assert_ne!(first, second);
    assert!(first.starts_with("$wp$2y$12$"), "{first}");
    assert!(check_password(b"secret", first.as_bytes()));
    assert_eq!(hash_password(&[b'p'; 4097]).unwrap(), "*");
    assert_eq!(hash_password(&[b'p'; 4096]).unwrap().len(), 63);
    assert!(hash_from(b"x", Err(io::Error::other("no"))).is_err());
}

/// Every case of `reference/auth/salt-golden.php`: salts, and nonces made,
/// then checked at their tick, the next one and the one after.
#[test]
fn salts_and_nonces_match_php() {
    use hyscribe_phpser::json_decode;
    let golden = include_str!("../tests/golden/salt.tsv");
    let constants = |hex: &str| -> Vec<(String, Vec<u8>)> {
        let constants = json_decode(&unhex(hex)).unwrap();
        let entries = constants.as_array().unwrap_or_default();
        entries
            .iter()
            .map(|(k, v)| {
                (
                    String::from_utf8(k.to_php_string()).unwrap(),
                    v.to_php_string(),
                )
            })
            .collect()
    };
    fn borrowed(owned: &[(String, Vec<u8>)]) -> Vec<(&str, &[u8])> {
        owned
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
            .collect()
    }
    // The constants every nonce and cookie case signs with.
    let golden_constants = golden
        .lines()
        .find(|line| line.starts_with("nonce\t"))
        .and_then(|line| line.split('\t').nth(1))
        .unwrap()
        .to_string();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f[0] == "cookie" {
            let owned = constants(&golden_constants);
            let salt = wp_salt(f[1], &borrowed(&owned), |_| None).unwrap();
            let (login, pass, token) = (unhex(f[2]), unhex(f[3]), unhex(f[5]));
            let expiration: i64 = f[4].parse().unwrap();
            let cookie = cookie::generate_auth_cookie(&login, &pass, expiration, &token, &salt);
            assert_eq!(cookie, unhex(f[6]), "{line}");
            let parsed = cookie::parse_auth_cookie(&cookie);
            let valid = |now, post| {
                parsed
                    .as_ref()
                    .is_some_and(|c| c.is_valid(&pass, &salt, now, post))
            };
            // A login or token holding `|` splits into more than four parts.
            let plain = !login.contains(&b'|') && !token.contains(&b'|');
            assert_eq!(valid(expiration, false), plain, "{line}");
            assert!(!valid(expiration + 1, false));
            assert_eq!(valid(expiration + 3_600, true), plain);
            assert!(!valid(expiration + 3_601, true));
            assert!(!parsed.is_some_and(|c| c.is_valid(b"other hash", &salt, expiration, false)));
            continue;
        }
        if f[0] == "salt" {
            let owned = constants(f[2]);
            let options = json_decode(&unhex(f[3])).unwrap();
            let ours = wp_salt(f[1], &borrowed(&owned), |name| {
                options.get_str(name).map(|v| v.to_php_string())
            });
            let php = (f[4] != "~").then(|| unhex(f[4]));
            assert_eq!(ours, php, "{line}");
            continue;
        }
        let owned = constants(f[1]);
        let salt = wp_salt("nonce", &borrowed(&owned), |_| None).unwrap();
        let (user, token) = (f[2].parse().unwrap(), unhex(f[3]));
        let action = String::from_utf8(unhex(f[4])).unwrap();
        let tick: i64 = f[5].parse().unwrap();
        assert_eq!(
            create_nonce(&salt, tick, &action, user, &token),
            f[6],
            "{line}"
        );
        let verify = |nonce: &[u8], tick| verify_nonce(nonce, &salt, tick, &action, user, &token);
        assert_eq!(verify(f[6].as_bytes(), tick), 1);
        assert_eq!(verify(f[6].as_bytes(), tick + 1), 2);
        assert_eq!(verify(f[6].as_bytes(), tick + 2), 0);
        assert_eq!(verify(b"", tick), 0);
        assert_eq!(verify(b"0", tick), 0);
    }
    assert_eq!(golden.lines().count(), 115);
}

/// Ticks change every half day, at the second after each boundary.
#[test]
fn nonce_ticks_are_half_days() {
    assert_eq!(nonce_tick(0), 0);
    assert_eq!(nonce_tick(1), 1);
    assert_eq!(nonce_tick(43_200), 1);
    assert_eq!(nonce_tick(43_201), 2);
}

/// `wp_generate_password( 24 )` draws its characters, special ones among
/// them, from the kernel.
#[test]
fn passwords_are_generated() {
    let password = generate_password(24).unwrap();
    assert_eq!(password.len(), 24);
    let chars = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%^&*()";
    assert!(password.chars().all(|c| chars.contains(c)));
    assert_ne!(password, generate_password(24).unwrap());
}
