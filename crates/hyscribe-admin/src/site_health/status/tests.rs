// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::page::json;
use crate::tests::Role;

fn level(check: &Check) -> &'static str {
    match check.level {
        Level::Good => "good",
        Level::Recommended => "recommended",
        Level::Critical => "critical",
    }
}

/// Versions compare as their numbers, not as text, and a missing part is a
/// zero.
#[test]
fn versions_compare_by_their_numbers() {
    assert_eq!(numbers("11.8.9-MariaDB-ubu2404"), [11, 8, 9]);
    assert_eq!(numbers("8.0"), [8, 0]);
    assert_eq!(numbers(""), [0]);
    assert_eq!(numbers("MariaDB"), [0]);
    for (server, wanted, expected) in [
        ("10.9", "10.11", false),
        ("10.11", "10.11", true),
        ("10.11.0", "10.11", true),
        ("10.11", "10.11.1", false),
        ("8", "8.0", true),
        ("5.7.44-log", "5.5", true),
        ("", "5.5", false),
    ] {
        assert_eq!(at_least(server, wanted), expected, "{server} {wanted}");
    }
}

/// The SQL server as `get_test_sql_server()` words it: up to date, outdated
/// for each engine, and too old to run.
#[test]
fn the_sql_server_is_graded_as_php_s() {
    let good = sql_server("11.8.9-MariaDB-ubu2404");
    assert_eq!((level(&good), good.badge), ("good", "Performance"));
    assert_eq!(good.label, "SQL server is up to date");
    assert_eq!(good.test, "sql_server");
    assert_eq!(
        good.description,
        "<p>The SQL server is a required piece of software for the database HyScribe uses to store all your site&#8217;s content and settings.</p>"
    );
    assert_eq!(
        good.actions,
        "<p><a href=\"https://wordpress.org/about/requirements/\" target=\"_blank\">Learn more about what HyScribe requires to run.<span class=\"screen-reader-text\"> (opens in a new tab)</span><span aria-hidden=\"true\" class=\"dashicons dashicons-external\"></span></a></p>"
    );
    assert_eq!(level(&sql_server("10.11.0-MariaDB")), "good");
    assert_eq!(level(&sql_server("8.0.36-0ubuntu0.22.04.1")), "good");
    let old = sql_server("10.6.18-MariaDB");
    assert_eq!((level(&old), old.badge), ("recommended", "Performance"));
    assert_eq!(old.label, "Outdated SQL server");
    assert!(old.description.ends_with("<p>For optimal performance and security reasons, you should consider running MariaDB version 10.11 or higher. Contact your web hosting company to correct this.</p>"));
    let mysql = sql_server("5.7.44");
    assert_eq!(level(&mysql), "recommended");
    assert!(
        mysql
            .description
            .contains("running MySQL version 8.0 or higher")
    );
    assert_eq!(level(&sql_server("5.5.0")), "recommended");
    let ancient = sql_server("5.1.73");
    assert_eq!((level(&ancient), ancient.badge), ("critical", "Security"));
    assert_eq!(ancient.label, "Severely outdated SQL server");
    assert!(
        ancient
            .description
            .contains("running MySQL version 8.0 or higher")
    );
    assert!(ancient.description.ends_with("<p>HyScribe requires MySQL version 5.5 or higher. Contact your web hosting company to correct this.</p>"));
    let maria = sql_server("5.1.0-MariaDB");
    assert!(maria.description.ends_with("<p>HyScribe requires MariaDB version 5.5 or higher. Contact your web hosting company to correct this.</p>"));
    assert!(
        maria
            .description
            .contains("MariaDB version 10.11 or higher")
    );
}

/// HTTPS as `get_test_https_status()` words the addresses it finds.
#[test]
fn https_is_graded_by_the_addresses() {
    let good = https_status(&Role("pssl:administrator"));
    assert_eq!((level(&good), good.badge), ("good", "Security"));
    assert_eq!(
        good.label,
        "Your website is using an active HTTPS connection"
    );
    assert!(
        good.description
            .starts_with("<p>An HTTPS connection is a more secure way of browsing the web.")
    );
    assert_eq!(
        good.actions,
        "<p><a href=\"https://developer.wordpress.org/advanced-administration/security/https/\" target=\"_blank\">Learn more about why you should use HTTPS<span class=\"screen-reader-text\"> (opens in a new tab)</span><span aria-hidden=\"true\" class=\"dashicons dashicons-external\"></span></a></p>"
    );
    let both = "<p>Your <a href=\"http://127.0.0.1:8081/hs-admin/options-general#siteurl\">HyScribe Address</a> and <a href=\"http://127.0.0.1:8081/hs-admin/options-general#home\">Site Address</a> are not set up to use HTTPS.</p><p>HyScribe serves plain HTTP.";
    // Neither address is https, or only the Site Address (`home`).
    for role in ["administrator", "h9home:administrator"] {
        let none = https_status(&Role(role));
        assert_eq!(
            (level(&none), none.badge),
            ("recommended", "Security"),
            "{role}"
        );
        assert_eq!(none.label, "Your website does not use HTTPS");
        assert!(none.description.starts_with(both), "{role}");
        assert!(
            none.actions
                .contains("Learn more about why you should use HTTPS")
        );
    }
    let site = https_status(&Role("h9ssl:administrator"));
    assert_eq!(level(&site), "recommended");
    assert!(site.description.starts_with("<p>Your <a href=\"http://127.0.0.1:8081/hs-admin/options-general#home\">Site Address</a> is not set up to use HTTPS.</p><p>HyScribe serves plain HTTP. Put a reverse proxy that terminates HTTPS in front of it, set <code>BEHIND_PROXY</code> in <code>.env</code>, then change the addresses.</p>"));
}

/// Any access by a group or by others to the `.env` file is a finding, by
/// others a critical one.
#[test]
fn the_env_file_is_graded_by_its_mode() {
    for mode in [0o600, 0o400, 0o700, 0o100, 0o600 | 0o170000] {
        let check = env_permissions(EnvFile::Mode(mode));
        assert_eq!(
            (level(&check), check.badge),
            ("good", "Security"),
            "{mode:o}"
        );
        assert_eq!(
            check.label,
            "The .env file can only be accessed by its owner"
        );
        assert!(check.description.contains(&format!(
            "Its mode is {:04o}, so no other user can read it.",
            mode & 0o777
        )));
    }
    for mode in [0o640, 0o660, 0o670, 0o610, 0o070, 0o010, 0o750] {
        let check = env_permissions(EnvFile::Mode(mode));
        assert_eq!(level(&check), "recommended", "{mode:o}");
        assert_eq!(
            check.label,
            "The .env file can be accessed by a group of users"
        );
        assert!(check.description.contains("chmod 600 .env"));
    }
    for mode in [0o644, 0o604, 0o606, 0o007, 0o001, 0o777, 0o674] {
        let check = env_permissions(EnvFile::Mode(mode));
        assert_eq!(level(&check), "critical", "{mode:o}");
        assert_eq!(
            check.label,
            "The .env file can be accessed by every user on this server"
        );
        assert!(
            check
                .description
                .contains(&format!("Its mode is {:04o}, so other users", mode & 0o777))
        );
    }
    let missing = env_permissions(EnvFile::Missing);
    assert_eq!(level(&missing), "good");
    assert_eq!(missing.label, "Settings come from the environment");
    let unreadable = env_permissions(EnvFile::Unreadable);
    assert_eq!(level(&unreadable), "recommended");
    assert_eq!(
        unreadable.label,
        "The permissions of the .env file could not be checked"
    );
    assert_eq!(unreadable.test, "hyscribe_env_permissions");
}

/// Disk space as `get_test_available_updates_disk_space()` grades it:
/// under 20 MB critical, under 100 MB recommended, unknown recommended.
#[test]
fn disk_space_is_graded_as_php_s() {
    let label = "Disk space available to safely perform updates";
    let critical = "Available disk space is critically low, less than 20 MB available. Proceed with caution, updates may fail.";
    let low = "Available disk space is low, less than 100 MB available.";
    let safe = |size: &str| {
        format!(
            "<p>{size} available disk space was detected, update routines can be performed safely.</p>"
        )
    };
    for (free, expected, description) in [
        (
            None,
            "recommended",
            "Could not determine available disk space for updates.".to_string(),
        ),
        (Some(0), "critical", critical.to_string()),
        (Some(20 * MB - 1), "critical", critical.to_string()),
        (Some(20 * MB), "recommended", low.to_string()),
        (Some(100 * MB - 1), "recommended", low.to_string()),
        (Some(100 * MB), "good", safe("100 MB")),
        (Some(5 << 30), "good", safe("5 GB")),
        (Some(u64::MAX), "good", safe("8 EB")),
    ] {
        let result = disk_space(free);
        assert_eq!(
            (level(&result), result.badge),
            (expected, "Security"),
            "{free:?}"
        );
        assert_eq!(result.label, label);
        assert_eq!(result.description, description, "{free:?}");
        assert_eq!(result.test, "available_updates_disk_space");
        assert_eq!(result.actions, "");
    }
}

/// The server counts as the service when the pid file names it.
#[test]
fn the_service_is_the_holder_of_the_pid_file() {
    let running = service(Some(4242), 4242);
    assert_eq!((level(&running), running.badge), ("good", "Performance"));
    assert_eq!(running.label, "HyScribe runs as a service");
    assert!(running.description.contains("(pid 4242)"));
    for named in [None, Some(7)] {
        let other = service(named, 4242);
        assert_eq!(level(&other), "recommended", "{named:?}");
        assert_eq!(other.label, "HyScribe does not run as a service");
        assert!(other.description.contains("./hyscribe service start"));
    }
    assert_eq!(running.test, "hyscribe_service");
}

#[test]
fn the_version_check_names_both_versions() {
    let check = hyscribe_version();
    let version = env!("CARGO_PKG_VERSION");
    assert_eq!(check.label, format!("Your version of HyScribe ({version})"));
    assert_eq!((level(&check), check.badge), ("good", "Performance"));
    assert!(check.description.contains(&format!(
        "HyScribe {version} on a database of version 7.1.3."
    )));
    assert_eq!(check.test, "hyscribe_version");
}

/// A result reaches the script in WordPress's order of keys.
#[test]
fn a_result_is_sent_as_php_sends_it() {
    let check = disk_space(Some(5 << 30));
    assert_eq!(
        json(&check.value()),
        "{\"label\":\"Disk space available to safely perform updates\",\"status\":\"good\",\"badge\":{\"label\":\"Security\",\"color\":\"blue\"},\"description\":\"\\u003Cp\\u003E5 GB available disk space was detected, update routines can be performed safely.\\u003C/p\\u003E\",\"actions\":\"\",\"test\":\"available_updates_disk_space\"}"
    );
    for (check, status) in [
        (env_permissions(EnvFile::Mode(0o644)), "critical"),
        (env_permissions(EnvFile::Mode(0o640)), "recommended"),
    ] {
        assert!(json(&check.value()).contains(&format!("\"status\":\"{status}\"")));
    }
}

/// Every check runs, in order, on the site's facts.
#[test]
fn the_checks_run_on_the_site() {
    let tests = |role| {
        checks(&Role(role))
            .into_iter()
            .map(|check| (check.test, level(&check)))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        tests("pssl:administrator"),
        [
            ("hyscribe_version", "good"),
            ("sql_server", "good"),
            ("https_status", "good"),
            ("hyscribe_env_permissions", "good"),
            ("available_updates_disk_space", "good"),
            ("hyscribe_service", "good"),
        ]
    );
    assert_eq!(
        tests("t34bad:administrator"),
        [
            ("hyscribe_version", "good"),
            ("sql_server", "good"),
            ("https_status", "recommended"),
            ("hyscribe_env_permissions", "critical"),
            ("available_updates_disk_space", "critical"),
            ("hyscribe_service", "recommended"),
        ]
    );
}
