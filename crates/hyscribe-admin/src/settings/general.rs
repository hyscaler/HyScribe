// SPDX-License-Identifier: AGPL-3.0-or-later

//! `options-general.php`, with the media library `wp_enqueue_media()`
//! brings for the Site Icon.

use hyscribe_kses::{esc_attr, esc_html};
use hyscribe_phpser::{Value, is_empty_string};
use hyscribe_render::date::{DAYS, Zone, format_unix, site_zone};
use hyscribe_render::formatting::{convert_chars, wptexturize};

use super::{
    Form, SettingsSite, checks, form_option, gmt_offset, option, replaced, settings_fields,
    settings_page, text, url, zone,
};
use crate::admin_bar::BarSite;
use crate::list_table::{checked, fill};
use crate::media::{MEDIA_SCRIPTS, MEDIA_STYLES, enqueue_media};
use crate::page::{HelpTab, Request, json, text as value_text};
use crate::posts::Answer;
use crate::users::{dropdown_roles, role_names};

const CONTENT: &str = include_str!("general.html");
const HEAD: &str = include_str!("general-head.html");
const ZONES: &str = include_str!("zones.html");
const HELP: [HelpTab; 1] = [HelpTab {
    id: "overview",
    title: "Overview",
    content: "<p>The fields on this screen determine some of the basics of your site setup.</p><p>Most themes show the site title at the top of every page, in the title bar of the browser, and as the identifying name for syndicated feeds. Many themes also show the tagline.</p><p>Two terms you will want to know are the WordPress URL and the site URL. The WordPress URL is where the core WordPress installation files are, and the site URL is the address a visitor uses in the browser to go to your site.</p><p>Though the terms refer to two different concepts, in practice, they can be the same address or different. For example, you can have the core WordPress installation files in the root directory (<code>https://example.com</code>), in which case the two URLs would be the same. Or the <a href=\"https://developer.wordpress.org/advanced-administration/server/wordpress-in-directory/\">WordPress files can be in a subdirectory</a> (<code>https://example.com/wordpress</code>). In that case, the WordPress URL and the site URL would be different.</p><p>Both WordPress URL and site URL can start with either <code>http://</code> or <code>https://</code>. A URL starting with <code>https://</code> requires an SSL certificate, so be sure that you have one before changing to <code>https://</code>. With <code>https://</code>, a padlock will appear next to the address in the browser address bar. Both <code>https://</code> and the padlock signal that your site meets some basic security requirements, which can build trust with your visitors and with search engines.</p><p>If you want site visitors to be able to register themselves, check the membership box. If you want the site administrator to register every new user, leave the box unchecked. In either case, you can set a default user role for all new users.</p><p>You can set the language, and WordPress will automatically download and install the translation files (available if your filesystem is writable).</p><p>UTC means Coordinated Universal Time.</p><p>You must click the Save Changes button at the bottom of the screen for new settings to take effect.</p>",
}];

/// The notice of a pending change of the admin email, with a link to
/// cancel it; none without one, or with one to the current address.
fn pending(site: &dyn SettingsSite) -> String {
    let new = option(site, "new_admin_email");
    if is_empty_string(new.as_bytes()) || new == option(site, "admin_email") {
        return String::new();
    }
    let cancel = BarSite::admin_url(site, "options.php?dismiss=new_admin_email");
    format!(
        "<div class=\"notice updated inline\"><p>There is a pending change of the admin email to <code>{}</code>. <a href=\"{}&amp;_wpnonce={}\">Cancel</a></p></div>",
        esc_html(new.as_bytes()),
        url(&cancel),
        site.nonce("dismiss-1-new_admin_email")
    )
}

/// The default role's options: the roles last first, without the
/// administrator and the editor unless one of them is chosen.
fn roles(site: &dyn SettingsSite) -> String {
    let chosen = option(site, "default_role");
    let mut roles = role_names(&site.user_roles().unwrap_or_default());
    roles.retain(|(role, _)| {
        !["administrator", "editor"].contains(&role.as_str()) || *role == chosen
    });
    dropdown_roles(&roles, &chosen)
}

/// `wp_timezone_choice( $zone )`: a valid name the list leaves out first,
/// and `zone` chosen.
pub(super) fn zone_choice(zone: &str) -> String {
    let option = format!(
        "<option value=\"{}\" dir=\"auto\">",
        esc_attr(zone.as_bytes())
    );
    let listed = ZONES.contains(&option);
    let deprecated = match site_zone(zone.as_bytes(), b"") {
        Zone::Named(named) if !listed && named.name == zone => format!(
            "<option selected=\"selected\" value=\"{}\" dir=\"auto\">{}</option>\n",
            esc_attr(zone.as_bytes()),
            esc_html(zone.as_bytes())
        ),
        _ => String::new(),
    };
    let chosen = option.replacen("<option ", "<option selected=\"selected\" ", 1);
    deprecated + &ZONES.replacen(&option, &chosen, 1)
}

/// Whether a named zone is in daylight saving time now, and when that
/// changes next, as the Timezone setting tells.
pub(super) fn zone_info(site: &dyn SettingsSite, name: &str) -> String {
    let Zone::Named(named) = site_zone(name.as_bytes(), b"") else {
        return String::new();
    };
    let now = site.clock().1;
    let state = if named.tz.period(now).0.dst {
        "daylight saving time"
    } else {
        "standard time"
    };
    let next = if named.name != name {
        String::new()
    } else {
        match named.tz.next_transition(now) {
            Some((at, kind)) => format!(
                " {} time begins on: <code>{}</code>.",
                if kind.dst {
                    "Daylight saving"
                } else {
                    "Standard"
                },
                format_unix("F j, Y g:i a", at, zone(site))
            ),
            None => "This timezone does not observe daylight saving time.".into(),
        }
    };
    format!(
        "<p class=\"timezone-info\">\n<span>\n\tThis timezone is currently in {state}.\t<br />\n\t{next}\t</span>\n</p>\n"
    )
}

/// The Timezone setting's choice, its times now and the zone's state.
fn timezone(site: &dyn SettingsSite) -> [(&'static str, String); 4] {
    let offset = gmt_offset(site);
    let stored = option(site, "timezone_string");
    let named = if stored.contains("Etc/GMT") {
        String::new()
    } else {
        stored.clone()
    };
    let (choice, info) = if is_empty_string(named.as_bytes()) {
        let hours = text(&offset.to_php_string());
        let choice = if offset.to_int() == 0 {
            "UTC+0".into()
        } else if hours.starts_with('-') {
            format!("UTC{hours}")
        } else {
            format!("UTC+{hours}")
        };
        (choice, String::new())
    } else {
        let info = zone_info(site, &named);
        (named, info)
    };
    let now = site.clock().1;
    let local = if !is_empty_string(stored.as_bytes()) || !offset.is_empty() {
        format!(
            "\t<span id=\"local-time\">\n\tLocal time is <code>{}</code>.\t</span>\n",
            format_unix("Y-m-d H:i:s", now, zone(site))
        )
    } else {
        String::new()
    };
    [
        ("zones", zone_choice(&choice)),
        ("utc", format_unix("Y-m-d H:i:s", now, Zone::Utc)),
        ("local", local),
        ("zone_info", info),
    ]
}

/// The radio buttons of the date or time format, `formats` first, with a
/// custom format and a preview of the chosen one.
fn formats(site: &dyn SettingsSite, kind: &str, formats: &[&str]) -> String {
    let current = option(site, &format!("{kind}_format"));
    let (now, zone) = (site.clock().1, zone(site));
    let mut out: String = formats
        .iter()
        .map(|format| {
            format!(
                "\t<label><input type='radio' name='{kind}_format' value='{}'{} /> <span class=\"date-time-text format-i18n\">{}</span><code>{}</code></label><br />\n",
                esc_attr(format.as_bytes()),
                checked(current == *format),
                format_unix(format, now, zone),
                esc_html(format.as_bytes())
            )
        })
        .collect();
    out.push_str(&format!(
        "<label><input type=\"radio\" name=\"{kind}_format\" id=\"{kind}_format_custom_radio\" value=\"\\c\\u\\s\\t\\o\\m\"{}/> <span class=\"date-time-text date-time-custom-text\">Custom:<span class=\"screen-reader-text\"> enter a custom {kind} format in the following field</span></span></label><label for=\"{kind}_format_custom\" class=\"screen-reader-text\">Custom {kind} format:</label><input type=\"text\" name=\"{kind}_format_custom\" id=\"{kind}_format_custom\" value=\"{}\" class=\"small-text\" /><br /><p><strong>Preview:</strong> <span class=\"example\">{}</span><span class='spinner'></span>\n</p>",
        checked(!formats.contains(&current.as_str())),
        esc_attr(current.as_bytes()),
        format_unix(&current, now, zone)
    ));
    out
}

/// The Week Starts On options.
fn weekdays(site: &dyn SettingsSite) -> String {
    let start = Value::String(option(site, "start_of_week").into_bytes()).to_int();
    DAYS.iter()
        .enumerate()
        .map(|(i, day)| {
            let selected = if start == i as i64 {
                "selected=\"selected\""
            } else {
                ""
            };
            format!("\n\t<option value='{i}' {selected}>{day}</option>")
        })
        .collect()
}

/// `options-general.php`; `None` with a Site Icon, not ported.
pub fn general(site: &dyn SettingsSite, request: &Request) -> Option<Answer> {
    if Value::String(option(site, "site_icon").into_bytes()).to_int() != 0 {
        return None;
    }
    let name = option(site, "blogname");
    let title = esc_html(&convert_chars(&wptexturize(name.as_bytes(), &[])));
    let disabled = |constant: &str| {
        if site.defined(constant) {
            (" disabled='disabled'".to_string(), " disabled".to_string())
        } else {
            (String::new(), String::new())
        }
    };
    let (siteurl_disabled, siteurl_class) = disabled("WP_SITEURL");
    let (home_disabled, home_class) = disabled("WP_HOME");
    let home_description = if site.defined("WP_HOME") {
        String::new()
    } else {
        "<p class=\"description\" id=\"home-description\">\n\t\tEnter the same address here unless you <a href=\"https://developer.wordpress.org/advanced-administration/server/wordpress-in-directory/\">want your site home page to be different from your WordPress installation directory</a>.</p>\n".into()
    };
    let mut values = vec![
        ("fields", settings_fields(site, "general", request.uri)),
        ("blogname", form_option(site, "blogname")),
        ("blogdescription", form_option(site, "blogdescription")),
        ("title", title),
        ("site_icon", form_option(site, "site_icon")),
        ("siteurl", form_option(site, "siteurl")),
        ("siteurl_disabled", siteurl_disabled),
        ("siteurl_class", siteurl_class),
        ("home", form_option(site, "home")),
        ("home_disabled", home_disabled),
        ("home_class", home_class),
        ("home_description", home_description),
        ("admin_email", form_option(site, "admin_email")),
        ("pending", pending(site)),
        ("register", checks(site, "users_can_register", "1").into()),
        ("roles", roles(site)),
        (
            "date_formats",
            formats(
                site,
                "date",
                &["F j, Y", "Y-m-d", "m/d/Y", "d/m/Y", "d.m.Y"],
            ),
        ),
        (
            "time_formats",
            formats(site, "time", &["g:i a", "g:i A", "H:i"]),
        ),
        ("weekdays", weekdays(site)),
    ];
    values.extend(timezone(site));
    let content = fill(CONTENT, &values);
    let home = BarSite::home_url(site);
    let head = HEAD.replace(
        "%home%",
        &json(&value_text(home.strip_suffix('/').unwrap_or(&home))),
    );
    let (templates, script_data) = enqueue_media(site, request.browser);
    let form = Form {
        file: "options-general.php",
        title: "General Settings",
        help: &HELP,
        content: &content,
        admin_head: &head,
        late_scripts: &[
            MEDIA_SCRIPTS[0],
            MEDIA_SCRIPTS[1],
            MEDIA_SCRIPTS[2],
            MEDIA_SCRIPTS[3],
            "site-icon",
        ],
        late_styles: &MEDIA_STYLES,
        script_data: &script_data,
        admin_footer: &templates,
        ..Form::default()
    };
    // The screen sets the global `$locale` to the installed language
    // chosen, none, which the session check's login link then carries.
    Some(replaced(
        settings_page(site, &form, request),
        "&#038;wp_lang=en_US",
        "&#038;wp_lang",
    ))
}
