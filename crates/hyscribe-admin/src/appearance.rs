// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Themes and Plugins screens, `themes.php` and `plugins.php`, with the
//! themes and plugins a HyScribe installation has: the embedded block
//! themes, and no PHP plugin.

use hyscribe_kses::{esc_html, esc_url, esc_url_raw, parse_url};
use hyscribe_phpser::{
    Key, Maybe, Value, add_query_arg, maybe_unserialize, stripslashes, urlencode,
};
use hyscribe_render::date::human_time_diff;
use hyscribe_render::theme;

use crate::admin_bar::BarSite;
use crate::list_table::fill;
use crate::menu::{Screen, link};
use crate::page::{HelpTab, Page, Request, admin_page, json, text};
use crate::posts::{Answer, PostsSite};
use crate::tools::NOTICES;

const THEMES: &str = include_str!("appearance/themes.html");
const SETTINGS: &str = include_str!("appearance/settings.json");
const THEME_JSON: &str = include_str!("appearance/theme.json");
const THEME_BLOCK: &str = include_str!("appearance/theme.html");
/// The update rows' templates `updates` prints on the Themes and Plugins screens.
pub(crate) const UPDATE_ROWS: &str = include_str!("appearance/update-rows.html");

const THEMES_OVERVIEW: &str = "<p>This screen is used for managing your installed themes. Aside from the default theme(s) included with your HyScribe installation, themes are designed and developed by third parties.</p><p>From this screen you can:</p><ul><li>Hover or tap to see Activate and Live Preview buttons.</li><li>Click Customize for the active theme or Live Preview for any other theme to see a live preview.</li><li>Click on a theme to open the Theme Details dialog and see the theme name, version, author, description, tags, and the Delete link.</li><li>Use the buttons at the top of the dialog, or <code>alt</code>/<code>option</code> plus the left or right arrow keys on your keyboard, to navigate between themes quickly.</li></ul><p>The active theme is displayed highlighted as the first theme.</p><p>The search for installed themes will search for terms in their name, description, author, or tag. <span id=\"live-search-desc\">The search results will be updated as you type.</span></p>";

const THEMES_CUSTOMIZE: &str = "<p>Tap or hover on any theme then click the Live Preview button to see a live preview of that theme and change theme options in a separate, full-screen view. You can also find a Live Preview button at the bottom of the theme details screen. Any installed theme can be previewed and customized in this way.</p><p>The theme being previewed is fully interactive &mdash; navigate to different pages to see how the theme handles posts, archives, and other page templates. The settings may differ depending on what theme features the theme being previewed supports. To accept the new settings and activate the theme all in one step, click the Activate &amp; Publish button above the menu.</p><p>When previewing on smaller monitors, you can use the collapse icon at the bottom of the left-hand pane. This will hide the pane, giving you more room to preview your site in the new theme. To bring the pane back, click on the collapse icon again.</p>";

const THEMES_SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/work-with-themes/\">Documentation on Using Themes</a></p><p><a href=\"https://wordpress.org/documentation/article/appearance-themes-screen/\">Documentation on Managing Themes</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

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

/// An embedded theme as the Themes screen lists it.
struct Listed {
    slug: &'static str,
    name: String,
    version: String,
    description: String,
    author: String,
    author_and_uri: String,
    tags: String,
    /// The name of the theme this one is a child of.
    parent: Option<String>,
}

impl Listed {
    fn of(slug: &'static str) -> Listed {
        let rest = theme::screen_entry(slug).unwrap_or_default();
        let field = |name: &str| {
            let text = rest.get_str(name).map(Value::to_php_string);
            String::from_utf8_lossy(&text.unwrap_or_default()).into_owned()
        };
        Listed {
            slug,
            name: field("name"),
            version: field("version"),
            description: field("description"),
            author: field("author"),
            author_and_uri: field("authorAndUri"),
            tags: field("tags"),
            parent: theme::parent_name(slug),
        }
    }

    /// The name as HTML.
    fn title(&self) -> String {
        esc_html(self.name.as_bytes())
    }
}

/// The notice `themes.php`'s arguments ask for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Notice {
    /// None.
    #[default]
    None,
    /// `broken`: the active theme is broken.
    Broken,
    /// `activated`: a theme was activated.
    Activated,
    /// `activated` and `previewed`: a theme was activated from its preview.
    Previewed,
}

/// `themes.php` for `request` with the notice its arguments ask for. An active theme or parent the site does not have is shown as PHP
/// shows it, without `switch_theme()`'s write.
pub fn themes(site: &dyn PostsSite, request: &Request, asked: Notice) -> Answer {
    let stylesheet = site.option("stylesheet").unwrap_or_default();
    let template = site.option("template").unwrap_or_default();
    let mut themes: Vec<Listed> = theme::slugs().map(Listed::of).collect();
    // `wp_get_themes()` sorts by name, and the active theme comes first.
    themes.sort_by(|a, b| a.name.cmp(&b.name));
    let current = themes.iter().position(|t| t.slug.as_bytes() == stylesheet);
    let active = current.is_some();
    let parent = theme::slugs().any(|slug| slug.as_bytes() == template);
    if let Some(at) = current {
        let first = themes.remove(at);
        themes.insert(0, first);
    }
    let options = site.can("edit_theme_options");
    let switch = site.can("switch_themes");
    // A user who may not switch themes is shown the active theme alone,
    // which a missing one leaves out.
    let shown: Vec<&Listed> = themes
        .iter()
        .filter(|t| switch || (active && t.slug.as_bytes() == stylesheet))
        .collect();
    let listed = !shown.is_empty();
    // `$_SERVER['REQUEST_URI']` without `wp_removable_query_args()`.
    let uri = request
        .canonical
        .strip_prefix(request.origin)
        .unwrap_or_default();
    let back = urlencode(&esc_url_raw(&stripslashes(uri.as_bytes())));
    let escaped = |url: &[u8]| String::from_utf8_lossy(&esc_url(url)).into_owned();
    let enabled = match maybe_unserialize(&site.option("auto_update_themes").unwrap_or_default()) {
        Maybe::Value(Value::Array(items)) => items,
        _ => Vec::new(),
    };
    let string = |value: Option<Vec<u8>>| value.map_or("null".into(), |v| json(&Value::String(v)));
    let mut blocks = Vec::new();
    let mut entries = Vec::new();
    for theme in &shown {
        let slug = theme.slug;
        let is_active = active && slug.as_bytes() == stylesheet;
        let screenshot = format!(
            "{}/wp-content/themes/{slug}/screenshot.png",
            crate::assets::Urls::site_url(site)
        );
        let customize = options.then(|| {
            let preview = (!is_active).then_some(slug.as_bytes());
            let url = add_query_arg(
                &[(b"wp_theme_preview", preview), (b"return", Some(&back))],
                BarSite::admin_url(site, "site-editor.php").as_bytes(),
            );
            esc_url(&url)
        });
        let activate = switch.then(|| {
            // `wp_nonce_url()` escapes with `esc_html()`.
            let url = BarSite::admin_url(
                site,
                &format!(
                    "themes.php?action=activate&stylesheet={slug}&_wpnonce={}",
                    site.nonce(&format!("switch-theme_{slug}"))
                ),
            );
            esc_html(url.as_bytes()).into_bytes()
        });
        // `customize` maps to `edit_theme_options`.
        let buttons = if is_active {
            let button = customize.as_deref().map_or(String::new(), |url| {
                format!(
                    "\t\t<a class=\"button button-compact button-primary customize load-customize hide-if-no-customize\"\n\t\t\t\t\thref=\"{}\"\n\t\t\t\t\taria-label=\"Customize {}\"\n\t\t\t\t>Customize</a>\n\t\t\t\t\t",
                    escaped(url),
                    theme.title()
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
        let auto_update = enabled
            .iter()
            .any(|(_, value)| value.as_bytes() == Some(slug.as_bytes()));
        let text = |value: &str| json(&Value::String(value.as_bytes().to_vec()));
        entries.push(fill(
            THEME_JSON,
            &[
                ("id", text(slug)),
                ("name", text(&theme.name)),
                ("screenshot", string(Some(screenshot.clone().into_bytes()))),
                ("description", text(&theme.description)),
                ("author", text(&theme.author)),
                ("author_and_uri", text(&theme.author_and_uri)),
                ("tags", text(&theme.tags)),
                ("version", text(&theme.version)),
                (
                    "parent",
                    theme.parent.as_deref().map_or("false".into(), text),
                ),
                ("activate", string(activate)),
                ("customize", string(customize)),
                // The parent of the active theme cannot be deleted.
                (
                    "delete",
                    if active && stylesheet != template && slug.as_bytes() == template {
                        String::new()
                    } else {
                        "\"delete\":null,".into()
                    },
                ),
                ("enabled", auto_update.to_string()),
                ("active", is_active.to_string()),
            ],
        ));
        let name = if is_active {
            format!("\n\t\t\t\t<span>Active:</span> {}\t\t\t", theme.title())
        } else {
            theme.title()
        };
        blocks.push(fill(
            THEME_BLOCK,
            &[
                ("class", if is_active { " active" } else { "" }.into()),
                (
                    "screenshot",
                    escaped(format!("{screenshot}?ver={}", theme.version).as_bytes()),
                ),
                ("slug", slug.into()),
                ("title", theme.title()),
                ("author", esc_html(theme.author.as_bytes())),
                ("name", name),
                ("actions", buttons),
            ],
        ));
    }
    let admin_path = parse_url(BarSite::admin_url(site, "").as_bytes())
        .and_then(|parts| parts.path)
        .unwrap_or_default();
    let settings = fill(
        SETTINGS,
        &[
            ("theme", entries.join(",")),
            ("admin_path", json(&Value::String(admin_path))),
        ],
    );
    let block = blocks.join("\n");
    let visit = || {
        let home = String::from_utf8_lossy(&site.option("home").unwrap_or_default()).into_owned();
        let url = esc_url(format!("{}/", home.trim_end_matches('/')).as_bytes());
        String::from_utf8_lossy(&url).into_owned()
    };
    let notice = if asked == Notice::Broken || !active || !parent {
        "<div id=\"message1\" class=\"notice is-dismissible updated\"><p>The active theme is broken. Reverting to the default theme.</p></div>".to_string()
    } else if asked == Notice::Previewed {
        format!(
            "<div id=\"message2\" class=\"notice is-dismissible updated\"><p>Settings saved and theme activated. <a href=\"{}\">Visit site</a></p></div>",
            visit()
        )
    } else if asked == Notice::Activated {
        format!(
            "<div id=\"message2\" class=\"notice is-dismissible updated\"><p>New theme activated. <a href=\"{}\">Visit site</a></p></div>",
            visit()
        )
    } else {
        String::new()
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
            ("count", shown.len().to_string()),
            ("notice", notice),
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

#[cfg(test)]
pub(crate) mod tests;
