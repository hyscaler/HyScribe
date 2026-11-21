// SPDX-License-Identifier: AGPL-3.0-or-later

//! HyScribe's side of the switch-back suite (MVP 8.5, GOALS S7): the
//! release binary serves the fixture database in primary mode and is sent
//! the writes of `reference/switch-back/actions.list` as real HTTP
//! requests, logged in with its own sessions. `reference/switch-back/run.sh`
//! runs it over a fresh `db.sh up`, then serves the same database with PHP
//! WordPress and checks it there. Ignored: it writes to the fixture. The
//! answers of its REST writes go to `$HYSCRIBE_SWITCH_BACK_OUT/rest.tsv`.

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

/// The site's address as PHP WordPress serves it afterwards (run.sh's
/// `HYSCRIBE_PHP_PORT`), so every URL HyScribe writes is the oracle's.
fn host() -> String {
    let port = std::env::var("HYSCRIBE_PHP_PORT").unwrap_or_else(|_| "8081".into());
    format!("127.0.0.1:{port}")
}

fn host_port() -> String {
    host().split_once(':').unwrap().1.to_string()
}

struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

/// One HTTP/1.1 request to `port` with `headers`, read to the end of the
/// connection: a body starting with `{` is sent as JSON, else as a form,
/// unless `kind` names its content type (a multipart upload).
fn send(
    port: u16,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<&[u8]>,
    kind: Option<&str>,
) -> Answer {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(120)))
        .unwrap();
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\
         User-Agent: switch-back\r\n",
        host()
    );
    for (name, value) in headers.iter().filter(|(_, value)| !value.is_empty()) {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    let body = body.unwrap_or_default();
    if ["POST", "PUT", "PATCH"].contains(&method) {
        let kind = kind.unwrap_or(match body.starts_with(b"{") {
            true => "application/json",
            false => "application/x-www-form-urlencoded",
        });
        request.push_str(&format!(
            "Content-Type: {kind}\r\nContent-Length: {}\r\n",
            body.len()
        ));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    stream.write_all(body).unwrap();
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
    /// Started with the `.env` lines `extra` besides the database's.
    fn start(extra: &str) -> Server {
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
        let db_name = std::env::var("HYSCRIBE_TEST_DB_NAME").unwrap_or_else(|_| "wordpress".into());
        // The uploads folder PHP WordPress serves afterwards, kept across restarts.
        let uploads = std::env::var("HYSCRIBE_SWITCH_BACK_OUT")
            .map(|out| format!("UPLOADS_DIR={out}/uploads\n"))
            .unwrap_or_default();
        let extra = format!("{uploads}{extra}");
        let url = format!("http://{}", host());
        let mut env = format!(
            "DB_HOST=127.0.0.1\nDB_PORT={db_port}\nDB_NAME={db_name}\nDB_USER=wordpress\n\
             DB_PASSWORD=reference-only\nPORT={port}\nMODE=primary\nWP_HOME={url}\n\
             WP_SITEURL={url}\n{extra}"
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

    /// `hyscribe cron run`: the due events once, the lines it prints.
    fn cron(&self) -> String {
        let output = Command::new(self.home.join("hyscribe"))
            .env_clear()
            .args(["cron", "run"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&[output.stdout, output.stderr].concat()).into_owned()
    }

    /// `hyscribe plugin install` of the test extension (MVP 11.3), its
    /// component built from source, packaged with its manifest.
    fn install(&self) -> String {
        let script = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../hyscribe-ext/guest/build.sh"
        );
        let built = Command::new("sh").arg(script).output().unwrap();
        assert!(
            built.status.success(),
            "{}",
            String::from_utf8_lossy(&built.stderr)
        );
        let wasm = std::fs::read(String::from_utf8_lossy(&built.stdout).trim()).unwrap();
        let manifest = include_bytes!("../../hyscribe-ext/guest/hyscribe.ext.toml");
        let package = hyscribe_testkit::hyx::zip(&[
            ("hyscribe.ext.toml", manifest, true),
            ("extension.wasm", &wasm, true),
        ]);
        let file = self.home.join("hyscribe-test.hyx");
        std::fs::write(&file, package).unwrap();
        let output = Command::new(self.home.join("hyscribe"))
            .env_clear()
            .arg("plugin")
            .arg("install")
            .arg(&file)
            .output()
            .unwrap();
        String::from_utf8_lossy(&[output.stdout, output.stderr].concat()).into_owned()
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
        &[],
        Some(b"log=admin&pwd=reference-only-password"),
        None,
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

/// The first `"id":` number of a JSON answer: the object's, or a list's
/// first object's.
fn first_id(body: &str) -> Option<String> {
    let (_, rest) = body.split_once("\"id\":")?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    (!digits.is_empty()).then_some(digits)
}

/// The ID an answer gives: a REST object's, the user global styles post a
/// theme's links name, or the bare number an async upload answers.
fn answered_id(body: &str) -> Option<String> {
    let plain = body.trim();
    if !plain.is_empty() && plain.chars().all(|c| c.is_ascii_digit()) {
        return Some(plain.to_string());
    }
    let unescaped = body.replace('\\', "");
    if let Some((_, rest)) = unescaped.split_once("wp/v2/global-styles/") {
        let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if !digits.is_empty() {
            return Some(digits);
        }
    }
    first_id(body)
}

/// The text with each `%now+N%` the time N seconds from now, as REST's
/// `date_gmt` takes it.
fn timed(text: &str) -> String {
    let Some((head, rest)) = text.split_once("%now+") else {
        return text.to_string();
    };
    let (seconds, tail) = rest.split_once('%').unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        + seconds.parse::<u64>().unwrap();
    let (days, secs) = (i64::try_from(now / 86400).unwrap(), now % 86400);
    // Civil date from the day count (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let stamp = format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    );
    timed(&format!("{head}{stamp}{tail}"))
}

/// A multipart form of the fields `k=v&k=v` and a file of the image corpus
/// as `part`, and its content type.
fn multipart(part: &str, file: &str, fields: &str) -> (Vec<u8>, String) {
    let boundary = "----switchback7e1c";
    let mut body = Vec::new();
    for pair in fields.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap();
        body.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{}\r\n",
                value.replace('+', " ")
            )
            .as_bytes(),
        );
    }
    let corpus = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../hyscribe-image/tests/corpus/"
    );
    let bytes = std::fs::read(format!("{corpus}{file}")).unwrap();
    let kind = match file.rsplit_once('.').unwrap().1 {
        "png" => "image/png",
        "gif" => "image/gif",
        _ => "image/jpeg",
    };
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"{part}\"; filename=\"{file}\"\r\n\
             Content-Type: {kind}\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(&bytes);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (body, format!("multipart/form-data; boundary={boundary}"))
}

/// What no answer of HyScribe's may show.
const BROKEN: [&str; 4] = ["database error", "Fatal error", "Warning:", "Notice:"];

#[test]
#[ignore = "writes to the fixture database: run by reference/switch-back/run.sh"]
fn writes_for_php_to_read() {
    // Only `hyscribe cron run` runs the cron: an event that is due now is the CRON line's.
    let mut server = Server::start("DISABLE_WP_CRON=true\n");
    let (mut cookies, mut token) = log_in(server.port);
    let list = match std::env::var("HYSCRIBE_SWITCH_BACK_LIST") {
        Ok(path) => std::fs::read_to_string(path).unwrap(),
        Err(_) => include_str!("../../../reference/switch-back/actions.list").to_string(),
    };
    let mut failures = Vec::new();
    // The REST writes' answers, for check.php to compare with PHP's reads.
    let mut rest = String::new();
    // The ID each REST answer gives first, which `%case%` in a later line names.
    let mut ids: Vec<(String, String)> = Vec::new();
    for line in list
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
    {
        let f: Vec<&str> = line.split('\t').collect();
        let [case, user, method, path, body, action, field, status] = f[..] else {
            panic!("bad line: {line}");
        };
        let named = |text: &str| {
            ids.iter().fold(text.to_string(), |text, (case, id)| {
                text.replace(&format!("%{case}%"), id)
            })
        };
        let (path, body, action) = (named(path), timed(&named(body)), named(action));
        let (path, body, action) = (path.as_str(), body.as_str(), action.as_str());
        match method {
            "SLEEP" => {
                std::thread::sleep(Duration::from_secs(path.parse().unwrap()));
                continue;
            }
            // The system's cron: the due events once, as `hyscribe cron run`.
            "CRON" => {
                let out = server.cron();
                eprintln!("{case}\t{}", out.trim());
                if !out.contains("cron: ") {
                    failures.push(format!("{case}: cron said {out:?}"));
                }
                continue;
            }
            // Time a day count cannot wait for: SQL run in the fixture database.
            "SQL" => {
                let docker = std::env::var("DOCKER").unwrap_or_else(|_| "docker".into());
                let container = std::env::var("HYSCRIBE_DB_CONTAINER")
                    .unwrap_or_else(|_| "hyscribe-default-install-db".into());
                let db =
                    std::env::var("HYSCRIBE_TEST_DB_NAME").unwrap_or_else(|_| "wordpress".into());
                let out = Command::new(docker)
                    .args(["exec", &container, "mariadb", "-uroot", "-preference-only"])
                    .args(["-N", "-B", &db, "-e", &path.replace("%port%", &host_port())])
                    .output()
                    .unwrap();
                let shown = String::from_utf8_lossy(&out.stdout).trim().to_string();
                eprintln!("{case}\t{}\t{shown}", out.status);
                // A last line that is a number names an ID, as an answer's would.
                if let Some(id) = shown.lines().last().and_then(answered_id) {
                    ids.push((case.to_string(), id));
                }
                if !out.status.success() {
                    failures.push(format!("{case}: {}", String::from_utf8_lossy(&out.stderr)));
                }
                continue;
            }
            _ => {}
        }
        // `hyscribe plugin install` of the test extension, packaged here.
        if method == "INSTALL" {
            let out = server.install();
            eprintln!("{case}\t{}", out.trim());
            if !out.starts_with("Installed ") {
                failures.push(format!("{case}: {out:?}"));
            }
            continue;
        }
        if method == "RESTART" {
            server.service("stop");
            server = Server::start(&format!("{}\n", path.replace(' ', "\n")));
            (cookies, token) = log_in(server.port);
            eprintln!("{case}\trestarted with {path}");
            continue;
        }
        // A nonce in a field, or in the header `field` names (`X-WP-Nonce`).
        let nonce = (action != "-").then(|| nonce(action, &token));
        let header = field.starts_with("X-");
        let signed = |text: &str| match &nonce {
            Some(nonce) if !header => {
                let sep = if text.is_empty() { "" } else { "&" };
                format!("{text}{sep}{field}={nonce}")
            }
            _ => text.to_string(),
        };
        let body = if body == "-" { "" } else { body };
        let (path, body) = match method {
            "GET" | "DELETE" => {
                let (bare, query) = path.split_once('?').unwrap_or((path, ""));
                let query = signed(query);
                (
                    format!("{bare}?{query}").trim_end_matches('?').to_string(),
                    None,
                )
            }
            _ => (path.to_string(), Some(signed(body))),
        };
        // `upload:<part>:<corpus file>:<fields>` is a multipart form.
        let (body, kind) = match body.as_deref().and_then(|b| b.strip_prefix("upload:")) {
            Some(spec) => {
                let [part, file, fields] = spec.splitn(3, ':').collect::<Vec<_>>()[..] else {
                    panic!("bad upload: {line}");
                };
                let (bytes, kind) = multipart(part, file, fields);
                (Some(bytes), Some(kind))
            }
            None => (body.map(String::into_bytes), None),
        };
        let mut headers = vec![(
            "Cookie",
            if user == "admin" {
                cookies.as_str()
            } else {
                ""
            },
        )];
        if let Some(nonce) = nonce.as_deref().filter(|_| header) {
            headers.push((field, nonce));
        }
        let answer = send(
            server.port,
            method,
            &path,
            &headers,
            body.as_deref(),
            kind.as_deref(),
        );
        let uploaded = path.contains("async-upload");
        if (path.contains("rest_route=") || uploaded) && [200, 201].contains(&answer.status) {
            if let Some(id) = answered_id(&answer.body) {
                ids.push((case.to_string(), id));
            }
            if method != "GET" && !uploaded {
                rest.push_str(&format!("{case}\t{path}\t{}\n", answer.body));
            }
        }
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
    if let Ok(out) = std::env::var("HYSCRIBE_SWITCH_BACK_OUT") {
        std::fs::write(format!("{out}/rest.tsv"), rest).unwrap();
        // The IDs of each case, which run.sh names in pages.list as `%case%`.
        let table: String = ids.iter().map(|(c, id)| format!("{c}\t{id}\n")).collect();
        std::fs::write(format!("{out}/ids.tsv"), table).unwrap();
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
