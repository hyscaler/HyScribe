// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use hyscribe_config::{Constants, Secret};
use std::io::Cursor;
use std::os::unix::fs::PermissionsExt;

struct Ran {
    result: Result<String, String>,
    out: String,
}

fn wizard(home: &Path, answers: &str) -> Ran {
    let mut input = Cursor::new(answers.as_bytes().to_vec());
    let mut out = Vec::new();
    let result = run(
        home,
        &mut Terminal {
            input: &mut input,
            out: &mut out,
            tty: None,
        },
    );
    Ran {
        result,
        out: String::from_utf8(out).unwrap(),
    }
}

/// An installation folder `hyscribe/` with a WordPress folder `site/` beside
/// it.
struct Install {
    _dir: tempfile::TempDir,
    home: PathBuf,
    site: PathBuf,
}

impl Install {
    fn new(wp_config: Option<&str>) -> Install {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("hyscribe");
        let site = dir.path().join("site");
        std::fs::create_dir(&home).unwrap();
        std::fs::create_dir(&site).unwrap();
        if let Some(text) = wp_config {
            std::fs::write(site.join("wp-config.php"), text).unwrap();
        }
        Install {
            _dir: dir,
            home,
            site,
        }
    }

    fn env(&self) -> PathBuf {
        self.home.join(".env")
    }

    fn config(&self) -> Config {
        hyscribe_config::load(&self.home, &HashMap::new())
            .unwrap()
            .unwrap()
    }

    fn written(&self) -> String {
        format!(
            "Wrote {} (mode 600)\nStart the site with: ./hyscribe service start\n",
            self.env().display()
        )
    }
}

fn salt(key: &str) -> String {
    format!("{key} $alt\"#\\")
}

/// A `wp-config.php` for the default install fixture database `name`, as
/// `wp config create` writes one, with `extra` before the prefix.
fn wp_config_for(name: &str, salts: bool, extra: &str) -> String {
    let db = hyscribe_testkit::fixture_database();
    let mut text = format!(
        "<?php\ndefine( 'DB_NAME', '{name}' );\ndefine( 'DB_USER', '{}' );\n\
         define( 'DB_PASSWORD', '{}' );\ndefine( 'DB_HOST', '{}:{}' );\n\
         define( 'DB_CHARSET', 'utf8mb4' );\ndefine( 'DB_COLLATE', '' );\n",
        db.user,
        db.password.expose(),
        db.host,
        db.port
    );
    for key in SALT_KEYS {
        let value = if salts {
            salt(key)
        } else {
            "put your unique phrase here".into()
        };
        text.push_str(&format!(
            "define( '{key}', '{}' );\n",
            value.replace('\\', "\\\\")
        ));
    }
    text.push_str(extra);
    text.push_str(
        "\n$table_prefix = 'wp_';\nif ( ! defined( 'ABSPATH' ) ) {\n\
         \tdefine( 'ABSPATH', __DIR__ . '/' );\n}\nrequire_once ABSPATH . 'wp-settings.php';\n",
    );
    text
}

fn wp_config(extra: &str) -> String {
    wp_config_for("wordpress", true, extra)
}

const CONSTANTS_SET: &str = "define( 'DISALLOW_UNFILTERED_HTML', false );\n\
    define( 'ALLOW_UNFILTERED_UPLOADS', true );\ndefine( 'MULTISITE', false );\n\
    define( 'WP_HOME', 'https://example.com' );\ndefine( 'WP_SITEURL', 'https://example.com/wp' );";

/// Nothing secret reaches the terminal.
fn assert_no_secrets(out: &str) {
    assert!(!out.contains("reference-only"), "{out}");
    assert!(!out.contains("$alt"), "{out}");
}

#[test]
fn an_existing_site_is_copied() {
    let install = Install::new(Some(&wp_config(CONSTANTS_SET)));
    // The folder found, shadow mode, no read user.
    let ran = wizard(&install.home, "\n\n\n");
    assert_eq!(ran.result, Ok(install.written()));
    let db = hyscribe_testkit::fixture_database();
    let site = install.site.display();
    assert_eq!(
        ran.out,
        format!(
            "? Path to the WordPress folder or its wp-config.php (none if there is none) \
             [{site}]:   Read {site}/wp-config.php\n  Keys and salts: 8 copied, 0 generated\n\
             ? Start in shadow mode (read only) first? [Y/n]: \
             ? Database user granted only SELECT, for every read (empty for none): \
             \x20 Connected to wordpress on {}:{}, db_version 61833\n",
            db.host, db.port
        )
    );
    assert_no_secrets(&ran.out);
    let mode = std::fs::metadata(install.env())
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let config = install.config();
    assert_eq!(config.database, db);
    assert_eq!(config.mode, Mode::Shadow);
    assert_eq!(config.home_url.as_deref(), Some("https://example.com"));
    assert_eq!(config.site_url.as_deref(), Some("https://example.com/wp"));
    assert_eq!(
        config.constants,
        Constants {
            disallow_unfiltered_html: false,
            allow_unfiltered_uploads: true,
        }
    );
    let salts: Vec<_> = SALT_KEYS
        .iter()
        .map(|&key| (key, Secret::new(salt(key))))
        .collect();
    assert_eq!(config.salts, salts);
}

#[test]
fn unset_constants_keep_the_safer_values() {
    let install = Install::new(Some(&wp_config_for("wordpress", false, "")));
    let ran = wizard(&install.home, "\n\n\n");
    assert_eq!(ran.result, Ok(install.written()));
    assert!(ran.out.contains("Keys and salts: 0 copied, 8 generated"));
    let text = std::fs::read_to_string(install.env()).unwrap();
    assert!(!text.contains("UNFILTERED"), "{text}");
    assert!(!text.contains("MULTISITE"), "{text}");
    let config = install.config();
    assert_eq!(config.constants, Constants::default());
    assert_eq!(config.home_url, None);
    assert_eq!(config.salts.len(), 8);
    for (_, salt) in &config.salts {
        assert_eq!(salt.expose().len(), 64);
        assert!(!ran.out.contains(salt.expose()));
    }
}

#[test]
fn what_php_computes_is_asked_for_or_left_safe() {
    let extra = "define( 'DISALLOW_UNFILTERED_HTML', getenv( 'X' ) );\n\
                 define( 'ALLOW_UNFILTERED_UPLOADS', 1 );\n\
                 define( 'WP_HOME', 'https://' . $_SERVER['HTTP_HOST'] );\n\
                 define( 'WP_SITEURL', WP_HOME . '/wp' );\n\
                 define( 'WP_CONTENT_DIR', __DIR__ . '/content' );";
    let install = Install::new(Some(&wp_config(extra)));
    let content = install.site.join("content");
    std::fs::create_dir(&content).unwrap();
    std::fs::write(content.join("object-cache.php"), "<?php").unwrap();
    // WP_SITEURL left empty: the stored one.
    let answers = format!("\n{}\nhttps://asked.example\n\n\n\n", content.display());
    let ran = wizard(&install.home, &answers);
    assert_eq!(ran.result, Ok(install.written()));
    for expected in [
        "  Not a literal there, or missing: WP_HOME, WP_SITEURL, DISALLOW_UNFILTERED_HTML, \
         ALLOW_UNFILTERED_UPLOADS\n"
            .to_string(),
        format!(
            "? Path to wp-content (WP_CONTENT_DIR is computed by PHP) [{}/wp-content]: ",
            install.site.display()
        ),
        "? WP_HOME, the site address (empty for the stored one): ".into(),
        "  DISALLOW_UNFILTERED_HTML is not a literal true or false, so it is not copied: \
         the safer value holds\n"
            .into(),
        "  ALLOW_UNFILTERED_UPLOADS is not a literal".into(),
        format!(
            "  Warning: {}/object-cache.php is a persistent object cache.",
            content.display()
        ),
    ] {
        assert!(ran.out.contains(&expected), "{expected}\n{}", ran.out);
    }
    let config = install.config();
    assert_eq!(config.home_url.as_deref(), Some("https://asked.example"));
    assert_eq!(config.site_url, None);
    assert_eq!(config.constants, Constants::default());
}

#[test]
fn literal_content_dirs_are_used() {
    let content = tempfile::tempdir().unwrap();
    std::fs::write(content.path().join("object-cache.php"), "<?php").unwrap();
    let extra = format!(
        "define( 'WP_CONTENT_DIR', '{}' );",
        content.path().display()
    );
    let install = Install::new(Some(&wp_config(&extra)));
    let ran = wizard(&install.home, "\n\n\n");
    assert_eq!(ran.result, Ok(install.written()));
    assert!(ran.out.contains("Warning: "), "{}", ran.out);
    // The default wp-content has none.
    let install = Install::new(Some(&wp_config("")));
    std::fs::create_dir(install.site.join("wp-content")).unwrap();
    let ran = wizard(&install.home, "\n\n\n");
    assert!(!ran.out.contains("Warning"), "{}", ran.out);
}

#[test]
fn multisite_is_refused() {
    let install = Install::new(Some(&wp_config("define( 'MULTISITE', true );")));
    let ran = wizard(&install.home, "\n\n\n");
    assert_eq!(
        ran.result,
        Err("MULTISITE is invalid: multisite networks are not supported".into())
    );
    assert!(!install.env().exists());
}

#[test]
fn everything_is_asked_without_wp_config() {
    let install = Install::new(None);
    let db = hyscribe_testkit::fixture_database();
    let answers = format!(
        "\n{}\n{}\n{}\n{}\n{}\n\nn\nwordpress_read\n{}\n",
        db.name,
        db.user,
        db.password.expose(),
        db.host,
        db.port,
        db.password.expose()
    );
    let ran = wizard(&install.home, &answers);
    assert_eq!(ran.result, Ok(install.written()));
    assert_no_secrets(&ran.out);
    for expected in [
        "? Path to the WordPress folder or its wp-config.php (none if there is none): ",
        "? Database name: ",
        "? Database password: ",
        "? Database host [localhost]: ",
        "? Database port [3306]: ",
        "? Table prefix [wp_]: ",
        "Keys and salts: 0 copied, 8 generated",
        "? Start in shadow mode (read only) first? [Y/n]: ",
        "? Its password: ",
        "  DB_READ_USER wordpress_read may only read\n",
    ] {
        assert!(ran.out.contains(expected), "{expected}\n{}", ran.out);
    }
    let config = install.config();
    assert_eq!(config.mode, Mode::Primary);
    assert_eq!(config.database.read_user.as_deref(), Some("wordpress_read"));
    assert_eq!(config.database.read_password, db.password);
    assert_eq!(config.database.prefix.as_str(), "wp_");
}

#[test]
fn none_skips_a_wp_config_found() {
    let install = Install::new(Some(&wp_config("")));
    let ran = wizard(&install.home, "none\n");
    assert!(ran.out.ends_with("? Database name: "), "{}", ran.out);
}

#[test]
fn a_read_user_that_may_write_is_refused() {
    let install = Install::new(Some(&wp_config("")));
    let ran = wizard(&install.home, "\n\nwordpress\nreference-only\n");
    let err = ran.result.unwrap_err();
    assert!(
        err.starts_with("DB_READ_USER wordpress may only be granted SELECT; it has "),
        "{err}"
    );
    assert!(err.contains("INSERT"), "{err}");
    assert!(!install.env().exists());
}

#[test]
fn unreachable_databases_are_reported() {
    let config = wp_config("").replace(
        &format!(":{}'", hyscribe_testkit::fixture_database().port),
        ":1'",
    );
    let install = Install::new(Some(&config));
    let err = wizard(&install.home, "\n\n\n").result.unwrap_err();
    assert!(
        err.starts_with("cannot connect to wordpress on 127.0.0.1:1 as wordpress: "),
        "{err}"
    );
    assert!(!err.contains("reference-only"), "{err}");
    let config = wp_config("").replace("'wp_'", "'missing_'");
    let install = Install::new(Some(&config));
    let err = wizard(&install.home, "\n\n\n").result.unwrap_err();
    assert!(err.starts_with("cannot read db_version: "), "{err}");
}

/// `wordpress_saves`, the scratch database, is changed one test at a time.
static SAVES: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Loads the default install into `wordpress_saves` and runs `statements`.
fn saves_with(statements: &[String]) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        use sqlx::Connection as _;
        let db = hyscribe_config::Database {
            name: "wordpress_saves".into(),
            ..hyscribe_testkit::fixture_database()
        };
        hyscribe_testkit::writes::load_default_install(&db)
            .await
            .unwrap();
        let options = sqlx::mysql::MySqlConnectOptions::new()
            .host(&db.host)
            .port(db.port)
            .database(&db.name)
            .username(&db.user)
            .password(db.password.expose());
        let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
        for statement in statements {
            sqlx::raw_sql(sqlx::AssertSqlSafe(statement.clone()))
                .execute(&mut conn)
                .await
                .unwrap();
        }
    });
}

#[test]
fn unsupported_db_versions_are_refused() {
    let _one = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    let install = Install::new(Some(&wp_config_for("wordpress_saves", true, "")));
    let newer = |version: &str| {
        format!(
            "the database's db_version ({version}) is not one this release supports \
             (61833 to 61833, WordPress 7.0 and 7.1)"
        )
    };
    let older = "the database's db_version (61832) is older than WordPress 7.0's (61833): \
                 upgrade the site with PHP WordPress first";
    let set = "UPDATE wp_options SET option_value = ? WHERE option_name = 'db_version'";
    for (statement, refusal) in [
        (set.replace('?', "'61834'"), newer("61834")),
        (set.replace('?', "'61832'"), older.to_string()),
        (set.replace('?', "'new'"), newer("new")),
        (
            "DELETE FROM wp_options WHERE option_name = 'db_version'".into(),
            newer("missing"),
        ),
    ] {
        saves_with(&[statement]);
        let ran = wizard(&install.home, "\n\n\n");
        assert_eq!(ran.result, Err(refusal));
        assert!(!install.env().exists());
    }
}

#[test]
fn myisam_tables_refuse_primary_mode() {
    let _one = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    saves_with(&["ALTER TABLE wp_links ENGINE=MyISAM".into()]);
    let install = Install::new(Some(&wp_config_for("wordpress_saves", true, "")));
    let refused = "writes refused: wp_links uses the MyISAM engine, which cannot roll back, so \
                   HyScribe does not write to it; convert it with ALTER TABLE wp_links \
                   ENGINE=InnoDB";
    let ran = wizard(&install.home, "\nn\n\n");
    assert_eq!(
        ran.result,
        Err(format!(
            "primary mode cannot write as it should:\n{refused}\n"
        ))
    );
    assert!(!install.env().exists());
    let ran = wizard(&install.home, "\n\n\n");
    assert_eq!(ran.result, Ok(install.written()));
    assert!(
        ran.out
            .contains(&format!("  Warning, in primary mode: {refused}\n")),
        "{}",
        ran.out
    );
    saves_with(&[]);
}

#[test]
fn an_existing_env_is_kept_unless_replaced() {
    let install = Install::new(Some(&wp_config("")));
    std::fs::write(install.env(), "OLD=1\n").unwrap();
    let ran = wizard(&install.home, "maybe\nn\n");
    let env = install.env();
    assert_eq!(
        ran.result,
        Ok(format!("Kept {}; nothing written.\n", env.display()))
    );
    assert_eq!(
        ran.out,
        format!(
            "? {} exists. Replace it? [y/N]:   Answer yes or no.\n? {} exists. Replace it? [y/N]: ",
            env.display(),
            env.display()
        )
    );
    assert_eq!(std::fs::read_to_string(&env).unwrap(), "OLD=1\n");
    let ran = wizard(&install.home, "\n");
    assert!(ran.result.unwrap().starts_with("Kept "));
    let ran = wizard(&install.home, "YES\n\n\n\n");
    assert_eq!(ran.result, Ok(install.written()));
    assert_eq!(install.config().database.name, "wordpress");
}

#[test]
fn wp_config_is_found_as_wordpress_finds_it() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let home = root.join("hyscribe");
    std::fs::create_dir(&home).unwrap();
    assert_eq!(find_wordpress(&home), "");
    for name in ["b-site", "a-file"] {
        std::fs::create_dir(root.join(name)).unwrap();
    }
    std::fs::write(root.join("b-site/wp-config.php"), "<?php").unwrap();
    assert_eq!(
        find_wordpress(&home),
        root.join("b-site").display().to_string()
    );
    std::fs::write(root.join("wp-config.php"), "<?php").unwrap();
    assert_eq!(find_wordpress(&home), root.display().to_string());

    // A file, a folder with it, or the folder below.
    let file = root.join("b-site/wp-config.php");
    assert_eq!(locate(&file), Ok((file.clone(), root.join("b-site"))));
    let site = root.join("b-site");
    assert_eq!(locate(&site), Ok((file.clone(), site.clone())));
    let below = site.join("wordpress");
    std::fs::create_dir(&below).unwrap();
    assert_eq!(
        locate(&below),
        Ok((below.join("../wp-config.php"), below.clone()))
    );
    // Not when the folder above is a WordPress folder itself.
    std::fs::write(site.join("wp-settings.php"), "<?php").unwrap();
    let missing = format!(
        "no wp-config.php in {} or the folder above it",
        below.display()
    );
    assert_eq!(locate(&below), Err(missing));
    assert!(locate(&root.join("nowhere")).is_err());
}

#[test]
fn unreadable_wp_configs_are_reported() {
    let install = Install::new(Some(&wp_config("")));
    let file = install.site.join("wp-config.php");
    std::fs::write(&file, vec![b' '; wpconfig::MAX_SIZE + 1]).unwrap();
    let err = wizard(&install.home, "\n").result.unwrap_err();
    assert!(err.starts_with("wp-config.php is 1048577 bytes"), "{err}");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    let err = wizard(&install.home, "\n").result.unwrap_err();
    assert_eq!(
        err,
        format!(
            "cannot read {}: Permission denied (os error 13)",
            file.display()
        )
    );
    let err = wizard(&install.home, "/nowhere\n").result.unwrap_err();
    assert!(err.starts_with("no wp-config.php in /nowhere"), "{err}");
}

#[test]
fn a_carriage_return_cannot_go_into_env() {
    let extra = "define( 'WP_HOME', 'https://' . $_SERVER['HTTP_HOST'] );";
    let install = Install::new(Some(&wp_config(extra)));
    let ran = wizard(&install.home, "\na\rb\n\n\n");
    assert_eq!(
        ran.result,
        Err("an answer holds a carriage return, which .env cannot hold".into())
    );
}

#[test]
fn a_folder_that_cannot_be_written_is_reported() {
    let install = Install::new(Some(&wp_config("")));
    std::fs::set_permissions(&install.home, std::fs::Permissions::from_mode(0o500)).unwrap();
    let err = wizard(&install.home, "\n\n\n").result.unwrap_err();
    std::fs::set_permissions(&install.home, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        err,
        format!(
            "cannot write {}: Permission denied (os error 13)",
            install.env().display()
        )
    );
}

/// A reader that fails.
struct Broken;

impl std::io::Read for Broken {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("broken"))
    }
}

#[test]
fn input_and_output_failures_stop_the_wizard() {
    let install = Install::new(None);
    let ended: Result<String, String> =
        Err("setup stopped: the input ended before every question was answered".into());
    assert_eq!(wizard(&install.home, "").result, ended);
    // Ended at the last question, the read user.
    let site = Install::new(Some(&wp_config("")));
    assert_eq!(wizard(&site.home, "\n\n").result, ended);
    let mut out = Vec::new();
    let mut terminal = Terminal {
        input: &mut std::io::BufReader::new(Broken),
        out: &mut out,
        tty: None,
    };
    assert_eq!(
        run(&install.home, &mut terminal),
        Err("setup stopped: broken".into())
    );
    let mut full: &mut [u8] = &mut [];
    let mut terminal = Terminal {
        input: &mut std::io::empty(),
        out: &mut full,
        tty: None,
    };
    assert_eq!(
        run(&install.home, &mut terminal),
        Err("failed to write whole buffer".into())
    );
}

/// Output that fails after `0` more writes.
struct FailAfter(usize);

impl Write for FailAfter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self.0.checked_sub(1) {
            Some(left) => {
                self.0 = left;
                Ok(buf.len())
            }
            None => Err(std::io::Error::other("closed")),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Every way `answers` can stop early: the input ending after each line,
/// and the output failing at each write. Then the whole run, which must
/// succeed with `result`.
fn stopped_anywhere(install: &Install, answers: &str, result: &Result<String, String>) {
    let lines: Vec<&str> = answers.split_inclusive('\n').collect();
    for end in 0..lines.len() {
        let ran = wizard(&install.home, &lines[..end].concat());
        assert_eq!(
            ran.result,
            Err("setup stopped: the input ended before every question was answered".into()),
            "after {end} lines"
        );
    }
    for writes in 0..200 {
        let mut input = Cursor::new(answers.as_bytes().to_vec());
        let mut out = FailAfter(writes);
        let ran = run(
            &install.home,
            &mut Terminal {
                input: &mut input,
                out: &mut out,
                tty: None,
            },
        );
        if ran != Err("closed".into()) {
            assert_eq!(&ran, result);
            return;
        }
    }
    panic!("the output never ran out");
}

#[test]
fn every_question_can_stop_the_wizard() {
    // Computed values, a read user and an object cache.
    let extra = "define( 'DB_CHARSET', getenv( 'CHARSET' ) );\n\
                 define( 'WP_HOME', 'https://' . $_SERVER['HTTP_HOST'] );\n\
                 define( 'MULTISITE', getenv( 'MULTISITE' ) );\n\
                 define( 'WP_CONTENT_DIR', __DIR__ . '/content' );";
    let config = wp_config(extra).replacen("define( 'DB_CHARSET', 'utf8mb4' );", "", 1);
    let install = Install::new(Some(&config));
    let content = install.site.join("content");
    std::fs::create_dir(&content).unwrap();
    std::fs::write(content.join("object-cache.php"), "<?php").unwrap();
    let answers = format!(
        "\n{}\nutf8mb4\nhttps://a.example\n\nwordpress_read\nreference-only\n",
        content.display()
    );
    stopped_anywhere(&install, &answers, &Ok(install.written()));
    let config = install.config();
    assert_eq!(config.home_url.as_deref(), Some("https://a.example"));
    assert_eq!(config.database.read_user.as_deref(), Some("wordpress_read"));
    // With `.env` there, kept after a wrong answer.
    let kept = Ok(format!(
        "Kept {}; nothing written.\n",
        install.env().display()
    ));
    stopped_anywhere(&install, "maybe\nn\n", &kept);
    // Everything asked.
    let install = Install::new(None);
    let db = hyscribe_testkit::fixture_database();
    let answers = format!(
        "\n{}\n{}\n{}\n{}\n{}\n\n\n\n",
        db.name,
        db.user,
        db.password.expose(),
        db.host,
        db.port
    );
    stopped_anywhere(&install, &answers, &Ok(install.written()));
}

/// The read user connects, but the main user cannot.
#[test]
fn the_main_user_must_connect_too() {
    let config = wp_config("").replace("'reference-only'", "'wrong'");
    let install = Install::new(Some(&config));
    let err = wizard(&install.home, "\n\nwordpress_read\nreference-only\n")
        .result
        .unwrap_err();
    assert!(err.starts_with("cannot open the writer: "), "{err}");
    assert!(!err.contains("wrong"), "{err}");
}

/// Input that notes whether the terminal echoed while it was read.
struct Watched<'a> {
    tty: BorrowedFd<'a>,
    answer: Cursor<Vec<u8>>,
    echoed: Option<bool>,
}

impl std::io::Read for Watched<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.answer.read(buf)
    }
}

impl BufRead for Watched<'_> {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        let modes = tcgetattr(self.tty).unwrap().local_modes;
        self.echoed.get_or_insert(modes.contains(LocalModes::ECHO));
        self.answer.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.answer.consume(amount);
    }
}

#[test]
fn passwords_are_read_with_echo_off() {
    use rustix::pty::{OpenptFlags, grantpt, openpt, ptsname, unlockpt};
    use std::os::fd::AsFd;
    let master = openpt(OpenptFlags::RDWR | OpenptFlags::NOCTTY).unwrap();
    grantpt(&master).unwrap();
    unlockpt(&master).unwrap();
    let name = ptsname(&master, Vec::new()).unwrap();
    let slave = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(name.to_str().unwrap())
        .unwrap();
    let tty = slave.as_fd();
    let echo = || {
        tcgetattr(tty)
            .unwrap()
            .local_modes
            .contains(LocalModes::ECHO)
    };
    assert!(echo());
    let mut input = Watched {
        tty,
        answer: Cursor::new(b"s3cret\n".to_vec()),
        echoed: None,
    };
    let mut out = Vec::new();
    let mut terminal = Terminal {
        input: &mut input,
        out: &mut out,
        tty: Some(tty),
    };
    assert_eq!(terminal.secret("Password"), Ok("s3cret".into()));
    assert_eq!(input.echoed, Some(false));
    assert!(echo());
    assert_eq!(out, b"? Password: \n");
    // The newline after the hidden answer cannot be written.
    let mut input = Cursor::new(b"s3cret\n".to_vec());
    let mut terminal = Terminal {
        input: &mut input,
        out: &mut FailAfter(1),
        tty: Some(tty),
    };
    assert_eq!(terminal.secret("Password"), Err("closed".into()));
    assert!(echo());
    // Input that is no terminal is read as it is.
    let null = std::fs::File::open("/dev/null").unwrap();
    let mut terminal = Terminal {
        input: &mut Cursor::new(b"open\n".to_vec()),
        out: &mut Vec::new(),
        tty: Some(null.as_fd()),
    };
    assert_eq!(terminal.secret("Password"), Ok("open".into()));
}
