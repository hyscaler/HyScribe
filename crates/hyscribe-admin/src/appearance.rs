// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Themes and Plugins screens, `themes.php` and `plugins.php`, with the
//! themes and plugins a HyScribe installation has: Twenty Twenty-Five alone,
//! and no PHP plugin.

use hyscribe_kses::{esc_html, esc_url, esc_url_raw, parse_url};
use hyscribe_phpser::{
    Key, Maybe, Value, add_query_arg, maybe_unserialize, stripslashes, urlencode,
};
use hyscribe_render::date::human_time_diff;

use crate::admin_bar::BarSite;
use crate::list_table::wp_referer_field;
use crate::list_table::{Column, Order, column_headers, fill, screen_options};
use crate::menu::{Screen, link};
use crate::page::{HelpTab, Page, Request, THEME, admin_page, json, text, wp_admin_notice};
use crate::posts::{Answer, PostsSite, hidden_columns, per_page};
use crate::tools::NOTICES;

const THEMES: &str = include_str!("appearance/themes.html");
const SETTINGS: &str = include_str!("appearance/settings.json");
const THEME_JSON: &str = include_str!("appearance/theme.json");
const THEME_BLOCK: &str = include_str!("appearance/theme.html");
const PLUGINS: &str = include_str!("appearance/plugins.html");
const UPDATE_ROWS: &str = include_str!("appearance/update-rows.html");

const THEMES_OVERVIEW: &str = "<p>This screen is used for managing your installed themes. Aside from the default theme(s) included with your WordPress installation, themes are designed and developed by third parties.</p><p>From this screen you can:</p><ul><li>Hover or tap to see Activate and Live Preview buttons.</li><li>Click Customize for the active theme or Live Preview for any other theme to see a live preview.</li><li>Click on a theme to open the Theme Details dialog and see the theme name, version, author, description, tags, and the Delete link.</li><li>Use the buttons at the top of the dialog, or <code>alt</code>/<code>option</code> plus the left or right arrow keys on your keyboard, to navigate between themes quickly.</li></ul><p>The active theme is displayed highlighted as the first theme.</p><p>The search for installed themes will search for terms in their name, description, author, or tag. <span id=\"live-search-desc\">The search results will be updated as you type.</span></p>";

const THEMES_CUSTOMIZE: &str = "<p>Tap or hover on any theme then click the Live Preview button to see a live preview of that theme and change theme options in a separate, full-screen view. You can also find a Live Preview button at the bottom of the theme details screen. Any installed theme can be previewed and customized in this way.</p><p>The theme being previewed is fully interactive &mdash; navigate to different pages to see how the theme handles posts, archives, and other page templates. The settings may differ depending on what theme features the theme being previewed supports. To accept the new settings and activate the theme all in one step, click the Activate &amp; Publish button above the menu.</p><p>When previewing on smaller monitors, you can use the collapse icon at the bottom of the left-hand pane. This will hide the pane, giving you more room to preview your site in the new theme. To bring the pane back, click on the collapse icon again.</p>";

const THEMES_SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/work-with-themes/\">Documentation on Using Themes</a></p><p><a href=\"https://wordpress.org/documentation/article/appearance-themes-screen/\">Documentation on Managing Themes</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

const PLUGINS_OVERVIEW: &str = "<p>Plugins extend and expand the functionality of WordPress. Once a plugin is installed, you may activate it or deactivate it here.</p><p>The search for installed plugins will search for terms in their name, description, or author. <span id=\"live-search-desc\" class=\"hide-if-no-js\">The search results will be updated as you type.</span></p><p>If you would like to see more plugins to choose from, click on the &#8220;Add Plugin&#8221; button and you will be able to browse or search for additional plugins from the <a href=\"https://wordpress.org/plugins/\">WordPress Plugin Directory</a>. Plugins in the WordPress Plugin Directory are designed and developed by third parties, and are compatible with the license WordPress uses. Oh, and they are free!</p>";

const PLUGINS_SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/manage-plugins/\">Documentation on Managing Plugins</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// `wp_get_auto_update_message()` at `now`, from the `cron` option.
pub fn auto_update_message(cron: &[u8], now: i64) -> String {
    // `wp_next_scheduled( 'wp_version_check' )`: the first event without
    // arguments, `md5( serialize( array() ) )`.
    let next = match maybe_unserialize(cron) {
        Maybe::Value(Value::Array(events)) => events.into_iter().find_map(|(time, hooks)| {
            hooks
                .get_str("wp_version_check")?
                .get_str("40cd750bba9870f18aada2478b24840a")?;
            Some(match time {
                Key::Int(time) => time,
                Key::String(time) => Value::String(time).to_int(),
            })
        }),
        _ => None,
    };
    match next {
        None => "Automatic update not scheduled. There may be a problem with WP-Cron.".into(),
        Some(next) if now > next => format!(
            "Automatic update overdue by {}. There may be a problem with WP-Cron.",
            human_time_diff(next, now)
        ),
        Some(next) => format!(
            "Automatic update scheduled in {}.",
            human_time_diff(next, now)
        ),
    }
}

/// `themes.php` for `request`, `broken` when it asks for the broken theme
/// notice. An active theme or parent the site does not have is shown as PHP
/// shows it, without `switch_theme()`'s write.
pub fn themes(site: &dyn PostsSite, request: &Request, broken: bool) -> Answer {
    let bundled = |name| site.option(name).as_deref() == Some(THEME.as_bytes());
    let (active, parent) = (bundled("stylesheet"), bundled("template"));
    let screenshot = format!(
        "{}/wp-content/themes/{THEME}/screenshot.png",
        crate::assets::Urls::site_url(site)
    );
    let options = site.can("edit_theme_options");
    let customize = options.then(|| {
        let back = urlencode(&esc_url_raw(&stripslashes(request.uri.as_bytes())));
        let url = add_query_arg(
            &[
                (b"wp_theme_preview", (!active).then_some(THEME.as_bytes())),
                (b"return", Some(&back)),
            ],
            BarSite::admin_url(site, "site-editor.php").as_bytes(),
        );
        esc_url(&url)
    });
    let switch = site.can("switch_themes");
    let activate = switch.then(|| {
        // `wp_nonce_url()` escapes with `esc_html()`.
        let url = BarSite::admin_url(
            site,
            &format!(
                "themes.php?action=activate&stylesheet={THEME}&_wpnonce={}",
                site.nonce(&format!("switch-theme_{THEME}"))
            ),
        );
        esc_html(url.as_bytes()).into_bytes()
    });
    let escaped = |url: &[u8]| String::from_utf8_lossy(&esc_url(url)).into_owned();
    // `customize` maps to `edit_theme_options`.
    let buttons = if active {
        let button = customize.as_deref().map_or(String::new(), |url| {
            format!(
                "\t\t<a class=\"button button-compact button-primary customize load-customize hide-if-no-customize\"\n\t\t\t\t\thref=\"{}\"\n\t\t\t\t\taria-label=\"Customize Twenty Twenty-Five\"\n\t\t\t\t>Customize</a>\n\t\t\t\t\t",
                escaped(url)
            )
        });
        format!("\t\t\t\t\t\t\t{button}\n")
    } else {
        let preview = customize.as_deref().map_or(String::new(), |url| {
            format!(
                "\t\t\t\t\t\t\t<a class=\"button button-compact button-primary load-customize hide-if-no-customize\"\n\t\t\t\t\thref=\"{}\"\n\t\t\t\t\taria-label=\"Live Preview {{{{ data.name }}}}\"\n\t\t\t\t>Live Preview</a>\n",
                escaped(url)
            )
        });
        // Only a user who may switch themes is shown an inactive one.
        format!(
            "\t\t\t\t\t\t\t\t<a class=\"button button-compact activate\"\n\t\t\t\thref=\"{}\"\n\t\t\t\taria-label=\"Activate {{{{ data.name }}}}\"\n\t\t\t>Activate</a>\n\n{preview}\t\t\t\t\t\n",
            escaped(activate.as_deref().unwrap_or_default())
        )
    };
    let auto_update =
        match maybe_unserialize(&site.option("auto_update_themes").unwrap_or_default()) {
            Maybe::Value(Value::Array(items)) => items
                .iter()
                .any(|(_, value)| value.as_bytes() == Some(THEME.as_bytes())),
            _ => false,
        };
    let string =
        |value: Option<Vec<u8>>| value.map_or("null".into(), |value| json(&Value::String(value)));
    let admin_path = parse_url(BarSite::admin_url(site, "").as_bytes())
        .and_then(|parts| parts.path)
        .unwrap_or_default();
    // A user who may not switch themes is shown the active theme alone,
    // which a missing one leaves out.
    let listed = active || switch;
    let theme = fill(
        THEME_JSON,
        &[
            ("screenshot", string(Some(screenshot.clone().into_bytes()))),
            ("activate", string(activate)),
            ("customize", string(customize)),
            ("enabled", auto_update.to_string()),
            ("active", active.to_string()),
        ],
    );
    let settings = fill(
        SETTINGS,
        &[
            ("theme", if listed { theme } else { String::new() }),
            ("admin_path", json(&Value::String(admin_path))),
        ],
    );
    let block = fill(
        THEME_BLOCK,
        &[
            ("class", if active { " active" } else { "" }.into()),
            (
                "screenshot",
                escaped(format!("{screenshot}?ver=1.5").as_bytes()),
            ),
            (
                "name",
                if active {
                    "\n\t\t\t\t<span>Active:</span> Twenty Twenty-Five\t\t\t"
                } else {
                    "Twenty Twenty-Five"
                }
                .into(),
            ),
            ("actions", buttons),
        ],
    );
    let notice = if broken || !active || !parent {
        "<div id=\"message1\" class=\"notice is-dismissible updated\"><p>The active theme is broken. Reverting to the default theme.</p></div>"
    } else {
        ""
    };
    // The Appearance submenu's items but the theme's own screens, of which
    // Design and Editor are.
    let actions = match (options, active || parent) {
        (false, _) => String::new(),
        (true, true) => format!(
            "<a class='button button-compact' href='{}'>Fonts</a>",
            link("font-library.php")
        ),
        (true, false) => format!(
            "<a class='button button-compact' href='{}'>Patterns</a> <a class='button button-compact' href='{}'>Fonts</a>",
            link("site-editor.php?p=/pattern"),
            link("font-library.php")
        ),
    };
    let cron = site.option("cron").unwrap_or_default();
    let content = fill(
        THEMES,
        &[
            ("count", u8::from(listed).to_string()),
            ("notice", notice.into()),
            (
                "theme",
                if listed {
                    format!("\n{block}")
                } else {
                    String::new()
                },
            ),
            ("actions", actions),
            ("auto_update", auto_update_message(&cron, site.clock().1)),
        ],
    ) + NOTICES
        + UPDATE_ROWS;
    let mut help = Vec::new();
    if switch {
        help.push(HelpTab {
            id: "overview",
            title: "Overview",
            content: THEMES_OVERVIEW,
        });
    }
    if options {
        help.push(HelpTab {
            id: "customize-preview-themes",
            title: "Previewing and Customizing",
            content: THEMES_CUSTOMIZE,
        });
    }
    let page = Page {
        screen: Screen {
            pagenow: "themes.php",
            typenow: "",
            taxonomy: "",
            parent_file: Some("themes.php"),
            submenu_file: None,
        },
        id: "themes",
        title: "Themes",
        help: &help,
        sidebar: THEMES_SIDEBAR,
        screen_options: "",
        styles: &["thickbox"],
        scripts: &["thickbox", "theme", "updates"],
        late_scripts: &[],
        script_data: &[
            ("theme", format!("var _wpThemeSettings = {settings};")),
            ("updates", updates_settings(site, Some(""))),
        ],
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical: request.canonical,
        origin: request.origin,
        view: None,
    };
    Answer::from_page(admin_page(site, &page, request.browser))
}

/// `_wpUpdatesSettings`, which `updates` gets, then the screen's
/// `_wpUpdatesItemCounts`, if any, with `items` and no update.
pub(crate) fn updates_settings(site: &dyn PostsSite, items: Option<&str>) -> String {
    let settings = format!(
        "var _wpUpdatesSettings = {{\"ajax_nonce\":{}}};",
        json(&text(&site.nonce("updates")))
    );
    match items {
        Some(items) => format!(
            "{settings}\nvar _wpUpdatesItemCounts = {{{items}\"totals\":{{\"counts\":{{\"plugins\":0,\"themes\":0,\"wordpress\":0,\"translations\":0,\"total\":0}},\"title\":\"\"}}}};"
        ),
        None => settings,
    }
}

/// `validate_file()`: a path that climbs out of its folder or names a drive.
fn invalid_path(file: &str) -> bool {
    let file = file.replace('\\', "/");
    let climbs = file.matches("../").count();
    climbs > 1
        || file == "../"
        || (climbs == 1 && !file.ends_with("../"))
        || file.as_bytes().get(1) == Some(&b':')
}

/// `validate_active_plugins()`'s notices, without the write that
/// deactivates the plugins: HyScribe has no plugin files, so every
/// plugin the site names is missing. `None` where PHP would write the
/// option back or find a file.
pub(crate) fn missing_plugins(stored: Option<Vec<u8>>) -> Option<String> {
    // `get_option( 'active_plugins', array() )`.
    let Some(stored) = stored else {
        return Some(String::new());
    };
    let Maybe::Value(Value::Array(listed)) = maybe_unserialize(&stored) else {
        return None;
    };
    let mut files: Vec<String> = Vec::new();
    for (_, plugin) in &listed {
        let Value::String(bytes) = plugin else {
            return None;
        };
        if bytes.is_empty() {
            return None;
        }
        let file = crate::text(bytes);
        if !files.contains(&file) {
            files.push(file);
        }
    }
    let notices = files.iter().map(|file| {
        let error = if invalid_path(file) {
            "Invalid plugin path."
        } else {
            "Plugin file does not exist."
        };
        let message = format!(
            "The plugin <code>{}</code> has been deactivated due to an error: {error}",
            esc_html(file.as_bytes())
        );
        wp_admin_notice(&message, "message", "error", true, "")
    });
    Some(notices.collect())
}

/// `plugins.php` with no plugin installed, its help naming `plugin_dir`.
pub fn plugins(site: &dyn PostsSite, plugin_dir: &str, request: &Request) -> Option<Answer> {
    let missing = missing_plugins(site.option("active_plugins"))?;
    let hidden = hidden_columns(site, "plugins");
    let hidden: Vec<&str> = hidden.iter().map(String::as_str).collect();
    let columns = [
        Column {
            key: "cb",
            label: String::new(),
            sort: None,
            primary: false,
        },
        Column {
            key: "name",
            label: "Plugin".into(),
            sort: None,
            primary: true,
        },
        Column {
            key: "description",
            label: "Description".into(),
            sort: None,
            primary: false,
        },
    ];
    let headers = |counter, with_id| {
        column_headers(
            &columns,
            &hidden,
            &Order::default(),
            request.canonical,
            counter,
            with_id,
        )
    };
    let shown = columns.iter().filter(|c| !hidden.contains(&c.key)).count();
    let content = missing
        + &fill(
            PLUGINS,
            &[
                ("nonce", site.nonce("bulk-plugins")),
                ("referer", wp_referer_field(request.uri)),
                ("head", headers(1, true)),
                ("foot", headers(2, false)),
                ("colspan", shown.to_string()),
            ],
        )
        + NOTICES
        + UPDATE_ROWS;
    let options = screen_options(
        &columns,
        &hidden.iter().map(|h| (*h).to_string()).collect::<Vec<_>>(),
        "plugins_per_page",
        per_page(site, "plugins_per_page", 999),
        "",
        site.nonce("screen-options-nonce"),
    );
    let troubleshooting = format!(
        "<p>Most of the time, plugins play nicely with the core of WordPress and with other plugins. Sometimes, though, a plugin&#8217;s code will get in the way of another plugin, causing compatibility issues. If your site starts doing strange things, this may be the problem. Try deactivating all your plugins and re-activating them in various combinations until you isolate which one(s) caused the issue.</p><p>If something goes wrong with a plugin and you cannot use WordPress, delete or rename that file in the <code>{plugin_dir}</code> directory and it will be automatically deactivated.</p>"
    );
    let help = [
        HelpTab {
            id: "overview",
            title: "Overview",
            content: PLUGINS_OVERVIEW,
        },
        HelpTab {
            id: "compatibility-problems",
            title: "Troubleshooting",
            content: &troubleshooting,
        },
    ];
    let page = Page {
        screen: Screen {
            pagenow: "plugins.php",
            typenow: "",
            taxonomy: "",
            parent_file: Some("plugins.php"),
            submenu_file: None,
        },
        id: "plugins",
        title: "Plugins",
        help: &help,
        sidebar: PLUGINS_SIDEBAR,
        screen_options: &options,
        styles: &["thickbox"],
        scripts: &["updates", "plugin-install", "thickbox"],
        late_scripts: &[],
        script_data: &[("updates", updates_settings(site, Some(PLUGIN_COUNTS)))],
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical: request.canonical,
        origin: request.origin,
        view: None,
    };
    Some(Answer::from_page(admin_page(site, &page, request.browser)))
}

/// `_wpUpdatesItemCounts`' plugins, with none installed.
const PLUGIN_COUNTS: &str = "\"plugins\":{\"all\":[],\"search\":[],\"active\":[],\"inactive\":[],\"recently_activated\":[],\"upgrade\":[],\"mustuse\":[],\"dropins\":[],\"paused\":[]},";

#[cfg(test)]
pub(crate) mod tests;
