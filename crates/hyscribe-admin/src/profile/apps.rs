// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Application Passwords section of the profile: its offer over HTTPS,
//! the list of a user's passwords and the templates the script fills.

use hyscribe_kses::{esc_attr, esc_html};
use hyscribe_phpser::{Maybe, Value, maybe_unserialize};
use hyscribe_render::date::{Zone, format_unix};

use crate::list_table::{Column, Order, column_headers, fill};
use crate::page::{json, object, text};
use crate::posts::hidden_columns;
use crate::profile::ProfileSite;
use crate::text as lossy;

const HTTP: &str = include_str!("apps-http.html");
const HTTPS: &str = include_str!("apps.html");
const TEMPLATES: &str = include_str!("apps-templates.html");

/// The list's screen, `WP_Application_Passwords_List_Table`'s.
const SCREEN: &str = "application-passwords-user";

/// The columns, `WP_Application_Passwords_List_Table::get_columns()`.
const COLUMNS: [(&str, &str); 5] = [
    ("name", "Name"),
    ("created", "Created"),
    ("last_used", "Last Used"),
    ("last_ip", "Last IP"),
    ("revoke", "Revoke"),
];

const TOGGLE: &str = "<button type=\"button\" class=\"toggle-row\"><span class=\"screen-reader-text\">Show more details</span></button>";

/// One of `_application_passwords`' items.
pub(super) struct Item {
    uuid: String,
    name: String,
    created: i64,
    last_used: i64,
    last_ip: String,
}

/// A string entry of `item`, `None` when it is not one.
fn string(item: &Value, key: &str) -> Option<String> {
    match item.get_str(key)? {
        Value::String(bytes) => Some(lossy(bytes)),
        _ => None,
    }
}

/// A timestamp entry of `item`, 0 for none; `None` for any other type.
fn stamp(item: &Value, key: &str) -> Option<i64> {
    match item.get_str(key)? {
        Value::Null => Some(0),
        Value::Int(time) => Some(*time),
        _ => None,
    }
}

/// An address entry of `item`, empty for none; `None` for any other type.
fn address(item: &Value, key: &str) -> Option<String> {
    match item.get_str(key)? {
        Value::Null => Some(String::new()),
        _ => string(item, key),
    }
}

/// `WP_Application_Passwords::get_user_application_passwords()`, newest
/// first. `None` where PHP would write the passwords back (an item without
/// a `uuid`) or warn of a missing entry.
pub(super) fn items(stored: Option<Vec<u8>>) -> Option<Vec<Item>> {
    let stored = stored.unwrap_or_default();
    let Maybe::Value(Value::Array(list)) = maybe_unserialize(&stored) else {
        return Some(Vec::new());
    };
    let mut items = Vec::new();
    for (_, item) in list.iter().rev() {
        items.push(Item {
            uuid: string(item, "uuid")?,
            name: string(item, "name")?,
            created: stamp(item, "created")?,
            last_used: stamp(item, "last_used")?,
            last_ip: address(item, "last_ip")?,
        });
    }
    Some(items)
}

/// `date_i18n( 'F j, Y', $time )`, or a dash for none.
fn date(time: i64) -> String {
    if time == 0 {
        return "&mdash;".into();
    }
    format_unix("F j, Y", time, Zone::Utc)
}

/// The classes of a column's cell, hidden when the user hid it.
fn classes(key: &str, hidden: &[&str]) -> String {
    let primary = if key == "name" {
        " has-row-actions column-primary"
    } else {
        ""
    };
    let hide = if hidden.contains(&key) { " hidden" } else { "" };
    format!("{key} column-{key}{primary}{hide}")
}

/// A row of the list, `single_row()`.
fn row(item: &Item, hidden: &[&str]) -> String {
    let uuid = esc_attr(item.uuid.as_bytes());
    let label = esc_attr(format!("Revoke \"{}\"", item.name).as_bytes());
    let mut out = format!("<tr data-uuid=\"{uuid}\">");
    for (key, heading) in COLUMNS {
        let class = classes(key, hidden);
        let cell = match key {
            "name" => format!("{}{TOGGLE}", esc_html(item.name.as_bytes())),
            "created" => date(item.created),
            "last_used" => date(item.last_used),
            "last_ip" if item.last_ip.is_empty() => "&mdash;".into(),
            "last_ip" => esc_html(item.last_ip.as_bytes()),
            _ => format!(
                "<button type=\"button\" name=\"revoke-application-password-{uuid}\" id=\"revoke-application-password-{uuid}\" class=\"button delete\" aria-label=\"{label}\">Revoke</button>"
            ),
        };
        let (tag, scope) = if key == "name" {
            ("th", " scope=\"row\"")
        } else {
            ("td", "")
        };
        out.push_str(&format!(
            "<{tag} class='{class}' data-colname=\"{heading}\"{scope}>{cell}</{tag}>"
        ));
    }
    out + "</tr>"
}

/// `print_js_template_row()`, the row the script adds.
fn js_row(hidden: &[&str]) -> String {
    let mut out = String::from("<tr data-uuid=\"{{ data.uuid }}\">");
    for (key, heading) in COLUMNS {
        let class = classes(key, hidden);
        let cell = match key {
            "name" => format!("{{{{ data.name }}}}{TOGGLE}"),
            "created" => "<# print( wp.date.dateI18n( \"F j, Y\", data.created ) ) #>".into(),
            "last_used" => "<# print( data.last_used !== null ? wp.date.dateI18n( \"F j, Y\", data.last_used ) : '\u{2014}' ) #>".into(),
            "last_ip" => "{{ data.last_ip || '\u{2014}' }}".into(),
            _ => "<button type=\"button\" class=\"button delete\" aria-label=\"Revoke &quot;{{ data.name }}&quot;\">Revoke</button>".into(),
        };
        out.push_str(&format!(
            "<td class=\"{class}\" data-colname=\"{heading}\">{cell}</td>"
        ));
    }
    out + "</tr>"
}

/// `wpApiSettings`, which `wp-api-request` carries.
pub(super) fn api_settings(site: &dyn ProfileSite) -> String {
    let settings = object(vec![
        ("root", text(&site.fill("%rest_url%"))),
        ("nonce", text(&site.nonce("wp_rest"))),
        ("versionString", text("wp/v2/")),
    ]);
    format!("var wpApiSettings = {};", json(&settings))
}

/// What the profile of user `id` prints for application passwords: the
/// section, and the templates after the form. The offer needs HTTPS; `None`
/// for stored passwords PHP would repair or warn of.
pub(super) fn section(site: &dyn ProfileSite, id: u64, https: bool) -> Option<(String, String)> {
    if !https {
        return Some((HTTP.into(), String::new()));
    }
    let items = items(site.application_passwords(id))?;
    let hidden = hidden_columns(site, SCREEN);
    let hidden: Vec<&str> = hidden.iter().map(String::as_str).collect();
    let columns: Vec<Column> = COLUMNS
        .iter()
        .map(|(key, heading)| Column {
            key,
            label: (*heading).into(),
            sort: None,
            primary: *key == "name",
        })
        .collect();
    let headers = |with_id| column_headers(&columns, &hidden, &Order::default(), "", 0, with_id);
    let shown = columns.iter().filter(|c| !hidden.contains(&c.key)).count();
    let rows = if items.is_empty() {
        format!(
            "<tr class=\"no-items\"><td class=\"colspanchange\" colspan=\"{shown}\">No items found.</td></tr>"
        )
    } else {
        items.iter().map(|item| row(item, &hidden)).collect()
    };
    let section = fill(
        HTTPS,
        &[
            ("head", headers(true)),
            ("foot", headers(false)),
            ("rows", rows),
        ],
    );
    let templates = format!("{TEMPLATES}{}\t</script>\n", js_row(&hidden));
    Some((section, templates))
}
