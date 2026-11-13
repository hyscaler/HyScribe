// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Discussion, Media, Permalinks and Privacy settings screens:
//! `options-discussion.php`, `options-media.php`, `options-permalink.php`
//! and `options-privacy.php`.

use hyscribe_kses::{esc_attr, esc_html, esc_textarea, esc_url};
use hyscribe_model::{OrderBy, PostQuery, StatusFilter};
use hyscribe_phpser::{Value, remove_query_arg};
use hyscribe_render::date::{Zone, format_unix};
use hyscribe_render::default_avatar;

use crate::admin_bar::{BarSite, Browser};
use crate::dashboard::DashboardSite;
use crate::list_table::{checked, fill, selected};
use crate::menu::Screen;
use crate::page::{HelpTab, Page, admin_page};
use crate::posts::{Answer, page_options};

const DISCUSSION: &str = include_str!("settings_more/discussion.html");
const MEDIA: &str = include_str!("settings_more/media.html");
const PERMALINK: &str = include_str!("settings_more/permalink.html");
const PRIVACY: &str = include_str!("settings_more/privacy.html");

/// The editable upload folder and URL, shown when either is not the default.
const UPLOAD_PATHS: &str = "<tr>\n<th scope=\"row\"><label for=\"upload_path\">Store uploads in this folder</label></th>\n<td><input name=\"upload_path\" type=\"text\" id=\"upload_path\" value=\"%upload_path%\" class=\"regular-text code\" />\n<p class=\"description\">\n\t\tDefault is <code>wp-content/uploads</code></p>\n</td>\n</tr>\n\n<tr>\n<th scope=\"row\"><label for=\"upload_url_path\">Full URL path to files</label></th>\n<td><input name=\"upload_url_path\" type=\"text\" id=\"upload_url_path\" value=\"%upload_url_path%\" class=\"regular-text code\" />\n<p class=\"description\">Configuring this is optional. By default, it should be blank.</p>\n</td>\n</tr>\n<tr>\n<td colspan=\"2\" class=\"td-full\">\n";

const STRUCTURE: &str = "\t\t\t<div class=\"row\">\n\t\t\t\t<input id=\"permalink-input-%id%\"\n\t\t\t\t\tname=\"selection\" aria-describedby=\"permalink-%id%\"\n\t\t\t\t\ttype=\"radio\" value=\"%value%\"\n\t\t\t\t\t%checked%\t\t\t\t/>\n\t\t\t\t<div>\n\t\t\t\t\t<label for=\"permalink-input-%id%\">\n\t\t\t\t\t\t%label%\t\t\t\t\t</label>\n\t\t\t\t\t<p>\n\t\t\t\t\t\t<code id=\"permalink-%id%\">\n\t\t\t\t\t\t\t%example%\t\t\t\t\t\t</code>\n\t\t\t\t\t</p>\n\t\t\t\t</div>\n\t\t\t</div><!-- .row -->\n";

const CHOOSE: &str = "\t\t\t\t<tr>\n\t\t\t<th scope=\"row\">\n\t\t\t\t<label for=\"page_for_privacy_policy\">\n\t\t\t\t\t%label%\t\t\t\t</label>\n\t\t\t</th>\n\t\t\t<td>\n\t\t\t\t<form method=\"post\">\n\t\t\t\t\t<input type=\"hidden\" name=\"action\" value=\"set-privacy-page\" />\n\t\t\t\t\t%select%<input type=\"hidden\" id=\"_wpnonce\" name=\"_wpnonce\" value=\"%nonce%\" />%referer%<input type=\"submit\" name=\"submit\" id=\"set-page\" class=\"button button-primary\" value=\"Use This Page\"  />\t\t\t\t</form>\n\t\t\t</td>\n\t\t</tr>\n";

const SUPPORT: &str = "<p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

const SAVE: &str = "<p>You must click the Save Changes button at the bottom of the screen for new settings to take effect.</p>";

/// `options_discussion_add_js()`, which shows the avatar settings with
/// avatars.
const DISCUSSION_JS: &str = "\t<script>\n\t(function($){\n\t\tvar parent = $( '#show_avatars' ),\n\t\t\tchildren = $( '.avatar-settings' );\n\t\tparent.on( 'change', function(){\n\t\t\tchildren.toggleClass( 'hide-if-js', ! this.checked );\n\t\t});\n\t})(jQuery);\n\t</script>\n\t";

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `get_option( $name )`, empty when unset.
fn option(site: &dyn DashboardSite, name: &str) -> Vec<u8> {
    site.option(name).unwrap_or_default()
}

/// `checked( $value, get_option( $name ) )`.
fn checks(site: &dyn DashboardSite, name: &str, value: &str) -> String {
    checked(option(site, name) == value.as_bytes()).into()
}

/// `esc_attr( get_option( $name ) )`.
fn attr(site: &dyn DashboardSite, name: &str) -> String {
    esc_attr(&option(site, name))
}

/// `wp_referer_field()` for the request.
fn referer(request_uri: &str) -> String {
    let url = remove_query_arg(&[b"_wp_http_referer"], request_uri.as_bytes());
    format!(
        "<input type=\"hidden\" name=\"_wp_http_referer\" value=\"{}\" />",
        text(&esc_url(&url))
    )
}

/// `wp_nonce_field( $action )`.
fn nonce_field(site: &dyn DashboardSite, action: &str, request_uri: &str) -> String {
    format!(
        "<input type=\"hidden\" id=\"_wpnonce\" name=\"_wpnonce\" value=\"{}\" />{}",
        site.nonce(action),
        referer(request_uri)
    )
}

/// `settings_fields( $group )`.
fn settings_fields(site: &dyn DashboardSite, group: &str, request_uri: &str) -> String {
    format!(
        "<input type='hidden' name='option_page' value='{group}' /><input type=\"hidden\" name=\"action\" value=\"update\" />{}",
        nonce_field(site, &format!("{group}-options"), request_uri)
    )
}

/// A settings screen: its file, title, help and markup.
struct Settings<'a> {
    file: &'a str,
    title: &'a str,
    help: &'a [HelpTab<'a>],
    sidebar: &'a str,
    scripts: &'a [&'a str],
    content: &'a str,
}

/// The page of a screen under Settings, or `wp_die()`'s refusal.
fn settings_page(
    site: &dyn DashboardSite,
    settings: &Settings,
    (canonical, origin): (&str, &str),
    browser: &Browser,
) -> Option<String> {
    let page = Page {
        screen: Screen {
            pagenow: settings.file,
            typenow: "",
            taxonomy: "",
            parent_file: Some("options-general.php"),
            submenu_file: None,
        },
        id: settings.file.trim_end_matches(".php"),
        title: settings.title,
        help: settings.help,
        sidebar: settings.sidebar,
        screen_options: "",
        styles: &[],
        scripts: settings.scripts,
        late_scripts: &[],
        script_data: &[],
        admin_footer: "",
        footer_scripts: "",
        content: settings.content,
        canonical,
        origin,
        view: None,
        script_after: &[],
        late_styles: &[],
        admin_head: "",
    };
    admin_page(site, &page, browser)
}

/// The page, or `wp_die()`'s refusal from `wp-admin/includes/menu.php`.
fn answer(page: Option<String>) -> Answer {
    page.map_or(
        Answer::Denied("Sorry, you are not allowed to access this page."),
        Answer::Page,
    )
}

/// `options-discussion.php`.
pub fn discussion(
    site: &dyn DashboardSite,
    (canonical, origin): (&str, &str),
    request_uri: &str,
    browser: &Browser,
) -> Answer {
    let depth = Value::String(option(site, "thread_comments_depth")).to_int();
    let mut select =
        String::from("<select name=\"thread_comments_depth\" id=\"thread_comments_depth\">");
    for i in 2..=10 {
        let chosen = selected(depth == i);
        select.push_str(&format!("<option value='{i}'{chosen}>{i}</option>"));
    }
    select.push_str("</select>");
    let shows = option(site, "show_avatars");
    let rating = option(site, "avatar_rating");
    let ratings: String = [
        ("G", "G &#8212; Suitable for all audiences"),
        (
            "PG",
            "PG &#8212; Possibly offensive, usually for audiences 13 and above",
        ),
        ("R", "R &#8212; Intended for adult audiences above 17"),
        ("X", "X &#8212; Even more mature than above"),
    ]
    .iter()
    .map(|(key, label)| {
        let chosen = if rating == key.as_bytes() {
            "checked=\"checked\""
        } else {
            ""
        };
        format!("\n\t<label><input type='radio' name='avatar_rating' value='{key}' {chosen}/> {label}</label><br />")
    })
    .collect();
    let current = site.option("avatar_default").unwrap_or(b"mystery".to_vec());
    let email = site.user_email();
    let avatars: String = [
        ("mystery", "Mystery Person"),
        ("blank", "Blank"),
        ("gravatar_default", "Gravatar Logo"),
        ("identicon", "Identicon (Generated)"),
        ("wavatar", "Wavatar (Generated)"),
        ("monsterid", "MonsterID (Generated)"),
        ("retro", "Retro (Generated)"),
        ("robohash", "RoboHash (Generated)"),
        ("initials", "Initials (Generated)"),
        ("color", "Color (Generated)"),
    ]
    .iter()
    .map(|(key, name)| {
        let chosen = if current == key.as_bytes() {
            "checked=\"checked\" "
        } else {
            ""
        };
        let avatar = default_avatar(&|name| site.option(name), &email, key, 32);
        format!(
            "\n\t<label><input type='radio' name='avatar_default' id='avatar_{key}' value='{key}' {chosen}/> {} {name}</label><br />",
            text(&avatar)
        )
    })
    .collect();
    let mut values = vec![
        ("fields", settings_fields(site, "discussion", request_uri)),
        (
            "default_ping_status",
            checks(site, "default_ping_status", "open"),
        ),
        (
            "default_comment_status",
            checks(site, "default_comment_status", "open"),
        ),
        ("depth", select),
        ("show_avatars", checked(shows == b"1").into()),
        (
            "hide",
            if Value::String(shows).is_empty() {
                " hide-if-js"
            } else {
                ""
            }
            .into(),
        ),
        ("ratings", ratings),
        ("avatars", avatars),
        (
            "wp_notes_notify",
            checked(site.option("wp_notes_notify").is_none_or(|v| v == b"1")).into(),
        ),
    ];
    for name in [
        "default_pingback_flag",
        "require_name_email",
        "comment_registration",
        "close_comments_for_old_posts",
        "show_comments_cookies_opt_in",
        "thread_comments",
        "page_comments",
        "comments_notify",
        "moderation_notify",
        "comment_moderation",
        "comment_previously_approved",
    ] {
        values.push((name, checks(site, name, "1")));
    }
    for name in [
        "close_comments_days_old",
        "comments_per_page",
        "comment_max_links",
    ] {
        values.push((name, attr(site, name)));
    }
    for (name, choice) in [
        ("default_comments_page", "newest"),
        ("default_comments_page", "oldest"),
        ("comment_order", "asc"),
        ("comment_order", "desc"),
    ] {
        values.push((
            choice,
            selected(option(site, name) == choice.as_bytes()).into(),
        ));
    }
    for name in ["moderation_keys", "disallowed_keys"] {
        values.push((name, text(&esc_textarea(&option(site, name)))));
    }
    let content = fill(DISCUSSION, &values);
    let help = [HelpTab {
        id: "overview",
        title: "Overview",
        content: &format!(
            "<p>This screen provides many options for controlling the management and display of comments and links to your posts/pages. So many, in fact, they will not all fit here! :) Use the documentation links to get information on what each discussion setting does.</p>{SAVE}"
        ),
    }];
    let sidebar = format!(
        "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/settings-discussion-screen/\">Documentation on Discussion Settings</a></p>{SUPPORT}"
    );
    let settings = Settings {
        file: "options-discussion.php",
        title: "Discussion Settings",
        help: &help,
        sidebar: &sidebar,
        scripts: &[],
        content: &content,
    };
    // `admin_print_footer_scripts` prints the script before the emoji
    // detection script.
    let page = settings_page(site, &settings, (canonical, origin), browser).map(|page| {
        page.replacen(
            "<script id=\"wp-emoji-settings\"",
            &format!("{DISCUSSION_JS}<script id=\"wp-emoji-settings\""),
            1,
        )
    });
    answer(page)
}

/// `options-media.php`.
pub fn media(
    site: &dyn DashboardSite,
    (canonical, origin): (&str, &str),
    request_uri: &str,
    browser: &Browser,
) -> Answer {
    let upload_path = option(site, "upload_path");
    // The folder and URL can be changed only once either is not the default.
    let editable = !option(site, "upload_url_path").is_empty()
        || !upload_path.is_empty() && upload_path != b"wp-content/uploads";
    let paths = if editable {
        fill(
            UPLOAD_PATHS,
            &[
                ("upload_path", attr(site, "upload_path")),
                ("upload_url_path", attr(site, "upload_url_path")),
            ],
        )
    } else {
        "<tr>\n<td class=\"td-full\">\n".into()
    };
    let mut values = vec![
        ("fields", settings_fields(site, "media", request_uri)),
        ("paths", paths),
    ];
    for name in [
        "thumbnail_size_w",
        "thumbnail_size_h",
        "medium_size_w",
        "medium_size_h",
        "large_size_w",
        "large_size_h",
    ] {
        values.push((name, attr(site, name)));
    }
    for name in ["thumbnail_crop", "uploads_use_yearmonth_folders"] {
        values.push((name, checks(site, name, "1")));
    }
    let content = fill(MEDIA, &values);
    let uploading = if editable {
        "<p>Uploading Files allows you to choose the folder and path for storing your uploaded files.</p>"
    } else {
        ""
    };
    let help = [HelpTab {
        id: "overview",
        title: "Overview",
        content: &format!(
            "<p>You can set maximum sizes for images inserted into your written content; you can also insert an image as Full Size.</p>{uploading}{SAVE}"
        ),
    }];
    let sidebar = format!(
        "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/settings-media-screen/\">Documentation on Media Settings</a></p>{SUPPORT}"
    );
    let settings = Settings {
        file: "options-media.php",
        title: "Media Settings",
        help: &help,
        sidebar: &sidebar,
        scripts: &[],
        content: &content,
    };
    answer(settings_page(site, &settings, (canonical, origin), browser))
}

/// `options-permalink.php`, on a server that rewrites URLs and whose
/// rewrite rules need no hand editing.
pub fn permalink(
    site: &dyn DashboardSite,
    (canonical, origin): (&str, &str),
    request_uri: &str,
    browser: &Browser,
) -> Answer {
    let structure = option(site, "permalink_structure");
    let home = BarSite::home_url(site);
    let base = home.trim_end_matches('/');
    let now = site.clock().1;
    let date = |format| format_unix(format, now, Zone::Utc);
    let structures = [
        ("plain", "Plain", String::new(), format!("{base}/?p=123")),
        (
            "day-name",
            "Day and name",
            "/%year%/%monthnum%/%day%/%postname%/".into(),
            format!("{base}/{}/sample-post/", date("Y/m/d")),
        ),
        (
            "month-name",
            "Month and name",
            "/%year%/%monthnum%/%postname%/".into(),
            format!("{base}/{}/sample-post/", date("Y/m")),
        ),
        (
            "numeric",
            "Numeric",
            "/archives/%post_id%".into(),
            format!("{base}/archives/123"),
        ),
        (
            "post-name",
            "Post name",
            "/%postname%/".into(),
            format!("{base}/sample-post/"),
        ),
    ];
    let custom = !structures
        .iter()
        .any(|(_, _, value, _)| structure == value.as_bytes());
    let rows: Vec<String> = structures
        .iter()
        .map(|(id, label, value, example)| {
            fill(
                STRUCTURE,
                &[
                    ("id", (*id).into()),
                    ("value", esc_attr(value.as_bytes())),
                    ("checked", checked(structure == value.as_bytes()).into()),
                    ("label", (*label).into()),
                    ("example", esc_html(example.as_bytes())),
                ],
            )
        })
        .collect();
    let content = fill(
        PERMALINK,
        &[
            ("fields", nonce_field(site, "update-permalink", request_uri)),
            ("structures", rows.join("\t\t\t")),
            ("custom", checked(custom).into()),
            ("base_url", text(&esc_url(base.as_bytes()))),
            ("structure", esc_attr(&structure)),
            ("base", base.into()),
            ("category_base", attr(site, "category_base")),
            ("tag_base", attr(site, "tag_base")),
        ],
    );
    let help = [
        HelpTab {
            id: "overview",
            title: "Overview",
            content: &format!(
                "<p>Permalinks are the permanent URLs to your individual pages and blog posts, as well as your category and tag archives. A permalink is the web address used to link to your content. The URL to each post should be permanent, and never change &#8212; hence the name permalink.</p><p>This screen allows you to choose your permalink structure. You can choose from common settings or create custom URL structures.</p>{SAVE}"
            ),
        },
        HelpTab {
            id: "permalink-settings",
            title: "Permalink Settings",
            content: &format!(
                "<p>Permalinks can contain useful information, such as the post date, title, or other elements. You can choose from any of the suggested permalink formats, or you can craft your own if you select Custom Structure.</p><p>If you pick an option other than Plain, your general URL path with structure tags (terms surrounded by <code>%</code>) will also appear in the custom structure field and your path can be further modified there.</p><p>When you assign multiple categories or tags to a post, only one can show up in the permalink: the lowest numbered category. This applies if your custom structure includes <code>%category%</code> or <code>%tag%</code>.</p>{SAVE}"
            ),
        },
        HelpTab {
            id: "custom-structures",
            title: "Custom Structures",
            content: &format!(
                "<p>The Optional fields let you customize the &#8220;category&#8221; and &#8220;tag&#8221; base names that will appear in archive URLs. For example, the page listing all posts in the &#8220;Uncategorized&#8221; category could be <code>/topics/uncategorized</code> instead of <code>/category/uncategorized</code>.</p>{SAVE}"
            ),
        },
    ];
    let sidebar = format!(
        "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/settings-permalinks-screen/\">Documentation on Permalinks Settings</a></p><p><a href=\"https://wordpress.org/documentation/article/customize-permalinks/\">Documentation on Using Permalinks</a></p>{SUPPORT}"
    );
    let settings = Settings {
        file: "options-permalink.php",
        title: "Permalink Settings",
        help: &help,
        sidebar: &sidebar,
        scripts: &[],
        content: &content,
    };
    answer(settings_page(site, &settings, (canonical, origin), browser))
}

/// `settings_errors()` of one error for `page_for_privacy_policy`.
fn privacy_error(message: &str) -> String {
    format!(
        "<div id='setting-error-page_for_privacy_policy' class='notice notice-error settings-error is-dismissible'> \n<p><strong>{message}</strong></p></div> \n"
    )
}

/// `options-privacy.php`.
pub fn privacy(
    site: &dyn DashboardSite,
    (canonical, origin): (&str, &str),
    request_uri: &str,
    browser: &Browser,
) -> Answer {
    let id = Value::String(option(site, "wp_page_for_privacy_policy")).to_int();
    let policy = (id != 0).then(|| u64::try_from(id).ok().and_then(|id| site.post(id)));
    let (error, page) = match policy {
        None => (String::new(), None),
        Some(None) => (
            privacy_error(
                "The currently selected Privacy Policy page does not exist. Please create or select a new page.",
            ),
            None,
        ),
        Some(Some(page)) if page.status == "trash" => (
            privacy_error(
                "The currently selected Privacy Policy page is in the Trash. Please create or select a new Privacy Policy page or <a href=\"edit?post_status=trash&post_type=page\">restore the current page</a>.",
            ),
            None,
        ),
        Some(page) => (String::new(), page),
    };
    let links = page.as_ref().map_or("\t\t".into(), |page| {
        let edit = BarSite::admin_url(site, &format!("post.php?post={id}&action=edit"));
        format!(
            "\t\t\t\t\t\t<strong>\n\t\t\t\t<a href=\"{}\">Edit</a> or <a href=\"{}\">{}</a> your Privacy Policy page content.\t\t\t\t</strong>\n\t\t\t",
            text(&esc_url(edit.as_bytes())),
            text(&esc_url(site.permalink(page).as_bytes())),
            if page.status == "publish" {
                "view"
            } else {
                "preview"
            }
        )
    });
    let query = PostQuery {
        post_types: vec!["page".into()],
        statuses: ["publish", "draft"]
            .map(|status| StatusFilter {
                status: status.into(),
                author: None,
            })
            .to_vec(),
        order: vec![(OrderBy::MenuOrder, true), (OrderBy::Title, true)],
        limit: u32::MAX,
        ..PostQuery::default()
    };
    let (pages, _) = site.list_posts(&query);
    let choose = if pages.is_empty() {
        String::new()
    } else {
        let options = page_options(&pages, &id.to_string());
        let select = if options.is_empty() {
            String::new()
        } else {
            format!(
                "<select name='page_for_privacy_policy' id='page_for_privacy_policy'>\n\t<option value=\"0\">&mdash; Select &mdash;</option>\n{options}</select>\n"
            )
        };
        fill(
            CHOOSE,
            &[
                (
                    "label",
                    if page.is_some() {
                        "Change your Privacy Policy page"
                    } else {
                        "Select a Privacy Policy page"
                    }
                    .into(),
                ),
                ("select", select),
                ("nonce", site.nonce("set-privacy-page")),
                ("referer", referer(request_uri)),
            ],
        )
    };
    let url = |path: &str| text(&esc_url(BarSite::admin_url(site, path).as_bytes()));
    let content = error
        + &fill(
            PRIVACY,
            &[
                ("settings_url", url("options-privacy.php")),
                ("guide_url", url("options-privacy.php?tab=policyguide")),
                ("links", links),
                (
                    "create",
                    if pages.is_empty() {
                        "There are no pages."
                    } else {
                        "Create a new Privacy Policy page"
                    }
                    .into(),
                ),
                ("create_nonce", site.nonce("create-privacy-page")),
                ("referer", referer(request_uri)),
                ("choose", choose),
            ],
        );
    let help = [HelpTab {
        id: "overview",
        title: "Overview",
        content: "<p>The Privacy screen lets you either build a new privacy-policy page or choose one you already have to show.</p><p>This screen includes suggestions to help you write your own privacy policy. However, it is your responsibility to use these resources correctly, to provide the information required by your privacy policy, and to keep this information current and accurate.</p>",
    }];
    let settings = Settings {
        file: "options-privacy.php",
        title: "Privacy",
        help: &help,
        sidebar: "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/settings-privacy-screen/\">Documentation on Privacy Settings</a></p>",
        scripts: &["privacy-tools"],
        content: &content,
    };
    // The `admin_body_class` filter's class comes before the screen's own.
    let page = settings_page(site, &settings, (canonical, origin), browser).map(|page| {
        page.replacen(
            "no-js options-privacy-php ",
            "no-js privacy-settings  options-privacy-php ",
            1,
        )
    });
    answer(page)
}

#[cfg(test)]
mod tests;
