// SPDX-License-Identifier: AGPL-3.0-or-later

//! Login sessions in files (ADR 0011): `WP_User_Meta_Session_Tokens` over
//! one file per user, holding what the `session_tokens` user meta holds, in
//! PHP's serialization, keyed by the SHA-256 of each token.

use std::fs::{DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::Mutex;

use hyscribe_phpser::{Key, Value, bin2hex, decode, encode};
use sha2::{Digest, Sha256};

/// `wp_generate_password()`'s characters without special ones.
const TOKEN_CHARS: &[u8; 62] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// What a session records besides its expiry: where and when it began.
pub struct Origin<'a> {
    /// `$_SERVER['REMOTE_ADDR']`, left out when empty.
    pub ip: &'a [u8],
    /// The user agent, left out when empty.
    pub ua: &'a [u8],
    /// `time()` at login.
    pub login: i64,
}

/// The sessions of every user, in a folder of their own.
#[derive(Debug)]
pub struct Sessions {
    dir: PathBuf,
    /// One request changes a user's file at a time.
    lock: Mutex<()>,
}

/// `WP_Session_Tokens::hash_token()`.
fn verifier(token: &[u8]) -> Vec<u8> {
    bin2hex(&Sha256::digest(token)).into_bytes()
}

/// `wp_generate_password( 43, false, false )`, from `fill`'s bytes.
fn new_token(fill: impl FnMut(&mut [u8]) -> io::Result<()>) -> io::Result<String> {
    generated(fill, 43, TOKEN_CHARS)
}

/// `length` characters of `chars`, each drawn evenly from `fill`'s bytes.
pub(crate) fn generated(
    mut fill: impl FnMut(&mut [u8]) -> io::Result<()>,
    length: usize,
    chars: &[u8],
) -> io::Result<String> {
    let mut out = String::with_capacity(length);
    let (mut bytes, mut read) = ([0; 64], Ok(()));
    // Below this, the bytes fall evenly on the characters.
    let even = 256 - 256 % chars.len();
    while read.is_ok() && out.len() < length {
        read = fill(&mut bytes);
        let drawn = bytes
            .iter()
            .filter(|&&b| usize::from(b) < even)
            .take(length - out.len());
        out.extend(drawn.filter_map(|&b| {
            chars
                .get(usize::from(b) % chars.len())
                .map(|&c| char::from(c))
        }));
    }
    read.map(|()| out)
}

impl Sessions {
    /// The sessions kept in `dir`, which is made `0700` when first written.
    pub fn new(dir: impl Into<PathBuf>) -> Sessions {
        Sessions {
            dir: dir.into(),
            lock: Mutex::new(()),
        }
    }

    fn file(&self, user: u64) -> PathBuf {
        self.dir.join(user.to_string())
    }

    /// `get_sessions()`: the user's unexpired sessions; none when the file
    /// is missing or unreadable.
    fn sessions(&self, user: u64, now: i64) -> Vec<(Key, Value)> {
        let data = std::fs::read(self.file(user)).unwrap_or_default();
        let Ok(Value::Array(sessions)) = decode(&data) else {
            return Vec::new();
        };
        sessions
            .into_iter()
            .filter(|(_, session)| {
                session
                    .get_str("expiration")
                    .is_some_and(|expiration| expiration.to_int() >= now)
            })
            .collect()
    }

    /// `update_sessions()`: the file rewritten whole, or removed when no
    /// session is left.
    fn save(&self, user: u64, sessions: Vec<(Key, Value)>) -> io::Result<()> {
        let path = self.file(user);
        if sessions.is_empty() {
            return match std::fs::remove_file(&path) {
                Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
                _ => Ok(()),
            };
        }
        let temporary = self.dir.join(format!("{user}.new"));
        DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.dir)
            .and_then(|()| {
                OpenOptions::new()
                    .write(true)
                    .create(true)
                    .truncate(true)
                    .mode(0o600)
                    .open(&temporary)
            })
            .and_then(|mut file| {
                file.write_all(&encode(&Value::Array(sessions)))
                    .and_then(|()| file.sync_all())
            })
            .and_then(|()| std::fs::rename(&temporary, path))
    }

    /// `count( $sessions->get_all() )`: how many of `user`'s sessions are
    /// unexpired.
    pub fn count(&self, user: u64, now: i64) -> usize {
        self.sessions(user, now).len()
    }

    /// `WP_Session_Tokens::create()`: a new session for `user` until
    /// `expiration`, and its token.
    pub fn create(&self, user: u64, expiration: i64, origin: &Origin) -> io::Result<String> {
        let mut session = vec![(Key::String(b"expiration".to_vec()), Value::Int(expiration))];
        for (name, value) in [("ip", origin.ip), ("ua", origin.ua)] {
            if !value.is_empty() {
                session.push((Key::String(name.into()), Value::String(value.to_vec())));
            }
        }
        session.push((Key::String(b"login".to_vec()), Value::Int(origin.login)));
        new_token(crate::random_bytes).and_then(|token| {
            // Held even if poisoned: a panic leaves no file half written.
            let _held = self.lock.lock();
            let mut sessions = self.sessions(user, origin.login);
            sessions.push((
                Key::String(verifier(token.as_bytes())),
                Value::Array(session),
            ));
            self.save(user, sessions).map(|()| token)
        })
    }

    /// `WP_Session_Tokens::verify()`: the token names an unexpired session.
    pub fn verify(&self, user: u64, token: &[u8], now: i64) -> bool {
        let verifier = Key::String(verifier(token));
        self.sessions(user, now)
            .iter()
            .any(|(key, _)| *key == verifier)
    }

    /// `WP_Session_Tokens::destroy()`: the token's session ends.
    pub fn destroy(&self, user: u64, token: &[u8], now: i64) -> io::Result<()> {
        let verifier = Key::String(verifier(token));
        let _held = self.lock.lock();
        let mut sessions = self.sessions(user, now);
        sessions.retain(|(key, _)| *key != verifier);
        self.save(user, sessions)
    }
}

#[cfg(test)]
mod tests;
