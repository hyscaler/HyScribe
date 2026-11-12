// SPDX-License-Identifier: AGPL-3.0-or-later

use std::os::unix::fs::PermissionsExt;

use super::*;

const NOW: i64 = 1_700_000_000;

fn origin() -> Origin<'static> {
    Origin {
        ip: b"203.0.113.9",
        ua: b"Agent/1.0",
        login: NOW,
    }
}

#[test]
fn a_session_lives_until_it_expires_or_ends() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = Sessions::new(dir.path().join("sessions"));
    let token = sessions.create(1, NOW + 60, &origin()).unwrap();
    assert_eq!(token.len(), 43);
    assert!(token.bytes().all(|b| b.is_ascii_alphanumeric()));
    assert!(sessions.verify(1, token.as_bytes(), NOW + 60));
    assert!(!sessions.verify(1, token.as_bytes(), NOW + 61));
    assert!(!sessions.verify(2, token.as_bytes(), NOW));
    assert!(!sessions.verify(1, b"other", NOW));
    let second = sessions.create(1, NOW + 120, &origin()).unwrap();
    assert_ne!(token, second);
    sessions.destroy(1, token.as_bytes(), NOW).unwrap();
    assert!(!sessions.verify(1, token.as_bytes(), NOW));
    assert!(sessions.verify(1, second.as_bytes(), NOW));
    // The last session gone, so is the file; ending one again is no error.
    sessions.destroy(1, second.as_bytes(), NOW).unwrap();
    assert!(!dir.path().join("sessions/1").exists());
    sessions.destroy(1, second.as_bytes(), NOW).unwrap();
}

#[test]
fn files_are_private_and_hold_what_session_tokens_holds() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = Sessions::new(dir.path().join("sessions"));
    let token = sessions.create(7, NOW + 60, &origin()).unwrap();
    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode(&dir.path().join("sessions")), 0o700);
    assert_eq!(mode(&dir.path().join("sessions/7")), 0o600);
    let file = std::fs::read(dir.path().join("sessions/7")).unwrap();
    let hash = bin2hex(&Sha256::digest(token.as_bytes()));
    let expected = format!(
        "a:1:{{s:64:\"{hash}\";a:4:{{s:10:\"expiration\";i:{};s:2:\"ip\";s:11:\"203.0.113.9\";s:2:\"ua\";s:9:\"Agent/1.0\";s:5:\"login\";i:{NOW};}}}}",
        NOW + 60
    );
    assert_eq!(String::from_utf8(file).unwrap(), expected);
    // Without an address or agent, neither is kept; expired sessions are
    // dropped when the file is written.
    let bare = Origin {
        ip: b"",
        ua: b"",
        login: NOW + 61,
    };
    let token = sessions.create(7, NOW + 120, &bare).unwrap();
    let file = std::fs::read(dir.path().join("sessions/7")).unwrap();
    let hash = bin2hex(&Sha256::digest(token.as_bytes()));
    let expected = format!(
        "a:1:{{s:64:\"{hash}\";a:2:{{s:10:\"expiration\";i:{};s:5:\"login\";i:{};}}}}",
        NOW + 120,
        NOW + 61
    );
    assert_eq!(String::from_utf8(file).unwrap(), expected);
}

#[test]
fn unreadable_files_hold_no_session() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = Sessions::new(dir.path());
    std::fs::write(dir.path().join("3"), b"not serialized").unwrap();
    assert!(!sessions.verify(3, b"x", NOW));
    std::fs::write(dir.path().join("3"), b"a:1:{s:1:\"x\";i:1;}").unwrap();
    assert!(!sessions.verify(3, b"x", NOW));
    // A folder where the file should be cannot be written or removed.
    std::fs::create_dir(dir.path().join("4")).unwrap();
    assert!(sessions.destroy(4, b"x", NOW).is_err());
    std::fs::write(dir.path().join("4.new"), b"").unwrap();
    std::fs::remove_dir(dir.path().join("4")).unwrap();
    std::fs::create_dir(dir.path().join("5.new")).unwrap();
    assert!(sessions.create(5, NOW + 60, &origin()).is_err());
}

/// Bytes from 248 up are skipped, the rest fall on the characters by their
/// remainder, and a short read is topped up from another.
#[test]
fn tokens_take_bytes_below_248() {
    let mut reads = 0;
    let token = new_token(|bytes| {
        reads += 1;
        bytes.fill(255);
        if reads == 1 {
            bytes[..6].copy_from_slice(&[0, 61, 62, 247, 248, 123]);
        } else {
            bytes[..40].fill(1);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(reads, 2);
    assert_eq!(token, format!("a9a99{}", "b".repeat(38)));
    let failed = new_token(|_| Err(io::Error::other("no randomness")));
    assert!(failed.is_err());
}
