// SPDX-License-Identifier: AGPL-3.0-or-later

//! HyScribe's side of the switch-back suite (MVP 8.5, GOALS S7): the
//! release binary serves the fixture database in primary mode and is sent
//! the writes of `reference/switch-back/actions.list` as real HTTP
//! requests, logged in with its own sessions. `reference/switch-back/run.sh`
//! runs it over a fresh `db.sh up`, then serves the same database with PHP
//! WordPress and checks it there. Ignored: it writes to the fixture.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The keys and salts of the run's `.env`, so the test signs its nonces
/// as the server does.
const SALTS: [(&str, &str); 8] = [
    ("AUTH_KEY", "switch-back-auth-key"),
    ("SECURE_AUTH_KEY", "switch-back-secure-auth-key"),
    ("LOGGED_IN_KEY", "switch-back-logged-in-key"),
    ("NONCE_KEY", "switch-back-nonce-key"),
    ("AUTH_SALT", "switch-back-auth-salt"),
    ("SECURE_AUTH_SALT", "switch-back-secure-auth-salt"),
    ("LOGGED_IN_SALT", "switch-back-logged-in-salt"),
    ("NONCE_SALT", "switch-back-nonce-salt"),
];

/// The site's address as PHP WordPress serves it afterwards, so every URL
/// HyScribe writes is the oracle's.
const HOME: &str = "http://127.0.0.1:8081";

struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

/// One HTTP/1.1 request to `port`, read to the end of the connection.
fn send(port: u16, method: &str, path: &str, cookies: &str, body: Option<&str>) -> Answer {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(120)))
        .unwrap();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:8081\r\nConnection: close\r\n\
         User-Agent: switch-back\r\n"
    );
    if !cookies.is_empty() {
        request.push_str(&format!("Cookie: {cookies}\r\n"));
    }
    let body = body.unwrap_or_default();
    if method == "POST" {
        request.push_str(&format!(
            "Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream.read_to_end(&mut raw).unwrap();
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .unwrap()
        .split(' ')
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(": "))
        .map(|(name, value)| (name.to_ascii_lowercase(), value.to_string()))
        .collect();
    let mut rest = raw[split + 4..].to_vec();
    if headers
        .iter()
        .any(|(n, v)| n == "transfer-encoding" && v.contains("chunked"))
    {
        rest = dechunk(&rest);
    }
    Answer {
        status,
        headers,
        body: String::from_utf8_lossy(&rest).into_owned(),
    }
}

/// A chunked body joined.
fn dechunk(mut raw: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let end = raw.windows(2).position(|w| w == b"\r\n").unwrap();
        let size = std::str::from_utf8(&raw[..end]).unwrap();
        let size = usize::from_str_radix(size.split(';').next().unwrap().trim(), 16).unwrap();
        if size == 0 {
            return out;
        }
        out.extend_from_slice(&raw[end + 2..end + 2 + size]);
        raw = &raw[end + 4 + size..];
    }
}

/// A cookie value as `urldecode()` reads it, `+` aside.
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3).unwrap_or_default();
            if let Ok(byte) = u8::from_str_radix(hex, 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

/// The release binary in a folder of its own with a `.env` for the
/// fixture database in primary mode, started as a service.
struct Server {
    home: std::path::PathBuf,
    _root: tempfile::TempDir,
    port: u16,
}

impl Server {
    fn start() -> Server {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("hyscribe");
        std::fs::create_dir(&home).unwrap();
        std::fs::copy(env!("CARGO_BIN_EXE_hyscribe"), home.join("hyscribe")).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let db_port = std::env::var("HYSCRIBE_TEST_DB_PORT").unwrap_or_else(|_| "33061".into());
        let mut env = format!(
            "DB_HOST=127.0.0.1\nDB_PORT={db_port}\nDB_NAME=wordpress\nDB_USER=wordpress\n\
             DB_PASSWORD=reference-only\nPORT={port}\nMODE=primary\nWP_HOME={HOME}\n\
             WP_SITEURL={HOME}\n"
        );
        for (name, value) in SALTS {
            env.push_str(&format!("{name}={value}\n"));
        }
        let env_file = home.join(".env");
        std::fs::write(&env_file, env).unwrap();
        std::fs::set_permissions(&env_file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let server = Server {
            home,
            _root: root,
            port,
        };
        let out = server.service("start");
        assert!(out.starts_with("started (pid "), "{out}");
        // The service answers once it has read the site and bound its port.
        for _ in 0..300 {
            if TcpStream::connect(("127.0.0.1", port)).is_ok() {
                return server;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        panic!("the server did not start");
    }

    fn service(&self, verb: &str) -> String {
        let output = Command::new(self.home.join("hyscribe"))
            .env_clear()
            .args(["service", verb])
            .output()
            .unwrap();
        let (out, err) = (output.stdout, output.stderr);
        String::from_utf8_lossy(&[out, err].concat()).into_owned()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.service("stop");
        if let Ok(log) = std::fs::read_to_string(self.home.join("logs/hyscribe.log")) {
            eprintln!("server log:\n{log}");
        }
    }
}

/// Logs `admin` in through `/hs-login`: the cookies and the session token.
fn log_in(port: u16) -> (String, String) {
    let answer = send(
        port,
        "POST",
        "/hs-login",
        "",
        Some("log=admin&pwd=reference-only-password"),
    );
    assert_eq!(answer.status, 302, "login: {}", answer.body);
    let cookies: Vec<String> = answer
        .headers
        .iter()
        .filter(|(name, _)| name == "set-cookie")
        .map(|(_, value)| value.split(';').next().unwrap().to_string())
        .collect();
    let logged_in = cookies
        .iter()
        .find(|c| c.starts_with("hyscribe_logged_in_"))
        .unwrap();
    let value = percent_decoded(logged_in.split_once('=').unwrap().1);
    let token = value.split('|').nth(2).unwrap().to_string();
    (cookies.join("; "), token)
}

/// `wp_create_nonce( $action )` for user 1 in the session `token`.
fn nonce(action: &str, token: &str) -> String {
    let salt = format!(
        "{}{}",
        SALTS.iter().find(|(n, _)| *n == "NONCE_KEY").unwrap().1,
        SALTS.iter().find(|(n, _)| *n == "NONCE_SALT").unwrap().1
    );
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let tick = hyscribe_auth::nonce_tick(i64::try_from(now).unwrap());
    hyscribe_auth::create_nonce(salt.as_bytes(), tick, action, 1, token.as_bytes())
}

/// What no answer of HyScribe's may show.
const BROKEN: [&str; 4] = ["database error", "Fatal error", "Warning:", "Notice:"];

#[test]
#[ignore = "writes to the fixture database: run by reference/switch-back/run.sh"]
fn writes_for_php_to_read() {
    let server = Server::start();
    let (cookies, token) = log_in(server.port);
    let list = include_str!("../../../reference/switch-back/actions.list");
    let mut failures = Vec::new();
    for line in list
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        let [case, user, method, path, body, action, field, status] = f[..] else {
            panic!("bad line: {line}");
        };
        let signed = |text: &str| match action {
            "-" => text.to_string(),
            _ => {
                let sep = if text.is_empty() { "" } else { "&" };
                format!("{text}{sep}{field}={}", nonce(action, &token))
            }
        };
        let body = if body == "-" { "" } else { body };
        let (path, body) = match method {
            "GET" => {
                let (bare, query) = path.split_once('?').unwrap_or((path, ""));
                let query = signed(query);
                (
                    format!("{bare}?{query}").trim_end_matches('?').to_string(),
                    None,
                )
            }
            _ => (path.to_string(), Some(signed(body))),
        };
        let sent = if user == "admin" {
            cookies.as_str()
        } else {
            ""
        };
        let answer = send(server.port, method, &path, sent, body.as_deref());
        let location = answer
            .headers
            .iter()
            .find(|(n, _)| n == "location")
            .map_or("", |(_, v)| v.as_str());
        eprintln!("{case}\t{}\t{location}", answer.status);
        if answer.status.to_string() != status {
            failures.push(format!(
                "{case}: {} where {status} was expected ({location}): {}",
                answer.status,
                answer.body.chars().take(400).collect::<String>()
            ));
        }
        // `wp_die()`'s refusals in Ajax: -1 for a nonce, 0 or an error.
        let refused =
            ["-1", "0"].contains(&answer.body.trim()) || answer.body.contains("<wp_error");
        if path.contains("admin-ajax") && refused {
            failures.push(format!("{case}: refused with {:?}", answer.body));
        }
        if let Some(text) = BROKEN.iter().find(|t| answer.body.contains(*t)) {
            failures.push(format!("{case}: the answer shows {text:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
