// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared test helpers: fixture locations and a reader for the `mysqldump`
//! files under `fixtures/`, so tests can check real WordPress rows without a
//! running database, and the write oracle's HyScribe side ([`writes`]).

use std::path::PathBuf;

pub mod writes;

/// A column value from an `INSERT` statement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SqlValue {
    /// `NULL`
    Null,
    /// An unquoted number, as written.
    Number(String),
    /// A quoted string with MySQL escapes resolved.
    Text(Vec<u8>),
}

impl SqlValue {
    /// The bytes of a text value.
    pub fn as_text(&self) -> Option<&[u8]> {
        match self {
            SqlValue::Text(bytes) => Some(bytes),
            _ => None,
        }
    }
}

/// One row of one table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Table name without backticks.
    pub table: String,
    /// Column values in table order.
    pub values: Vec<SqlValue>,
}

/// All rows inserted by a dump, in file order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dump {
    /// Rows in file order.
    pub rows: Vec<Row>,
}

impl Dump {
    /// Parses the `INSERT INTO` statements of a `mysqldump` file and ignores
    /// everything else.
    pub fn parse(sql: &[u8]) -> Result<Dump, String> {
        const PREFIX: &[u8] = b"INSERT INTO `";
        let mut rows = Vec::new();
        for line in sql.split(|&b| b == b'\n') {
            let Some(rest) = line.strip_prefix(PREFIX) else {
                continue;
            };
            let name_len = rest
                .iter()
                .position(|&b| b == b'`')
                .ok_or("unterminated table name")?;
            let (name, rest) = rest.split_at(name_len);
            let table = String::from_utf8(name.to_vec()).map_err(|_| "table name is not UTF-8")?;
            let mut rest = rest.strip_prefix(b"` VALUES ").ok_or("expected VALUES")?;
            loop {
                let (values, after) = tuple(rest)?;
                rows.push(Row {
                    table: table.clone(),
                    values,
                });
                match after {
                    [b',', tail @ ..] => rest = tail,
                    [b';'] => break,
                    _ => return Err("expected ',' or ';' after a row".into()),
                }
            }
        }
        Ok(Dump { rows })
    }

    /// Values of every row in `table`.
    pub fn table<'a>(&'a self, table: &'a str) -> impl Iterator<Item = &'a [SqlValue]> + 'a {
        self.rows
            .iter()
            .filter(move |row| row.table == table)
            .map(|row| row.values.as_slice())
    }
}

/// Parses `(v, v, ...)` and returns the values and what follows.
fn tuple(input: &[u8]) -> Result<(Vec<SqlValue>, &[u8]), String> {
    let mut rest = input.strip_prefix(b"(").ok_or("expected '('")?;
    let mut values = Vec::new();
    loop {
        let (value, after) = value(rest)?;
        values.push(value);
        match after {
            [b',', tail @ ..] => rest = tail,
            [b')', tail @ ..] => return Ok((values, tail)),
            _ => return Err("expected ',' or ')' in a row".into()),
        }
    }
}

fn value(input: &[u8]) -> Result<(SqlValue, &[u8]), String> {
    if let Some(rest) = input.strip_prefix(b"NULL") {
        return Ok((SqlValue::Null, rest));
    }
    let Some(mut rest) = input.strip_prefix(b"'") else {
        let len = input
            .iter()
            .take_while(|b| b.is_ascii_digit() || matches!(b, b'-' | b'.' | b'e' | b'E' | b'+'))
            .count();
        if len == 0 {
            return Err("expected a value".into());
        }
        let (number, rest) = input.split_at(len);
        // Only ASCII digits and signs were taken.
        let number = String::from_utf8_lossy(number).into_owned();
        return Ok((SqlValue::Number(number), rest));
    };
    let mut text = Vec::new();
    loop {
        match rest {
            [b'\'', b'\'', tail @ ..] => {
                text.push(b'\'');
                rest = tail;
            }
            [b'\'', tail @ ..] => return Ok((SqlValue::Text(text), tail)),
            [b'\\', escaped, tail @ ..] => {
                text.push(match escaped {
                    b'0' => b'\0',
                    b'n' => b'\n',
                    b'r' => b'\r',
                    b't' => b'\t',
                    b'b' => 0x08,
                    b'Z' => 0x1A,
                    other => *other,
                });
                rest = tail;
            }
            [byte, tail @ ..] => {
                text.push(*byte);
                rest = tail;
            }
            [] => return Err("unterminated string".into()),
        }
    }
}

/// An admin `page` with what differs by chance in PHP's settled:
/// `antispambot()`'s random references in `mailto:` links decoded, and the
/// time a comments list was made masked as `%pagegen%`.
pub fn settled_page(page: &str) -> String {
    let mut out = String::new();
    let mut rest = page;
    while let Some(at) = rest.find("mailto:") {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        let end = tail.find("</a>").unwrap_or(tail.len());
        let (link, tail) = tail.split_at(end);
        out.push_str(&decoded(link));
        rest = tail;
    }
    out.push_str(rest);
    match out.split_once("name=\"pagegen_timestamp\" value=\"") {
        Some((head, tail)) => {
            let tail = tail.split_once('"').map_or("", |(_, tail)| tail);
            format!("{head}name=\"pagegen_timestamp\" value=\"%pagegen%\"{tail}")
        }
        None => out,
    }
}

/// `text` with its decimal character references as the characters.
fn decoded(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some((head, tail)) = rest.split_once("&#") {
        out.push_str(head);
        let digits: String = tail.chars().take_while(char::is_ascii_digit).collect();
        let after = tail.strip_prefix(digits.as_str()).unwrap_or(tail);
        let code = digits.parse::<u8>().ok().filter(u8::is_ascii);
        match (after.strip_prefix(';'), code) {
            (Some(after), Some(code)) => {
                out.push(char::from(code));
                rest = after;
            }
            _ => {
                out.push_str("&#");
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// `page` with each `wp-admin` path after `from` put under `to`, without
/// `.php` before the first of `stops`.
fn admin_paths(page: &str, from: &str, to: &str, stops: &[char]) -> String {
    let mut out = String::new();
    let mut rest = page;
    while let Some(at) = rest.find(from) {
        let (head, tail) = rest.split_at(at);
        out.push_str(head);
        out.push_str(to);
        let tail = tail.get(from.len()..).unwrap_or_default();
        let end = tail.find(stops).unwrap_or(tail.len());
        let (file, tail) = tail.split_at(end);
        out.push_str(file.strip_suffix(".php").unwrap_or(file));
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// The PHP oracle's page under hs-admin's URLs, without the update notice
/// hs-admin leaves out until an open question is decided.
pub fn hs_admin_page(page: &str) -> String {
    let nag = "<div class=\"notice notice-warning update-nag inline\">";
    let page = match page.split_once(nag) {
        Some((head, tail)) => format!(
            "{head}{}",
            tail.split_once("</div>").map_or("", |(_, rest)| rest)
        ),
        None => page.to_string(),
    };
    // Paths plain and URL-encoded, as a `wp_http_referer` carries them.
    let page = admin_paths(&page, "/wp-admin/", "/hs-admin/", &['\'', '?', '"']);
    let encoded = admin_paths(
        &page,
        "%2Fwp-admin%2F",
        "%2Fhs-admin%2F",
        &['%', '\'', '"', '&'],
    );
    let mut page = encoded.replace("/wp-login.php", "/hs-login");
    for prefix in ["href='", "href=\"", "\"url\":\""] {
        let mut out = String::new();
        let mut rest = page.as_str();
        while let Some(at) = rest.find(prefix) {
            let (head, tail) = rest.split_at(at + prefix.len());
            out.push_str(head);
            let end = tail.find(['\'', '?', '"']).unwrap_or(tail.len());
            let (file, tail) = tail.split_at(end);
            let local = file.strip_suffix(".php").filter(|f| !f.contains('/'));
            out.push_str(local.unwrap_or(file));
            rest = tail;
        }
        out.push_str(rest);
        page = out;
    }
    page
}

/// Path of a file or folder under the repository's `fixtures/` directory.
pub fn fixture_path(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(relative)
}

/// WordPress versions that have a default install fixture, sorted.
pub fn default_install_versions() -> Vec<String> {
    let mut versions: Vec<String> = std::fs::read_dir(fixture_path("default-install"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().join("database.sql").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    versions.sort();
    versions
}

/// The parsed default install dump for one WordPress version.
pub fn default_install(version: &str) -> Result<Dump, String> {
    let path = fixture_path(&format!("default-install/{version}/database.sql"));
    let sql = std::fs::read(&path).map_err(|err| format!("{}: {err}", path.display()))?;
    Dump::parse(&sql)
}

/// The default install fixture database that
/// `reference/default-install/db.sh up` starts. `HYSCRIBE_TEST_DB_HOST` and
/// `HYSCRIBE_TEST_DB_PORT` move it.
pub fn fixture_database() -> hyscribe_config::Database {
    let env = |key: &str, default: &str| std::env::var(key).unwrap_or_else(|_| default.to_string());
    hyscribe_config::Database {
        host: env("HYSCRIBE_TEST_DB_HOST", "127.0.0.1"),
        port: env("HYSCRIBE_TEST_DB_PORT", "33061")
            .parse()
            .unwrap_or(33061),
        name: "wordpress".into(),
        user: "wordpress".into(),
        password: hyscribe_config::Secret::new("reference-only"),
        prefix: hyscribe_config::Prefix::default(),
        charset: "utf8mb4".into(),
    }
}

/// The bytes a golden file's hex field spells. Golden files hex encode
/// their fields so any bytes survive a TSV line; a malformed pair is
/// skipped, which the comparison against PHP's output then reports.
pub fn unhex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

/// A request `reference/rest/golden.sh` recorded and PHP's answer.
pub struct RestCase {
    /// `visitor` or the role of the user who asked.
    pub who: String,
    /// The method.
    pub method: String,
    /// The path and query.
    pub target: String,
    /// The flags, comma separated.
    pub flags: String,
    /// The status code.
    pub status: u16,
    /// The headers kept, one `name: value` a line.
    pub headers: Vec<String>,
    /// The body, or for the Theme Test Data fixture its SHA-256 in hex.
    pub body: Vec<u8>,
}

impl RestCase {
    /// Whether `flags` has `name`.
    pub fn flag(&self, name: &str) -> bool {
        self.flags.split(',').any(|flag| flag == name)
    }
}

/// The cases of a golden file of `reference/rest/golden.sh`.
pub fn rest_cases(golden: &str) -> Vec<RestCase> {
    golden
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('\t').collect();
            let field = |i: usize| fields.get(i).copied().unwrap_or_default();
            let text = |i: usize| String::from_utf8_lossy(&unhex(field(i))).into_owned();
            RestCase {
                who: field(0).into(),
                method: field(1).into(),
                target: text(2),
                flags: field(3).into(),
                status: field(4).parse().unwrap_or_default(),
                headers: text(5).lines().map(String::from).collect(),
                body: unhex(field(6)),
            }
        })
        .collect()
}

/// The headers a REST golden keeps, as it writes them, sorted.
pub fn rest_headers(headers: &[(String, String)]) -> Vec<String> {
    let kept = |name: &str| {
        [
            "content-type",
            "x-robots-tag",
            "link",
            "allow",
            "vary",
            "expires",
            "cache-control",
        ]
        .contains(&name)
            || name.starts_with("access-control-")
            || name.starts_with("x-wp-")
    };
    let mut headers: Vec<String> = headers
        .iter()
        .map(|(name, value)| (name.to_ascii_lowercase(), value))
        .filter(|(name, _)| kept(name))
        .map(|(name, value)| format!("{name}: {value}"))
        .collect();
    headers.sort();
    headers
}

/// What a test prints when the fixture database is not running.
pub const NO_FIXTURE_DATABASE: &str = "no fixture database: run reference/default-install/db.sh up";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn php_pages_under_hs_admin_urls() {
        assert_eq!(
            hs_admin_page(
                "<div class=\"notice notice-warning update-nag inline\">n</div><a href='edit.php?x'>\
                 <a href=\"https://w.org/a.php\">'/wp-admin/css/a.css' '/wp-admin/load-styles.php?c' /wp-login.php \"url\":\"index.php\"/wp-admin/"
            ),
            "<a href='edit?x'><a href=\"https://w.org/a.php\">'/hs-admin/css/a.css' '/hs-admin/load-styles?c' /hs-login \"url\":\"index\"/hs-admin/"
        );
        assert_eq!(
            hs_admin_page("<div class=\"notice notice-warning update-nag inline\">"),
            ""
        );
        assert_eq!(hs_admin_page("plain"), "plain");
        let page = "x <a href=\"mailto:a&#064;b\">a&#64;b&#9999;&#x41;&#</a> value=\"pagegen_timestamp\" value=\"1\" y";
        assert_eq!(
            settled_page(&page.replace("value=\"pagegen", "name=\"pagegen")),
            "x <a href=\"mailto:a@b\">a@b&#9999;&#x41;&#</a> name=\"pagegen_timestamp\" value=\"%pagegen%\" y"
        );
        assert_eq!(settled_page("no links"), "no links");
        assert_eq!(
            hs_admin_page("r=%2Fwp-admin%2Fedit-tags.php%3Ft r=%2Fwp-admin%2Fx.php"),
            "r=%2Fhs-admin%2Fedit-tags%3Ft r=%2Fhs-admin%2Fx"
        );
    }

    #[test]
    fn unhex_reads_pairs_and_skips_malformed_ones() {
        assert_eq!(unhex("00ff7A"), vec![0, 255, 122]);
        assert_eq!(unhex("zz41"), b"A");
        assert_eq!(unhex("\u{e9}41"), b"A");
        assert_eq!(unhex("a\u{e9}"), b"");
        assert_eq!(unhex(""), b"");
    }

    #[test]
    fn parses_rows_and_escapes() {
        let sql = b"-- comment\nCREATE TABLE `t` (x int);\n\
INSERT INTO `t` VALUES (1,'a\\'b''c\\\\\\0\\n\\r\\t\\b\\Z\\\"',NULL,-2.5e+3),(2,'',NULL,0);\n\
INSERT INTO `u` VALUES (3,'x');\n";
        let dump = Dump::parse(sql).unwrap();
        let t: Vec<_> = dump.table("t").collect();
        assert_eq!(t.len(), 2);
        assert_eq!(
            t[0],
            &[
                SqlValue::Number("1".into()),
                SqlValue::Text(b"a'b'c\\\0\n\r\t\x08\x1A\"".to_vec()),
                SqlValue::Null,
                SqlValue::Number("-2.5e+3".into()),
            ][..]
        );
        assert_eq!(t[1][1].as_text(), Some(&b""[..]));
        assert_eq!(t[1][0].as_text(), None);
        assert_eq!(dump.table("u").count(), 1);
        assert_eq!(dump.table("missing").count(), 0);
    }

    #[test]
    fn reports_malformed_statements() {
        let cases: &[(&[u8], &str)] = &[
            (b"INSERT INTO `t", "unterminated table name"),
            (b"INSERT INTO `\xff` VALUES (1);", "table name is not UTF-8"),
            (b"INSERT INTO `t` (1);", "expected VALUES"),
            (b"INSERT INTO `t` VALUES 1;", "expected '('"),
            (
                b"INSERT INTO `t` VALUES (1);x",
                "expected ',' or ';' after a row",
            ),
            (
                b"INSERT INTO `t` VALUES (1 2);",
                "expected ',' or ')' in a row",
            ),
            (b"INSERT INTO `t` VALUES (x);", "expected a value"),
            (b"INSERT INTO `t` VALUES ('abc", "unterminated string"),
        ];
        for (sql, message) in cases {
            assert_eq!(Dump::parse(sql).unwrap_err(), *message);
        }
    }

    #[test]
    fn finds_default_install_fixtures() {
        let versions = default_install_versions();
        assert!(
            versions.iter().any(|v| v.starts_with("7.0.")),
            "{versions:?}"
        );
        for version in &versions {
            let dump = default_install(version).unwrap();
            assert!(dump.table("wp_options").count() > 100);
        }
        assert!(default_install("0.0.0").unwrap_err().contains("0.0.0"));
    }

    #[test]
    fn fixture_database_matches_db_sh() {
        let db = fixture_database();
        assert_eq!(
            (db.name.as_str(), db.user.as_str()),
            ("wordpress", "wordpress")
        );
        assert_eq!(db.password.expose(), "reference-only");
        assert_eq!(db.prefix.as_str(), "wp_");
        assert!(NO_FIXTURE_DATABASE.contains("db.sh up"));
    }
}
