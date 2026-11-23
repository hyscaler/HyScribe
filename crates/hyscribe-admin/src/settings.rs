// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Settings screens: General, Writing and Reading, each a form of
//! options `options.php` saves.

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_phpser::{Float, Key, Value};
use hyscribe_render::date::{DAYS, MONTHS, Zone, override_offset, site_zone};

use crate::list_table::{checked, wp_nonce_field};
use crate::menu::Screen;
use crate::page::{HelpTab, Page, Request, admin_page, json, object, text as value_text};
use crate::posts::Answer;
use crate::text;
use crate::users::UsersSite;

mod general;
mod reading;
mod writing;

pub use general::general;
pub use reading::reading;
pub use writing::writing;

/// What the Settings screens ask of the site beyond the users list's.
pub trait SettingsSite: UsersSite {
    /// Whether `wp-config.php` defines the constant `name`.
    fn defined(&self, name: &str) -> bool;
    /// `wp_generate_password( 8, false )`.
    fn random_key(&self) -> String;
}

const SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/settings-%s-screen/\">Documentation on %S Settings</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// `get_option( $name )`, as stored, empty when unset.
pub(crate) fn option(site: &dyn SettingsSite, name: &str) -> String {
    text(&site.option(name).unwrap_or_default())
}

/// `form_option( $name )`.
pub(crate) fn form_option(site: &dyn SettingsSite, name: &str) -> String {
    esc_attr(&site.option(name).unwrap_or_default())
}

/// `checked( $value, get_option( $name ) )`.
pub(crate) fn checks(site: &dyn SettingsSite, name: &str, value: &str) -> &'static str {
    checked(option(site, name) == value)
}

/// `settings_fields( $group )` for the request `request_uri`.
pub(crate) fn settings_fields(site: &dyn SettingsSite, group: &str, request_uri: &str) -> String {
    format!(
        "<input type='hidden' name='option_page' value='{group}' /><input type=\"hidden\" name=\"action\" value=\"update\" />{}",
        wp_nonce_field(site, &format!("{group}-options"), "_wpnonce", request_uri)
    )
}

/// `wp_dropdown_pages()` of `name` over its `options`, none without.
pub(crate) fn pages_dropdown(name: &str, options: &str) -> String {
    if options.is_empty() {
        return String::new();
    }
    format!(
        "<select name='{name}' id='{name}'>\n\t<option value=\"0\">&mdash; Select &mdash;</option>\n{options}</select>\n"
    )
}

/// `get_option( 'gmt_offset' )` through `wp_timezone_override_offset()`:
/// a named zone's offset now in hours, else the stored one.
fn gmt_offset(site: &dyn SettingsSite) -> Value {
    let zone = option(site, "timezone_string");
    override_offset(zone.as_bytes(), site.clock().1).map_or_else(
        || Value::String(option(site, "gmt_offset").into_bytes()),
        |hours| Value::Float(Float::from_f64(hours)),
    )
}

/// The site's time zone, as `wp_timezone()` reads the options.
fn zone(site: &dyn SettingsSite) -> Zone {
    site_zone(
        option(site, "timezone_string").as_bytes(),
        &gmt_offset(site).to_php_string(),
    )
}

/// The `moment` and `wp-date` settings `wp_default_packages_inline_scripts()`
/// prints from the date and time options.
fn date_scripts(site: &dyn SettingsSite) -> Vec<(&'static str, String)> {
    let list = |names: &[&str], len: usize| {
        Value::Array(
            names
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    (
                        Key::Int(i as i64),
                        value_text(name.get(..len).unwrap_or(name)),
                    )
                })
                .collect(),
        )
    };
    let week = Value::Int(Value::String(option(site, "start_of_week").into_bytes()).to_int());
    let time = value_text(&option(site, "time_format"));
    let date = value_text(&option(site, "date_format"));
    let moment = object(vec![
        ("months", list(&MONTHS, 99)),
        ("monthsShort", list(&MONTHS, 3)),
        ("weekdays", list(&DAYS, 99)),
        ("weekdaysShort", list(&DAYS, 3)),
        ("week", object(vec![("dow", week.clone())])),
        (
            "longDateFormat",
            object(vec![
                ("LT", time.clone()),
                ("LTS", Value::Null),
                ("L", Value::Null),
                ("LL", date.clone()),
                ("LLL", value_text("F j, Y g:i a")),
                ("LLLL", Value::Null),
            ]),
        ),
    ]);
    let string = option(site, "timezone_string");
    let abbr = match site_zone(string.as_bytes(), b"") {
        Zone::Named(named) => named.tz.period(site.clock().1).0.abbreviation.clone(),
        _ => String::new(),
    };
    let offset = gmt_offset(site);
    let formatted = text(&offset.to_php_string())
        .replace(".25", ":15")
        .replace(".5", ":30")
        .replace(".75", ":45");
    let pairs = |pairs: &[(&str, &str)]| {
        object(
            pairs
                .iter()
                .map(|(key, value)| (*key, value_text(value)))
                .collect(),
        )
    };
    let relative = pairs(&[
        ("future", "%s from now"),
        ("past", "%s ago"),
        ("s", "a second"),
        ("ss", "%d seconds"),
        ("m", "a minute"),
        ("mm", "%d minutes"),
        ("h", "an hour"),
        ("hh", "%d hours"),
        ("d", "a day"),
        ("dd", "%d days"),
        ("M", "a month"),
        ("MM", "%d months"),
        ("y", "a year"),
        ("yy", "%d years"),
    ]);
    let settings = object(vec![
        (
            "l10n",
            object(vec![
                ("locale", value_text("en_US")),
                ("months", list(&MONTHS, 99)),
                ("monthsShort", list(&MONTHS, 3)),
                ("weekdays", list(&DAYS, 99)),
                ("weekdaysShort", list(&DAYS, 3)),
                (
                    "meridiem",
                    pairs(&[("am", "am"), ("pm", "pm"), ("AM", "AM"), ("PM", "PM")]),
                ),
                ("relative", relative),
                ("startOfWeek", week),
            ]),
        ),
        (
            "formats",
            object(vec![
                ("time", time),
                ("date", date),
                ("datetime", value_text("F j, Y g:i a")),
                ("datetimeAbbreviated", value_text("M j, Y g:i a")),
            ]),
        ),
        (
            "timezone",
            object(vec![
                ("offset", Value::Float(Float::from_f64(offset.to_float()))),
                ("offsetFormatted", value_text(&formatted)),
                ("string", value_text(&string)),
                ("abbr", value_text(&abbr)),
            ]),
        ),
    ]);
    vec![
        (
            "moment",
            format!("moment.updateLocale( 'en_US', {} );", json(&moment)),
        ),
        (
            "wp-date",
            format!("wp.date.setSettings( {} );", json(&settings)),
        ),
    ]
}

/// `settings_errors()`'s notices of `errors`, each `setting`, `code`,
/// `message` and `type`.
fn notices(errors: &[(Key, Value)]) -> String {
    let mut out = String::new();
    for (_, error) in errors {
        let field = |name| {
            text(
                &error
                    .get_str(name)
                    .map(Value::to_php_string)
                    .unwrap_or_default(),
            )
        };
        let kind = match field("type").as_str() {
            "updated" => "notice-success".to_string(),
            kind @ ("error" | "success" | "warning" | "info") => format!("notice-{kind}"),
            kind => kind.to_string(),
        };
        out.push_str(&format!(
            "<div id='setting-error-{}' class='notice {} settings-error is-dismissible'> \n<p><strong>{}</strong></p></div> \n",
            esc_attr(field("code").as_bytes()),
            esc_attr(kind.as_bytes()),
            field("message")
        ));
    }
    out
}

/// What `options-head.php`'s `settings_errors()` prints after the screen's
/// own `errors`: on the screen a save sends back to (`settings-updated`),
/// the notices it kept. Only read here: the page view's writes delete
/// them in primary mode, as PHP does.
fn options_head(site: &dyn SettingsSite, errors: &str, uri: &str) -> String {
    let query = uri.split_once('?').map_or("", |(_, query)| query);
    let args = hyscribe_phpser::parse_str(query.as_bytes());
    let updated = args
        .get_str("settings-updated")
        .is_some_and(|updated| !updated.is_empty());
    let timeout = site.option("_transient_timeout_settings_errors");
    let expired = timeout.is_some_and(|timeout| Value::String(timeout).to_int() < site.clock().1);
    let kept = site
        .option("_transient_settings_errors")
        .filter(|_| updated && !expired);
    let kept = kept.map(|kept| match hyscribe_phpser::maybe_unserialize(&kept) {
        hyscribe_phpser::Maybe::Value(Value::Array(kept)) => kept,
        _ => Vec::new(),
    });
    format!("{errors}{}", notices(&kept.unwrap_or_default()))
}

/// What a Settings screen adds to the frame.
#[derive(Default)]
pub(crate) struct Form<'a> {
    /// The notices of the settings errors the screen adds itself.
    pub(crate) errors: &'a str,
    pub(crate) file: &'a str,
    pub(crate) title: &'a str,
    pub(crate) help: &'a [HelpTab<'a>],
    /// The help sidebar, when not the usual links.
    pub(crate) sidebar: Option<&'a str>,
    pub(crate) content: &'a str,
    pub(crate) admin_head: &'a str,
    pub(crate) scripts: &'a [&'a str],
    pub(crate) late_scripts: &'a [&'a str],
    pub(crate) late_styles: &'a [&'a str],
    pub(crate) script_data: &'a [(&'a str, String)],
    pub(crate) admin_footer: &'a str,
}

/// The screen in the frame, for those the menu lets in.
pub(crate) fn settings_page(site: &dyn SettingsSite, screen: &Form, request: &Request) -> Answer {
    let name = screen.title.split(' ').next().unwrap_or_default();
    let sidebar = screen.sidebar.map_or_else(
        || {
            SIDEBAR
                .replace("%s", &name.to_ascii_lowercase())
                .replace("%S", name)
        },
        str::to_string,
    );
    let id = screen.file.trim_end_matches(".php");
    let content = options_head(site, screen.errors, request.uri) + screen.content;
    let page = Page {
        screen: Screen {
            pagenow: screen.file,
            typenow: "",
            taxonomy: "",
            parent_file: Some("options-general.php"),
            submenu_file: None,
        },
        id,
        title: screen.title,
        help: screen.help,
        sidebar: &sidebar,
        screen_options: "",
        styles: &[],
        scripts: screen.scripts,
        late_scripts: screen.late_scripts,
        script_data: screen.script_data,
        script_after: &date_scripts(site),
        late_styles: screen.late_styles,
        admin_head: screen.admin_head,
        admin_footer: screen.admin_footer,
        footer_scripts: "",
        content: &content,
        canonical: request.canonical,
        origin: request.origin,
        view: None,
    };
    Answer::from_page(admin_page(site, &page, request.browser))
}

/// The page with `from` replaced by `to` once, for what the screen's hooks
/// print out of the frame's order.
pub(crate) fn replaced(answer: Answer, from: &str, to: &str) -> Answer {
    match answer {
        Answer::Page(page) => Answer::Page(page.replacen(from, to, 1)),
        refused => refused,
    }
}

/// `esc_url()` as text.
pub(crate) fn url(value: &str) -> String {
    text(&esc_url(value.as_bytes()))
}

#[cfg(test)]
pub(crate) mod tests;
