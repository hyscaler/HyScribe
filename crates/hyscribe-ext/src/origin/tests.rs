// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const KEY: &str = "E7620F1842B4E81F";

#[test]
fn reads_each_signer_and_writes_it_back() {
    for (word, signer) in [
        ("registry", Signer::Registry),
        ("signed", Signer::Signed),
        ("trusted", Signer::Trusted),
    ] {
        let text = format!("{word} {KEY} {HASH}\n");
        let origin = Origin::parse(text.as_bytes()).unwrap();
        assert_eq!(origin.signer, Some((signer, KEY.to_string())));
        assert_eq!((origin.sha256.as_str(), &origin.revoked), (HASH, &None));
        assert_eq!(origin.text(), text);
    }
    let text = format!("unsigned {HASH}\nrevoked 1795161600 CVE-2026-0001: fixed in 0.2.1\n");
    let origin = Origin::parse(text.as_bytes()).unwrap();
    assert_eq!(origin.signer, None);
    let reason = "CVE-2026-0001: fixed in 0.2.1".to_string();
    assert_eq!(origin.revoked, Some((1_795_161_600, reason)));
    assert_eq!(origin.text(), text);
}

#[test]
fn refuses_anything_else() {
    let line = format!("signed {KEY} {HASH}");
    for refused in [
        line.clone(),
        format!("{line}\n\n"),
        format!("Signed {KEY} {HASH}\n"),
        format!("Registry {KEY} {HASH}\n"),
        format!("signed {} {HASH}\n", KEY.to_lowercase()),
        format!("signed {HASH}\n"),
        format!("unsigned {KEY} {HASH}\n"),
        format!("unsigned {}\n", HASH.to_uppercase()),
        format!("unsigned {HASH}0\n"),
        format!("{line} x\n"),
        format!("{line}\nrevoked 1 \n"),
        format!("{line}\nrevoked 1\n"),
        format!("{line}\nrevoked x reason\n"),
        format!("{line}\nrevoked +5 reason\n"),
        format!("{line}\nrevoked  reason\n"),
        format!("{line}\nrevoked 123456789012345678901 reason\n"),
        format!("{line}\nrevoked 99999999999999999999 reason\n"),
        format!("{line}\nrevoked 1 a\treason\n"),
        format!("{line}\nrevoked 1 {}\n", "r".repeat(MAX_REASON + 1)),
        format!("{line}\nRevoked 1 reason\n"),
        format!("{line}\nrevoked 1 reason\nrevoked 2 reason\n"),
    ] {
        assert_eq!(Origin::parse(refused.as_bytes()), None, "{refused:?}");
    }
    assert_eq!(Origin::parse(b"\xff\n"), None);
    let longest = format!(
        "{line}\nrevoked 18446744073709551615 {}\n",
        "r".repeat(MAX_REASON)
    );
    assert!(Origin::parse(longest.as_bytes()).is_some());
}

#[test]
fn checks_reasons_and_hashes() {
    assert!(reason_text("a b"));
    assert!(!reason_text(""));
    assert!(!reason_text("caf\u{e9}"));
    assert!(hex64(HASH));
    assert!(!hex64(HASH.get(1..).unwrap()));
    assert!(!hex64(&HASH.replace('a', "g")));
}

#[test]
fn reads_and_writes_the_file() {
    let folder = tempfile::tempdir().unwrap();
    assert_eq!(read(folder.path()), Ok(None));
    let origin = Origin {
        signer: None,
        sha256: HASH.into(),
        revoked: None,
    };
    write(folder.path(), &origin).unwrap();
    assert_eq!(read(folder.path()), Ok(Some(origin)));
    std::fs::write(folder.path().join(FILE), "unsigned x\n").unwrap();
    assert_eq!(read(folder.path()), Err("origin.txt does not read".into()));
    std::fs::remove_file(folder.path().join(FILE)).unwrap();
    std::fs::create_dir(folder.path().join(FILE)).unwrap();
    let unread = read(folder.path()).unwrap_err();
    assert!(
        unread.starts_with("origin.txt cannot be read: "),
        "{unread}"
    );
    assert!(write(&folder.path().join("gone"), &origin_of_none()).is_err());
}

fn origin_of_none() -> Origin {
    Origin::parse(format!("unsigned {HASH}\n").as_bytes()).unwrap()
}
