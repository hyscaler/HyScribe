// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Plugins screen, `plugins.php`, with the extensions HyScribe has
//! installed, each listed as the plugin it stands in for, on a site whose
//! files never change from the browser (`DISALLOW_FILE_MODS`, so no
//! delete, update or auto-update links). Before activation each row says
//! what the extension asks for: HyScribe's own part of the screen (MVP 11.3).

use hyscribe_kses::{esc_attr, esc_html, esc_url, sanitize_key};
use hyscribe_phpser::{
    Key, Maybe, Value, add_query_arg, maybe_unserialize, md5, remove_query_arg, urlencode,
};
use hyscribe_render::formatting::sanitize_title;
use hyscribe_render::kses::wp_kses_plugin_data;

use crate::appearance::{UPDATE_ROWS, updates_settings};
use crate::list_table::{
    Column, Order, View, bulk_actions, column_headers, fill, pagination, row_actions,
    screen_options, views, wp_referer_field,
};
use crate::menu::{Screen, link};
use crate::page::{HelpTab, Page, Request, admin_page, json, query_strings, wp_admin_notice};
use crate::posts::{Answer, PostsSite, hidden_columns, per_page};
use crate::text;
use crate::tools::NOTICES;

const PAGE: &str = include_str!("plugins/page.html");

const OVERVIEW: &str = "<p>Plugins extend and expand the functionality of HyScribe. Once a plugin is installed, you may activate it or deactivate it here.</p><p>The search for installed plugins will search for terms in their name, description, or author. <span id=\"live-search-desc\" class=\"hide-if-no-js\">The search results will be updated as you type.</span></p><p>If you would like to see more plugins to choose from, click on the &#8220;Add Plugin&#8221; button and you will be able to browse or search for additional plugins from the <a href=\"https://wordpress.org/plugins/\">Plugin Directory</a>. Plugins in the Plugin Directory are designed and developed by third parties, and are compatible with the license HyScribe uses. Oh, and they are free!</p>";

const SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/manage-plugins/\">Documentation on Managing Plugins</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// Where the Troubleshooting help names the plugins folder: relative to the
/// HyScribe folder, in place of PHP's `WP_PLUGIN_DIR`.
pub const PLUGIN_DIR: &str = "hyscribe/plugins/";

/// `WEEK_IN_SECONDS`, how long a plugin stays recently active.
const WEEK: i64 = 604_800;

/// The arguments `plugins.php` takes out of the request URI before it
/// shows it.
const TEMPORARY: [&[u8]; 11] = [
    b"error",
    b"deleted",
    b"activate",
    b"activate-multi",
    b"deactivate",
    b"deactivate-multi",
    b"enabled-auto-update",
    b"disabled-auto-update",
    b"enabled-auto-update-multi",
    b"disabled-auto-update-multi",
    b"_error_nonce",
];

/// An installed extension, as the screen lists it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plugin {
    /// The plugin file it stands in for, as `active_plugins` names it.
    pub file: String,
    /// `Plugin Name`.
    pub name: String,
    /// `Description`.
    pub description: String,
    /// `Version`.
    pub version: String,
    /// `Author`.
    pub author: String,
    /// `Author URI`.
    pub author_uri: String,
    /// `Plugin URI`.
    pub plugin_uri: String,
    /// What it asks for: each grant's label and names.
    pub grants: Vec<(&'static str, Vec<String>)>,
    /// The security sensitive hooks it asks for.
    pub sensitive: Vec<String>,
    /// Its limits, when it asks for more than the defaults: a call's and a
    /// request's milliseconds, and an instance's MiB.
    pub limits: Option<(u64, u64, u64)>,
    /// Whether what it asks for was approved at its activation.
    pub approved: bool,
    /// Whether HyScribe stopped running it after its faults.
    pub disabled: bool,
}

/// What a view of the screen asks for: the status shown and the notice
/// an action sent back.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Args {
    status: String,
    notice: Option<&'static str>,
}

impl Args {
    /// `$_GET` of `query`; `None` for an argument the port does not take:
    /// a search, a sort, a later page, another action's notice.
    pub fn parse(query: &str) -> Option<Args> {
        let (mut status, mut activate, mut deactivate) = ("all".to_string(), false, false);
        for (name, value) in query_strings(query)? {
            match (name.as_str(), value.as_str()) {
                ("plugin_status", given) => status = sanitize_key(given.as_bytes()),
                ("paged", "" | "1") | ("s", "") => {}
                ("activate", _) => activate = true,
                ("deactivate", _) => deactivate = true,
                _ => return None,
            }
        }
        // `plugins.php` shows the first notice asked for, in this order.
        let notice = if activate {
            Some("Plugin activated.")
        } else {
            deactivate.then_some("Plugin deactivated.")
        };
        Some(Args { status, notice })
    }
}

/// `validate_file()`: a path that climbs out of its folder or names a drive.
pub fn invalid_path(file: &str) -> bool {
    let file = file.replace('\\', "/");
    let climbs = file.matches("../").count();
    climbs > 1
        || file == "../"
        || (climbs == 1 && !file.ends_with("../"))
        || file.as_bytes().get(1) == Some(&b':')
}

/// The plugin files `active_plugins` names, each once; `None` where PHP
/// would write the option back.
pub fn active_plugins(stored: Option<Vec<u8>>) -> Option<Vec<String>> {
    // `get_option( 'active_plugins', array() )`.
    let Some(stored) = stored else {
        return Some(Vec::new());
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
        let file = text(bytes);
        if !files.contains(&file) {
            files.push(file);
        }
    }
    Some(files)
}

/// `validate_active_plugins()`'s notices for the `active` plugins that are
/// not `installed`, without the write that deactivates them.
fn missing_plugins(active: &[String], installed: &[Plugin]) -> String {
    let missing = active
        .iter()
        .filter(|file| !installed.iter().any(|plugin| &&plugin.file == file));
    let notices = missing.map(|file| {
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
    notices.collect()
}

/// `recently_activated` as `prepare_items()` keeps it at `now`: the plugins
/// deactivated within a week, by file; `None` when it is not an array.
pub fn recently_activated(stored: Option<&[u8]>, now: i64) -> Option<Vec<(Key, Value)>> {
    let Some(stored) = stored else {
        return Some(Vec::new());
    };
    let Maybe::Value(Value::Array(recent)) = maybe_unserialize(stored) else {
        return None;
    };
    let kept = recent
        .into_iter()
        .filter(|(_, time)| matches!(time, Value::Int(time) if time.saturating_add(WEEK) >= now));
    Some(kept.collect())
}

/// `plugins.php` for `request`, listing `installed` as `args` ask.
pub fn plugins(
    site: &dyn PostsSite,
    request: &Request,
    installed: &[Plugin],
    args: &Args,
) -> Option<Answer> {
    let active = active_plugins(site.option("active_plugins"))?;
    let now = site.clock().1;
    let recent = recently_activated(site.option("recently_activated").as_deref(), now)?;
    let missing = missing_plugins(&active, installed);
    let is_active = |plugin: &&Plugin| active.contains(&plugin.file);
    let is_recent = |plugin: &&Plugin| {
        recent
            .iter()
            .any(|(file, _)| file.to_php_string() == plugin.file.as_bytes())
    };
    let mut sorted: Vec<&Plugin> = installed.iter().collect();
    sorted.sort_by_key(|plugin| plugin.name.to_lowercase());
    let lists: [(&str, Vec<&Plugin>); 9] = [
        ("all", sorted.clone()),
        ("search", Vec::new()),
        ("active", sorted.iter().copied().filter(is_active).collect()),
        (
            "inactive",
            sorted.iter().copied().filter(|p| !is_active(p)).collect(),
        ),
        (
            "recently_activated",
            sorted
                .iter()
                .copied()
                .filter(|p| !is_active(p) && is_recent(p))
                .collect(),
        ),
        ("upgrade", Vec::new()),
        ("mustuse", Vec::new()),
        ("dropins", Vec::new()),
        ("paused", Vec::new()),
    ];
    let listed = |status: &str| lists.iter().find(|(name, _)| *name == status);
    let status = match listed(&args.status) {
        Some((name, items)) if !items.is_empty() || *name == "search" => *name,
        _ => "all",
    };
    let items = listed(status)
        .map(|(_, items)| items.as_slice())
        .unwrap_or_default();
    let uri = text(&remove_query_arg(&TEMPORARY, request.uri.as_bytes()));
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
    let total = u64::try_from(items.len()).unwrap_or_default();
    let mut actions = Vec::new();
    if status != "active" {
        actions.push(("activate-selected", "Activate"));
    }
    if status != "inactive" {
        actions.push(("deactivate-selected", "Deactivate"));
    }
    let extra = if status == "recently_activated" {
        "<div class=\"alignleft actions\"><input type=\"submit\" name=\"clear-recent-list\" id=\"clear-recent-list\" class=\"button\" value=\"Clear List\"  /></div>"
    } else {
        ""
    };
    let pages = |which| {
        pagination(
            which,
            "Plugins list navigation",
            total,
            1,
            1,
            request.canonical,
        )
    };
    let rows = if items.is_empty() {
        let message = if installed.is_empty() {
            "No plugins are currently available."
        } else {
            "No plugins found."
        };
        format!(
            "<tr class=\"no-items\"><td class=\"colspanchange\" colspan=\"{shown}\">{message}</td></tr>"
        )
    } else {
        let mut ids = Vec::new();
        let row = |plugin: &&Plugin| {
            single_row(
                site,
                plugin,
                status,
                active.contains(&plugin.file),
                &hidden,
                &mut ids,
            )
        };
        items.iter().map(row).collect()
    };
    let view_links: Vec<View> = lists
        .iter()
        .filter(|(_, items)| !items.is_empty())
        .map(|(name, items)| {
            let label = match *name {
                "all" => "All",
                "active" => "Active",
                "inactive" => "Inactive",
                _ => "Recently Active",
            };
            View {
                class: (*name).to_string(),
                url: format!("plugins.php?plugin_status={name}"),
                label: format!("{label} <span class=\"count\">({})</span>", items.len()),
                current: *name == status,
            }
        })
        .collect();
    let search = if items.is_empty() {
        String::new()
    } else {
        "\t\t<p class=\"search-box\">\n\t\t\t<label for=\"plugin-search-input\">Search installed plugins</label>\n\t\t\t<input type=\"search\" id=\"plugin-search-input\" class=\"wp-filter-search\" name=\"s\" value=\"\" />\n\t\t\t<input type=\"submit\" id=\"search-submit\" class=\"button hide-if-js\" value=\"Search installed plugins\"  />\t\t</p>\n\t\t".to_string()
    };
    let bottom = if items.is_empty() {
        String::new()
    } else {
        format!(
            "\t\t\t<div class=\"tablenav bottom\">\n\n\t\t\t\t<div class=\"alignleft actions bulkactions\">\n\t\t\t{}\t\t</div>\n\t\t{}{}\n\t\t<br class=\"clear\" />\n\t</div>\n",
            bulk_actions("bottom", &actions),
            extra,
            pages("bottom")
        )
    };
    let notice = args.notice.map_or_else(String::new, |notice| {
        wp_admin_notice(notice, "message", "is-dismissible updated", true, "")
    });
    let content = missing
        + &notice
        + &fill(
            PAGE,
            &[
                (
                    "views",
                    if installed.is_empty() {
                        String::new()
                    } else {
                        views("Filter plugins list", &view_links)
                    },
                ),
                ("search", search),
                ("status", status.to_string()),
                ("nonce", site.nonce("bulk-plugins")),
                ("referer", wp_referer_field(&uri)),
                (
                    "hidden",
                    if items.is_empty() {
                        " hidden".into()
                    } else {
                        String::new()
                    },
                ),
                ("bulk_top", bulk_actions("top", &actions)),
                ("extra_top", extra.to_string()),
                ("pages_top", pages("top")),
                ("head", headers(1, true)),
                ("rows", rows),
                ("foot", headers(2, false)),
                ("bottom", bottom),
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
        "<p>Most of the time, plugins play nicely with the core of HyScribe and with other plugins. Sometimes, though, a plugin&#8217;s code will get in the way of another plugin, causing compatibility issues. If your site starts doing strange things, this may be the problem. Try deactivating all your plugins and re-activating them in various combinations until you isolate which one(s) caused the issue.</p><p>If something goes wrong with a plugin and you cannot use HyScribe, delete or rename that file in the <code>{PLUGIN_DIR}</code> directory and it will be automatically deactivated.</p>"
    );
    let help = [
        HelpTab {
            id: "overview",
            title: "Overview",
            content: OVERVIEW,
        },
        HelpTab {
            id: "compatibility-problems",
            title: "Troubleshooting",
            content: &troubleshooting,
        },
    ];
    let counts = lists
        .iter()
        .map(|(name, items)| {
            let files = items
                .iter()
                .map(|plugin| Value::String(plugin.file.clone().into_bytes()));
            let files = (0..).map(Key::Int).zip(files);
            format!("\"{name}\":{}", json(&Value::Array(files.collect())))
        })
        .collect::<Vec<_>>()
        .join(",");
    let counts = format!("\"plugins\":{{{counts}}},");
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
        sidebar: SIDEBAR,
        screen_options: &options,
        styles: &["thickbox"],
        scripts: &["updates", "plugin-install", "thickbox"],
        late_scripts: &[],
        script_data: &[("updates", updates_settings(site, Some(&counts)))],
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

/// `wp_nonce_url( $url, $action )` of a relative `wp-admin` URL, as its
/// hs-admin link.
fn nonce_link(site: &dyn PostsSite, url: &str, action: &str) -> String {
    let nonce = site.nonce(action);
    let url = add_query_arg(
        &[(b"_wpnonce", Some(nonce.as_bytes()))],
        url.replace("&amp;", "&").as_bytes(),
    );
    link(&esc_html(&url))
}

/// `single_row()` of `plugin` in the list of `status`, its row actions'
/// ids kept unique across the rows in `ids`.
fn single_row(
    site: &dyn PostsSite,
    plugin: &Plugin,
    status: &str,
    active: bool,
    hidden: &[&str],
    ids: &mut Vec<String>,
) -> String {
    let name = text(&wp_kses_plugin_data(plugin.name.as_bytes(), false));
    let kept = |field: &str| text(&wp_kses_plugin_data(field.as_bytes(), true));
    let (description, version, author) = (
        kept(&plugin.description),
        kept(&plugin.version),
        kept(&plugin.author),
    );
    let author_uri = text(&esc_url(plugin.author_uri.as_bytes()));
    let plugin_uri = text(&esc_url(plugin.plugin_uri.as_bytes()));
    let slug = text(&sanitize_title(name.as_bytes()));
    let mut id = slug.clone();
    let mut suffix = 2;
    while ids.contains(&id) {
        id = format!("{slug}-{suffix}");
        suffix += 1;
    }
    ids.push(id.clone());
    let url = |action: &str| {
        format!(
            "plugins.php?action={action}&amp;plugin={}&amp;plugin_status={status}&amp;paged=1&amp;s=",
            text(&urlencode(plugin.file.as_bytes()))
        )
    };
    let mut actions = Vec::new();
    // `activate_plugin` and `deactivate_plugin` both map to `activate_plugins`.
    let may = site.can("activate_plugins");
    if active && may {
        let href = nonce_link(
            site,
            &url("deactivate"),
            &format!("deactivate-plugin_{}", plugin.file),
        );
        let label = esc_attr(format!("Deactivate {name}").as_bytes());
        actions.push((
            "deactivate",
            format!(
                "<a href=\"{href}\" id=\"deactivate-{}\" aria-label=\"{label}\">Deactivate</a>",
                esc_attr(id.as_bytes())
            ),
        ));
    } else if may {
        let href = nonce_link(
            site,
            &url("activate"),
            &format!("activate-plugin_{}", plugin.file),
        );
        let label = esc_attr(format!("Activate {name}").as_bytes());
        actions.push((
            "activate",
            format!("<a href=\"{href}\" id=\"activate-{}\" class=\"edit\" aria-label=\"{label}\">Activate</a>", esc_attr(id.as_bytes())),
        ));
    }
    let class = if active { "active" } else { "inactive" };
    let checkbox = format!(
        "<label class=\"label-covers-full-cell\" for=\"checkbox_{0}\"><span class=\"screen-reader-text\">Select {name}</span></label><input type=\"checkbox\" name=\"checked[]\" value=\"{1}\" id=\"checkbox_{0}\" />",
        md5(plugin.file.as_bytes()),
        esc_attr(plugin.file.as_bytes())
    );
    let description = if description.is_empty() {
        "&nbsp;".to_string()
    } else {
        description
    };
    let mut meta = Vec::new();
    if !version.is_empty() {
        meta.push(format!("Version {version}"));
    }
    if !author.is_empty() {
        let author = if author_uri.is_empty() {
            author
        } else {
            format!("<a href=\"{author_uri}\">{author}</a>")
        };
        meta.push(format!("By {author}"));
    }
    if !plugin_uri.is_empty() {
        let label = esc_attr(format!("Visit plugin site for {name}").as_bytes());
        meta.push(format!(
            "<a href=\"{plugin_uri}\" aria-label=\"{label}\">Visit plugin site</a>"
        ));
    }
    let hidden_class = if hidden.contains(&"description") {
        " hidden"
    } else {
        ""
    };
    format!(
        "<tr class=\"{class}\" data-slug=\"{}\" data-plugin=\"{}\"><td class='check-column'>{checkbox}</td><th scope='row' class='plugin-title column-primary' aria-label='{}'><strong>{name}</strong>{}</th><td class='column-description desc{hidden_class}'>\n\t\t\t\t\t\t<div class='plugin-description'><p>{description}</p></div>\n\t\t\t\t\t\t<div class='{class} second plugin-version-author-uri'>{}</div>{}</td></tr>",
        esc_attr(slug.as_bytes()),
        esc_attr(plugin.file.as_bytes()),
        esc_attr(name.as_bytes()),
        row_actions(&actions, true),
        meta.join(" | "),
        grants(plugin, active)
    )
}

/// HyScribe's own part of a row, where `after_plugin_row_meta` prints:
/// what an extension asks for, until its activation approves it, and why
/// HyScribe stopped one. Nothing for an active, approved extension.
pub fn grants(plugin: &Plugin, active: bool) -> String {
    let mut out = String::new();
    if plugin.disabled {
        out.push_str("<p><strong>HyScribe stopped running this plugin after repeated faults.</strong> Deactivate and activate it again to run it.</p>");
    }
    if active && plugin.approved {
        return wrap(out);
    }
    out.push_str(if active {
        "<p><strong>This plugin asks for more than was approved, and does not run until it is approved.</strong> Deactivate and activate it again to approve:</p>"
    } else {
        "<p><strong>Activating this plugin allows it:</strong></p>"
    });
    let items: Vec<String> = plugin
        .grants
        .iter()
        .map(|(label, names)| {
            let names: Vec<String> = names.iter().map(|name| esc_html(name.as_bytes())).collect();
            format!(
                "<li>{label}: <code>{}</code></li>",
                names.join("</code>, <code>")
            )
        })
        .collect();
    out.push_str(&if items.is_empty() {
        "<p>Nothing beyond its own code.</p>".to_string()
    } else {
        format!("<ul class=\"ul-disc\">{}</ul>", items.concat())
    });
    if !plugin.sensitive.is_empty() {
        let names: Vec<String> = plugin
            .sensitive
            .iter()
            .map(|name| esc_html(name.as_bytes()))
            .collect();
        out.push_str(&format!(
            "<p><strong>Security sensitive hooks:</strong> <code>{}</code>. They can change who is logged in, what they may do, or what is filtered.</p>",
            names.join("</code>, <code>")
        ));
    }
    if let Some((call, request, memory)) = plugin.limits {
        out.push_str(&format!(
            "<p>Limits: up to {call} ms a call, {request} ms a request and {memory} MiB of memory.</p>"
        ));
    }
    wrap(out)
}

fn wrap(notice: String) -> String {
    if notice.is_empty() {
        notice
    } else {
        format!(
            "<div class=\"hyscribe-grants notice notice-warning notice-alt inline\">{notice}</div>"
        )
    }
}

#[cfg(test)]
mod tests;
