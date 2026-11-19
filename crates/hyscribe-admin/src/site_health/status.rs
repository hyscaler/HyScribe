// SPDX-License-Identifier: AGPL-3.0-or-later

//! Site Health's Status tab, `site-health.php`: HyScribe's own checks
//! as the `direct` results `WP_Site_Health` hands the page's
//! script, which sorts them into critical, recommended and good. Checks
//! that mean what WordPress's do carry its labels and words; the rest are
//! documented in docs/ADMIN.md.

use hyscribe_kses::esc_url;
use hyscribe_phpser::Value;

use super::HealthSite;
use crate::admin_bar::BarSite;
use crate::assets::Urls;
use crate::media::size_format;
use crate::page::{VERSION, object, text};
use crate::text as lossy;

/// `MB_IN_BYTES`.
const MB: u64 = 1 << 20;

/// The versions `WP_Site_Health` requires and recommends.
const MYSQL_REQUIRED: &str = "5.5";
const MYSQL_RECOMMENDED: &str = "8.0";
const MARIADB_RECOMMENDED: &str = "10.11";

const DISK_LABEL: &str = "Disk space available to safely perform updates";

/// The `.env` file beside the binary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnvFile {
    /// Settings come from the environment alone.
    Missing,
    /// The file's permission bits.
    Mode(u32),
    /// Its metadata could not be read.
    Unreadable,
}

/// What the checks ask of the machine HyScribe runs on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Environment {
    /// The `.env` file beside the binary.
    pub env_file: EnvFile,
    /// The bytes free where the `hyscribe/` folder is, if known.
    pub disk_free: Option<u64>,
    /// The pid `run/hyscribe.pid` names while a server holds it.
    pub service_pid: Option<u32>,
    /// This process's pid.
    pub pid: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Level {
    Good,
    Recommended,
    Critical,
}

/// A result of a test, as `perform_test()` returns it.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Check {
    pub(crate) label: String,
    pub(crate) level: Level,
    /// The badge's label: Performance, or Security.
    pub(crate) badge: &'static str,
    pub(crate) description: String,
    pub(crate) actions: String,
    pub(crate) test: &'static str,
}

impl Check {
    fn new(test: &'static str, badge: &'static str, label: impl Into<String>) -> Check {
        Check {
            label: label.into(),
            level: Level::Good,
            badge,
            description: String::new(),
            actions: String::new(),
            test,
        }
    }

    fn level(self, level: Level, label: impl Into<String>) -> Check {
        Check {
            level,
            label: label.into(),
            ..self
        }
    }

    fn describe(self, description: impl Into<String>) -> Check {
        Check {
            description: description.into(),
            ..self
        }
    }

    /// The result as the page's script receives it.
    pub(crate) fn value(&self) -> Value {
        let level = match self.level {
            Level::Good => "good",
            Level::Recommended => "recommended",
            Level::Critical => "critical",
        };
        object(vec![
            ("label", text(&self.label)),
            ("status", text(level)),
            (
                "badge",
                object(vec![("label", text(self.badge)), ("color", text("blue"))]),
            ),
            ("description", text(&self.description)),
            ("actions", text(&self.actions)),
            ("test", text(self.test)),
        ])
    }
}

/// An external link as the checks print it, opening a new tab.
fn external(url: &str, label: &str) -> String {
    format!(
        "<p><a href=\"{url}\" target=\"_blank\">{label}<span class=\"screen-reader-text\"> (opens in a new tab)</span><span aria-hidden=\"true\" class=\"dashicons dashicons-external\"></span></a></p>"
    )
}

/// The version of HyScribe, and of WordPress it stands in for; it never
/// looks for an update.
pub(crate) fn hyscribe_version() -> Check {
    let version = env!("CARGO_PKG_VERSION");
    Check::new(
        "hyscribe_version",
        "Performance",
        format!("Your version of HyScribe ({version})"),
    )
    .describe(format!(
        "<p>This site runs HyScribe {version} in place of WordPress {}. HyScribe does not look for new versions by itself: to update, replace the binary with the new release and restart the service.</p>",
        VERSION.trim()
    ))
}

/// The leading dotted number of a version, as numbers.
fn numbers(version: &str) -> Vec<u64> {
    let end = version
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(version.len());
    version
        .get(..end)
        .unwrap_or_default()
        .split('.')
        .map(|part| part.parse().unwrap_or(0))
        .collect()
}

/// `version_compare( $wanted, $server, '<=' )` over the numbers alone.
fn at_least(server: &str, wanted: &str) -> bool {
    let (mut server, mut wanted) = (numbers(server), numbers(wanted));
    let length = server.len().max(wanted.len());
    server.resize(length, 0);
    wanted.resize(length, 0);
    server >= wanted
}

/// `get_test_sql_server()` for the `SELECT VERSION()` the server gave.
pub(crate) fn sql_server(server: &str) -> Check {
    let mariadb = server.to_ascii_lowercase().contains("mariadb");
    let (engine, recommended) = if mariadb {
        ("MariaDB", MARIADB_RECOMMENDED)
    } else {
        ("MySQL", MYSQL_RECOMMENDED)
    };
    let mut check = Check::new("sql_server", "Performance", "SQL server is up to date");
    let mut description = String::from(
        "<p>The SQL server is a required piece of software for the database WordPress uses to store all your site&#8217;s content and settings.</p>",
    );
    if !at_least(server, recommended) {
        check.level = Level::Recommended;
        check.label = "Outdated SQL server".into();
        description.push_str(&format!("<p>For optimal performance and security reasons, you should consider running {engine} version {recommended} or higher. Contact your web hosting company to correct this.</p>"));
    }
    if !at_least(server, MYSQL_REQUIRED) {
        check.level = Level::Critical;
        check.label = "Severely outdated SQL server".into();
        check.badge = "Security";
        description.push_str(&format!("<p>WordPress requires {engine} version {MYSQL_REQUIRED} or higher. Contact your web hosting company to correct this.</p>"));
    }
    Check {
        description,
        actions: external(
            "https://wordpress.org/about/requirements/",
            "Learn more about what WordPress requires to run.",
        ),
        ..check
    }
}

/// `get_test_https_status()` for the addresses the site is set to; there is
/// no connection to tell `is_ssl()` from, and no request to find out
/// whether HTTPS is supported.
pub(crate) fn https_status(site: &dyn HealthSite) -> Check {
    let https = |url: String| url.to_ascii_lowercase().starts_with("https:");
    let home = https(BarSite::home_url(site));
    let address = https(Urls::site_url(site));
    let link = |anchor: &str| {
        let url = BarSite::admin_url(site, "options-general.php") + "#" + anchor;
        lossy(&esc_url(url.as_bytes()))
    };
    let check = Check {
        actions: external(
            "https://developer.wordpress.org/advanced-administration/security/https/",
            "Learn more about why you should use HTTPS",
        ),
        ..Check::new(
            "https_status",
            "Security",
            "Your website is using an active HTTPS connection",
        )
    };
    if home && address {
        return check.describe("<p>An HTTPS connection is a more secure way of browsing the web. Many services now have HTTPS as a requirement. HTTPS allows you to take advantage of new features that can increase site speed, improve search rankings, and gain the trust of your visitors by helping to protect their online privacy.</p>");
    }
    let what = if address {
        format!(
            "Your <a href=\"{}\">Site Address</a> is not set up to use HTTPS.",
            link("home")
        )
    } else {
        format!(
            "Your <a href=\"{}\">WordPress Address</a> and <a href=\"{}\">Site Address</a> are not set up to use HTTPS.",
            link("siteurl"),
            link("home")
        )
    };
    check
        .level(Level::Recommended, "Your website does not use HTTPS")
        .describe(format!(
            "<p>{what}</p><p>HyScribe serves plain HTTP. Put a reverse proxy that terminates HTTPS in front of it, set <code>BEHIND_PROXY</code> in <code>.env</code>, then change the addresses.</p>"
        ))
}

/// The `.env` file's permissions: it holds the database password and the
/// keys, so no one but its owner has any business with it.
pub(crate) fn env_permissions(file: EnvFile) -> Check {
    let only_owner = "The .env file can only be accessed by its owner";
    let check = Check::new("hyscribe_env_permissions", "Security", only_owner);
    let holds = "The <code>.env</code> file holds the database password and the authentication keys of your site.";
    match file {
        EnvFile::Missing => check
            .level(Level::Good, "Settings come from the environment")
            .describe("<p>No <code>.env</code> file was found beside the binary, so every setting comes from the environment of the service.</p>"),
        EnvFile::Unreadable => check
            .level(
                Level::Recommended,
                "The permissions of the .env file could not be checked",
            )
            .describe(format!("<p>{holds} Its permissions could not be read.</p>")),
        EnvFile::Mode(mode) if mode & 0o077 == 0 => check.describe(format!(
            "<p>{holds} Its mode is {:04o}, so no other user can read it.</p>",
            mode & 0o777
        )),
        EnvFile::Mode(mode) => {
            let (level, label) = if mode & 0o007 == 0 {
                (
                    Level::Recommended,
                    "The .env file can be accessed by a group of users",
                )
            } else {
                (
                    Level::Critical,
                    "The .env file can be accessed by every user on this server",
                )
            };
            check.level(level, label).describe(format!(
                "<p>{holds} Its mode is {:04o}, so other users of this server can read or change it.</p><p>Run <code>chmod 600 .env</code> in the HyScribe folder to restrict it to its owner.</p>",
                mode & 0o777
            ))
        }
    }
}

/// `get_test_available_updates_disk_space()` for the disk the `hyscribe/`
/// folder is on, where an update's binary and the uploads go.
pub(crate) fn disk_space(free: Option<u64>) -> Check {
    let size = |bytes: u64| size_format(i64::try_from(bytes).unwrap_or(i64::MAX));
    let (level, description) = match free {
        None => (
            Level::Recommended,
            "Could not determine available disk space for updates.".to_string(),
        ),
        Some(free) if free < 20 * MB => (
            Level::Critical,
            format!(
                "Available disk space is critically low, less than {} available. Proceed with caution, updates may fail.",
                size(20 * MB)
            ),
        ),
        Some(free) if free < 100 * MB => (
            Level::Recommended,
            format!(
                "Available disk space is low, less than {} available.",
                size(100 * MB)
            ),
        ),
        Some(free) => (
            Level::Good,
            format!(
                "<p>{} available disk space was detected, update routines can be performed safely.</p>",
                size(free)
            ),
        ),
    };
    Check::new("available_updates_disk_space", "Security", DISK_LABEL)
        .level(level, DISK_LABEL)
        .describe(description)
}

/// Whether this process is the server `./hyscribe service` controls: the
/// one that holds the pid file.
pub(crate) fn service(service_pid: Option<u32>, pid: u32) -> Check {
    let check = Check::new(
        "hyscribe_service",
        "Performance",
        "HyScribe runs as a service",
    );
    if service_pid == Some(pid) {
        return check.describe(format!(
            "<p>This server holds <code>run/hyscribe.pid</code> (pid {pid}), so <code>./hyscribe service status</code>, <code>stop</code> and <code>restart</code> reach it.</p>"
        ));
    }
    check
        .level(Level::Recommended, "HyScribe does not run as a service")
        .describe("<p>The pid file <code>run/hyscribe.pid</code> does not name this process, so <code>./hyscribe service stop</code> and <code>restart</code> cannot reach it. Start it with <code>./hyscribe service start</code>.</p>")
}

/// `WP_Site_Health::get_tests()`'s direct tests, in HyScribe's order.
pub(crate) fn checks(site: &dyn HealthSite) -> Vec<Check> {
    let env = site.environment();
    vec![
        hyscribe_version(),
        sql_server(&site.database_server().0),
        https_status(site),
        env_permissions(env.env_file),
        disk_space(env.disk_free),
        service(env.service_pid, env.pid),
    ]
}

#[cfg(test)]
mod tests;
