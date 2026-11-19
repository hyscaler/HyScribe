// SPDX-License-Identifier: AGPL-3.0-or-later

//! Site Health's Info tab, `site-health.php?tab=debug`: the sections of
//! `WP_Debug_Data::debug_data()` that hold for HyScribe, and HyScribe's own
//! in place of PHP's server, PHP and Imagick details; and its Status
//! tab, [`status()`], with HyScribe's own checks.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_phpser::{Value, is_empty_string};

use crate::admin_bar::BarSite;
use crate::assets::Urls;
use crate::dashboard::{DashboardSite, site_health_data};
use crate::list_table::fill;
use crate::media::{max_upload, size_format};
use crate::page::{HelpTab, Request, VERSION};
use crate::text;
use crate::tools::{Tool, tool_page};

mod status;

pub use status::{EnvFile, Environment};

const INFO: &str = include_str!("site_health/info.html");
const STATUS: &str = include_str!("site_health/status.html");

const OVERVIEW: &str = "<p>This screen allows you to obtain a health diagnosis of your site, and displays an overall rating of the status of your installation.</p><p>In the Status tab, you can see critical information about your WordPress configuration, along with anything else that requires your attention.</p><p>In the Info tab, you will find all the details about the configuration of your WordPress site, server, and database. There is also an export feature that allows you to copy all of the information about your site to the clipboard, to help solve problems on your site when obtaining support.</p>";

const SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/site-health-screen/\">Documentation on Site Health tool</a></p>";

/// The features core and Twenty Twenty-Five add, `$_wp_theme_features`.
const THEME_FEATURES: &str = "core-block-patterns, post-thumbnails, responsive-embeds, editor-styles, html5, automatic-feed-links, widgets-block-editor, block-templates, post-formats, editor-style";

/// The system of the target HyScribe was built for, after its architecture.
#[cfg(all(target_os = "linux", target_env = "musl"))]
const SYSTEM: &str = "unknown-linux-musl";
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const SYSTEM: &str = "unknown-linux-gnu";
#[cfg(not(all(target_os = "linux", any(target_env = "musl", target_env = "gnu"))))]
const SYSTEM: &str = std::env::consts::OS;

/// The installation as `.env` sets it up (ADR 0005).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    /// The binary's path, inside the `hyscribe/` folder.
    pub binary: String,
    /// `UPLOAD_MAX_SIZE`, in bytes.
    pub upload_max_size: u64,
    /// `DB_USER`.
    pub database_user: String,
    /// `DB_NAME`.
    pub database_name: String,
    /// `DB_CHARSET`.
    pub charset: String,
}

/// What the Info tab asks of the site beyond the dashboard's.
pub trait HealthSite: DashboardSite {
    /// The installation as configured.
    fn install(&self) -> Install;
    /// `MODE` is `primary`.
    fn primary(&self) -> bool;
    /// `LOGIN_LIMIT_ADDRESS`, `LOGIN_LIMIT_ACCOUNT` and `LOGIN_LIMIT_WINDOW`.
    fn login_limits(&self) -> [u32; 3];
    /// `$wpdb->prefix`.
    fn table_prefix(&self) -> String;
    /// The database server's `VERSION()` and the connection's collation.
    fn database_server(&self) -> (String, String);
    /// The `.env` file, the disk and the service, as they are now.
    fn environment(&self) -> Environment;
}

/// A field's `debug`, what the copied text says in place of its value.
enum Debug {
    Text(String),
    Bool(bool),
}

/// A field of a section: its ID, label and value, and whether the copied
/// text leaves it out.
struct Field {
    name: &'static str,
    label: &'static str,
    value: String,
    debug: Option<Debug>,
    private: bool,
}

impl Field {
    fn new(name: &'static str, label: &'static str, value: impl Into<String>) -> Field {
        Field {
            name,
            label,
            value: value.into(),
            debug: None,
            private: false,
        }
    }

    fn debug(self, debug: Debug) -> Field {
        Field {
            debug: Some(debug),
            ..self
        }
    }

    fn private(self) -> Field {
        Field {
            private: true,
            ..self
        }
    }
}

/// A section of `debug_data()`.
struct Section {
    id: &'static str,
    label: &'static str,
    description: &'static str,
    fields: Vec<Field>,
}

impl Section {
    fn new(id: &'static str, label: &'static str, fields: Vec<Field>) -> Section {
        Section {
            id,
            label,
            description: "",
            fields,
        }
    }

    /// A counted list of what HyScribe has none of, drop-ins, plugins or
    /// other themes, which prints nothing, so no count.
    fn none(id: &'static str, label: &'static str) -> Section {
        Section::new(id, label, Vec::new())
    }
}

fn yes(yes: bool) -> &'static str {
    if yes { "Yes" } else { "No" }
}

/// `get_option( $name )` as text.
fn option(site: &dyn HealthSite, name: &str) -> String {
    text(&site.option(name).unwrap_or_default())
}

/// `wp_timezone_string()`: the zone's name, else its offset.
fn timezone_string(site: &dyn HealthSite) -> String {
    let name = option(site, "timezone_string");
    if !name.is_empty() {
        return name;
    }
    offset_string(Value::String(option(site, "gmt_offset").into_bytes()).to_float())
}

/// `wp_timezone_string()` of `gmt_offset`: its whole hours and minutes.
fn offset_string(offset: f64) -> String {
    let sign = if offset < 0.0 { '-' } else { '+' };
    let minutes = (offset.fract() * 60.0).abs() as i64;
    format!("{sign}{:02}:{minutes:02}", offset.trunc().abs() as i64)
}

/// `get_wp_core()`, without the reachability of WordPress.org, which
/// HyScribe never calls, and with no update to offer.
fn core(site: &dyn HealthSite) -> Section {
    let locale = Some(option(site, "WPLANG"))
        .filter(|lang| !lang.is_empty())
        .unwrap_or("en_US".into());
    let user_locale = site
        .user_option("locale")
        .filter(|locale| !locale.is_empty())
        .map_or(locale.clone(), |locale| text(&locale));
    let home = BarSite::home_url(site);
    let permalink = option(site, "permalink_structure");
    // `is_ssl()`: HyScribe serves HTTP, so an https address is a TLS proxy's.
    let ssl = Urls::site_url(site)
        .to_ascii_lowercase()
        .starts_with("https:");
    let register = option(site, "users_can_register");
    let public = option(site, "blog_public");
    let comments = option(site, "default_comment_status");
    let users = site.option("user_count").unwrap_or(b"-1".to_vec());
    let fields = vec![
        Field::new("version", "Version", VERSION.trim()),
        Field::new("site_language", "Site Language", locale),
        Field::new("user_language", "User Language", user_locale),
        Field::new("timezone", "Timezone", timezone_string(site)),
        Field::new(
            "home_url",
            "Home URL",
            home.strip_suffix('/').unwrap_or(&home),
        )
        .private(),
        Field::new("site_url", "Site URL", Urls::site_url(site)).private(),
        Field::new(
            "permalink",
            "Permalink structure",
            if permalink.is_empty() {
                "No permalink structure set"
            } else {
                &permalink
            },
        )
        .debug(Debug::Text(permalink.clone())),
        Field::new("https_status", "Is this site using HTTPS?", yes(ssl)).debug(Debug::Bool(ssl)),
        Field::new("multisite", "Is this a multisite?", "No").debug(Debug::Bool(false)),
        Field::new(
            "user_registration",
            "Can anyone register on this site?",
            yes(!is_empty_string(register.as_bytes())),
        )
        .debug(Debug::Text(register)),
        Field::new(
            "blog_public",
            "Is this site discouraging search engines?",
            yes(is_empty_string(public.as_bytes())),
        )
        .debug(Debug::Text(public)),
        Field::new(
            "default_comment_status",
            "Default comment status",
            if comments == "open" { "Open" } else { "Closed" },
        )
        .debug(Debug::Text(comments)),
        Field::new("environment_type", "Environment type", "production"),
        Field::new(
            "user_count",
            "User count",
            Value::String(users).to_int().to_string(),
        ),
    ];
    Section::new("wp-core", "WordPress", fields)
}

/// `get_wp_active_theme()` for the bundled Twenty Twenty-Five, which is
/// built into the binary: no theme folder to name.
fn active_theme() -> Section {
    let fields = vec![
        Field::new("name", "Name", "Twenty Twenty-Five (twentytwentyfive)"),
        Field::new("version", "Version", "1.5"),
        Field::new("author", "Author", "the WordPress team"),
        Field::new("author_website", "Author website", "https://wordpress.org"),
        Field::new("parent_theme", "Parent theme", "None").debug(Debug::Text("none".into())),
        Field::new("theme_features", "Theme features", THEME_FEATURES),
    ];
    Section::new("wp-active-theme", "Active Theme", fields)
}

/// HyScribe's own section, in place of `get_wp_server()`.
fn hyscribe(site: &dyn HealthSite, install: &Install) -> Section {
    let (mode, debug) = if site.primary() {
        ("Primary", "primary")
    } else {
        ("Shadow (read only)", "shadow")
    };
    let [address, account, window] = site.login_limits();
    let size = max_upload(site);
    let fields = vec![
        Field::new("version", "HyScribe version", env!("CARGO_PKG_VERSION")),
        Field::new("mode", "Mode", mode).debug(Debug::Text(debug.into())),
        Field::new("binary", "Binary location", install.binary.clone()),
        Field::new(
            "build_target",
            "Build target",
            format!("{}-{SYSTEM}", std::env::consts::ARCH),
        ),
        Field::new(
            "upload_max_size",
            "Maximum upload file size (UPLOAD_MAX_SIZE)",
            size_format(size),
        )
        .debug(Debug::Text(size.to_string())),
        Field::new(
            "login_limit_address",
            "Failed logins allowed per address (LOGIN_LIMIT_ADDRESS)",
            address.to_string(),
        ),
        Field::new(
            "login_limit_account",
            "Failed logins allowed per account (LOGIN_LIMIT_ACCOUNT)",
            account.to_string(),
        ),
        Field::new(
            "login_limit_window",
            "Login limit window (LOGIN_LIMIT_WINDOW)",
            format!("{window} seconds"),
        )
        .debug(Debug::Text(window.to_string())),
    ];
    Section {
        description: "HyScribe serves this site in place of PHP and a web server. The limits below are set in its <code>.env</code> file.",
        ..Section::new("hyscribe", "HyScribe", fields)
    }
}

/// `get_wp_database()` through HyScribe's driver.
fn database(site: &dyn HealthSite, install: Install) -> Section {
    let (server, collation) = site.database_server();
    let fields = vec![
        Field::new("extension", "Database Extension", "sqlx-mysql"),
        Field::new("server_version", "Server version", server),
        Field::new("database_user", "Database username", install.database_user).private(),
        Field::new("database_name", "Database name", install.database_name).private(),
        Field::new("database_prefix", "Table prefix", site.table_prefix()).private(),
        Field::new("database_charset", "Database charset", install.charset).private(),
        Field::new("database_collate", "Database collation", collation).private(),
    ];
    Section::new("wp-database", "Database", fields)
}

/// `WP_Debug_Data::debug_data()` as it holds for HyScribe.
fn debug_data(site: &dyn HealthSite) -> Vec<Section> {
    let install = site.install();
    let media = vec![Field::new(
        "max_effective_size",
        "Max effective file size",
        size_format(max_upload(site)),
    )];
    vec![
        core(site),
        Section::none("wp-dropins", "Drop-ins"),
        active_theme(),
        Section::new("wp-parent-theme", "Parent Theme", Vec::new()),
        Section::none("wp-themes-inactive", "Inactive Themes"),
        Section::none("wp-mu-plugins", "Must Use Plugins"),
        Section::none("wp-plugins-active", "Active Plugins"),
        Section::none("wp-plugins-inactive", "Inactive Plugins"),
        Section::new("wp-media", "Media Handling", media),
        hyscribe(site, &install),
        database(site, install),
    ]
}

/// `WP_Debug_Data::format( $info, 'debug' )`: the copied text, without the
/// private fields.
fn format(sections: &[Section]) -> String {
    let mut out = String::from("`\n");
    for section in sections.iter().filter(|section| !section.fields.is_empty()) {
        out.push_str(&format!("### {} ###\n\n", section.id));
        for field in section.fields.iter().filter(|field| !field.private) {
            let value = match &field.debug {
                Some(Debug::Bool(true)) => "true",
                Some(Debug::Bool(false)) => "false",
                Some(Debug::Text(text)) => text,
                None => &field.value,
            };
            let value = if value.is_empty() { "undefined" } else { value };
            out.push_str(&format!("{}: {value}\n", field.name));
        }
        out.push('\n');
    }
    out.push('`');
    out
}

/// The accordion of `site-health-info.php`: a section for each with fields.
fn accordion(sections: &[Section]) -> String {
    let mut out = String::new();
    for section in sections.iter().filter(|section| !section.fields.is_empty()) {
        let id = esc_attr(section.id.as_bytes());
        let description = if section.description.is_empty() {
            String::new()
        } else {
            format!("<p>{}</p>", section.description)
        };
        let rows: String = section
            .fields
            .iter()
            .map(|field| {
                format!(
                    "<tr><th scope=\"row\">{}</th><td>{}</td></tr>",
                    esc_html(field.label.as_bytes()),
                    esc_html(field.value.as_bytes())
                )
            })
            .collect();
        out.push_str(&format!(
            "\t\t\t\t\t<h3 class=\"health-check-accordion-heading\">\n\t\t\t\t<button id=\"health-check-section-{id}\" aria-expanded=\"false\" class=\"health-check-accordion-trigger\" aria-controls=\"health-check-accordion-block-{id}\" type=\"button\">\n\t\t\t\t\t<span class=\"title\">\n\t\t\t\t\t\t{}\t\t\t\t\t\t\t\t\t\t\t</span>\n\t\t\t\t\t\t\t\t\t\t<span class=\"icon\"></span>\n\t\t\t\t</button>\n\t\t\t</h3>\n\n\t\t\t<div id=\"health-check-accordion-block-{id}\" class=\"health-check-accordion-panel\" hidden=\"hidden\">\n\t\t\t\t{description}\t\t\t\t<table class=\"widefat striped health-check-table\">\n\t\t\t\t\t<tbody>\n\t\t\t\t\t{rows}\t\t\t\t\t</tbody>\n\t\t\t\t</table>\n\t\t\t</div>\n",
            esc_html(section.label.as_bytes())
        ));
    }
    out
}

/// `site-health.php?tab=debug`, or `None` for one who may not see it.
pub fn info(site: &dyn HealthSite, request: &Request) -> Option<String> {
    if !site.can("view_site_health_checks") {
        return None;
    }
    let url = |path: &str| text(&esc_url(BarSite::admin_url(site, path).as_bytes()));
    let sections = debug_data(site);
    let content = fill(
        INFO,
        &[
            ("status_url", url("site-health.php?tab")),
            ("info_url", url("site-health.php?tab=debug")),
            ("status_link", url("site-health.php")),
            ("clipboard", esc_attr(format(&sections).as_bytes())),
            ("sections", accordion(&sections)),
        ],
    );
    let help = [HelpTab {
        id: "overview",
        title: "Overview",
        content: OVERVIEW,
    }];
    let tool = Tool {
        file: "site-health.php",
        title: "Site Health - Info",
        help: &help,
        sidebar: SIDEBAR,
        screen_options: "",
        styles: &["site-health"],
        scripts: &["site-health"],
        script_data: &[(
            "site-health",
            site_health_data(site, "site-health", Vec::new()),
        )],
        admin_head: "",
        content: &content,
    };
    tool_page(site, &tool, request)
}

/// `site-health.php`, the Status tab: the frame PHP prints, and HyScribe's
/// checks as the results the page's script lists by status.
pub fn status(site: &dyn HealthSite, request: &Request) -> Option<String> {
    if !site.can("view_site_health_checks") {
        return None;
    }
    let url = |path: &str| text(&esc_url(BarSite::admin_url(site, path).as_bytes()));
    let content = fill(
        STATUS,
        &[
            ("status_url", url("site-health.php?tab")),
            ("info_url", url("site-health.php?tab=debug")),
        ],
    );
    let results = status::checks(site).into_iter().map(|check| check.value());
    let help = [HelpTab {
        id: "overview",
        title: "Overview",
        content: OVERVIEW,
    }];
    let tool = Tool {
        file: "site-health.php",
        title: "Site Health - Status",
        help: &help,
        sidebar: SIDEBAR,
        screen_options: "",
        styles: &["site-health"],
        scripts: &["site-health"],
        script_data: &[(
            "site-health",
            site_health_data(site, "site-health", results.collect()),
        )],
        admin_head: "",
        content: &content,
    };
    tool_page(site, &tool, request)
}

#[cfg(test)]
mod tests;
