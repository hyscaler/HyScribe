// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::{SAVES, empty_saves, saves_scalar, saves_with};
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
    let result = command(
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
        std::fs::create_dir_all(site.join("wp-content/uploads")).unwrap();
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

pub(crate) fn salt(key: &str) -> String {
    format!("{key} $alt\"#\\")
}

/// A `wp-config.php` for the default install fixture database `name`, as
/// `wp config create` writes one, with `extra` before the prefix.
pub(crate) fn wp_config_for(name: &str, salts: bool, extra: &str) -> String {
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

pub(crate) fn wp_config(extra: &str) -> String {
    wp_config_for("wordpress", true, extra)
}

const CONSTANTS_SET: &str = "define( 'DISALLOW_UNFILTERED_HTML', false );\n\
    define( 'ALLOW_UNFILTERED_UPLOADS', true );\ndefine( 'MULTISITE', false );\n\
    define( 'WP_HOME', 'https://example.com' );\ndefine( 'WP_SITEURL', 'https://example.com/wp' );\n\
    define( 'EMPTY_TRASH_DAYS', 7 );\ndefine( 'MEDIA_TRASH', true );";

/// Every answer the default, with a `wp-config.php` and no `.env`.
pub(crate) const DEFAULTS: &str = "\n\n\n\n\n\n\n";

/// Nothing secret reaches the terminal.
fn assert_no_secrets(out: &str) {
    assert!(!out.contains("reference-only"), "{out}");
    assert!(!out.contains("$alt"), "{out}");
}

#[test]
fn an_existing_site_is_copied() {
    let install = Install::new(Some(&wp_config(CONSTANTS_SET)));
    // The folder found, shadow mode, no read user, the default network
    // and uploads folder.
    let ran = wizard(&install.home, DEFAULTS);
    assert_eq!(ran.result, Ok(install.written()));
    let db = hyscribe_testkit::fixture_database();
    let site = install.site.display();
    assert_eq!(
        ran.out,
        format!(
            "? Path to the WordPress folder or its wp-config.php (none if there is none, \
             new for a new site) [{site}]:   Read {site}/wp-config.php\n\
             ? Start in shadow mode (read only) first? [Y/n]: \
             ? Database user granted only SELECT, for every read (empty for none): \
             ? Running HyScribe behind a proxy? [y/N]: ? Address to listen on [127.0.0.1]: \
             ? Port to listen on [8080]: \
             ? The site's uploads folder (empty for hyscribe/uploads) [{site}/wp-content/uploads]: \
             \x20 Keys and salts: 8 copied, 0 generated\n\
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
    assert_eq!(
        config.server,
        hyscribe_config::server(&HashMap::new()).unwrap()
    );
    assert_eq!(
        config.uploads_dir,
        Some(install.site.join("wp-content/uploads"))
    );
    assert_eq!(config.home_url.as_deref(), Some("https://example.com"));
    assert_eq!(config.site_url.as_deref(), Some("https://example.com/wp"));
    assert_eq!(
        config.constants,
        Constants {
            disallow_unfiltered_html: false,
            allow_unfiltered_uploads: true,
            empty_trash_days: 7,
            media_trash: true,
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
    let ran = wizard(&install.home, DEFAULTS);
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
                 define( 'WP_CONTENT_DIR', __DIR__ . '/content' );\n\
                 define( 'EMPTY_TRASH_DAYS', 7 * 2 );";
    let install = Install::new(Some(&wp_config(extra)));
    let content = install.site.join("content");
    std::fs::create_dir(&content).unwrap();
    std::fs::write(content.join("object-cache.php"), "<?php").unwrap();
    let answers = format!(
        "\n{}\nhttps://asked.example\nhttps://asked.example/wp\n\n\n\n\n\n\n",
        content.display()
    );
    let ran = wizard(&install.home, &answers);
    assert_eq!(ran.result, Ok(install.written()));
    for expected in [
        "  Not a literal there, or missing: WP_HOME, WP_SITEURL, DISALLOW_UNFILTERED_HTML, \
         ALLOW_UNFILTERED_UPLOADS, EMPTY_TRASH_DAYS\n"
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
        "  EMPTY_TRASH_DAYS is not a literal number of days, so it is not copied: \
         WordPress's 30 holds\n"
            .into(),
        format!(
            "  Warning: UPLOADS_DIR {}/uploads is not a folder yet\n",
            content.display()
        ),
        format!(
            "  Warning: {}/object-cache.php is a persistent object cache.",
            content.display()
        ),
    ] {
        assert!(ran.out.contains(&expected), "{expected}\n{}", ran.out);
    }
    let config = install.config();
    assert_eq!(config.home_url.as_deref(), Some("https://asked.example"));
    assert_eq!(config.site_url.as_deref(), Some("https://asked.example/wp"));
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
    let ran = wizard(&install.home, DEFAULTS);
    assert_eq!(ran.result, Ok(install.written()));
    assert!(ran.out.contains("object cache"), "{}", ran.out);
    // The default wp-content has none.
    let install = Install::new(Some(&wp_config("")));
    let ran = wizard(&install.home, DEFAULTS);
    assert!(!ran.out.contains("Warning"), "{}", ran.out);
}

/// `wp_config( extra )` with the fixture database's socket as `DB_HOST`.
pub(crate) fn socket_wp_config(extra: &str) -> (String, PathBuf) {
    let db = hyscribe_testkit::fixture_database();
    let socket = hyscribe_testkit::fixture_socket();
    let host = format!("'{}:{}'", db.host, db.port);
    let text = wp_config(extra).replace(&host, &format!("':{}'", socket.display()));
    (text, socket)
}

/// A socket host is copied, not asked for, and the wizard checks the
/// database through it.
#[test]
fn a_socket_host_is_copied() {
    let (text, socket) = socket_wp_config("");
    let install = Install::new(Some(&text));
    let ran = wizard(&install.home, DEFAULTS);
    assert_eq!(ran.result, Ok(install.written()));
    let connected = format!(
        "  Connected to wordpress on localhost:{}, db_version 61833\n",
        socket.display()
    );
    assert!(ran.out.ends_with(&connected), "{}", ran.out);
    assert!(!ran.out.contains("Database host"), "{}", ran.out);
    let database = install.config().database;
    assert_eq!(database.host, "localhost");
    assert_eq!(database.socket, Some(socket));
}

/// A literal `UPLOADS` gives the uploads folder in the WordPress folder,
/// as `wp_upload_dir()` takes it; one PHP computes is noted.
#[test]
fn the_uploads_constant_gives_the_uploads_folder() {
    let install = Install::new(Some(&wp_config("define( 'UPLOADS', 'files/media' );")));
    std::fs::create_dir_all(install.site.join("files/media")).unwrap();
    let ran = wizard(&install.home, DEFAULTS);
    assert_eq!(ran.result, Ok(install.written()));
    let site = install.site.display();
    let asked = format!("(empty for hyscribe/uploads) [{site}/files/media]: ");
    assert!(ran.out.contains(&asked), "{}", ran.out);
    assert!(!ran.out.contains("Warning"), "{}", ran.out);
    let uploads = install.config().uploads_dir;
    assert_eq!(uploads, Some(install.site.join("files/media")));
    let computed = "define( 'UPLOADS', 'wp-content/' . 'files' );";
    let install = Install::new(Some(&wp_config(computed)));
    let ran = wizard(&install.home, DEFAULTS);
    for expected in [
        "  Not a literal there, or missing: UPLOADS\n".to_string(),
        "  UPLOADS is not a literal in a WordPress folder given, so it is not copied: \
         give the site's uploads folder\n"
            .into(),
        format!(
            "(empty for hyscribe/uploads) [{site}/wp-content/uploads]: ",
            site = install.site.display()
        ),
    ] {
        assert!(ran.out.contains(&expected), "{expected}\n{}", ran.out);
    }
}

#[test]
fn multisite_is_refused() {
    let install = Install::new(Some(&wp_config("define( 'MULTISITE', true );")));
    let ran = wizard(&install.home, DEFAULTS);
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
    let uploads = tempfile::tempdir().unwrap();
    let answers = format!(
        "\n{}\n{}\n{}\n{}\n{}\n\nn\nwordpress_read\n{}\ny\n::1\n8081\n{}\n",
        db.name,
        db.user,
        db.password.expose(),
        db.host,
        db.port,
        db.password.expose(),
        uploads.path().display()
    );
    let ran = wizard(&install.home, &answers);
    assert_eq!(ran.result, Ok(install.written()));
    assert_no_secrets(&ran.out);
    for expected in [
        "? Path to the WordPress folder or its wp-config.php (none if there is none, new for a new \
         site): ",
        "? Database name: ",
        "? Database password: ",
        "? Database host [localhost]: ",
        "? Database port [3306]: ",
        "? Table prefix [wp_]: ",
        "Keys and salts: 0 copied, 8 generated",
        "? Start in shadow mode (read only) first? [Y/n]: ",
        "? Its password: ",
        "  DB_READ_USER wordpress_read may only read\n",
        "? The site's uploads folder (empty for hyscribe/uploads): ",
    ] {
        assert!(ran.out.contains(expected), "{expected}\n{}", ran.out);
    }
    let config = install.config();
    assert_eq!(config.mode, Mode::Primary);
    assert_eq!(config.database.read_user.as_deref(), Some("wordpress_read"));
    assert_eq!(config.database.read_password, db.password);
    assert_eq!(config.database.prefix.as_str(), "wp_");
    let listen = hyscribe_config::Server {
        bind_host: "::1".parse().unwrap(),
        port: 8081,
        behind_proxy: true,
    };
    assert_eq!(config.server, listen);
    assert_eq!(config.uploads_dir.as_deref(), Some(uploads.path()));
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
    let ran = wizard(&install.home, "\n\nwordpress\nreference-only\n\n\n\n\n");
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
    let err = wizard(&install.home, DEFAULTS).result.unwrap_err();
    assert!(
        err.starts_with("cannot connect to wordpress on 127.0.0.1:1 as wordpress: "),
        "{err}"
    );
    assert!(!err.contains("reference-only"), "{err}");
    let config = wp_config("").replace("'wp_'", "'missing_'");
    let install = Install::new(Some(&config));
    let err = wizard(&install.home, DEFAULTS).result.unwrap_err();
    assert!(err.starts_with("cannot read db_version: "), "{err}");
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
        let ran = wizard(&install.home, DEFAULTS);
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
    let ran = wizard(&install.home, "\nn\n\n\n\n\n\n");
    assert_eq!(
        ran.result,
        Err(format!(
            "primary mode cannot write as it should:\n{refused}\n"
        ))
    );
    assert!(!install.env().exists());
    let ran = wizard(&install.home, DEFAULTS);
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
    let ran = wizard(&install.home, &format!("YES\n{DEFAULTS}"));
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
    let ran = wizard(&install.home, "\na\rb\n\n\n\n\n\n\n");
    assert_eq!(
        ran.result,
        Err("an answer holds a carriage return, which .env cannot hold".into())
    );
}

#[test]
fn a_folder_that_cannot_be_written_is_reported() {
    let install = Install::new(Some(&wp_config("")));
    std::fs::set_permissions(&install.home, std::fs::Permissions::from_mode(0o500)).unwrap();
    let err = wizard(&install.home, DEFAULTS).result.unwrap_err();
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
    // Ended before the network questions.
    let site = Install::new(Some(&wp_config("")));
    assert_eq!(wizard(&site.home, "\n\n").result, ended);
    let mut out = Vec::new();
    let mut terminal = Terminal {
        input: &mut std::io::BufReader::new(Broken),
        out: &mut out,
        tty: None,
    };
    assert_eq!(
        command(&install.home, &mut terminal),
        Err("setup stopped: broken".into())
    );
    let mut full: &mut [u8] = &mut [];
    let mut terminal = Terminal {
        input: &mut std::io::empty(),
        out: &mut full,
        tty: None,
    };
    assert_eq!(
        command(&install.home, &mut terminal),
        Err("failed to write whole buffer".into())
    );
}

/// Output that fails after `0` more writes.
pub(crate) struct FailAfter(pub(crate) usize);

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
        let ran = command(
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
                 define( 'WP_SITEURL', WP_HOME );\n\
                 define( 'MULTISITE', getenv( 'MULTISITE' ) );\n\
                 define( 'WP_CONTENT_DIR', __DIR__ . '/content' );";
    let config = wp_config(extra).replacen("define( 'DB_CHARSET', 'utf8mb4' );", "", 1);
    let install = Install::new(Some(&config));
    let content = install.site.join("content");
    std::fs::create_dir(&content).unwrap();
    std::fs::write(content.join("object-cache.php"), "<?php").unwrap();
    let answers = format!(
        "\n{}\nutf8mb4\nhttps://a.example\n\n\nwordpress_read\nreference-only\n\n\n\n\n",
        content.display()
    );
    stopped_anywhere(&install, &answers, &Ok(install.written()));
    let config = install.config();
    assert_eq!(config.home_url.as_deref(), Some("https://a.example"));
    assert_eq!(config.site_url, None);
    assert_eq!(config.database.read_user.as_deref(), Some("wordpress_read"));
    // With `.env` there, kept after a wrong answer.
    let kept = Ok(format!(
        "Kept {}; nothing written.\n",
        install.env().display()
    ));
    stopped_anywhere(&install, "maybe\nn\n", &kept);
    // Three answers that are neither yes nor no stop it.
    let question = format!("{} exists. Replace it?", install.env().display());
    assert_eq!(
        wizard(&install.home, "maybe\nperhaps\n?\ny\n").result,
        Err(format!(
            "setup stopped: \"{question}\" was not answered yes or no"
        ))
    );
    // Everything asked.
    let install = Install::new(None);
    let db = hyscribe_testkit::fixture_database();
    let answers = format!(
        "\n{}\n{}\n{}\n{}\n{}\n\n\n\n\n\n\n\n",
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
    let err = wizard(
        &install.home,
        "\n\nwordpress_read\nreference-only\n\n\n\n\n",
    )
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

/// The answers of a new site in `wordpress_saves`, from the path question to
/// the uploads folder: the database details and `prefix`, the optional read
/// user, the default network, and `site` for the site's own questions.
fn new_site_answers(prefix: &str, read_user: &str, site: &str) -> String {
    let db = hyscribe_testkit::fixture_database();
    format!(
        "new\nwordpress_saves\n{}\n{}\n{}\n{}\n{prefix}\n{read_user}\n{}\n\n\n{site}\n",
        db.user,
        db.password.expose(),
        db.host,
        db.port,
        if read_user.is_empty() {
            ""
        } else {
            "reference-only\n"
        },
    )
}

/// The site questions' answers of a good site, with `password` twice.
fn good_site(password: &str) -> String {
    format!("My Site\nowner\n{password}\n{password}\nowner@example.org\n\n\n")
}

#[test]
fn a_new_site_is_installed_in_an_empty_database() {
    let _one = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    empty_saves();
    let install = Install::new(Some(&wp_config("")));
    let ran = wizard(
        &install.home,
        &new_site_answers("", "", &good_site("pass phrase")),
    );
    assert_eq!(ran.result, Ok(install.written()), "{}", ran.out);
    assert_no_secrets(&ran.out);
    for expected in [
        "? Site title: ",
        "? Administrator user name: ",
        "? Administrator password (empty for a generated one): ",
        "? Password again: ",
        "? Administrator email address: ",
        "? Discourage search engines from indexing this site? [y/N]: ",
        "? Site address, where visitors will open it [http://127.0.0.1:8080]: ",
        "  Installed WordPress 7.1.3's 12 tables and first content in wordpress_saves\n",
        "  Connected to wordpress_saves on ",
    ] {
        assert!(ran.out.contains(expected), "{expected}\n{}", ran.out);
    }
    // A new site is written to: no shadow question, primary mode.
    assert!(!ran.out.contains("shadow mode"), "{}", ran.out);
    assert!(!ran.out.contains("shown once"), "{}", ran.out);
    assert_eq!(install.config().mode, Mode::Primary);
    let option = |name: &str| {
        saves_scalar(&format!(
            "SELECT option_value FROM wp_options WHERE option_name = '{name}'"
        ))
    };
    assert_eq!(option("blogname").as_deref(), Some("My Site"));
    assert_eq!(option("siteurl").as_deref(), Some("http://127.0.0.1:8080"));
    assert_eq!(option("blog_public").as_deref(), Some("1"));
    let hash = saves_scalar("SELECT user_pass FROM wp_users WHERE user_login = 'owner'").unwrap();
    assert!(hyscribe_auth::check_password(
        b"pass phrase",
        hash.as_bytes()
    ));
    empty_saves();
}

#[test]
fn a_made_password_is_shown_once_and_works() {
    let _one = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    empty_saves();
    let install = Install::new(None);
    // Discouraging search engines, at another address, in another prefix.
    let site = "\nowner\n\n\nowner@example.org\ny\nhttps://example.org/blog/\n";
    let ran = wizard(&install.home, &new_site_answers("fresh_", "", site));
    assert_eq!(ran.result, Ok(install.written()), "{}", ran.out);
    let shown = ran
        .out
        .split("Administrator password, shown once: ")
        .nth(1)
        .unwrap();
    let password = shown.lines().next().unwrap();
    assert_eq!(password.len(), 12, "{password}");
    let hash = saves_scalar("SELECT user_pass FROM fresh_users").unwrap();
    assert!(hyscribe_auth::check_password(
        password.as_bytes(),
        hash.as_bytes()
    ));
    let blog_public = "SELECT option_value FROM fresh_options WHERE option_name = 'blog_public'";
    assert_eq!(saves_scalar(blog_public).as_deref(), Some("0"));
    let home = "SELECT option_value FROM fresh_options WHERE option_name = 'home'";
    assert_eq!(
        saves_scalar(home).as_deref(),
        Some("https://example.org/blog")
    );
    let nag = "SELECT meta_value FROM fresh_usermeta WHERE meta_key = 'default_password_nag'";
    assert_eq!(saves_scalar(nag).as_deref(), Some("1"));
    assert_eq!(install.config().database.prefix.as_str(), "fresh_");
    empty_saves();
}

#[test]
fn a_new_site_asks_again_after_a_refusal() {
    let _one = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    empty_saves();
    let install = Install::new(None);
    let refused = "T\nbad/name\npw\npw\na@b.co\n\n\n";
    let site = format!("{refused}{}", good_site("pw"));
    let ran = wizard(&install.home, &new_site_answers("", "", &site));
    assert_eq!(ran.result, Ok(install.written()), "{}", ran.out);
    assert!(
        ran.out
            .contains("  The username you provided has invalid characters.\n"),
        "{}",
        ran.out
    );
    empty_saves();
    // Three refusals stop it, and nothing is made.
    let install = Install::new(None);
    let site = format!("{refused}{refused}{refused}");
    let ran = wizard(&install.home, &new_site_answers("", "", &site));
    assert_eq!(
        ran.result,
        Err("setup stopped: the new site's answers were refused three times".into())
    );
    assert_eq!(
        saves_scalar(
            "SELECT COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE()"
        )
        .as_deref(),
        Some("0")
    );
}

#[test]
fn a_new_site_is_not_installed_over_tables() {
    let _one = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    saves_with(&[]);
    let install = Install::new(None);
    let ran = wizard(&install.home, &new_site_answers("", "", &good_site("pw")));
    let err = ran.result.unwrap_err();
    assert!(
        err.contains("already has tables with the prefix wp_: wp_commentmeta"),
        "{err}"
    );
    assert!(!install.env().exists());
    // The site that was there is as it was.
    assert_eq!(
        saves_scalar("SELECT COUNT(*) FROM wp_posts").as_deref(),
        Some("3")
    );
    saves_with(&[]);
}

#[test]
fn a_failure_after_the_install_drops_the_new_site() {
    let _one = SAVES.lock().unwrap_or_else(|e| e.into_inner());
    empty_saves();
    let tables = "SELECT COUNT(*) FROM information_schema.TABLES WHERE TABLE_SCHEMA = DATABASE()";
    let install = Install::new(None);
    // A read user that cannot connect fails the checks.
    let site = good_site("pw");
    let ran = wizard(&install.home, &new_site_answers("", "nobody", &site));
    let err = ran.result.unwrap_err();
    assert!(
        err.starts_with("cannot connect to wordpress_saves on "),
        "{err}"
    );
    assert!(
        err.ends_with("; the new site's tables were dropped"),
        "{err}"
    );
    assert_eq!(saves_scalar(tables).as_deref(), Some("0"));
    // So does an `.env` that cannot be written.
    std::fs::set_permissions(&install.home, std::fs::Permissions::from_mode(0o500)).unwrap();
    let ran = wizard(&install.home, &new_site_answers("", "", &site));
    std::fs::set_permissions(&install.home, std::fs::Permissions::from_mode(0o700)).unwrap();
    let err = ran.result.unwrap_err();
    assert!(err.starts_with("cannot write "), "{err}");
    assert!(
        err.ends_with("; the new site's tables were dropped"),
        "{err}"
    );
    assert_eq!(saves_scalar(tables).as_deref(), Some("0"));
}

#[test]
fn the_site_address_offered_is_where_a_browser_opens_it() {
    for (host, port, home) in [
        ("127.0.0.1", "8080", "http://127.0.0.1:8080"),
        ("0.0.0.0", "80", "http://127.0.0.1"),
        ("::", "8080", "http://127.0.0.1:8080"),
        ("::1", "8080", "http://[::1]:8080"),
        ("example.org", "80", "http://example.org"),
    ] {
        assert_eq!(default_home(host, port), home);
    }
}

#[tokio::test]
async fn tables_that_cannot_be_dropped_are_reported() {
    let unreachable = Database {
        port: 1,
        ..hyscribe_testkit::fixture_database()
    };
    let installed = Installed {
        generated: None,
        tables: vec!["options".into()],
    };
    assert_eq!(unmade(None, "failed".into()).await, "failed");
    let err = unmade(Some((unreachable, installed)), "failed".into()).await;
    assert!(
        err.starts_with("failed; dropping the new site's tables failed too: "),
        "{err}"
    );
}
