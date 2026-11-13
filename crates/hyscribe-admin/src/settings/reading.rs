// SPDX-License-Identifier: AGPL-3.0-or-later

//! `options-reading.php`.

use hyscribe_model::Post;
use hyscribe_phpser::is_empty_string;

use super::{
    Form, SettingsSite, checks, form_option, option, pages_dropdown, settings_fields, settings_page,
};
use crate::list_table::{checked, fill};
use crate::page::{HelpTab, Request};
use crate::posts::{Answer, PAGE, page_options, published};

const CONTENT: &str = include_str!("reading.html");
const HEAD: &str = include_str!("reading-head.html");

const HELP: [HelpTab; 2] = [
    HelpTab {
        id: "overview",
        title: "Overview",
        content: "<p>This screen contains the settings that affect the display of your content.</p><p>You can choose what&#8217;s displayed on the homepage of your site. It can be posts in reverse chronological order (classic blog), or a fixed/static page. To set a static homepage, you first need to create two <a href=\"post-new?post_type=page\">Pages</a>. One will become the homepage, and the other will be where your posts are displayed.</p><p>You can also control the display of your content in RSS feeds, including the maximum number of posts to display and whether to show full text or an excerpt. <a href=\"https://developer.wordpress.org/advanced-administration/wordpress/feeds/\">Learn more about feeds</a>.</p><p>You must click the Save Changes button at the bottom of the screen for new settings to take effect.</p>",
    },
    HelpTab {
        id: "site-visibility",
        title: "Search engine visibility",
        content: "<p>You can choose whether or not your site will be crawled by robots, ping services, and spiders. If you want those services to ignore your site, click the checkbox next to &#8220;Discourage search engines from indexing this site&#8221; and click the Save Changes button at the bottom of the screen.</p><p>Note that even when set to discourage search engines, your site is still visible on the web and not all search engines adhere to this directive.</p><p>When this setting is in effect, a reminder is shown in the At a Glance box of the Dashboard that says, &#8220;Search engines discouraged&#8221;, to remind you that you have directed search engines to not crawl your site.</p>",
    },
];

/// `wp_admin_notice()` of a warning that the chosen pages clash.
fn warning(id: &str, message: &str) -> String {
    format!(
        "<div id=\"{id}\" class=\"notice notice-warning inline\"><p><strong>Warning:</strong> {message}</p></div>"
    )
}

/// The homepage choice over the published `pages`, or the hidden field
/// when none is listed; `None` where PHP would save `show_on_front` first.
fn front(site: &dyn SettingsSite, pages: &[Post]) -> Option<String> {
    let show = option(site, "show_on_front");
    let (on_front, for_posts) = (
        option(site, "page_on_front"),
        option(site, "page_for_posts"),
    );
    let unset = |id: &str| is_empty_string(id.as_bytes());
    if page_options(pages, "").is_empty() {
        return (show == "posts").then(|| {
            "<input name=\"show_on_front\" type=\"hidden\" value=\"posts\" />\n<table class=\"form-table\" role=\"presentation\">\n\t".to_string()
        });
    }
    if show == "page" && unset(&on_front) && unset(&for_posts) {
        return None;
    }
    let mut warnings = String::new();
    if show == "page" && for_posts == on_front {
        warnings.push_str(&warning(
            "front-page-warning",
            "these pages should not be the same!",
        ));
    }
    let privacy = option(site, "wp_page_for_privacy_policy");
    if !unset(&privacy) && (for_posts == privacy || on_front == privacy) {
        warnings.push_str(&warning(
            "privacy-policy-page-warning",
            "these pages should not be the same as your Privacy Policy page!",
        ));
    }
    Some(format!(
        "<table class=\"form-table\" role=\"presentation\">\n<tr>\n<th scope=\"row\">Your homepage displays</th>\n<td id=\"front-static-pages\"><fieldset>\n\t<legend class=\"screen-reader-text\"><span>Your homepage displays</span></legend>\n\t<p><label>\n\t\t<input name=\"show_on_front\" type=\"radio\" value=\"posts\" {} />\n\t\tYour latest posts\t</label>\n\t</p>\n\t<p><label>\n\t\t<input name=\"show_on_front\" type=\"radio\" value=\"page\" {} />\n\t\tA <a href=\"edit?post_type=page\">static page</a> (select below)\t</label>\n\t</p>\n<ul>\n\t<li><label for=\"page_on_front\">\n\tHomepage: {}</label></li>\n\t<li><label for=\"page_for_posts\">\n\tPosts page: {}</label></li>\n</ul>\n\t{warnings}</fieldset></td>\n</tr>\n",
        checked(show == "posts"),
        checked(show == "page"),
        pages_dropdown("page_on_front", &page_options(pages, &on_front)),
        pages_dropdown("page_for_posts", &page_options(pages, &for_posts)),
    ))
}

/// `options-reading.php`; `None` where PHP would save an option first.
pub fn reading(site: &dyn SettingsSite, request: &Request) -> Option<Answer> {
    let pages = published(site, PAGE);
    let excerpt = option(site, "rss_use_excerpt");
    let content = fill(
        CONTENT,
        &[
            ("fields", settings_fields(site, "reading", request.uri)),
            ("front", front(site, &pages)?),
            ("posts_per_page", form_option(site, "posts_per_page")),
            ("posts_per_rss", form_option(site, "posts_per_rss")),
            ("full", checked(excerpt == "0").into()),
            ("excerpt", checked(excerpt == "1").into()),
            ("private", checks(site, "blog_public", "0").into()),
        ],
    );
    let form = Form {
        file: "options-reading.php",
        title: "Reading Settings",
        help: &HELP,
        content: &content,
        admin_head: HEAD,
        ..Form::default()
    };
    Some(settings_page(site, &form, request))
}
