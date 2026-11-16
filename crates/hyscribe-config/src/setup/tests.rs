// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::wpconfig::parse;
use std::collections::HashMap;
use std::os::unix::fs::PermissionsExt;

fn php(body: &str) -> WpConfig {
    parse(format!("<?php\n{body}").as_bytes()).unwrap()
}

fn pairs(values: &[(&'static str, &str)]) -> Vec<(&'static str, String)> {
    values.iter().map(|(k, v)| (*k, v.to_string())).collect()
}

#[test]
fn the_wordpress_sample_gives_its_database_details() {
    let config = parse(include_bytes!("../../tests/corpus/wp-config/sample.php")).unwrap();
    assert_eq!(
        import(&config),
        Import {
            values: pairs(&[
                ("DB_NAME", "database_name_here"),
                ("DB_USER", "username_here"),
                ("DB_PASSWORD", "password_here"),
                ("DB_HOST", "localhost"),
                ("DB_CHARSET", "utf8mb4"),
                ("DB_PREFIX", "wp_"),
            ]),
            // The salts are the sample's placeholder, so none is copied.
            unread: Vec::new(),
        }
    );
}

#[test]
fn literals_are_copied() {
    let config = php(r#"
        define('DB_NAME', 'site');
        define('DB_USER', 'owner');
        define('DB_PASSWORD', 'pa$$"word');
        define('DB_HOST', 'db.example:3307');
        define('DB_CHARSET', 'utf8');
        define('AUTH_KEY', 'k1');
        define('NONCE_SALT', 's8');
        define('WP_HOME', 'https://example.com');
        define('WP_SITEURL', 'https://example.com/wp');
        define('DISALLOW_UNFILTERED_HTML', true);
        define('ALLOW_UNFILTERED_UPLOADS', false);
        define('MULTISITE', TRUE);
        $table_prefix = 'site_';
    "#);
    assert_eq!(
        import(&config),
        Import {
            values: pairs(&[
                ("DB_NAME", "site"),
                ("DB_USER", "owner"),
                ("DB_PASSWORD", "pa$$\"word"),
                ("DB_HOST", "db.example"),
                ("DB_PORT", "3307"),
                // `wpdb::determine_charset()`'s upgrade.
                ("DB_CHARSET", "utf8mb4"),
                ("DB_PREFIX", "site_"),
                ("AUTH_KEY", "k1"),
                ("NONCE_SALT", "s8"),
                ("WP_HOME", "https://example.com"),
                ("WP_SITEURL", "https://example.com/wp"),
                ("DISALLOW_UNFILTERED_HTML", "true"),
                ("ALLOW_UNFILTERED_UPLOADS", "false"),
                ("MULTISITE", "true"),
            ]),
            unread: Vec::new(),
        }
    );
}

#[test]
fn what_only_php_can_tell_is_left_to_ask() {
    let config = php(r#"
        define('DB_NAME', getenv('DB_NAME'));
        define('DB_PASSWORD', 42);
        define('DB_HOST', 'localhost:/run/mysqld/mysqld.sock');
        define('DB_CHARSET', getenv('CHARSET'));
        define('AUTH_KEY', getenv('AUTH_KEY'));
        define('SECURE_AUTH_KEY', '');
        if (true) { define('WP_HOME', 'https://example.com'); }
        define('WP_SITEURL', 'https://' . $_SERVER['HTTP_HOST']);
        define('DISALLOW_UNFILTERED_HTML', 1);
        defined('MULTISITE') || define('MULTISITE', false);
        define('ALLOW_UNFILTERED_UPLOADS', getenv('UPLOADS') === 'on');
        $table_prefix = getenv('PREFIX');
    "#);
    assert_eq!(
        import(&config),
        Import {
            values: Vec::new(),
            unread: vec![
                "DB_NAME",
                "DB_USER",
                "DB_PASSWORD",
                "DB_HOST",
                "DB_CHARSET",
                "DB_PREFIX",
                "WP_HOME",
                "WP_SITEURL",
                "DISALLOW_UNFILTERED_HTML",
                "ALLOW_UNFILTERED_UPLOADS",
                "MULTISITE",
            ],
        }
    );
}

#[test]
fn db_host_is_split_as_wordpress_splits_it() {
    let host = |value: &str| {
        let config = php(&format!("define('DB_HOST', '{value}');"));
        let import = import(&config);
        let found: Vec<_> = import
            .values
            .into_iter()
            .filter(|(key, _)| key.starts_with("DB_"))
            .collect();
        (found, import.unread.contains(&"DB_HOST"))
    };
    assert_eq!(
        host(":3307"),
        (
            pairs(&[("DB_HOST", "localhost"), ("DB_PORT", "3307")]),
            false
        )
    );
    assert_eq!(
        host("[::1]:3306"),
        (pairs(&[("DB_HOST", "::1"), ("DB_PORT", "3306")]), false)
    );
    assert_eq!(
        host("db:65535").0,
        pairs(&[("DB_HOST", "db"), ("DB_PORT", "65535")])
    );
    assert_eq!(host("db:65536"), (Vec::new(), true));
    assert_eq!(host("db:0"), (pairs(&[("DB_HOST", "db")]), false));
    assert_eq!(host("db:00"), (Vec::new(), true));
    assert_eq!(host("[]::1"), (Vec::new(), true));
}

#[test]
fn an_empty_charset_is_left_to_the_default() {
    let config = php("define('DB_CHARSET', '');");
    assert!(import(&config).values.is_empty());
    let config = php("define('DB_CHARSET', 'latin1');");
    assert_eq!(import(&config).values, pairs(&[("DB_CHARSET", "latin1")]));
}

#[test]
fn setting_tells_literals_from_php() {
    let config = php("define('A', 1); if (x) { define('B', 2); } define('C', f());");
    assert_eq!(setting(&config, "A"), Setting::Literal(&Value::Int(1)));
    assert_eq!(setting(&config, "B"), Setting::Unreadable);
    assert_eq!(setting(&config, "C"), Setting::Unreadable);
    assert_eq!(setting(&config, "D"), Setting::Unset);
}

#[test]
fn values_are_quoted_and_escaped() {
    let text = render(&pairs(&[("A", "plain"), ("B", "a\\b\"c$d\ne'f #g")])).unwrap();
    assert_eq!(
        text,
        "# Written by hyscribe setup. .env.example documents every key.\n\
         A=\"plain\"\nB=\"a\\\\b\\\"c\\$d\\ne'f #g\"\n"
    );
    assert_eq!(render(&pairs(&[("A", "a"), ("B", "\r")])), None);
}

/// The values `.env` gives back when `values` are rendered and read.
fn read_back(values: &[(&'static str, String)]) -> HashMap<String, String> {
    let text = render(values).unwrap();
    dotenvy::from_read_iter(text.as_bytes())
        .map(Result::unwrap)
        .collect()
}

proptest::proptest! {
    #[test]
    fn any_value_without_a_carriage_return_reads_back(value in "[^\r]*") {
        let values = vec![("A", value.clone()), ("B", "after".to_string())];
        let read = read_back(&values);
        proptest::prop_assert_eq!(read.get("A"), Some(&value));
        proptest::prop_assert_eq!(read.get("B").map(String::as_str), Some("after"));
    }
}

fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[test]
fn env_is_written_owner_only_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let values = pairs(&[
        ("DB_NAME", "site"),
        ("DB_USER", "owner"),
        ("DB_PASSWORD", "p\"$'\\#w"),
        ("AUTH_KEY", "k$y"),
        ("MODE", "primary"),
        ("DISALLOW_UNFILTERED_HTML", "false"),
    ]);
    let path = write(dir.path(), &render(&values).unwrap(), false).unwrap();
    assert_eq!(path, dir.path().join(".env"));
    assert_eq!(mode(&path), 0o600);
    let loaded = crate::load(dir.path(), &HashMap::new()).unwrap().unwrap();
    let map = values
        .iter()
        .map(|(k, v)| (k.to_string(), v.clone()))
        .collect();
    assert_eq!(loaded, crate::from_map(&map).unwrap());
    assert_eq!(loaded.database.password.expose(), "p\"$'\\#w");
    assert!(!loaded.constants.disallow_unfiltered_html);
    // Only the new file remains.
    let names: Vec<_> = std::fs::read_dir(dir.path()).unwrap().collect();
    assert_eq!(names.len(), 1);
}

#[test]
fn an_existing_env_is_replaced_only_when_asked() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(".env");
    std::fs::write(&path, "OLD=1\n").unwrap();
    let err = write(dir.path(), "NEW=1\n", false).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "OLD=1\n");
    assert!(!dir.path().join(".env.new").exists());
    // A file left by an interrupted run is replaced.
    std::fs::write(dir.path().join(".env.new"), "STALE=1\n").unwrap();
    write(dir.path(), "NEW=1\n", true).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "NEW=1\n");
    assert_eq!(mode(&path), 0o600);
    assert!(!dir.path().join(".env.new").exists());
}

#[test]
fn write_failures_are_reported() {
    let dir = tempfile::tempdir().unwrap();
    // `.env.new` cannot be removed while it is a folder with files.
    std::fs::create_dir_all(dir.path().join(".env.new/x")).unwrap();
    assert!(write(dir.path(), "A=1\n", false).is_err());
    assert!(!dir.path().join(".env").exists());
    let missing = dir.path().join("missing");
    let err = write(&missing, "A=1\n", false).unwrap_err();
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
}
