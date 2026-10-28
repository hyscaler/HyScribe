// SPDX-License-Identifier: AGPL-3.0-or-later

//! Shared test helpers: fixture locations and a reader for the `mysqldump`
//! files under `fixtures/`, so tests can check real WordPress rows without a
//! running database.

use std::path::PathBuf;

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

/// What a test prints when the fixture database is not running.
pub const NO_FIXTURE_DATABASE: &str = "no fixture database: run reference/default-install/db.sh up";

#[cfg(test)]
mod tests {
    use super::*;

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
