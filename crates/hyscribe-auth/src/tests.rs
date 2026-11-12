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

/// Every case of `reference/auth/salt-golden.php`.
#[test]
fn salts_match_php() {
    use hyscribe_phpser::json_decode;
    let golden = include_str!("../tests/golden/salt.tsv");
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let constants = json_decode(&unhex(fields[0])).unwrap();
        let options = json_decode(&unhex(fields[1])).unwrap();
        let constants: Vec<(String, Vec<u8>)> = constants
            .as_array()
            .unwrap_or_default()
            .iter()
            .map(|(k, v)| {
                (
                    String::from_utf8(k.to_php_string()).unwrap(),
                    v.to_php_string(),
                )
            })
            .collect();
        let constants: Vec<(&str, &[u8])> = constants
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_slice()))
            .collect();
        let ours = wp_salt_auth(&constants, |name| {
            options.get_str(name).map(|v| v.to_php_string())
        });
        let php = (fields[2] != "~").then(|| unhex(fields[2]));
        assert_eq!(ours, php, "{line}");
    }
    assert_eq!(golden.lines().count(), 15);
}
