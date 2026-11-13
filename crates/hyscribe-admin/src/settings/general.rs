// SPDX-License-Identifier: AGPL-3.0-or-later

//! `options-general.php`, with the media library `wp_enqueue_media()`
//! brings for the Site Icon.

use hyscribe_kses::{esc_attr, esc_html};
use hyscribe_phpser::{Key, Value, is_empty_string};
use hyscribe_render::date::{DAYS, MONTHS, Zone, format_unix, site_zone};
use hyscribe_render::formatting::{convert_chars, wptexturize};

use super::{
    Form, SettingsSite, form_option, gmt_offset, option, settings_fields, settings_page, text, url,
    zone,
};
use crate::admin_bar::{BarSite, Browser};
use crate::assets::{cookie_path, registered_script_data};
use crate::list_table::{checked, fill};
use crate::page::{HelpTab, esc_js, json, object, text as value_text};
use crate::posts::Answer;
use crate::users::role_names;

const CONTENT: &str = include_str!("general.html");
const HEAD: &str = include_str!("general-head.html");
const ZONES: &str = include_str!("zones.html");
const MEDIA_TEMPLATES: &str = include_str!("media.html");
const MEDIA_VIEWS: &str = include_str!("media-views.js");

const HELP: [HelpTab; 1] = [HelpTab {
    id: "overview",
    title: "Overview",
    content: "<p>The fields on this screen determine some of the basics of your site setup.</p><p>Most themes show the site title at the top of every page, in the title bar of the browser, and as the identifying name for syndicated feeds. Many themes also show the tagline.</p><p>Two terms you will want to know are the WordPress URL and the site URL. The WordPress URL is where the core WordPress installation files are, and the site URL is the address a visitor uses in the browser to go to your site.</p><p>Though the terms refer to two different concepts, in practice, they can be the same address or different. For example, you can have the core WordPress installation files in the root directory (<code>https://example.com</code>), in which case the two URLs would be the same. Or the <a href=\"https://developer.wordpress.org/advanced-administration/server/wordpress-in-directory/\">WordPress files can be in a subdirectory</a> (<code>https://example.com/wordpress</code>). In that case, the WordPress URL and the site URL would be different.</p><p>Both WordPress URL and site URL can start with either <code>http://</code> or <code>https://</code>. A URL starting with <code>https://</code> requires an SSL certificate, so be sure that you have one before changing to <code>https://</code>. With <code>https://</code>, a padlock will appear next to the address in the browser address bar. Both <code>https://</code> and the padlock signal that your site meets some basic security requirements, which can build trust with your visitors and with search engines.</p><p>If you want site visitors to be able to register themselves, check the membership box. If you want the site administrator to register every new user, leave the box unchecked. In either case, you can set a default user role for all new users.</p><p>You can set the language, and WordPress will automatically download and install the translation files (available if your filesystem is writable).</p><p>UTC means Coordinated Universal Time.</p><p>You must click the Save Changes button at the bottom of the screen for new settings to take effect.</p>",
}];

/// The uploader's buttons and limits, for a browser that can upload.
const UPLOAD: &str = "\t\t\t\t<h2 class=\"upload-instructions drop-instructions\">Drop files to upload</h2>\n\t\t\t\t<p class=\"upload-instructions drop-instructions\">or</p>\n\t\t\t\t<button type=\"button\" class=\"browser button button-hero\" aria-describedby=\"post-upload-info\">Select Files</button>\n\t\t\t</div>\n\n\t\t\t<div class=\"upload-inline-status\"></div>\n\n\t\t\t<div class=\"post-upload-ui\" id=\"post-upload-info\">\n\t\t\t\t\n\t\t\t\t<p class=\"max-upload-size\">\n\t\t\t\tMaximum upload file size: 2 MB.\t\t\t\t</p>\n\n\t\t\t\t<# if ( data.suggestedWidth && data.suggestedHeight ) { #>\n\t\t\t\t\t<p class=\"suggested-dimensions\">\n\t\t\t\t\t\tSuggested image dimensions: {{data.suggestedWidth}} by {{data.suggestedHeight}} pixels.\t\t\t\t\t</p>\n\t\t\t\t<# } #>\n\n\t\t\t\t\t\t\t</div>\n";

/// What the uploader says to a browser that cannot upload.
const NO_UPLOAD: &str = "\t\t\t\t<h2 class=\"upload-instructions\">Your browser cannot upload files</h2>\n\t\t\t\t<p>\n\t\t\t\tThe web browser on your device cannot be used to upload files. You may be able to use the <a href=\"https://apps.wordpress.org/\">native app for your device</a> instead.\t\t\t\t</p>\n\t\t\t</div>\n";

/// The extensions `get_allowed_mime_types()` gives an administrator
/// without unfiltered uploads.
const EXTENSIONS: &str = "jpg,jpeg,jpe,gif,png,bmp,tiff,tif,webp,avif,ico,heic,heif,heics,heifs,asf,asx,wmv,wmx,wm,avi,divx,flv,mov,qt,mpeg,mpg,mpe,mp4,m4v,ogv,webm,mkv,3gp,3gpp,3g2,3gp2,txt,asc,c,cc,h,srt,csv,tsv,ics,rtx,css,vtt,dfxp,mp3,m4a,m4b,aac,ra,ram,wav,x-wav,ogg,oga,flac,mid,midi,wma,wax,mka,rtf,pdf,class,tar,zip,gz,gzip,rar,7z,psd,xcf,doc,pot,pps,ppt,wri,xla,xls,xlt,xlw,mdb,mpp,docx,docm,dotx,dotm,xlsx,xlsm,xlsb,xltx,xltm,xlam,pptx,pptm,ppsx,ppsm,potx,potm,ppam,sldx,sldm,onetoc,onetoc2,onetmp,onepkg,oxps,xps,odt,odp,ods,odg,odc,odb,odf,wp,wpd,key,numbers,pages";

/// `wp_max_upload_size()`, the oracle's `upload_max_filesize`.
const MAX_UPLOAD: &str = "2097152b";

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
    role_names(&site.user_roles().unwrap_or_default())
        .into_iter()
        .rev()
        .filter(|(role, _)| {
            !["administrator", "editor"].contains(&role.as_str()) || *role == chosen
        })
        .map(|(role, name)| {
            let selected = if role == chosen {
                "selected='selected' "
            } else {
                ""
            };
            format!(
                "\n\t<option {selected}value='{}'>{name}</option>",
                esc_attr(role.as_bytes())
            )
        })
        .collect()
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

/// `_device_can_upload()`: not on iOS before 6.
pub(super) fn can_upload(browser: &Browser) -> bool {
    let agent = String::from_utf8_lossy(browser.user_agent);
    if !browser.is_mobile() || !["iPhone", "iPad", "iPod"].iter().any(|d| agent.contains(d)) {
        return true;
    }
    agent
        .split_once("OS ")
        .filter(|(_, rest)| rest.contains(" like Mac OS X"))
        .and_then(|(_, rest)| {
            rest.split(['_', ' '])
                .next()
                .unwrap_or_default()
                .parse::<u32>()
                .ok()
        })
        .is_some_and(|major| major >= 6)
}

/// `wp_plupload_default_settings()`'s data after the registered strings.
fn plupload(site: &dyn SettingsSite, browser: &Browser) -> String {
    let agent = String::from_utf8_lossy(browser.user_agent);
    let ajax = cookie_path(&BarSite::admin_url(site, "async-upload.php"));
    let mut defaults = vec![
        ("file_data_name", value_text("async-upload")),
        ("url", value_text(ajax.strip_suffix('/').unwrap_or(&ajax))),
        (
            "filters",
            object(vec![
                ("max_file_size", value_text(MAX_UPLOAD)),
                (
                    "mime_types",
                    Value::Array(vec![(
                        Key::Int(0),
                        object(vec![("extensions", value_text(EXTENSIONS))]),
                    )]),
                ),
            ]),
        ),
    ];
    if browser.is_mobile() && agent.contains("OS 7_") && agent.contains("like Mac OS X") {
        defaults.push(("multi_selection", Value::Bool(false)));
    }
    defaults.push((
        "multipart_params",
        object(vec![
            ("action", value_text("upload-attachment")),
            ("_wpnonce", value_text(&site.nonce("media-form"))),
        ]),
    ));
    let settings = object(vec![
        ("defaults", object(defaults)),
        (
            "browser",
            object(vec![
                ("mobile", Value::Bool(browser.is_mobile())),
                ("supported", Value::Bool(can_upload(browser))),
            ]),
        ),
        ("limitExceeded", Value::Bool(false)),
    ]);
    format!(
        "{}\nvar _wpPluploadSettings = {};",
        registered_script_data("wp-plupload", "data"),
        json(&settings)
    )
}

/// `wp_enqueue_media()`'s `_wpMediaViewsL10n`, its settings from the
/// site: the months with attachments newest first among them.
fn media_views(site: &dyn SettingsSite) -> String {
    let mut months = site.months("attachment", false);
    months.extend(site.months("attachment", true));
    months.sort_by(|a, b| b.cmp(a));
    months.dedup();
    let months = Value::Array(
        months
            .into_iter()
            .enumerate()
            .map(|(i, (year, month))| {
                let name = MONTHS.get((month as usize).wrapping_sub(1)).copied();
                let name = name.unwrap_or_default();
                let entry = object(vec![
                    ("year", value_text(&year.to_string())),
                    ("month", value_text(&month.to_string())),
                    ("text", value_text(&format!("{name} {year}"))),
                ]);
                (Key::Int(i as i64), entry)
            })
            .collect(),
    );
    let props = object(vec![
        ("link", value_text(&option(site, "image_default_link_type"))),
        ("align", value_text(&option(site, "image_default_align"))),
        ("size", value_text(&option(site, "image_default_size"))),
    ]);
    let scrolling = site.user_option("infinite_scrolling").as_deref() != Some(b"false");
    fill(
        MEDIA_VIEWS,
        &[
            (
                "tab_url",
                json(&value_text(&BarSite::admin_url(
                    site,
                    "media-upload.php?chromeless=1",
                ))),
            ),
            (
                "nonce:media-send-to-editor",
                site.nonce("media-send-to-editor"),
            ),
            (
                "nonce:set-attachment-thumbnail",
                site.nonce("set-attachment-thumbnail"),
            ),
            ("props", json(&props)),
            (
                "proxy",
                json(&value_text(&site.fill("%rest_url%oembed/1.0/proxy"))),
            ),
            ("months", json(&months)),
            ("scrolling", u8::from(scrolling).to_string()),
        ],
    )
}

/// `options-general.php`; `None` with a Site Icon, not ported.
pub fn general(
    site: &dyn SettingsSite,
    canonical: &str,
    origin: &str,
    request_uri: &str,
    browser: &Browser,
) -> Option<Answer> {
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
        ("fields", settings_fields(site, "general", request_uri)),
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
        (
            "register",
            checked(option(site, "users_can_register") == "1").into(),
        ),
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
    let upload = if can_upload(browser) {
        UPLOAD
    } else {
        NO_UPLOAD
    };
    let templates = site.fill(&fill(
        MEDIA_TEMPLATES,
        &[("title", esc_js(&name)), ("upload", upload.into())],
    ));
    let script_data = [
        ("media-views", media_views(site)),
        ("wp-plupload", plupload(site, browser)),
        // `wp_default_scripts()` lists the shortcodes core adds by then.
        (
            "mce-view",
            "var mceViewL10n = {\"shortcodes\":[\"wp_caption\",\"caption\",\"gallery\",\"playlist\",\"audio\",\"video\",\"embed\"]};".into(),
        ),
    ];
    let form = Form {
        file: "options-general.php",
        title: "General Settings",
        help: &HELP,
        content: &content,
        admin_head: &head,
        scripts: &[],
        late_scripts: &[
            "media-editor",
            "media-audiovideo",
            "mce-view",
            "image-edit",
            "site-icon",
        ],
        late_styles: &["media-views", "imgareaselect"],
        script_data: &script_data,
        admin_footer: &templates,
    };
    // The screen sets the global `$locale` to the installed language
    // chosen, none, which the session check's login link then carries.
    Some(
        match settings_page(site, &form, canonical, origin, browser) {
            Answer::Page(page) => {
                Answer::Page(page.replacen("&#038;wp_lang=en_US", "&#038;wp_lang", 1))
            }
            refused => refused,
        },
    )
}
