// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use std::os::unix::fs::PermissionsExt;

fn map(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn minimal() -> HashMap<String, String> {
    map(&[("DB_NAME", "wordpress"), ("DB_USER", "wp")])
}

fn with(extra: &[(&str, &str)]) -> HashMap<String, String> {
    let mut values = minimal();
    values.extend(map(extra));
    values
}

fn write_env(dir: &Path, contents: &str, mode: u32) -> PathBuf {
    let path = dir.join(ENV_FILE);
    std::fs::write(&path, contents).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
    path
}

#[test]
fn defaults_are_safe() {
    let config = from_map(&minimal()).unwrap();
    assert_eq!(
        config,
        Config {
            database: Database {
                host: "localhost".into(),
                port: 3306,
                socket: None,
                name: "wordpress".into(),
                user: "wp".into(),
                password: Secret::new(""),
                read_user: None,
                read_password: Secret::new(""),
                prefix: Prefix("wp_".into()),
                charset: "utf8mb4".into(),
            },
            server: Server {
                bind_host: IpAddr::from([127, 0, 0, 1]),
                port: 8080,
                behind_proxy: false,
            },
            mode: Mode::Shadow,
            uploads_dir: None,
            home_url: None,
            site_url: None,
            salts: Vec::new(),
            rest_public_users: true,
            http_outbound: false,
            constants: Constants {
                disallow_unfiltered_html: true,
                allow_unfiltered_uploads: false,
                empty_trash_days: 30,
            },
            login_limits: LoginLimits {
                address: 5,
                account: 10,
                window: 900,
            },
            upload_max_size: 64 << 20,
            image_limits: ImageLimits {
                bytes: 64 << 20,
                width: 32_000,
                height: 32_000,
                area: 256_000_000,
                memory: 1 << 30,
            },
        }
    );
}

/// The image limits as set, each a size or a whole number.
#[test]
fn reads_image_limits() {
    let limits = |pairs: &[(&str, &str)]| from_map(&with(pairs)).map(|c| c.image_limits);
    let set = limits(&[
        ("IMAGE_MAX_BYTES", "1M"),
        ("IMAGE_MAX_WIDTH", "100"),
        ("IMAGE_MAX_HEIGHT", "4294967295"),
        ("IMAGE_MAX_AREA", "5000"),
        ("IMAGE_MAX_MEMORY", "2G"),
    ]);
    let expected = ImageLimits {
        bytes: 1 << 20,
        width: 100,
        height: u32::MAX,
        area: 5000,
        memory: 2 << 30,
    };
    assert_eq!(set, Ok(expected));
    for key in ["IMAGE_MAX_BYTES", "IMAGE_MAX_AREA", "IMAGE_MAX_MEMORY"] {
        let refused = limits(&[(key, "0")]).unwrap_err().to_string();
        assert!(refused.starts_with(&format!("{key} is invalid: must be a size")));
    }
    for key in ["IMAGE_MAX_WIDTH", "IMAGE_MAX_HEIGHT"] {
        assert_eq!(
            limits(&[(key, "0")]).unwrap_err().to_string(),
            format!("{key} is invalid: must be a whole number from 1 to 4294967295")
        );
    }
}

/// The capability constants as `define()` gives them, the safer value when
/// a key is missing or empty, and a multisite network refused.
#[test]
fn reads_the_capability_constants() {
    let constants = |pairs: &[(&str, &str)]| from_map(&with(pairs)).map(|c| c.constants);
    let set = constants(&[
        ("DISALLOW_UNFILTERED_HTML", "False"),
        ("ALLOW_UNFILTERED_UPLOADS", "TRUE"),
        ("MULTISITE", "false"),
    ])
    .unwrap();
    assert!(!set.disallow_unfiltered_html && set.allow_unfiltered_uploads);
    assert!(!set.defined("DISALLOW_UNFILTERED_HTML") && set.defined("ALLOW_UNFILTERED_UPLOADS"));
    let numbers = constants(&[
        ("DISALLOW_UNFILTERED_HTML", "0"),
        ("ALLOW_UNFILTERED_UPLOADS", "1"),
        ("MULTISITE", "0"),
    ]);
    assert_eq!(numbers, Ok(set));
    let reset = constants(&[
        ("DISALLOW_UNFILTERED_HTML", "1"),
        ("ALLOW_UNFILTERED_UPLOADS", "false"),
    ]);
    assert_eq!(reset, Ok(Constants::default()));
    let empty = constants(&[("DISALLOW_UNFILTERED_HTML", ""), ("MULTISITE", "")]);
    assert_eq!(empty, Ok(Constants::default()));
    let safer = Constants::default();
    assert!(
        safer.defined("DISALLOW_UNFILTERED_HTML") && !safer.defined("ALLOW_UNFILTERED_UPLOADS")
    );
    assert!(!set.defined("MULTISITE") && !safer.defined("MULTISITE"));
    for key in [
        "MULTISITE",
        "DISALLOW_UNFILTERED_HTML",
        "ALLOW_UNFILTERED_UPLOADS",
    ] {
        for value in ["yes", "no", "2", "'true'", " true"] {
            assert_eq!(
                constants(&[(key, value)]).unwrap_err().to_string(),
                format!("{key} is invalid: must be true or false")
            );
        }
    }
    for value in ["true", "TRUE", "1"] {
        assert_eq!(
            constants(&[("MULTISITE", value)]).unwrap_err().to_string(),
            "MULTISITE is invalid: multisite networks are not supported"
        );
    }
}

#[test]
fn reads_every_key() {
    let config = from_map(&with(&[
        ("DB_HOST", "db.internal"),
        ("DB_PORT", "3307"),
        ("DB_PASSWORD", "hunter2"),
        ("DB_READ_USER", "reader"),
        ("DB_READ_PASSWORD", "hunter3"),
        ("DB_PREFIX", "site2_"),
        ("DB_CHARSET", "utf8"),
        ("BIND_HOST", "::"),
        ("PORT", "80"),
        ("BEHIND_PROXY", "TRUE"),
        ("MODE", "primary"),
        ("UPLOADS_DIR", "/srv/wp/wp-content/uploads"),
        ("WP_HOME", "https://example.test"),
        ("WP_SITEURL", "https://example.test/wp"),
        ("AUTH_KEY", "a"),
        ("NONCE_SALT", "z"),
        ("REST_PUBLIC_USERS", "off"),
        ("HTTP_OUTBOUND", "on"),
        ("EMPTY_TRASH_DAYS", "0"),
        ("LOGIN_LIMIT_ADDRESS", "1"),
        ("LOGIN_LIMIT_ACCOUNT", "1000"),
        ("LOGIN_LIMIT_WINDOW", "86400"),
        ("UPLOAD_MAX_SIZE", "512k"),
    ]))
    .unwrap();
    assert_eq!(config.database.host, "db.internal");
    assert_eq!(config.database.port, 3307);
    assert_eq!(config.database.password.expose(), "hunter2");
    assert_eq!(config.database.read_user.as_deref(), Some("reader"));
    assert_eq!(config.database.read_password.expose(), "hunter3");
    assert_eq!(config.database.prefix.as_str(), "site2_");
    assert_eq!(config.database.charset, "utf8");
    assert_eq!(config.server.bind_host, "::".parse::<IpAddr>().unwrap());
    assert_eq!(config.server.port, 80);
    assert!(config.server.behind_proxy);
    assert_eq!(config.mode, Mode::Primary);
    assert_eq!(
        config.uploads_dir,
        Some(PathBuf::from("/srv/wp/wp-content/uploads"))
    );
    assert_eq!(config.home_url.as_deref(), Some("https://example.test"));
    assert_eq!(config.site_url.as_deref(), Some("https://example.test/wp"));
    assert_eq!(
        config.salts,
        vec![
            ("AUTH_KEY", Secret::new("a")),
            ("NONCE_SALT", Secret::new("z"))
        ]
    );
    assert!(!config.rest_public_users);
    assert!(config.http_outbound);
    assert_eq!(config.constants.empty_trash_days, 0);
    assert_eq!(server(&with(&[("PORT", "80")])).map(|s| s.port), Ok(80));
    let limits = LoginLimits {
        address: 1,
        account: 1000,
        window: 86_400,
    };
    assert_eq!(config.login_limits, limits);
    assert_eq!(config.upload_max_size, 512 << 10);
    let on = from_map(&with(&[
        ("REST_PUBLIC_USERS", "on"),
        ("HTTP_OUTBOUND", "off"),
    ]))
    .unwrap();
    assert!(on.rest_public_users);
    assert!(!on.http_outbound);
}

/// A socket as `wp-config.php` gives one in `DB_HOST`, only for `localhost`.
#[test]
fn reads_a_socket_host() {
    let database = |host| from_map(&with(&[("DB_HOST", host), ("DB_PORT", "3307")]));
    let socket = database("localhost:/run/mysqld/mysqld.sock").unwrap();
    assert_eq!(socket.database.host, "localhost");
    assert_eq!(
        socket.database.socket.as_deref(),
        Some(Path::new("/run/mysqld/mysqld.sock"))
    );
    assert_eq!(
        socket.database.address(),
        "localhost:/run/mysqld/mysqld.sock"
    );
    let tcp = database("db.internal").unwrap();
    assert_eq!(tcp.database.socket, None);
    assert_eq!(tcp.database.address(), "db.internal:3307");
    for host in ["127.0.0.1:/run/mysqld/mysqld.sock", ":/tmp/mysql.sock"] {
        assert_eq!(
            database(host).unwrap_err().to_string(),
            "DB_HOST is invalid: a socket is given as localhost:/path/to/socket"
        );
    }
}

/// `UPLOAD_MAX_SIZE` in bytes or PHP's shorthand, and nothing else.
#[test]
fn reads_upload_sizes() {
    let size = |value| from_map(&with(&[("UPLOAD_MAX_SIZE", value)])).map(|c| c.upload_max_size);
    for (value, bytes) in [
        ("1", 1),
        ("2097152", 2 << 20),
        ("1K", 1 << 10),
        ("64M", 64 << 20),
        ("64m", 64 << 20),
        ("2G", 2 << 30),
        ("2g", 2 << 30),
        ("3k", 3 << 10),
        ("8589934591G", 8_589_934_591 << 30),
        ("9223372036854775807", i64::MAX.unsigned_abs()),
    ] {
        assert_eq!(size(value), Ok(bytes), "{value}");
    }
    for value in [
        "0",
        "0M",
        "M",
        "64 M",
        "64MB",
        "-1",
        "+1",
        "1.5M",
        "64T",
        "8589934592G",
        "17179869184G",
        "1k1",
        "9223372036854775808",
        "18446744073709551616",
        "99999999999999999999",
        " 64M",
        "64M ",
        "\u{ff11}M",
    ] {
        assert_eq!(
            size(value).unwrap_err().to_string(),
            "UPLOAD_MAX_SIZE is invalid: must be a size in bytes, or with a K, M or G suffix, such as 64M",
            "{value}"
        );
    }
}

/// `UPLOAD_MAX_SIZE` in bytes or PHP's shorthand, and nothing else.
#[test]
fn reads_upload_sizes() {
    let size = |value| from_map(&with(&[("UPLOAD_MAX_SIZE", value)])).map(|c| c.upload_max_size);
    for (value, bytes) in [
        ("1", 1),
        ("2097152", 2 << 20),
        ("1K", 1 << 10),
        ("64M", 64 << 20),
        ("64m", 64 << 20),
        ("2G", 2 << 30),
        ("2g", 2 << 30),
        ("3k", 3 << 10),
        ("8589934591G", 8_589_934_591 << 30),
        ("9223372036854775807", i64::MAX.unsigned_abs()),
    ] {
        assert_eq!(size(value), Ok(bytes), "{value}");
    }
    for value in [
        "0",
        "0M",
        "M",
        "64 M",
        "64MB",
        "-1",
        "+1",
        "1.5M",
        "64T",
        "8589934592G",
        "17179869184G",
        "1k1",
        "9223372036854775808",
        "18446744073709551616",
        "99999999999999999999",
        " 64M",
        "64M ",
        "\u{ff11}M",
    ] {
        assert_eq!(
            size(value).unwrap_err().to_string(),
            "UPLOAD_MAX_SIZE is invalid: must be a size in bytes, or with a K, M or G suffix, such as 64M",
            "{value}"
        );
    }
}

#[test]
fn empty_values_count_as_unset() {
    let config = from_map(&with(&[("DB_PREFIX", ""), ("MODE", "")])).unwrap();
    assert_eq!(config.database.prefix.as_str(), "wp_");
    assert_eq!(config.mode, Mode::Shadow);
    assert_eq!(
        from_map(&map(&[("DB_NAME", ""), ("DB_USER", "wp")])),
        Err(ConfigError::Missing("DB_NAME"))
    );
}

#[test]
fn rejects_bad_values() {
    let cases: &[(&str, &str, &str)] = &[
        ("DB_PREFIX", "wp-", "only letters, digits and underscores"),
        ("DB_PORT", "0", "must be a port number from 1 to 65535"),
        ("PORT", "70000", "must be a port number from 1 to 65535"),
        ("BIND_HOST", "localhost", "not an IP address"),
        ("BEHIND_PROXY", "maybe", "must be true or false"),
        ("DB_READ_PASSWORD", "hunter3", "is set without DB_READ_USER"),
        ("MODE", "write", "must be shadow or primary"),
        ("REST_PUBLIC_USERS", "true", "must be on or off"),
        ("HTTP_OUTBOUND", "yes", "must be on or off"),
        (
            "EMPTY_TRASH_DAYS",
            "-1",
            "must be a number of days, 0 or more",
        ),
        (
            "EMPTY_TRASH_DAYS",
            "7 days",
            "must be a number of days, 0 or more",
        ),
        (
            "LOGIN_LIMIT_ADDRESS",
            "0",
            "must be a whole number from 1 to 1000",
        ),
        (
            "LOGIN_LIMIT_ACCOUNT",
            "1001",
            "must be a whole number from 1 to 1000",
        ),
        (
            "LOGIN_LIMIT_WINDOW",
            "86401",
            "must be a whole number from 1 to 86400",
        ),
        (
            "LOGIN_LIMIT_WINDOW",
            "15m",
            "must be a whole number from 1 to 86400",
        ),
    ];
    for &(key, value, reason) in cases {
        assert_eq!(
            from_map(&with(&[(key, value)])).unwrap_err().to_string(),
            format!("{key} is invalid: {reason}")
        );
    }
    assert_eq!(
        from_map(&map(&[("DB_NAME", "wordpress")])),
        Err(ConfigError::Missing("DB_USER"))
    );
    for value in ["false", "0", "no", "1", "yes"] {
        assert!(from_map(&with(&[("BEHIND_PROXY", value)])).is_ok());
    }
}

#[test]
fn no_file_and_no_environment_means_setup_mode() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load(dir.path(), &HashMap::new()), Ok(None));
}

#[test]
fn environment_alone_is_enough() {
    let dir = tempfile::tempdir().unwrap();
    let config = load(dir.path(), &minimal()).unwrap().unwrap();
    assert_eq!(config.database.name, "wordpress");
}

#[test]
fn environment_overrides_file() {
    let dir = tempfile::tempdir().unwrap();
    write_env(
        dir.path(),
        "DB_NAME=from_file\nDB_USER=wp\nPORT=9000\n# comment\n",
        0o600,
    );
    let config = load(dir.path(), &map(&[("PORT", "9100")]))
        .unwrap()
        .unwrap();
    assert_eq!(config.database.name, "from_file");
    assert_eq!(config.server.port, 9100);
}

#[test]
fn refuses_readable_env_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_env(dir.path(), "DB_NAME=x\n", 0o644);
    let err = load(dir.path(), &HashMap::new()).unwrap_err();
    assert_eq!(
        err,
        ConfigError::InsecurePermissions {
            path: path.clone(),
            mode: 0o644,
        }
    );
    assert_eq!(
        err.to_string(),
        format!(
            "{} has mode 644; it holds secrets and must be readable by its owner only (chmod 600)",
            path.display()
        )
    );
}

#[test]
fn reports_unparsable_env_file() {
    let dir = tempfile::tempdir().unwrap();
    write_env(dir.path(), "DB_NAME=\"unterminated\n", 0o600);
    let err = load(dir.path(), &HashMap::new()).unwrap_err();
    assert!(matches!(err, ConfigError::Read { .. }));
    assert!(err.to_string().starts_with("cannot read "));
}

#[test]
fn reports_unreadable_env_file() {
    let dir = tempfile::tempdir().unwrap();
    write_env(dir.path(), "DB_NAME=x\n", 0o000);
    // Root can read anything; the binary refuses to run as root anyway.
    let err = load(dir.path(), &HashMap::new()).unwrap_err();
    assert!(matches!(err, ConfigError::Read { .. }), "{err}");
}

#[test]
fn permission_check_reports_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let err = check_permissions(&dir.path().join("missing")).unwrap_err();
    assert!(matches!(err, ConfigError::Read { .. }));
}

#[test]
fn secrets_are_redacted() {
    let secret = Secret::new("hunter2");
    assert_eq!(format!("{secret:?}"), "Secret([redacted])");
    let config = from_map(&with(&[("DB_PASSWORD", "hunter2"), ("AUTH_KEY", "k3y")])).unwrap();
    let debug = format!("{config:?}");
    assert!(!debug.contains("hunter2") && !debug.contains("k3y"));
    assert_eq!(
        ConfigError::Missing("DB_NAME").to_string(),
        "DB_NAME is not set"
    );
    let err: Box<dyn std::error::Error> = Box::new(ConfigError::Missing("DB_NAME"));
    assert!(err.source().is_none());
}

#[test]
fn prefixes_are_validated() {
    assert_eq!(Prefix::new("hs2_").unwrap().as_str(), "hs2_");
    assert_eq!(Prefix::default().as_str(), "wp_");
    for bad in ["wp-", "wp_;", "wp `x`", "\u{e9}"] {
        assert_eq!(Prefix::new(bad), None, "{bad:?}");
    }
}
