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
            author_archives: false,
            http_outbound: false,
            trusted_proxies: crate::proxies::default_list(),
            request_time_limit: 30,
            rss_allowed_hosts: Vec::new(),
            extension_trusted_keys: Vec::new(),
            extension_registry: None,
            extension_update_check: false,
            embed_allowed: EmbedAllowed {
                providers: true,
                hosts: Vec::new(),
            },
            constants: Constants {
                disallow_unfiltered_html: true,
                allow_unfiltered_uploads: false,
                empty_trash_days: 30,
                media_trash: false,
            },
            login_limits: LoginLimits {
                address: 5,
                account: 10,
                window: 900,
                registrations: Some(3),
            },
            disable_wp_cron: false,
            alternate_wp_cron: false,
            upload_max_size: 64 << 20,
            mail: Mail::default(),
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

/// The relay and sender of ADR 0016: ports by TLS mode, a login only with
/// encryption, plain text only to this machine.
#[test]
fn reads_the_mail_keys() {
    let mail = |pairs: &[(&str, &str)]| from_map(&with(pairs)).map(|config| config.mail);
    let relay = |host: &str, port, tls, login| Smtp {
        host: host.into(),
        port,
        tls,
        login,
    };
    let login = Some(("me".to_string(), Secret::new("pw")));
    assert_eq!(
        mail(&[
            ("SMTP_HOST", "smtp.example.test"),
            ("SMTP_USER", "me"),
            ("SMTP_PASSWORD", "pw"),
            ("MAIL_FROM", "site@example.test"),
        ]),
        Ok(Mail {
            smtp: Some(relay(
                "smtp.example.test",
                587,
                SmtpTls::StartTls,
                login.clone()
            )),
            from: Some("site@example.test".into()),
        })
    );
    let tls = [
        ("SMTP_HOST", "10.0.0.1"),
        ("SMTP_TLS", "tls"),
        ("SMTP_USER", "me"),
    ];
    let no_password = Some(("me".to_string(), Secret::new("")));
    let smtp = relay("10.0.0.1", 465, SmtpTls::Tls, no_password);
    assert_eq!(mail(&tls).map(|mail| mail.smtp), Ok(Some(smtp)));
    for host in ["localhost", "127.0.0.1", "::1"] {
        let none = [
            ("SMTP_HOST", host),
            ("SMTP_TLS", "none"),
            ("SMTP_PORT", "2525"),
        ];
        let smtp = relay(host, 2525, SmtpTls::None, None);
        assert_eq!(mail(&none).map(|mail| mail.smtp), Ok(Some(smtp)));
    }
    let local = [("SMTP_HOST", "localhost"), ("SMTP_TLS", "none")];
    let smtp = relay("localhost", 25, SmtpTls::None, None);
    assert_eq!(mail(&local).map(|mail| mail.smtp), Ok(Some(smtp)));
    // Without a relay the other keys are read but send nothing.
    let off = [("SMTP_TLS", "tls"), ("SMTP_PORT", "2465")];
    assert_eq!(mail(&off), Ok(Mail::default()));
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

/// The cron constants read as every yes or no key, false by default.
#[test]
fn reads_the_cron_constants() {
    let cron = |pairs: &[(&str, &str)]| {
        let config = from_map(&with(pairs))?;
        Ok::<_, ConfigError>((config.disable_wp_cron, config.alternate_wp_cron))
    };
    let set = [("DISABLE_WP_CRON", "TRUE"), ("ALTERNATE_WP_CRON", "1")];
    assert_eq!(cron(&set).unwrap(), (true, true));
    let words = [("DISABLE_WP_CRON", "yes"), ("ALTERNATE_WP_CRON", "On")];
    assert_eq!(cron(&words).unwrap(), (true, true));
    let unset = [("DISABLE_WP_CRON", "0"), ("ALTERNATE_WP_CRON", "")];
    assert_eq!(cron(&unset).unwrap(), (false, false));
    for key in ["DISABLE_WP_CRON", "ALTERNATE_WP_CRON"] {
        assert_eq!(
            cron(&[(key, "maybe")]).unwrap_err().to_string(),
            format!("{key} is invalid: must be true or false")
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
        ("MEDIA_TRASH", "true"),
        ("MULTISITE", "false"),
    ])
    .unwrap();
    assert!(!set.disallow_unfiltered_html && set.allow_unfiltered_uploads && set.media_trash);
    assert!(!set.defined("DISALLOW_UNFILTERED_HTML") && set.defined("ALLOW_UNFILTERED_UPLOADS"));
    assert!(set.defined("MEDIA_TRASH") && !Constants::default().defined("MEDIA_TRASH"));
    let numbers = constants(&[
        ("DISALLOW_UNFILTERED_HTML", "0"),
        ("ALLOW_UNFILTERED_UPLOADS", "1"),
        ("MEDIA_TRASH", "1"),
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
        "MEDIA_TRASH",
    ] {
        for value in ["y", "2", "'true'", " true"] {
            assert_eq!(
                constants(&[(key, value)]).unwrap_err().to_string(),
                format!("{key} is invalid: must be true or false")
            );
        }
    }
    for value in ["true", "TRUE", "1", "Yes", "on"] {
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
        ("AUTHOR_ARCHIVES", "on"),
        ("HTTP_OUTBOUND", "on"),
        (
            "RSS_ALLOWED_HOSTS",
            " Feeds.Example.org,,news.test , feeds.example.org",
        ),
        ("EMPTY_TRASH_DAYS", "0"),
        ("LOGIN_LIMIT_ADDRESS", "1"),
        ("LOGIN_LIMIT_ACCOUNT", "1000"),
        ("LOGIN_LIMIT_WINDOW", "86400"),
        ("REGISTRATION_LIMIT_ADDRESS", "-1"),
        ("TRUSTED_PROXIES", "10.0.0.0/8, 2001:db8::/32"),
        ("REQUEST_TIME_LIMIT", "5"),
        ("UPLOAD_MAX_SIZE", "512k"),
        ("EXTENSION_TRUSTED_KEYS", " RWQa,, RWQb "),
        ("EXTENSION_REGISTRY", "https://mirror.example/hyscribe"),
    ]))
    .unwrap();
    assert_eq!(config.extension_trusted_keys, ["RWQa", "RWQb"]);
    let registry = config.extension_registry.as_deref();
    assert_eq!(registry, Some("https://mirror.example/hyscribe/"));
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
    assert!(config.author_archives);
    assert!(config.http_outbound);
    assert_eq!(config.rss_allowed_hosts, ["feeds.example.org", "news.test"]);
    assert_eq!(config.constants.empty_trash_days, 0);
    assert_eq!(server(&with(&[("PORT", "80")])).map(|s| s.port), Ok(80));
    let limits = LoginLimits {
        address: 1,
        account: 1000,
        window: 86_400,
        registrations: None,
    };
    assert_eq!(config.login_limits, limits);
    assert_eq!(config.request_time_limit, 5);
    assert_eq!(config.trusted_proxies.len(), 2);
    assert_eq!(config.upload_max_size, 512 << 10);
    let on = from_map(&with(&[
        ("REST_PUBLIC_USERS", "on"),
        ("HTTP_OUTBOUND", "off"),
    ]))
    .unwrap();
    assert!(on.rest_public_users);
    assert!(!on.http_outbound);
}

/// The embed allowlist: the providers by default, plus hosts, hosts alone,
/// or nothing.
/// The registration cap: 3 by default, 0 refuses every registration, -1 none.
#[test]
fn reads_the_registration_cap() {
    let cap = |value: &str| {
        from_map(&with(&[("REGISTRATION_LIMIT_ADDRESS", value)]))
            .map(|c| c.login_limits.registrations)
    };
    assert_eq!(cap("0"), Ok(Some(0)));
    assert_eq!(cap("1000"), Ok(Some(1000)));
    assert_eq!(cap("-1"), Ok(None));
    assert!(cap("x").is_err());
}

#[test]
fn embed_hosts_have_forms() {
    let allowed =
        |value: &str| from_map(&with(&[("EMBED_ALLOWED_HOSTS", value)])).map(|c| c.embed_allowed);
    let of = |providers, hosts: &[&str]| EmbedAllowed {
        providers,
        hosts: hosts.iter().map(|h| h.to_string()).collect(),
    };
    assert_eq!(embed_allowed(None), Ok(of(true, &[])));
    for value in ["", " , ", "providers", "Providers,providers"] {
        assert_eq!(allowed(value), Ok(of(true, &[])), "{value:?}");
    }
    assert_eq!(
        allowed("providers, Media.Example.org,media.example.org"),
        Ok(of(true, &["media.example.org"]))
    );
    assert_eq!(
        allowed("a.test,b.test"),
        Ok(of(false, &["a.test", "b.test"]))
    );
    assert_eq!(allowed("none"), Ok(of(false, &[])));
    assert_eq!(allowed("NONE,"), Ok(of(false, &[])));
    for value in ["none,a.test", "https://a.test/", "a.test:8080"] {
        assert!(allowed(value).is_err(), "{value:?}");
    }
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

const MAIL_FROM: &str = "must be one address, such as wordpress@example.com";

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
        ("SMTP_TLS", "ssl", "must be starttls, tls or none"),
        ("SMTP_PORT", "0", "must be a port number from 1 to 65535"),
        ("SMTP_PASSWORD", "pw", "is set without SMTP_USER"),
        ("SMTP_USER", "me", "is set without SMTP_HOST"),
        (
            "SMTP_HOST",
            "smtp example",
            "must be a host name or an IP address",
        ),
        (
            "SMTP_HOST",
            "smtp/x",
            "must be a host name or an IP address",
        ),
        ("MAIL_FROM", "a b@example.test", MAIL_FROM),
        ("MAIL_FROM", "Site <a@example.test>", MAIL_FROM),
        ("MAIL_FROM", "a@example", MAIL_FROM),
        ("MAIL_FROM", "a\"b@example.test", MAIL_FROM),
        ("MAIL_FROM", "a\\b@example.test", MAIL_FROM),
        ("MAIL_FROM", "@example.test", MAIL_FROM),
        ("MAIL_FROM", "example.test", MAIL_FROM),
        (
            "MAIL_FROM",
            "a@example.test\r\nBcc: b@example.test",
            MAIL_FROM,
        ),
        ("REST_PUBLIC_USERS", "public", "must be true or false"),
        ("HTTP_OUTBOUND", "enabled", "must be true or false"),
        ("AUTHOR_ARCHIVES", "public", "must be true or false"),
        (
            "EXTENSION_REGISTRY",
            "http://mirror.example/",
            "must be an https: URL",
        ),
        ("EXTENSION_REGISTRY", "https://", "must be an https: URL"),
        (
            "EXTENSION_REGISTRY",
            "https://a b/",
            "must be an https: URL",
        ),
        (
            "EXTENSION_REGISTRY",
            "https://a/?b",
            "must be an https: URL",
        ),
        (
            "EXTENSION_REGISTRY",
            "https://a/#b",
            "must be an https: URL",
        ),
        (
            "RSS_ALLOWED_HOSTS",
            "https://feeds.example.org/",
            "must be host names separated by commas, without schemes, ports or paths",
        ),
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
        (
            "REGISTRATION_LIMIT_ADDRESS",
            "-2",
            "must be a whole number from 0 to 1000, or -1 for no cap",
        ),
        (
            "REGISTRATION_LIMIT_ADDRESS",
            "1001",
            "must be a whole number from 0 to 1000, or -1 for no cap",
        ),
        (
            "REQUEST_TIME_LIMIT",
            "0",
            "must be a whole number from 1 to 86400",
        ),
        (
            "TRUSTED_PROXIES",
            "10.0.0.0/33",
            "must be at most 256 IP addresses or ranges such as 10.0.0.0/8, separated by commas",
        ),
    ];
    let plain = "none is only for a relay on this machine (localhost, 127.0.0.1 or ::1)";
    let pairs = [("SMTP_HOST", "smtp.example.test"), ("SMTP_TLS", "none")];
    let refused = from_map(&with(&pairs)).unwrap_err().to_string();
    assert_eq!(refused, format!("SMTP_TLS is invalid: {plain}"));
    let pairs = [
        ("SMTP_HOST", "127.0.0.1"),
        ("SMTP_TLS", "none"),
        ("SMTP_USER", "me"),
    ];
    let refused = from_map(&with(&pairs)).unwrap_err().to_string();
    let reason = "none sends no credentials: SMTP_USER needs starttls or tls";
    assert_eq!(refused, format!("SMTP_TLS is invalid: {reason}"));
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
}

/// Every yes or no key reads the same words, in any case, through one
/// parser; missing or empty is the key's default.
#[test]
fn every_boolean_key_takes_the_same_values() {
    let read = |key: &str, value: &str| {
        let config = from_map(&with(&[(key, value)]))?;
        Ok::<_, ConfigError>(match key {
            "BEHIND_PROXY" => config.server.behind_proxy,
            "REST_PUBLIC_USERS" => config.rest_public_users,
            "HTTP_OUTBOUND" => config.http_outbound,
            "AUTHOR_ARCHIVES" => config.author_archives,
            "EXTENSION_UPDATE_CHECK" => config.extension_update_check,
            "DISALLOW_UNFILTERED_HTML" => config.constants.disallow_unfiltered_html,
            _ => config.constants.allow_unfiltered_uploads,
        })
    };
    let keys = [
        ("BEHIND_PROXY", false),
        ("REST_PUBLIC_USERS", true),
        ("HTTP_OUTBOUND", false),
        ("AUTHOR_ARCHIVES", false),
        ("EXTENSION_UPDATE_CHECK", false),
        ("DISALLOW_UNFILTERED_HTML", true),
        ("ALLOW_UNFILTERED_UPLOADS", false),
    ];
    for (key, default) in keys {
        for value in ["true", "TRUE", "yes", "Yes", "on", "ON", "1"] {
            assert_eq!(read(key, value), Ok(true), "{key}={value}");
        }
        for value in ["false", "False", "no", "NO", "off", "Off", "0"] {
            assert_eq!(read(key, value), Ok(false), "{key}={value}");
        }
        assert_eq!(read(key, ""), Ok(default), "{key}");
        assert_eq!(
            read(key, "maybe").unwrap_err().to_string(),
            format!("{key} is invalid: must be true or false")
        );
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

#[test]
fn a_registry_url_ends_in_one_slash() {
    let given = |url| super::registry(url).unwrap();
    assert_eq!(given("https://r.example/v/"), "https://r.example/v/");
    assert_eq!(given("https://r.example"), "https://r.example/");
}
