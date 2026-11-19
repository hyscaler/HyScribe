// SPDX-License-Identifier: AGPL-3.0-or-later

//! The install against what PHP's `wp_install()` wrote
//! (`reference/install/golden.sh`): the tables of `wordpress_saves`, the
//! scratch database, one test at a time, compared with `SHOW CREATE TABLE`
//! and every row, with the times at [`NOW`].

use super::*;
use crate::tests::SAVES;
use hyscribe_config::Prefix;
use sqlx::{Connection as _, MySqlConnection, Row as _};

/// 2023-11-14 22:13:20 UTC.
const NOW: i64 = 1_700_000_000;
const NOW_TEXT: &str = "2023-11-14 22:13:20";

/// A case of `reference/install/cases.list`.
struct Case {
    name: &'static str,
    title: &'static str,
    login: &'static str,
    email: &'static str,
    public: bool,
    password: &'static str,
    url: &'static str,
    prefix: &'static str,
}

fn cases() -> Vec<Case> {
    include_str!("../../../../reference/install/cases.list")
        .lines()
        .map(|line| {
            let fields: Vec<&str> = line.split('|').collect();
            Case {
                name: fields[0],
                title: fields[1],
                login: fields[2],
                email: fields[3],
                public: fields[4] == "1",
                password: fields[5],
                url: fields[6],
                prefix: fields[7],
            }
        })
        .collect()
}

/// The rows and the tables PHP wrote for `case`.
fn golden(case: &Case) -> (&'static str, &'static str) {
    match case.name {
        "default" => (ROWS, SCHEMA),
        "quoted" => (
            include_str!("../../tests/golden/install/quoted.tsv"),
            include_str!("../../tests/golden/install/quoted.sql"),
        ),
        "unicode" => (
            include_str!("../../tests/golden/install/unicode.tsv"),
            include_str!("../../tests/golden/install/unicode.sql"),
        ),
        "untitled" => (
            include_str!("../../tests/golden/install/untitled.tsv"),
            include_str!("../../tests/golden/install/untitled.sql"),
        ),
        "prefixed" => (
            include_str!("../../tests/golden/install/prefixed.tsv"),
            include_str!("../../tests/golden/install/prefixed.sql"),
        ),
        other => panic!("no golden for {other}"),
    }
}

fn answers(case: &Case) -> Answers<'static> {
    Answers {
        title: case.title,
        login: case.login,
        password: case.password,
        repeat: case.password,
        email: case.email,
        public: case.public,
        home: case.url,
    }
}

/// A table of a golden: its name, columns and rows.
type Table = (String, Vec<String>, Vec<Vec<Option<String>>>);

/// The golden's tables with its masks filled in as the case says (the
/// default case's, which has them; the password stays a mask).
fn parse(case: &Case) -> Vec<Table> {
    let fill = |value: &str| {
        let mut value = value
            .replace("%now%", NOW_TEXT)
            .replace("%unix%", &NOW.to_string())
            .replace("%home%", case.url)
            .replace("%title%", case.title)
            .replace("%email%", case.email)
            .replace("%login%", case.login)
            .replace("%nicename%", &case.login.to_lowercase())
            .replace("%public%", if case.public { "1" } else { "0" })
            .replace("%prefix%", case.prefix);
        for seconds in [3600, 5400, 7200, 15_552_000] {
            value = value.replace(&format!("%unix+{seconds}%"), &(NOW + seconds).to_string());
        }
        assert!(!value.contains("%unix+"), "{value}");
        value
    };
    let unescape = |value: &str| {
        if value == "\\N" {
            return None;
        }
        let mut out = String::new();
        let mut chars = value.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('t') => out.push('\t'),
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                other => out.extend(other),
            }
        }
        Some(fill(&out))
    };
    let mut tables: Vec<Table> = Vec::new();
    for line in golden(case).0.lines().filter(|line| !line.starts_with('#')) {
        if let Some(header) = line.strip_prefix('@') {
            let mut names = header.split('\t').map(String::from);
            let table = names.next().unwrap();
            tables.push((table, names.collect(), Vec::new()));
        } else {
            let row = line.split('\t').map(unescape).collect();
            tables.last_mut().unwrap().2.push(row);
        }
    }
    tables
}

fn config(prefix: &str) -> Database {
    Database {
        name: "wordpress_saves".into(),
        prefix: Prefix::new(prefix).unwrap(),
        ..hyscribe_testkit::fixture_database()
    }
}

async fn connect(config: &Database) -> MySqlConnection {
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&config.host)
        .port(config.port)
        .database(&config.name)
        .username(&config.user)
        .password(config.password.expose())
        .charset(&config.charset);
    MySqlConnection::connect_with(&options).await.unwrap()
}

/// Every table of the scratch database dropped.
async fn emptied(config: &Database) -> MySqlConnection {
    let mut conn = connect(config).await;
    let names = sqlx::raw_sql(
        "SELECT TABLE_NAME AS name FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE()",
    )
    .fetch_all(&mut conn)
    .await
    .unwrap();
    for name in names {
        let name = String::from_utf8(name.get::<Vec<u8>, _>("name")).unwrap();
        let drop = format!("DROP TABLE {name}");
        sqlx::raw_sql(sqlx::AssertSqlSafe(drop))
            .execute(&mut conn)
            .await
            .unwrap();
    }
    conn
}

/// The text of the first column of each row of `sql`.
async fn text_of(conn: &mut MySqlConnection, sql: String, column: usize) -> Vec<Option<String>> {
    let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
        .fetch_all(conn)
        .await
        .unwrap();
    rows.iter()
        .map(|row| {
            let value = row.try_get_unchecked::<Option<Vec<u8>>, _>(column).unwrap();
            value.map(|bytes| String::from_utf8(bytes).unwrap())
        })
        .collect()
}

/// What the install left, in the golden's tables, with the hash masked.
async fn snapshot(conn: &mut MySqlConnection, prefix: &str, golden: &[Table]) -> Vec<Table> {
    let mut tables = Vec::new();
    for (table, columns, _) in golden {
        let selected: Vec<String> = columns
            .iter()
            .map(|column| format!("CAST({column} AS CHAR)"))
            .collect();
        let sql = format!(
            "SELECT {} FROM {prefix}{table} ORDER BY {}, {}",
            selected.join(", "),
            columns[0],
            columns[1]
        );
        let rows = sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .fetch_all(&mut *conn)
            .await
            .unwrap();
        let rows = rows
            .iter()
            .map(|row| {
                (0..columns.len())
                    .map(|i| {
                        let value = row.try_get_unchecked::<Option<Vec<u8>>, _>(i).unwrap();
                        let value = value.map(|bytes| String::from_utf8(bytes).unwrap());
                        let hash = |v: &String| v.len() == 63 && v.starts_with("$wp$2y$12$");
                        value.map(|v| if hash(&v) { "%password%".into() } else { v })
                    })
                    .collect()
            })
            .collect();
        tables.push((table.clone(), columns.clone(), rows));
    }
    tables
}

/// PHP's install, and HyScribe's, leave the same tables and rows.
#[tokio::test]
async fn the_install_leaves_what_php_s_does() {
    let _one = SAVES.lock().await;
    for case in cases() {
        let config = config(case.prefix);
        let mut conn = emptied(&config).await;
        let plan = Plan::new(&answers(&case), NOW).unwrap();
        let installed = plan.install(&config).await.unwrap();
        assert_eq!(installed.tables.len(), 12, "{}", case.name);
        assert_eq!(installed.generated.is_some(), case.password.is_empty());
        let expected = parse(&case);
        let rows = snapshot(&mut conn, case.prefix, &expected).await;
        for (found, wanted) in rows.iter().zip(&expected) {
            assert_eq!(found, wanted, "{}: {}", case.name, wanted.0);
        }
        // The same tables, as `SHOW CREATE TABLE` prints them.
        for statement in golden(&case).1.split(";\n").filter(|s| !s.is_empty()) {
            let table = statement.split('`').nth(1).unwrap();
            let table = table.replace("{prefix}", case.prefix);
            let shown = text_of(&mut conn, format!("SHOW CREATE TABLE {table}"), 1).await;
            let wanted = statement.replace("{prefix}", case.prefix);
            assert_eq!(shown, [Some(wanted)], "{}: {table}", case.name);
        }
        // The password works, the chosen one or the generated one.
        let hash = format!("SELECT user_pass FROM {}users", case.prefix);
        let hash = text_of(&mut conn, hash, 0).await.remove(0).unwrap();
        let password = installed
            .generated
            .as_deref()
            .unwrap_or(case.password.trim());
        assert!(hyscribe_auth::check_password(
            password.as_bytes(),
            hash.as_bytes()
        ));
        if case.password.is_empty() {
            assert_eq!(password.len(), 12);
            let shown = format!("{installed:?}");
            assert!(!shown.contains(password), "{shown}");
            assert!(shown.contains("<password>"), "{shown}");
        }
    }
    emptied(&config("wp_")).await;
}

/// A database that has the prefix's tables is not installed over.
#[tokio::test]
async fn an_existing_site_is_left_alone() {
    let _one = SAVES.lock().await;
    let case = &cases()[0];
    let config = config(case.prefix);
    let mut conn = emptied(&config).await;
    let plan = Plan::new(&answers(case), NOW).unwrap();
    plan.install(&config).await.unwrap();
    let before = format!("SELECT option_value FROM {}options", case.prefix);
    let before = text_of(&mut conn, before, 0).await;
    let refused = String::from(plan.install(&config).await.unwrap_err());
    assert!(refused.contains("already has tables with the prefix wp_: wp_commentmeta, "));
    assert!(refused.contains("wp_users"), "{refused}");
    let after = format!("SELECT option_value FROM {}options", case.prefix);
    assert_eq!(text_of(&mut conn, after, 0).await, before);
    emptied(&config).await;
}

/// A site made can be taken out again, as when a later step fails.
#[tokio::test]
async fn an_install_can_be_removed() {
    let _one = SAVES.lock().await;
    let case = &cases()[0];
    let config = config(case.prefix);
    let mut conn = emptied(&config).await;
    let plan = Plan::new(&answers(case), NOW).unwrap();
    let installed = plan.install(&config).await.unwrap();
    Plan::remove(&config, &installed).await.unwrap();
    let left = "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE()";
    assert!(text_of(&mut conn, left.into(), 0).await.is_empty());
    let unreachable = Database {
        port: 1,
        ..config.clone()
    };
    assert!(Plan::remove(&unreachable, &installed).await.is_err());
}

/// An install that stops halfway drops the tables it made.
#[tokio::test]
async fn a_failed_install_drops_its_tables() {
    let _one = SAVES.lock().await;
    // 47 characters: `term_relationships` is the first name over 64.
    let prefix = format!("{}_", "p".repeat(46));
    let config = config(&prefix);
    let mut conn = emptied(&config).await;
    let plan = Plan::new(&answers(&cases()[0]), NOW).unwrap();
    let stopped = String::from(plan.install(&config).await.unwrap_err());
    assert!(
        stopped.starts_with("the install stopped and its tables were dropped: "),
        "{stopped}"
    );
    let left = "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE()";
    assert!(text_of(&mut conn, left.into(), 0).await.is_empty());
}

#[tokio::test]
async fn an_unreachable_database_is_reported() {
    let unreachable = Database {
        port: 1,
        ..config("wp_")
    };
    let plan = Plan::new(&answers(&cases()[0]), NOW).unwrap();
    let err = plan.install(&unreachable).await.unwrap_err();
    assert!(!err.to_string().is_empty());
}

#[tokio::test]
async fn failures_say_what_stopped() {
    let unreachable = Database {
        port: 1,
        ..config("wp_")
    };
    let Err(db) = Installer::open(&unreachable).await else {
        panic!("port 1 answered");
    };
    let dropped = stopped(&db, Ok(())).to_string();
    assert_eq!(
        dropped,
        format!("the install stopped and its tables were dropped: {db}")
    );
    let kept = stopped(&db, Err(db.clone())).to_string();
    assert_eq!(
        kept,
        format!("the install stopped: {db}; its tables were not all dropped: {db}")
    );
    let random = Failure::from(std::io::Error::other("no entropy"));
    assert_eq!(
        random.to_string(),
        "the kernel's random source failed: no entropy"
    );
}

fn plan(change: impl FnOnce(&mut Answers<'static>)) -> Result<Plan, String> {
    let mut given = answers(&cases()[0]);
    change(&mut given);
    Plan::new(&given, NOW)
}

#[test]
fn the_answers_are_checked_as_php_checks_them() {
    let refusal = |change: &dyn Fn(&mut Answers<'static>)| plan(change).unwrap_err();
    assert_eq!(
        refusal(&|a| a.login = " \t"),
        "Please provide a valid username."
    );
    assert_eq!(
        refusal(&|a| a.login = "bad/name"),
        "The username you provided has invalid characters."
    );
    assert_eq!(
        refusal(&|a| a.repeat = "other"),
        "Your passwords do not match. Please try again."
    );
    assert_eq!(
        refusal(&|a| a.email = ""),
        "You must provide an email address."
    );
    let invalid = "Sorry, that is not a valid email address. Email addresses look like \
                   username@example.com.";
    assert_eq!(refusal(&|a| a.email = "nobody"), invalid);
    let long: &'static str = "a".repeat(61).leak();
    let too_long = refusal(&|a| a.login = long);
    assert_eq!(too_long, "Username may not be longer than 60 characters.");
    assert_eq!(
        refusal(&|a| a.login = "@@@"),
        "Cannot create a user with an empty nicename."
    );
    assert!(refusal(&|a| a.home = "ftp://example.com").starts_with("The site address must be"));
    // The login is trimmed, as install.php trims it, and a long one is cut
    // to 50 characters for its slug.
    let made = plan(|a| a.login = "  Jane Doe  ").unwrap();
    assert_eq!(
        (made.login.as_str(), made.nicename.as_str()),
        ("Jane Doe", "jane-doe")
    );
    let sixty: &'static str = "a".repeat(60).leak();
    let made = plan(|a| a.login = sixty).unwrap();
    assert_eq!(made.nicename, "a".repeat(50));
    // What a plan prints holds no password.
    let kept = plan(|a| {
        a.password = "s3cret";
        a.repeat = "s3cret";
    });
    let shown = format!("{:?}", kept.unwrap());
    assert!(!shown.contains("s3cret"), "{shown}");
    assert!(shown.contains("sentinelLogin"), "{shown}");
    // A space around the password is not part of it; the title is escaped.
    let made = plan(|a| {
        a.password = " secret ";
        a.repeat = " secret ";
        a.title = " A & B ";
    })
    .unwrap();
    assert_eq!(
        (made.password.as_str(), made.title.as_str()),
        ("secret", "A &amp; B")
    );
}

#[test]
fn site_addresses_are_checked() {
    for (given, address) in [
        ("http://example.com", Some("http://example.com")),
        (" https://example.com/ ", Some("https://example.com")),
        (
            "https://example.com:8443/blog/",
            Some("https://example.com:8443/blog"),
        ),
        ("http://192.168.0.7:8090", Some("http://192.168.0.7:8090")),
        (
            "http://[::1]:8080/a_b.c~d-e",
            Some("http://[::1]:8080/a_b.c~d-e"),
        ),
        ("http://[::1]", Some("http://[::1]")),
        ("http://localhost", Some("http://localhost")),
        ("example.com", None),
        ("ftp://example.com", None),
        ("http://", None),
        ("http://.example.com", None),
        ("http://-example.com", None),
        ("http://exa mple.com", None),
        ("http://example.com:", None),
        ("http://example.com:0", None),
        ("http://example.com:65536", None),
        ("http://example.com:123456", None),
        ("http://example.com:80a", None),
        ("http://example.com:80:81", None),
        ("http://[::1", None),
        ("http://[]", None),
        ("http://[::g]", None),
        ("http://[::1]x", None),
        ("http://[::1]:", None),
        ("http://example.com/a b", None),
        ("http://example.com/a?b", None),
        ("http://example.com/a//b", None),
        ("http://example.com/\"onload=", None),
        ("http://user@example.com", None),
    ] {
        assert_eq!(site_address(given).as_deref(), address, "{given}");
    }
    let long = format!("http://{}.com", "a".repeat(254));
    assert_eq!(site_address(&long), None);
}

#[test]
fn only_the_known_masks_are_filled_in() {
    let fill = |token: &str| match token {
        "now" => Some("T".to_string()),
        "unix+7" => Some("S".to_string()),
        _ => None,
    };
    assert_eq!(expand("a %now% b %now%%unix+7%", &fill), "a T b TS");
    assert_eq!(expand("100% sure %x% %now", &fill), "100% sure %x% %now");
    assert_eq!(expand("%%now%", &fill), "%T");
    let site = Plan::new(&answers(&cases()[0]), NOW).unwrap();
    for token in [
        "unix+",
        "unix+x",
        "unix++5",
        "unix+-5",
        "unix+99999999999",
        "nope",
        "",
    ] {
        assert_eq!(site.fill(token, "hash", "wp_"), None, "{token}");
    }
    assert_eq!(
        site.fill("unix+5", "hash", "wp_"),
        Some("1700000005".into())
    );
    assert_eq!(site.fill("unix", "hash", "wp_"), Some("1700000000".into()));
    // An answer that looks like a mask is not filled in again.
    let tricky = plan(|a| a.title = "%password%").unwrap();
    let rows = tables(&|token| tricky.fill(token, "HASH", "wp_"), false);
    let options = rows.iter().find(|t| t.table == "options").unwrap();
    let blogname = options
        .rows
        .iter()
        .find(|row| row[1].as_deref() == Some("blogname"));
    assert_eq!(blogname.unwrap()[2].as_deref(), Some("%password%"));
}

#[test]
fn the_data_is_whole() {
    let rows = tables(&|_| Some(String::new()), false);
    let names: Vec<&str> = rows.iter().map(|t| t.table.as_str()).collect();
    assert_eq!(
        names,
        [
            "comments",
            "options",
            "postmeta",
            "posts",
            "term_relationships",
            "term_taxonomy",
            "terms",
            "usermeta",
            "users"
        ]
    );
    for table in &rows {
        for row in &table.rows {
            assert_eq!(row.len(), table.columns.len(), "{}", table.table);
        }
    }
    assert_eq!(SCHEMA.split(";\n").filter(|s| !s.is_empty()).count(), 12);
}

#[test]
fn a_made_password_adds_the_nag_before_the_welcome_panel() {
    let meta = |nag: bool| {
        let rows = tables(&|_| Some(String::new()), nag);
        let meta = rows.into_iter().find(|t| t.table == "usermeta").unwrap();
        let keys = meta.rows.iter().map(|row| (row[0].clone(), row[2].clone()));
        keys.skip(14).collect::<Vec<_>>()
    };
    let pair = |id: &str, key: &str| (Some(id.to_string()), Some(key.to_string()));
    assert_eq!(
        meta(false),
        [
            pair("15", "dismissed_wp_pointers"),
            pair("16", "show_welcome_panel")
        ]
    );
    assert_eq!(
        meta(true),
        [
            pair("15", "dismissed_wp_pointers"),
            pair("16", "default_password_nag"),
            pair("17", "show_welcome_panel")
        ]
    );
}

#[test]
fn the_rows_file_s_escapes_are_undone() {
    let fill = |_: &str| None;
    assert_eq!(unescape("\\N", &fill), None);
    assert_eq!(
        unescape("a\\tb\\nc\\rd\\\\e\\", &fill).as_deref(),
        Some("a\tb\nc\rd\\e\\")
    );
    assert_eq!(unescape("", &fill).as_deref(), Some(""));
}

fn made_password(_: usize, _: bool) -> std::io::Result<String> {
    Ok("a made password".into())
}

fn cheap_hash(_: &[u8]) -> std::io::Result<String> {
    Ok("$wp$cheap".into())
}

fn no_random(_: usize, _: bool) -> std::io::Result<String> {
    Err(std::io::Error::other("no entropy"))
}

fn no_hash(_: &[u8]) -> std::io::Result<String> {
    Err(std::io::Error::other("no hash"))
}

/// A password or a hash that cannot be made stops the install before any
/// table is.
#[tokio::test]
async fn no_table_is_made_without_a_password() {
    let _one = SAVES.lock().await;
    let config = config("wp_");
    emptied(&config).await;
    let mut db = Installer::open(&config).await.unwrap();
    let made = Plan::new(&answers(&cases()[2]), NOW).unwrap();
    assert!(cases()[2].password.is_empty());
    let secrets = Secrets {
        generate: no_random,
        hash: cheap_hash,
    };
    let err = made
        .install_on(&mut db, &config, &secrets)
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "the kernel's random source failed: no entropy"
    );
    let secrets = Secrets {
        generate: made_password,
        hash: no_hash,
    };
    let err = made
        .install_on(&mut db, &config, &secrets)
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "the kernel's random source failed: no hash"
    );
    assert!(db.existing().await.unwrap().is_empty());
}

/// Every statement of an install can fail, and none leaves a table behind
/// once the failure is cleared.
#[tokio::test]
async fn every_statement_of_an_install_can_fail() {
    let _one = SAVES.lock().await;
    let case = &cases()[0];
    let config = config(case.prefix);
    let plan = Plan::new(&answers(case), NOW).unwrap();
    let secrets = Secrets {
        generate: made_password,
        hash: cheap_hash,
    };
    let rows: usize = tables(&|_| Some(String::new()), false)
        .iter()
        .map(|table| table.rows.len())
        .sum();
    // The listing, the twelve tables, the begin, every row and the commit.
    let statements = 1 + 12 + 1 + rows + 1;
    for spent in 0..statements {
        let mut conn = emptied(&config).await;
        let mut db = Installer::open(&config).await.unwrap();
        db.fail_after(Some(spent));
        let err = plan
            .install_on(&mut db, &config, &secrets)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("the test's statement budget is spent"),
            "{err}"
        );
        db.fail_after(None);
        db.undo().await.unwrap();
        let left =
            "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE()";
        assert!(
            text_of(&mut conn, left.into(), 0).await.is_empty(),
            "{spent}"
        );
    }
    let mut conn = emptied(&config).await;
    let mut db = Installer::open(&config).await.unwrap();
    db.fail_after(Some(statements));
    let installed = plan.install_on(&mut db, &config, &secrets).await.unwrap();
    assert_eq!(installed.tables.len(), 12);
    // Removing it fails at the first drop and at every later one.
    for spent in 0..12 {
        let mut db = Installer::open(&config).await.unwrap();
        db.fail_after(Some(spent));
        let err = Plan::remove_on(&mut db, &installed).await.unwrap_err();
        assert!(err.to_string().contains("budget is spent"), "{err}");
    }
    let mut db = Installer::open(&config).await.unwrap();
    Plan::remove_on(&mut db, &installed).await.unwrap();
    let left = "SELECT TABLE_NAME FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE()";
    assert!(text_of(&mut conn, left.into(), 0).await.is_empty());
}
