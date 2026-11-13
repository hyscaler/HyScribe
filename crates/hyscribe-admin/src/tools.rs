// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Tools screens: `tools.php`, `import.php`, `export.php`,
//! `export-personal-data.php` and `erase-personal-data.php`.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_model::Term;
use hyscribe_render::date::MONTHS;
use hyscribe_render::formatting::wptexturize;

use crate::admin_bar::{BarSite, Browser};
use crate::appearance::updates_settings;
use crate::list_table::{Column, Order, Sort, column_headers, fill, screen_options};
use crate::menu::{Screen, link};
use crate::page::{HelpTab, Page, PageSite, admin_page};
use crate::posts::{Answer, PostsSite, hidden_columns, per_page};
use crate::settings_more::{answer, referer};
use crate::users::{UserQuery, UsersSite};

/// `wp_print_admin_notice_templates()`.
pub(crate) const NOTICES: &str = include_str!("tools/notices.html");
const IMPORT: &str = include_str!("tools/import.html");
const EXPORT: &str = include_str!("tools/export.html");
const EXPORT_REQUESTS: &str = include_str!("tools/export-personal-data.html");
const ERASE_REQUESTS: &str = include_str!("tools/erase-personal-data.html");

const HELP: &str = "<p>Categories have hierarchy, meaning that you can nest sub-categories. Tags do not have hierarchy and cannot be nested. Sometimes people start out using one on their posts, then later realize that the other would work better for their content.</p><p>The Categories and Tags Converter link on this screen will take you to the Import screen, where that Converter is one of the plugins you can install. Once that plugin is installed, the Activate Plugin &amp; Run Importer link will take you to a screen where you can choose to convert tags into categories or vice versa.</p>";

const SUPPORT: &str = "<p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

const SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/tools-screen/\">Documentation on Tools</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// `export_add_js()`, which shows a content type's filters once chosen.
const EXPORT_JS: &str = "<script>\n\tjQuery( function($) {\n\t\tvar form = $('#export-filters'),\n\t\t\tfilters = form.find('.export-filters');\n\t\tfilters.hide();\n\t\tform.find('input:radio').on( 'change', function() {\n\t\t\tfilters.slideUp('fast');\n\t\t\tswitch ( $(this).val() ) {\n\t\t\t\tcase 'attachment': $('#attachment-filters').slideDown(); break;\n\t\t\t\tcase 'posts': $('#post-filters').slideDown(); break;\n\t\t\t\tcase 'pages': $('#page-filters').slideDown(); break;\n\t\t\t}\n\t\t});\n\t} );\n</script>\n\t";

const EXPORT_OVERVIEW: &str = "<p>This screen is where you manage requests for an export of personal data.</p><p>Privacy Laws around the world require businesses and online services to provide an export of some of the data they collect about an individual, and to deliver that export on request. The rights those laws enshrine are sometimes called the \"Right of Data Portability\". It allows individuals to obtain and reuse their personal data for their own purposes across different services. It allows them to move, copy or transfer personal data easily from one IT environment to another.</p><p>The tool associates data stored in WordPress with a supplied email address, including profile data and comments.</p><p><strong>Note: Since this tool only gathers data from WordPress and participating plugins, you may need to do more to comply with export requests. For example, you should also send the requester some of the data collected from or stored with the 3rd party services your organization uses.</strong></p>";

const EXPORT_DATA: &str = "<p>WordPress collects (but <em>never</em> publishes) a limited amount of data from registered users who have logged in to the site. Generally, these users are people who contribute to the site in some way -- content, store management, and so on. With rare exceptions, these users do not include occasional visitors who might have registered to comment on articles or buy products. The data WordPress retains can include:</p><p><strong>Profile Information</strong> &mdash; user email address, username, display name, nickname, first name, last name, description/bio, and registration date.</p><p><strong>Community Events Location</strong> &mdash; The IP Address of the user, which populates the Upcoming Community Events dashboard widget with relevant information.</p><p><strong>Session Tokens</strong> &mdash; User login information, IP Addresses, Expiration Date, User Agent (Browser/OS), and Last Login.</p><p><strong>Comments</strong> &mdash; For user comments, Email Address, IP Address, User Agent (Browser/OS), Date/Time, Comment Content, and Content URL.</p><p><strong>Media</strong> &mdash; A list of URLs for media files the user uploads.</p>";

const EXPORT_PLUGINS: &str = "<p>Many plugins may collect or store personal data either in the WordPress database or remotely. Any Export Personal Data request should include data from plugins as well.</p><p>If you are not sure, check the plugin documentation or contact the plugin author to see if the plugin collects data and if it supports the Data Exporter tool. This information may be available in the <a href=\"%guide%\">Privacy Policy Guide</a>.</p><p>If you are a plugin author, you can learn more about <a href=\"https://developer.wordpress.org/plugins/privacy/adding-the-personal-data-exporter-to-your-plugin/\">how to add the Personal Data Exporter to a plugin</a>.</p>";

const ERASE_OVERVIEW: &str = "<p>This screen is where you manage requests to erase personal data.</p><p>Privacy Laws around the world require businesses and online services to delete, anonymize, or forget the data they collect about an individual. The rights those laws enshrine are sometimes called the \"Right to be Forgotten\".</p><p>The tool associates data stored in WordPress with a supplied email address, including profile data and comments.</p><p><strong>Note: As this tool only gathers data from WordPress and participating plugins, you may need to do more to comply with erasure requests. For example, you are also responsible for ensuring that data collected by or stored with the 3rd party services your organization uses gets deleted.</strong></p>";

const ERASE_DATA: &str = "<p>WordPress collects (but <em>never</em> publishes) a limited amount of data from logged-in users but then deletes it or anonymizes it. That data can include:</p><p><strong>Profile Information</strong> &mdash; user email address, username, display name, nickname, first name, last name, description/bio, and registration date.</p><p><strong>Community Events Location</strong> &mdash; The IP Address of the user which is used for the Upcoming Community Events shown in the dashboard widget.</p><p><strong>Session Tokens</strong> &mdash; User login information, IP Addresses, Expiration Date, User Agent (Browser/OS), and Last Login.</p><p><strong>Comments</strong> &mdash; WordPress does not delete comments. The software does anonymize (but, again, <em>never</em> publishes) the associated Email Address, IP Address, and User Agent (Browser/OS).</p><p><strong>Media</strong> &mdash; A list of URLs for all media file uploads made by the user.</p>";

const ERASE_PLUGINS: &str = "<p>Many plugins may collect or store personal data either in the WordPress database or remotely. Any Erase Personal Data request should delete data from plugins as well.</p><p>If you are not sure, check the plugin documentation or contact the plugin author to see if the plugin collects data and if it supports the Data Eraser tool. This information may be available in the <a href=\"%guide%\">Privacy Policy Guide</a>.</p><p>If you are a plugin author, you can learn more about <a href=\"https://developer.wordpress.org/plugins/privacy/adding-the-personal-data-eraser-to-your-plugin/\">how to add the Personal Data Eraser to a plugin</a>.</p>";

/// What the Export screen asks of the site beyond its users.
pub trait ToolsSite: UsersSite {
    /// The IDs of the authors of posts of `post_type`, of any status.
    fn post_authors(&self, post_type: &str) -> Vec<u64>;
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A screen under Tools: its file, title, help and markup.
struct Tool<'a> {
    file: &'a str,
    title: &'a str,
    help: &'a [HelpTab<'a>],
    sidebar: &'a str,
    screen_options: &'a str,
    styles: &'a [&'a str],
    scripts: &'a [&'a str],
    script_data: &'a [(&'a str, String)],
    admin_head: &'a str,
    content: &'a str,
}

/// The page of a screen under Tools, or `None` for one who may not see it.
fn tool_page(
    site: &dyn PageSite,
    tool: &Tool,
    (canonical, origin): (&str, &str),
    browser: &Browser,
) -> Option<String> {
    let page = Page {
        screen: Screen {
            pagenow: tool.file,
            typenow: "",
            taxonomy: "",
            parent_file: None,
            submenu_file: None,
        },
        id: tool.file.trim_end_matches(".php"),
        title: tool.title,
        help: tool.help,
        sidebar: tool.sidebar,
        screen_options: tool.screen_options,
        styles: tool.styles,
        scripts: tool.scripts,
        late_scripts: &[],
        script_data: tool.script_data,
        script_after: &[],
        late_styles: &[],
        admin_head: tool.admin_head,
        admin_footer: "",
        footer_scripts: "",
        content: tool.content,
        canonical,
        origin,
        view: None,
    };
    admin_page(site, &page, browser)
}

/// The Tools screen at `canonical`, requested from `origin`, or `None`
/// when the user may not see it.
pub fn tools(
    site: &dyn PageSite,
    canonical: &str,
    origin: &str,
    browser: &Browser,
) -> Option<String> {
    let mut content = String::from("<div class=\"wrap\">\n<h1>Tools</h1>\n");
    // Categories and tags both need `manage_categories`.
    if site.can("import") && site.can("manage_categories") {
        content.push_str(&format!(
            "\t\t<div class=\"card\">\n\t\t\t<h2 class=\"title\">Categories and Tags Converter</h2>\n\t\t\t<p>\n\t\t\tIf you want to convert your categories to tags (or vice versa), use the <a href=\"{}\">Categories and Tags Converter</a> available from the Import screen.\t\t\t</p>\n\t\t</div>\n\t\t",
            link("import.php")
        ));
    }
    content.push_str("</div>\n");
    let help = [HelpTab {
        id: "converter",
        title: "Categories and Tags Converter",
        content: HELP,
    }];
    let tool = Tool {
        file: "tools.php",
        title: "Tools",
        help: &help,
        sidebar: SIDEBAR,
        screen_options: "",
        styles: &[],
        scripts: &[],
        script_data: &[],
        admin_head: "",
        content: &content,
    };
    tool_page(site, &tool, (canonical, origin), browser)
}

/// `import.php`, with no importer: none is installed, and none can be.
pub fn import(site: &dyn PostsSite, request: (&str, &str), browser: &Browser) -> Answer {
    let help = [HelpTab {
        id: "overview",
        title: "Overview",
        content: "<p>This screen lists links to plugins to import data from blogging/content management platforms. Choose the platform you want to import from, and click Install Now when you are prompted in the popup window. If your platform is not listed, click the link to search the plugin directory for other importer plugins to see if there is one for your platform.</p><p>In previous versions of WordPress, all importers were built-in. They have been turned into plugins since most people only use them once or infrequently.</p>",
    }];
    let sidebar = format!(
        "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/tools-import-screen/\">Documentation on Import</a></p>{SUPPORT}"
    );
    let tool = Tool {
        file: "import.php",
        title: "Import",
        help: &help,
        sidebar: &sidebar,
        screen_options: "",
        styles: &["thickbox"],
        scripts: &["thickbox", "plugin-install", "updates"],
        script_data: &[("updates", updates_settings(site, None))],
        admin_head: "",
        content: &(IMPORT.to_string() + NOTICES),
    };
    answer(tool_page(site, &tool, request, browser))
}

/// `export_date_options( $post_type )`: the months with posts of
/// `post_type` but auto-drafts, newest first.
fn date_options(site: &dyn PostsSite, post_type: &str) -> String {
    let mut months = site.months(post_type, false);
    months.extend(site.months(post_type, true));
    months.sort_unstable_by(|a, b| b.cmp(a));
    months.dedup();
    if let [(_, 0)] = months[..] {
        return String::new();
    }
    months
        .iter()
        .filter(|(year, _)| *year != 0)
        .map(|(year, month)| {
            let name = MONTHS
                .get(usize::try_from(*month).unwrap_or_default().wrapping_sub(1))
                .copied()
                .unwrap_or_default();
            format!(
                "<option value=\"{}\">{name} {year}</option>",
                esc_attr(format!("{year}-{month:02}").as_bytes())
            )
        })
        .collect()
}

/// `wp_dropdown_users()` of the authors of posts of `post_type`, every
/// user when there is none.
fn authors_dropdown(site: &dyn ToolsSite, post_type: &str) -> String {
    let authors = site.post_authors(post_type);
    let query = UserQuery {
        roles: &[],
        include: &authors,
        search: None,
        order: &[("display_name", false)],
        offset: 0,
        number: u64::MAX,
    };
    let options: String = site
        .list_users(&query)
        .0
        .iter()
        .map(|listed| {
            let user = &listed.user;
            let name = [&user.display_name[..], b" (", &user.login, b")"].concat();
            format!(
                "\t<option value='{}'>{}</option>\n",
                user.id,
                esc_html(&name)
            )
        })
        .collect();
    format!(
        "<select name='{post_type}_author' class=''>\n\t<option value='0'>All</option>\n{options}</select>"
    )
}

/// `wp_dropdown_categories()` of the categories with posts, by ID.
fn categories_dropdown(site: &dyn PostsSite) -> String {
    let mut terms: Vec<Term> = site
        .terms("category")
        .into_iter()
        .filter(|term| term.count > 0)
        .collect();
    terms.sort_unstable_by_key(|term| term.id);
    let options: String = terms
        .iter()
        .map(|term| {
            format!(
                "\t<option class=\"level-0\" value=\"{}\">{}</option>\n",
                term.id,
                text(&wptexturize(&term.name, &[]))
            )
        })
        .collect();
    format!(
        "<select  name='cat' id='cat' class='postform'>\n\t<option value='0' selected='selected'>All</option>\n{options}</select>\n"
    )
}

/// `export.php`'s form, without its `download` and the rest of its
/// arguments.
pub fn export(site: &dyn ToolsSite, request: (&str, &str), browser: &Browser) -> Answer {
    let content = fill(
        EXPORT,
        &[
            ("categories", categories_dropdown(site)),
            ("post_authors", authors_dropdown(site, "post")),
            ("page_authors", authors_dropdown(site, "page")),
            ("post_months", date_options(site, "post")),
            ("page_months", date_options(site, "page")),
            ("attachment_months", date_options(site, "attachment")),
        ],
    );
    let help = [HelpTab {
        id: "overview",
        title: "Overview",
        content: "<p>You can export a file of your site&#8217;s content in order to import it into another installation or platform. The export file will be an XML file format called WXR. Posts, pages, comments, custom fields, categories, and tags can be included. You can choose for the WXR file to include only certain posts or pages by setting the dropdown filters to limit the export by category, author, date range by month, or publishing status.</p><p>Once generated, your WXR file can be imported by another WordPress site or by another blogging platform able to access this format.</p>",
    }];
    let sidebar = format!(
        "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/tools-export-screen/\">Documentation on Export</a></p>{SUPPORT}"
    );
    let tool = Tool {
        file: "export.php",
        title: "Export",
        help: &help,
        sidebar: &sidebar,
        screen_options: "",
        styles: &[],
        scripts: &[],
        script_data: &[],
        admin_head: EXPORT_JS,
        content: &content,
    };
    answer(tool_page(site, &tool, request, browser))
}

/// Which personal data requests a screen lists.
#[derive(Clone, Copy)]
pub enum Requests {
    /// `export-personal-data.php`.
    Export,
    /// `erase-personal-data.php`.
    Erase,
}

/// The screen of `requests` while there is no request, `None` once there
/// is one, as their list is not ported.
pub fn privacy_requests(
    site: &dyn PostsSite,
    requests: Requests,
    (canonical, origin): (&str, &str),
    request_uri: &str,
    browser: &Browser,
) -> Option<Answer> {
    if !site.status_counts("user_request", None).is_empty() {
        return None;
    }
    let (file, title, action, template, help) = match requests {
        Requests::Export => (
            "export-personal-data.php",
            "Export Personal Data",
            "export_personal_data",
            EXPORT_REQUESTS,
            [EXPORT_OVERVIEW, EXPORT_DATA, EXPORT_PLUGINS],
        ),
        Requests::Erase => (
            "erase-personal-data.php",
            "Erase Personal Data",
            "remove_personal_data",
            ERASE_REQUESTS,
            [ERASE_OVERVIEW, ERASE_DATA, ERASE_PLUGINS],
        ),
    };
    let url = BarSite::admin_url(site, file);
    let screen = file.trim_end_matches(".php");
    let hidden = hidden_columns(site, screen);
    let hidden: Vec<&str> = hidden.iter().map(String::as_str).collect();
    let sort = |orderby| {
        Some(Sort {
            orderby,
            desc_first: false,
            abbr: "",
            caption: "",
            initial: None,
        })
    };
    let column = |key, label: &str, sort| Column {
        key,
        label: label.into(),
        sort,
        primary: false,
    };
    let mut columns = [
        column("cb", "", None),
        column("email", "Requester", sort("requester")),
        column("status", "Status", None),
        column("created_timestamp", "Requested", sort("requested")),
        column("next_steps", "Next steps", None),
    ];
    // The Requester column holds the row actions, yet Screen Options can
    // hide it.
    let option = format!("{action}_requests_per_page");
    let options = screen_options(
        &columns,
        &hidden.iter().map(|h| (*h).to_string()).collect::<Vec<_>>(),
        &option,
        per_page(site, &option, 20),
        "",
        site.nonce("screen-options-nonce"),
    );
    columns[1].primary = true;
    let headers = |counter, with_id| {
        column_headers(&columns, &hidden, &Order::default(), &url, counter, with_id)
    };
    let shown = columns.iter().filter(|c| !hidden.contains(&c.key)).count();
    let content = fill(
        template,
        &[
            ("url", text(&esc_url(url.as_bytes()))),
            ("request_nonce", site.nonce("personal-data-request")),
            ("bulk_nonce", site.nonce("bulk-privacy_requests")),
            ("referer", referer(request_uri)),
            ("head", headers(1, true)),
            ("foot", headers(2, false)),
            ("colspan", shown.to_string()),
        ],
    );
    let guide = esc_url(BarSite::admin_url(site, "options-privacy.php?tab=policyguide").as_bytes());
    let plugins = help[2].replace("%guide%", &text(&guide));
    let help = [
        HelpTab {
            id: "overview",
            title: "Overview",
            content: help[0],
        },
        HelpTab {
            id: "default-data",
            title: "Default Data",
            content: help[1],
        },
        HelpTab {
            id: "plugin-data",
            title: "Plugin Data",
            content: &plugins,
        },
    ];
    let sidebar = format!(
        "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/tools-{screen}-screen/\">Documentation on {title}</a></p>{SUPPORT}"
    );
    let tool = Tool {
        file,
        title,
        help: &help,
        sidebar: &sidebar,
        screen_options: &options,
        styles: &[],
        scripts: &["privacy-tools"],
        script_data: &[],
        admin_head: "",
        content: &content,
    };
    Some(answer(tool_page(site, &tool, (canonical, origin), browser)))
}

#[cfg(test)]
mod tests;
