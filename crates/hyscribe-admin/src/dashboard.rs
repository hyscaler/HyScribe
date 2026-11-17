// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Dashboard: `wp-admin/index.php` and `wp-admin/includes/dashboard.php`.

use hyscribe_kses::{
    esc_attr, esc_html, esc_url, esc_url_protocols, parse_url, strip_tags, wp_strip_all_tags,
};
use hyscribe_model::{Comment, Post, User};
use hyscribe_phpser::is_empty_string;
use hyscribe_phpser::{Maybe, Value, json_decode, maybe_unserialize, parse_str};
use hyscribe_render::date::{self, Zone};
use hyscribe_render::formatting::convert_chars;
use hyscribe_render::{comment_avatar, number_format, wp_trim_words};

use crate::admin_bar::BarSite;
use crate::comments::{comment_classes, comment_reply, comment_views, editor_footer};
use crate::list_table::{view_list, wp_nonce_field};
use crate::menu::Screen;
use crate::menu::{esc_link, link};
use crate::page::{HelpTab, Page, Request, admin_page, json, object, text};
use crate::posts::{PostsSite, draft_or_post_title};

/// `$wp_version`.
pub(crate) const VERSION: &str = include_str!("../../../wordpress/VERSION");
const WELCOME: &str = include_str!("dashboard/welcome.html");
const EVENTS: &str = include_str!("dashboard/events.html");
const SCREEN_OPTIONS: &str = include_str!("dashboard/screen-options.html");
/// `wp_print_community_events_templates()`, after the wrap.
const TEMPLATES: &str = include_str!("dashboard/templates.html");

/// A comment the Activity widget lists, with what its row shows.
pub struct RecentComment {
    /// The comment.
    pub comment: Comment,
    /// `wp_get_comment_status()`: `approved`, `unapproved`, `spam` or `trash`.
    pub status: &'static str,
    /// Its post, if it has one.
    pub post: Option<Post>,
    /// Its author's user account, if they had one.
    pub user: Option<User>,
    /// `current_user_can( 'edit_comment', $comment->comment_ID )`.
    pub can_edit: bool,
    /// `get_comment_link( $comment )`.
    pub link: String,
    /// The post's permalink.
    pub permalink: String,
}

/// `wp_get_comment_status()` of a comment's `comment_approved`; empty for
/// a state it does not name.
pub fn comment_status(approved: &str) -> &'static str {
    match approved {
        "1" => "approved",
        "0" => "unapproved",
        "spam" => "spam",
        "trash" => "trash",
        _ => "",
    }
}

/// The `comment_approved` of a `wp_get_comment_status()` state.
pub(crate) fn approved_of(status: &str) -> &str {
    match status {
        "approved" => "1",
        "unapproved" => "0",
        other => other,
    }
}

/// `wp_count_comments()`'s counts, and the user's comments.
#[derive(Clone, Copy, Default)]
pub struct CommentCounts {
    /// Approved comments.
    pub approved: u64,
    /// Comments awaiting moderation.
    pub moderated: u64,
    /// Spam.
    pub spam: u64,
    /// Comments in the Trash.
    pub trash: u64,
    /// The user's approved and pending comments.
    pub mine: u64,
}

impl CommentCounts {
    /// The approved and pending comments: what All lists.
    pub fn all(&self) -> u64 {
        self.approved + self.moderated
    }
}

/// What the dashboard asks of the site and its user.
pub trait DashboardSite: PostsSite {
    /// `get_user_meta( $user, 'show_welcome_panel', true )`, as stored.
    fn show_welcome_panel(&self) -> Option<Vec<u8>>;
    /// The user's email.
    fn user_email(&self) -> Vec<u8>;
    /// `wp_count_posts( $post_type )->publish`.
    fn published(&self, post_type: &str) -> u64;
    /// `wp_count_comments()`, with the user's own comments.
    fn comment_counts(&self) -> CommentCounts;
    /// The five latest comments the user may see; `global` is the post a
    /// comment on no post reads, the last the Activity widget lists.
    fn recent_comments(&self, global: Option<&Post>) -> Vec<RecentComment>;
    /// The theme's `Name` header.
    fn theme_name(&self) -> String;
    /// The latest five posts of `status`, `future` ones the user may edit
    /// soonest first, others newest first.
    fn recent_posts(&self, status: &str) -> Vec<Post>;
    /// The Quick Draft's auto-draft: the user's stored one while it is
    /// still an auto-draft.
    fn quick_draft(&self) -> Option<u64>;
    /// The user's four latest drafts, last modified first.
    fn drafts(&self) -> Vec<Post>;
}

/// The welcome panel for a user who may `edit_theme_options`, as
/// `index.php` and `wp_welcome_panel()` print it: for the bundled block
/// theme, or for an active theme the site does not have, which is classic.
fn welcome_panel(site: &dyn DashboardSite) -> String {
    if !site.can("edit_theme_options") {
        return String::new();
    }
    let hide = welcome_hidden(site);
    let url = |path: &str| {
        String::from_utf8_lossy(&esc_url(BarSite::admin_url(site, path).as_bytes())).into_owned()
    };
    let version = VERSION.trim();
    // `customize` maps to `edit_theme_options`, so the Customizer link shows.
    let (site_column, look) = if site.stylesheet() == "twentytwentyfive" {
        (
            format!(
                "<h3>Customize your entire site with block themes</h3>\n\t\t\t\t<p>Design everything on your site &#8212; from the header down to the footer, all using blocks and patterns.</p>\n\t\t\t\t<a href=\"{}\">Open site editor</a>\n\t\t\t\t\t\t",
                url("site-editor.php")
            ),
            format!(
                "<h3>Switch up your site&#8217;s look & feel with Styles</h3>\n\t\t\t\t<p>Tweak your site, or give it a whole new look! Get creative &#8212; how about a new color palette or font?</p>\n\t\t\t\t<a href=\"{}\">Edit styles</a>\n\t\t\t\t\t\t",
                url("site-editor.php?p=%2Fstyles")
            ),
        )
    } else {
        (
            format!(
                "<h3>Start Customizing</h3>\n\t\t\t\t<p>Configure your site&#8217;s logo, header, menus, and more in the Customizer.</p>\n\t\t\t\t\t\t\t\t\t<a class=\"load-customize hide-if-no-customize\" href=\"{}\">Open the Customizer</a>\n\t\t\t\t\t\t\t\t\t\t",
                url("customize.php")
            ),
            "<h3>Discover a new way to build your site.</h3>\n\t\t\t\t<p>There is a new kind of WordPress theme, called a block theme, that lets you build the site you&#8217;ve always wanted &#8212; with blocks and styles.</p>\n\t\t\t\t<a href=\"https://wordpress.org/documentation/article/block-themes/\">Learn about block themes</a>\n\t\t\t\t\t\t".into(),
        )
    };
    WELCOME
        .replace(
            "%classes%",
            if hide {
                "welcome-panel hidden"
            } else {
                "welcome-panel"
            },
        )
        .replace("%nonce%", &site.nonce("welcome-panel-nonce"))
        .replace("%dismiss%", &url("?welcome=0"))
        .replace("%about%", &url("about.php"))
        .replace("%version%", version.split('-').next().unwrap_or(version))
        .replace("%new_page%", &url("post-new.php?post_type=page"))
        .replace("%site%", &site_column)
        .replace("%look%", &look)
}

/// The notice after the admin email check was postponed with
/// `admin_email_remind_later`, shown for a minute after.
fn remind_later(site: &dyn DashboardSite, request_uri: &str) -> &'static str {
    let query = request_uri.split_once('?').map_or("", |(_, query)| query);
    let asked = parse_str(query.as_bytes());
    if asked
        .get_str("admin_email_remind_later")
        .is_none_or(Value::is_empty)
    {
        return "";
    }
    const INTERVAL: i64 = 3 * 86_400;
    let postponed = Value::String(site.option("admin_email_lifespan").unwrap_or_default());
    if site.clock().1 - (postponed.to_int() - INTERVAL) < 60 {
        "<div class=\"notice notice-success is-dismissible\"><p>The admin email verification page will reappear after 3 days.</p></div>"
    } else {
        ""
    }
}

/// `show_welcome_panel`: 0 hides the panel, 2 shows it only to the owner
/// of the admin email.
fn welcome_hidden(site: &dyn DashboardSite) -> bool {
    let option = Value::String(site.show_welcome_panel().unwrap_or_default()).to_int();
    option == 0 || (option == 2 && site.option("admin_email") != Some(site.user_email()))
}

/// `wp_dashboard_site_health()`, from the last check's cached counts.
fn site_health(site: &dyn DashboardSite) -> String {
    let stored = site.transient("health-check-site-status-result");
    let counts = stored
        .as_deref()
        .and_then(json_decode)
        .filter(|counts| counts.as_array().is_some_and(|entries| !entries.is_empty()));
    let count = |key: &str| {
        counts
            .as_ref()
            .and_then(|counts| counts.get_str(key))
            .map_or(0, Value::to_int)
    };
    let (critical, recommended) = (count("critical"), count("recommended"));
    let total = critical + recommended;
    let screen = String::from_utf8_lossy(&esc_url(
        BarSite::admin_url(site, "site-health.php").as_bytes(),
    ))
    .into_owned();
    let paragraph = |text: &str| format!("\t\t\t\t<p>\n\t\t\t\t\t{text}\t\t\t\t</p>\n\t\t\t");
    let (label, details) = if stored.is_none() {
        (
            "No information yet&hellip;",
            paragraph(&format!(
                "Site health checks will automatically run periodically to gather information about your site. You can also <a href=\"{screen}\">visit the Site Health screen</a> to gather information about your site now."
            )),
        )
    } else {
        let message = if total <= 0 {
            "Great job! Your site currently passes all site health checks."
        } else if critical == 1 {
            "Your site has a critical issue that should be addressed as soon as possible to improve its performance and security."
        } else if critical > 0 {
            "Your site has critical issues that should be addressed as soon as possible to improve its performance and security."
        } else if recommended == 1 {
            "Your site&#8217;s health is looking good, but there is still one thing you can do to improve its performance and security."
        } else {
            "Your site&#8217;s health is looking good, but there are still some things you can do to improve its performance and security."
        };
        (
            "Results are still loading&hellip;",
            paragraph(&format!("\t\t\t\t\t\t{message}\t\t\t\t\t")),
        )
    };
    let mut details = format!("\t\t\t{details}\n\t\t\t");
    if total > 0 && stored.is_some() {
        let items = if total == 1 { "item" } else { "items" };
        details.push_str(&paragraph(&format!("Take a look at the <strong>{total} {items}</strong> on the <a href=\"{screen}\">Site Health screen</a>.")));
    }
    format!(
        "{SITE_HEALTH}\t\t\t\t\t\t\t\t\t{label}\t\t\t\t\t\t\t</div>\n\t\t</div>\n\n\t\t<div class=\"site-health-details\">\n{details}\t\t</div>\n\t</div>\n\n\t"
    )
}

/// The Site Health widget up to its label's text.
const SITE_HEALTH: &str = "\t<div class=\"health-check-widget\">\n\t\t<div class=\"health-check-widget-title-section site-health-progress-wrapper loading hide-if-no-js\">\n\t\t\t<div class=\"site-health-progress\">\n\t\t\t\t<svg aria-hidden=\"true\" focusable=\"false\" width=\"100%\" height=\"100%\" viewBox=\"0 0 200 200\" version=\"1.1\" xmlns=\"http://www.w3.org/2000/svg\">\n\t\t\t\t\t<circle r=\"90\" cx=\"100\" cy=\"100\" fill=\"transparent\" stroke-dasharray=\"565.48\" stroke-dashoffset=\"0\"></circle>\n\t\t\t\t\t<circle id=\"bar\" r=\"90\" cx=\"100\" cy=\"100\" fill=\"transparent\" stroke-dasharray=\"565.48\" stroke-dashoffset=\"0\"></circle>\n\t\t\t\t</svg>\n\t\t\t</div>\n\t\t\t<div class=\"site-health-progress-label\">\n";

/// `wp_dashboard_right_now()`, At a Glance.
fn right_now(site: &dyn DashboardSite) -> String {
    let mut out = String::from("\t<div class=\"main\">\n\t<ul>\n\t");
    for (post_type, singular, plural, cap) in [
        ("post", "Published post", "Published posts", "edit_posts"),
        ("page", "Published page", "Published pages", "edit_pages"),
    ] {
        let count = site.published(post_type);
        if count == 0 {
            continue;
        }
        let text = format!(
            "{} {}",
            number_format(count),
            if count == 1 { singular } else { plural }
        );
        if site.can(cap) {
            let url = BarSite::admin_url(
                site,
                &format!("edit.php?post_status=publish&post_type={post_type}"),
            );
            out.push_str(&format!(
                "<li class=\"{post_type}-count\"><a href=\"{}\">{}</a></li>",
                String::from_utf8_lossy(&esc_url(url.as_bytes())),
                esc_html(text.as_bytes())
            ));
        } else {
            out.push_str(&format!(
                "<li class=\"{post_type}-count\"><span>{text}</span></li>"
            ));
        }
    }
    let CommentCounts {
        approved,
        moderated,
        ..
    } = site.comment_counts();
    if approved + moderated > 0 {
        let comments = if approved == 1 { "Comment" } else { "Comments" };
        let pending = if moderated == 1 {
            "Comment"
        } else {
            "Comments"
        };
        let hidden = if moderated == 0 { " hidden" } else { "" };
        out.push_str(&format!(
            "\t\t<li class=\"comment-count\">\n\t\t\t<a href=\"{}\">{} {comments}</a>\n\t\t</li>\n\t\t\t\t<li class=\"comment-mod-count{hidden}\">\n\t\t\t<a href=\"{}?comment_status=moderated\" class=\"comments-in-moderation-text\">{} {pending} in moderation</a>\n\t\t</li>\n\t\t",
            link("edit-comments.php"),
            number_format(approved),
            link("edit-comments.php"),
            number_format(moderated)
        ));
    }
    // update_right_now_message(): nobody here may `update_core`.
    // A theme that is not found is named after its folder.
    let stylesheet = site.stylesheet();
    let name = if stylesheet == "twentytwentyfive" {
        site.theme_name()
    } else {
        stylesheet
    };
    let theme = esc_html(name.as_bytes());
    let theme = if site.can("switch_themes") {
        format!("<a href=\"{}\">{theme}</a>", link("themes.php"))
    } else {
        theme
    };
    out.push_str(&format!(
        "\t</ul>\n\t<p id='wp-version-message'><span id=\"wp-version\">WordPress {} running {theme} theme.</span></p>",
        VERSION.trim()
    ));
    let public = !is_empty_string(&site.option("blog_public").unwrap_or_default());
    if site.can("manage_options") && !public {
        out.push_str(&format!(
            "<p class='search-engines-info'><a href='{}'>Search engines discouraged</a></p>",
            link("options-reading.php")
        ));
    }
    out.push_str("\t</div>\n\t");
    out
}

/// `wp_dashboard_recent_posts()`: the posts of `status` under `title`.
fn recent_posts(site: &dyn DashboardSite, posts: &[Post], title: &str, id: &str) -> String {
    if posts.is_empty() {
        return String::new();
    }
    let (zone, now) = site.clock();
    let time_format = text_option(site, "time_format");
    let today = date::format_unix("Y-m-d", now, zone);
    // `current_datetime()->modify( '+1 day' )`: the next calendar day.
    let days = |date: &str| {
        let part = |range: std::ops::Range<usize>| {
            date.get(range).and_then(|p| p.parse().ok()).unwrap_or(0)
        };
        date::days_from_civil(part(0..4), part(5..7), part(8..10))
    };
    let tomorrow = date::format_unix("Y-m-d", (days(&today) + 1) * 86_400, Zone::Utc);
    let year = date::format_unix("Y", now, zone);
    let mut out = format!("<div id=\"{id}\" class=\"activity-block\"><h3>{title}</h3><ul>");
    for post in posts {
        // `get_the_time( 'U' )` is the local time as if it were UTC, which
        // `gmdate()` and `date_i18n()` read back as local.
        let utc = |format: &str| date::format(format, &post.date, Zone::Utc);
        let day = if utc("Y-m-d") == today {
            "Today".to_string()
        } else if utc("Y-m-d") == tomorrow {
            "Tomorrow".to_string()
        } else if utc("Y") != year {
            utc("M jS Y")
        } else {
            utc("M jS")
        };
        let time = date::format(&time_format, &post.date, zone);
        let link = if site.can_post("edit_post", post) {
            BarSite::admin_url(site, &format!("post.php?post={}&amp;action=edit", post.id))
        } else {
            site.permalink(post)
        };
        let title = draft_or_post_title(post);
        out.push_str(&format!(
            "<li><span>{day}, {time}</span> <a href=\"{link}\" aria-label=\"{}\">{title}</a></li>",
            esc_attr(format!("Edit &#8220;{title}&#8221;").as_bytes())
        ));
    }
    out.push_str("</ul></div>");
    out
}

/// `_wp_dashboard_recent_comments_row()` of a reply `admin-ajax.php` saved.
pub fn ajax_comment_row(site: &dyn DashboardSite, row: &RecentComment) -> String {
    comment_row(site, row, 0, None)
}

/// `_wp_dashboard_recent_comments_row()`, the `index`th row.
/// `global` is the last post the Activity widget listed, which PHP leaves as
/// the global post the rows' classes and a postless comment read.
fn comment_row(
    site: &dyn DashboardSite,
    row: &RecentComment,
    index: usize,
    global: Option<&Post>,
) -> String {
    let comment = &row.comment;
    let post_link = row.post.as_ref().map(|post| {
        let title = draft_or_post_title(post);
        format!(
            "<a href=\"{}\">{title}</a>",
            crate::text(&esc_url(row.permalink.as_bytes()))
        )
    });
    let mut actions = String::new();
    if row.can_edit {
        let id = comment.id;
        let url = |action: &str, nonce: &str| {
            // The nonce, hex digits, escapes to itself.
            let url = format!(
                "comment.php?action={action}&p={}&c={id}&_wpnonce=",
                comment.post_id
            );
            let url = esc_link(&url);
            format!("{url}{}", site.nonce(&format!("{nonce}_{id}")))
        };
        let links = [
            (
                "approve",
                format!(
                    "<a href=\"{}\" data-wp-lists=\"dim:the-comment-list:comment-{id}:unapproved:e7e7d3:e7e7d3:new=approved\" class=\"vim-a aria-button-if-js\" aria-label=\"Approve this comment\">Approve</a>",
                    url("approvecomment", "approve-comment")
                ),
            ),
            (
                "unapprove",
                format!(
                    "<a href=\"{}\" data-wp-lists=\"dim:the-comment-list:comment-{id}:unapproved:e7e7d3:e7e7d3:new=unapproved\" class=\"vim-u aria-button-if-js\" aria-label=\"Unapprove this comment\">Unapprove</a>",
                    url("unapprovecomment", "approve-comment")
                ),
            ),
            (
                "reply hide-if-no-js",
                format!(
                    "<button type=\"button\" onclick=\"window.commentReply && commentReply.open('{id}','{}');\" class=\"vim-r button-link hide-if-no-js\" aria-label=\"Reply to this comment\">Reply</button>",
                    comment.post_id
                ),
            ),
            (
                "edit",
                format!(
                    "<a href=\"{}?action=editcomment&amp;c={id}\" aria-label=\"Edit this comment\">Edit</a>",
                    link("comment.php")
                ),
            ),
            (
                "spam",
                format!(
                    "<a href=\"{}\" data-wp-lists=\"delete:the-comment-list:comment-{id}::spam=1\" class=\"vim-s vim-destructive aria-button-if-js\" aria-label=\"Mark this comment as spam\">Spam</a>",
                    url("spamcomment", "delete-comment")
                ),
            ),
            if site.uses_trash() {
                (
                    "trash",
                    format!(
                        "<a href=\"{}\" data-wp-lists=\"delete:the-comment-list:comment-{id}::trash=1\" class=\"delete vim-d vim-destructive aria-button-if-js\" aria-label=\"Move this comment to the Trash\">Trash</a>",
                        url("trashcomment", "delete-comment")
                    ),
                )
            } else {
                (
                    "delete",
                    format!(
                        "<a href=\"{}\" data-wp-lists=\"delete:the-comment-list:comment-{id}::trash=1\" class=\"delete vim-d vim-destructive aria-button-if-js\" aria-label=\"Delete this comment permanently\">Delete Permanently</a>",
                        url("deletecomment", "delete-comment")
                    ),
                )
            },
            (
                "view",
                format!(
                    "<a class=\"comment-link\" href=\"{}\" aria-label=\"View this comment\">View</a>",
                    crate::text(&esc_url(row.link.as_bytes()))
                ),
            ),
        ];
        for (i, (action, markup)) in links.iter().enumerate() {
            // Approve and Unapprove both open the list, one of them hidden.
            let separator = if i < 2 { "" } else { " | " };
            let hidden = if *action == "view" && row.status != "approved" {
                " hidden"
            } else {
                ""
            };
            actions.push_str(&format!(
                "<span class='{action}{hidden}'>{separator}{markup}</span>"
            ));
        }
    }
    let classes = comment_classes(
        comment,
        row.user.as_ref(),
        global.map(|post| post.author),
        index,
        &["comment-item", row.status],
    );
    let email = row
        .user
        .as_ref()
        .map_or(&comment.author_email, |user| &user.email);
    // `get_avatar()` draws only comments' avatars.
    let avatar = if matches!(&comment.comment_type[..], b"" | b"comment") {
        comment_avatar(&|name| site.option(name), email, 50)
    } else {
        Vec::new()
    };
    let has_avatar = if avatar.is_empty() { "" } else { " has-avatar" };
    let author = if is_empty_string(&comment.author) {
        row.user.as_ref().map_or("Anonymous".to_string(), |user| {
            crate::text(&user.display_name)
        })
    } else {
        crate::text(&comment.author)
    };
    let url = if comment.author_url == b"http://" {
        Vec::new()
    } else {
        esc_url_protocols(&comment.author_url, &["http", "https"])
    };
    let author_link = if url.is_empty() {
        author
    } else {
        // `wp_is_internal_link()`: the home URL's host.
        let host = |url: &[u8]| {
            parse_url(url)
                .and_then(|parts| parts.host)
                .map(|h| h.to_ascii_lowercase())
        };
        let scheme = parse_url(&url).and_then(|parts| parts.scheme);
        let web = scheme
            .is_some_and(|s| s.eq_ignore_ascii_case(b"http") || s.eq_ignore_ascii_case(b"https"));
        let internal = web && host(&url) == host(site.home_url().as_bytes());
        let rel = if internal {
            "ugc"
        } else {
            "ugc external nofollow"
        };
        format!(
            "<a href=\"{}\" class=\"url\" rel=\"{rel}\">{author}</a>",
            crate::text(&url)
        )
    };
    let pending = "<span class=\"approve\">[Pending]</span>";
    let kind = match &comment.comment_type[..] {
        b"" | b"comment" => None,
        b"pingback" => Some("Pingback".to_string()),
        b"trackback" => Some("Trackback".to_string()),
        other => Some(esc_html(&ucwords(other))),
    };
    let meta = match (&kind, &post_link) {
        (None, Some(post)) => {
            format!("From <cite class=\"comment-author\">{author_link}</cite> on {post} {pending}")
        }
        (None, None) => {
            format!("From <cite class=\"comment-author\">{author_link}</cite> {pending}")
        }
        (Some(kind), Some(post)) => format!("<strong>{kind}</strong> on {post} {pending}"),
        (Some(kind), None) => format!("<strong>{kind}</strong> {pending}"),
    };
    let body = match kind {
        None => format!(
            "\n\t\t\t<div class=\"dashboard-comment-wrap has-row-actions {has_avatar}\">\n\t\t\t<p class=\"comment-meta\">\n\t\t\t\t{meta}\t\t\t</p>\n\n\t\t\t\t"
        ),
        Some(_) => format!(
            "\t\t\t<div class=\"dashboard-comment-wrap has-row-actions\">\n\t\t\t<p class=\"comment-meta\">\n\t\t\t\t{meta}\t\t\t</p>\n\t\t\t<p class=\"comment-author\">{author_link}</p>\n\n\t\t\t"
        ),
    };
    let protected = row
        .post
        .as_ref()
        .or(global)
        .is_some_and(|post| !post.password.is_empty());
    let excerpt = if protected {
        "Password protected".to_string()
    } else {
        crate::text(&convert_chars(&wp_trim_words(
            &strip_tags(&comment.content),
            20,
            b"&hellip;",
        )))
    };
    let actions = if actions.is_empty() {
        String::new()
    } else {
        format!("\t\t\t<p class=\"row-actions\">{actions}</p>\n\t\t\t")
    };
    format!(
        "\n\t\t<li id=\"comment-{}\" class=\"{}\">\n\n\t\t\t{}\n\t\t\t{body}\t\t\t<blockquote><p>{excerpt}</p></blockquote>\n\t\t\t{actions}\t\t\t</div>\n\t\t</li>\n\t",
        comment.id,
        classes,
        crate::text(&avatar)
    )
}

/// `ucwords()`: each word's first letter in upper case.
fn ucwords(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut start = true;
    for &byte in text {
        out.push(if start {
            byte.to_ascii_uppercase()
        } else {
            byte
        });
        start = matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c);
    }
    out
}

/// `wp_dashboard_site_activity()`, the Activity widget.
fn activity(site: &dyn DashboardSite, data: &Activity) -> String {
    let future = recent_posts(site, &data.future, "Publishing Soon", "future-posts");
    let published = recent_posts(
        site,
        &data.published,
        "Recently Published",
        "published-posts",
    );
    let global = data.published.last().or(data.future.last());
    let comments = recent_comments(site, &data.comments, global);
    let none = if future.is_empty() && published.is_empty() && comments.is_empty() {
        "<div class=\"no-activity\"><p>No activity yet!</p></div>"
    } else {
        ""
    };
    format!("<div id=\"activity-widget\">{future}{published}{comments}{none}</div>")
}

/// `wp_dashboard_recent_comments()`: the comment rows, the counts of each
/// status, and the reply form and Trash notices the rows' actions use.
fn recent_comments(
    site: &dyn DashboardSite,
    comments: &[RecentComment],
    global: Option<&Post>,
) -> String {
    if comments.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "<div id=\"latest-comments\" class=\"activity-block table-view-list\"><h3>Recent Comments</h3><ul id=\"the-comment-list\" data-wp-lists=\"list:comment\">",
    );
    for (index, row) in comments.iter().enumerate() {
        out.push_str(&comment_row(site, row, index, global));
    }
    out.push_str("</ul>");
    if site.can("edit_posts") {
        out.push_str("<h3 class=\"screen-reader-text\">View more comments</h3>");
        out.push_str(&view_list(&comment_views(
            site,
            site.comment_counts(),
            "",
            "",
            0,
        )));
    }
    out.push_str(&comment_reply(site, None, (false, "dashboard"), -1, true));
    out.push_str("\t</div>");
    out
}

/// `wp_dashboard_quick_press( $message, $type )` for a user who may
/// `edit_posts`, the page requested at `request_uri`: the notice of
/// `notice`, a message and its type, first.
pub fn quick_press(
    site: &dyn DashboardSite,
    request_uri: &str,
    notice: Option<(&str, &str)>,
) -> String {
    let action = String::from_utf8_lossy(&esc_url(BarSite::admin_url(site, "post.php").as_bytes()))
        .into_owned();
    let notice = notice.map_or(String::new(), |(message, kind)| {
        format!("<div class=\"notice notice-{kind}\" role=\"alert\"><p>{message}</p></div>")
    });
    format!(
        "\n\t<form name=\"post\" action=\"{action}\" method=\"post\" id=\"quick-press\" class=\"hide-if-no-js\">\n\n\t\t{notice}\n\t\t<div class=\"input-text-wrap\" id=\"title-wrap\">\n\t\t\t<label for=\"title\">\n\t\t\t\tTitle\t\t\t</label>\n\t\t\t<input type=\"text\" name=\"post_title\" id=\"title\" autocomplete=\"off\" />\n\t\t</div>\n\n\t\t<div class=\"textarea-wrap\" id=\"description-wrap\">\n\t\t\t<label for=\"content\">Content</label>\n\t\t\t<textarea name=\"content\" id=\"content\" placeholder=\"What&#8217;s on your mind?\" class=\"mceEditor\" rows=\"3\" cols=\"15\" autocomplete=\"off\"></textarea>\n\t\t</div>\n\n\t\t<p class=\"submit\">\n\t\t\t<input type=\"hidden\" name=\"action\" id=\"quickpost-action\" value=\"post-quickdraft-save\" />\n\t\t\t<input type=\"hidden\" name=\"post_ID\" value=\"{}\" />\n\t\t\t<input type=\"hidden\" name=\"post_type\" value=\"post\" />\n\t\t\t{}\t\t\t<input type=\"submit\" name=\"save\" id=\"save-post\" class=\"button button-primary\" value=\"Save Draft\"  />\t\t\t<br class=\"clear\" />\n\t\t</p>\n\n\t</form>\n\t{}",
        site.quick_draft()
            .map(|id| id.to_string())
            .unwrap_or_default(),
        wp_nonce_field(site, "add-post", "_wpnonce", request_uri),
        recent_drafts(site)
    )
}

/// `wp_dashboard_recent_drafts()`: three of the user's drafts, and a link
/// to the rest when there are more.
fn recent_drafts(site: &dyn DashboardSite) -> String {
    let drafts = site.drafts();
    if drafts.is_empty() {
        return String::new();
    }
    let mut out = String::from("<div class=\"drafts\">");
    if drafts.len() > 3 {
        let all = esc_url(BarSite::admin_url(site, "edit.php?post_status=draft").as_bytes());
        out.push_str(&format!(
            "<p class=\"view-all\"><a href=\"{}\">View all drafts</a></p>\n",
            String::from_utf8_lossy(&all)
        ));
    }
    out.push_str("<h2 class=\"hide-if-no-js\">Your Recent Drafts</h2>\n<ul>");
    let (zone, _) = site.clock();
    for draft in drafts.iter().take(3) {
        let url = BarSite::admin_url(site, &format!("post.php?post={}&amp;action=edit", draft.id));
        let title = draft_or_post_title(draft);
        out.push_str(&format!(
            "<li>\n<div class=\"draft-title\"><a href=\"{}\" aria-label=\"{}\">{}</a><time datetime=\"{}\">{}</time></div>",
            String::from_utf8_lossy(&esc_url(url.as_bytes())),
            esc_attr(format!("Edit &#8220;{title}&#8221;").as_bytes()),
            esc_html(title.as_bytes()),
            date::format("c", &draft.date, zone),
            date::format("F j, Y", &draft.date, zone)
        ));
        let content = wp_trim_words(&draft.content, 10, b"&hellip;");
        if !content.is_empty() {
            out.push_str(&format!(
                "<p class=\"draft-content\">{}</p>",
                String::from_utf8_lossy(&content)
            ));
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n</div>");
    out
}

/// What the Activity widget lists: the scheduled and published posts and
/// the latest comments.
struct Activity {
    future: Vec<Post>,
    published: Vec<Post>,
    comments: Vec<RecentComment>,
}

/// An option as text, empty when unset.
fn text_option(site: &dyn DashboardSite, name: &str) -> String {
    crate::text(&site.option(name).unwrap_or_default())
}

/// A meta box, as a dashboard widget: ID, title, context and content.
pub(crate) struct Widget {
    pub(crate) id: &'static str,
    pub(crate) title: &'static str,
    pub(crate) context: String,
    pub(crate) inside: String,
}

/// `wp_dashboard_setup()`'s widgets for the user, in registration order.
fn widgets(site: &dyn DashboardSite, data: &Activity, request_uri: &str) -> Vec<Widget> {
    let mut widgets = Vec::new();
    let mut add = |id, title, context: &str, inside: String| {
        widgets.push(Widget {
            id,
            title,
            context: context.into(),
            inside,
        });
    };
    if site.can("view_site_health_checks") {
        add(
            "dashboard_site_health",
            "Site Health Status",
            "normal",
            site_health(site),
        );
    }
    if site.can("edit_posts") {
        add(
            "dashboard_right_now",
            "At a Glance",
            "normal",
            right_now(site),
        );
    }
    add(
        "dashboard_activity",
        "Activity",
        "normal",
        activity(site, data),
    );
    if site.can("edit_posts") {
        add(
            "dashboard_quick_press",
            "<span class=\"hide-if-no-js\">Quick Draft</span> <span class=\"hide-if-js\">Your Recent Drafts</span>",
            "side",
            quick_press(site, request_uri, None),
        );
    }
    add(
        "dashboard_primary",
        "WordPress Events and News",
        "side",
        events(site),
    );
    widgets
}

/// The meta boxes as `do_meta_boxes()` orders them: those the user sorted
/// in the option `name` first, in their order and columns.
pub(crate) fn sorted(site: &dyn PostsSite, name: &str, mut widgets: Vec<Widget>) -> Vec<Widget> {
    let order = user_array(site, name);
    let mut sorted: Vec<Widget> = Vec::new();
    for (context, ids) in &order {
        let context = String::from_utf8_lossy(&context.to_php_string()).into_owned();
        for id in String::from_utf8_lossy(&ids.to_php_string()).split(',') {
            if let Some(at) = widgets.iter().position(|w| w.id == id) {
                let mut widget = widgets.remove(at);
                widget.context = context.clone();
                sorted.push(widget);
            } else {
                // Sorted twice into one column, a box is left without a title.
                sorted.retain(|w| w.id != id || w.context != context);
            }
        }
    }
    sorted.extend(widgets);
    sorted
}

/// A user option holding a serialized array, as an array.
fn user_array(site: &dyn PostsSite, name: &str) -> Vec<(Value, Value)> {
    let stored = site.user_option(name).unwrap_or_default();
    let Maybe::Value(Value::Array(entries)) = maybe_unserialize(&stored) else {
        return Vec::new();
    };
    entries
        .into_iter()
        .map(|(key, value)| (Value::String(key.to_php_string()), value))
        .collect()
}

/// The IDs a user option lists, as `get_hidden_meta_boxes()` reads them.
pub(crate) fn listed(site: &dyn PostsSite, name: &str) -> Vec<String> {
    user_array(site, name)
        .iter()
        .map(|(_, id)| String::from_utf8_lossy(&id.to_php_string()).into_owned())
        .collect()
}

/// `wp_dashboard_events_news()`.
fn events(site: &dyn DashboardSite) -> String {
    let ajax = esc_url(BarSite::admin_url(site, "admin-ajax.php").as_bytes());
    EVENTS.replace("%ajax%", &String::from_utf8_lossy(&ajax))
}

/// The Dashboard for `request`, or `None` when the user may not see it.
pub fn dashboard(site: &dyn DashboardSite, request: &Request) -> Option<String> {
    let &Request {
        canonical,
        origin,
        uri: request_uri,
        browser,
    } = request;
    let hidden = listed(site, "metaboxhidden_dashboard");
    let closed = listed(site, "closedpostboxes_dashboard");
    let future = site.recent_posts("future");
    let published = site.recent_posts("publish");
    let comments = site.recent_comments(published.last().or(future.last()));
    let data = Activity {
        future,
        published,
        comments,
    };
    let registered = widgets(site, &data, request_uri);
    // The screen options list the widgets before `do_meta_boxes()` sorts them.
    let mut labels: String = registered
        .iter()
        .map(|widget| {
            let id = widget.id;
            let checked = if hidden.iter().any(|h| h == id) {
                ""
            } else {
                " checked='checked'"
            };
            format!("<label for=\"{id}-hide\"><input class=\"hide-postbox-tog\" name=\"{id}-hide\" type=\"checkbox\" id=\"{id}-hide\" value=\"{id}\" {checked} />{}</label>", widget.title)
        })
        .collect();
    let widgets = sorted(site, "meta-box-order_dashboard", registered);
    let mut columns = String::from("<div id=\"dashboard-widgets\" class=\"metabox-holder\">\n");
    for (number, context) in ["normal", "side", "column3", "column4"].iter().enumerate() {
        columns.push_str(&format!(
            "\t<div id=\"postbox-container-{}\" class=\"postbox-container\">\n\t<div id=\"{context}-sortables\" class=\"meta-box-sortables\">",
            number + 1
        ));
        for widget in widgets.iter().filter(|w| w.context == *context) {
            let id = widget.id.to_string();
            columns.push_str(&postbox(
                widget.id,
                widget.title,
                closed.contains(&id),
                hidden.contains(&id),
                &widget.inside,
            ));
        }
        columns.push_str("</div>\t</div>\n");
    }
    columns.push_str(&format!(
        "</div>\n\n\t<input type=\"hidden\" id=\"closedpostboxesnonce\" name=\"closedpostboxesnonce\" value=\"{}\" /><input type=\"hidden\" id=\"meta-box-order-nonce\" name=\"meta-box-order-nonce\" value=\"{}\" />\t",
        site.nonce("closedpostboxes"),
        site.nonce("meta-box-order")
    ));
    let content = format!(
        "\n<div class=\"wrap\">\n\t<h1>Dashboard</h1>\n\n\t{}\n\n{}\t<div id=\"dashboard-widgets-wrap\">\n\t{columns}</div><!-- dashboard-widgets-wrap -->\n\n</div><!-- wrap -->{TEMPLATES}",
        remind_later(site, request_uri),
        welcome_panel(site)
    );

    if site.can("edit_theme_options") {
        let checked = if welcome_hidden(site) {
            ""
        } else {
            " checked='checked'"
        };
        labels.push_str(&format!("<label for=\"wp_welcome_panel-hide\"><input type=\"checkbox\" id=\"wp_welcome_panel-hide\"{checked} />Welcome</label>\n"));
    }
    let screen_options = SCREEN_OPTIONS
        .replace("%labels%", &labels)
        .replace("%nonce%", &site.nonce("screen-options-nonce"));
    let help_content = help_content(site);
    let help = [
        HelpTab {
            id: "overview",
            title: "Overview",
            content: HELP_OVERVIEW,
        },
        HelpTab {
            id: "help-navigation",
            title: "Navigation",
            content: HELP_NAVIGATION,
        },
        HelpTab {
            id: "help-layout",
            title: "Layout",
            content: HELP_LAYOUT,
        },
        HelpTab {
            id: "help-content",
            title: "Content",
            content: &help_content,
        },
    ];
    let version = VERSION.trim();
    let sidebar = format!(
        "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/dashboard-screen/\">Documentation on Dashboard</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p><p><a href=\"https://wordpress.org/documentation/wordpress-version/version-{}/\">Version {version}</a></p>",
        version.replace('.', "-")
    );
    // `WP_List_Table::_js_vars()` of each comments list table the Activity
    // widget made: the counts' and the reply form's.
    let tables = if data.comments.is_empty() {
        0
    } else if site.can("edit_posts") {
        2
    } else {
        1
    };
    let admin_footer = "<script>list_args = {\"class\":\"WP_Comments_List_Table\",\"screen\":{\"id\":\"dashboard\",\"base\":\"dashboard\"}};</script>\n".repeat(tables);
    let time_format = text_option(site, "time_format");
    let community = object(vec![
        ("nonce", text(&site.nonce("community_events"))),
        ("cache", text("")),
        ("time_format", text(&time_format)),
    ]);
    let mut script_data = vec![(
        "dashboard",
        format!("var communityEventsData = {};", json(&community)),
    )];
    if site.can("view_site_health_checks") {
        script_data.push(("site-health", site_health_data(site, "dashboard")));
    }
    let footer_scripts = if tables > 0 {
        editor_footer(site)
    } else {
        String::new()
    };
    let mut styles = Vec::new();
    let mut scripts = Vec::new();
    if site.can("view_site_health_checks") {
        styles.push("site-health");
        scripts.push("site-health");
    }
    scripts.push("dashboard");
    if site.can("upload_files") {
        scripts.push("media-upload");
    }
    styles.push("thickbox");
    scripts.push("thickbox");
    if browser.is_mobile() {
        scripts.push("jquery-touch-punch");
    }
    let page = Page {
        screen: Screen {
            pagenow: "index.php",
            typenow: "",
            taxonomy: "",
            parent_file: Some("index.php"),
            submenu_file: None,
        },
        id: "dashboard",
        title: "Dashboard",
        help: &help,
        sidebar: &sidebar,
        screen_options: &screen_options,
        styles: &styles,
        scripts: &scripts,
        late_scripts: if tables > 0 {
            &["quicktags", "wplink", "jquery-ui-autocomplete"]
        } else {
            &[]
        },
        script_data: &script_data,
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: &admin_footer,
        footer_scripts: &footer_scripts,
        content: &content,
        canonical,
        origin,
        view: None,
    };
    admin_page(site, &page, browser)
}

const HELP_OVERVIEW: &str = "<p>Welcome to your WordPress Dashboard!</p><p>The Dashboard is the first place you will come to every time you log into your site. It is where you will find all your WordPress tools. If you need help, just click the &#8220;Help&#8221; tab above the screen title.</p>";
const HELP_NAVIGATION: &str = "<p>The left-hand navigation menu provides links to all of the WordPress administration screens, with submenu items displayed on hover. You can minimize this menu to a narrow icon strip by clicking on the Collapse Menu arrow at the bottom.</p><p>Links in the Toolbar at the top of the screen connect your dashboard and the front end of your site, and provide access to your profile and helpful WordPress information.</p>";
const HELP_LAYOUT: &str = "<p>You can use the following controls to arrange your Dashboard screen to suit your workflow. This is true on most other administration screens as well.</p><p><strong>Screen Options</strong> &mdash; Use the Screen Options tab to choose which Dashboard boxes to show.</p><p><strong>Drag and Drop</strong> &mdash; To rearrange the boxes, drag and drop by clicking on the title bar of the selected box and releasing when you see a gray dotted-line rectangle appear in the location you want to place the box.</p><p><strong>Box Controls</strong> &mdash; Click the title bar of the box to expand or collapse it. Some boxes added by plugins may have configurable content, and will show a &#8220;Configure&#8221; link in the title bar if you hover over it.</p>";

/// The Content help tab: the boxes the user sees.
fn help_content(site: &dyn DashboardSite) -> String {
    let mut help = String::from("<p>The boxes on your Dashboard screen are:</p>");
    if site.can("edit_theme_options") {
        help.push_str("<p><strong>Welcome</strong> &mdash; Shows links for some of the most common tasks when setting up a new site.</p>");
    }
    if site.can("view_site_health_checks") {
        help.push_str("<p><strong>Site Health Status</strong> &mdash; Informs you of any potential issues that should be addressed to improve the performance or security of your website.</p>");
    }
    if site.can("edit_posts") {
        help.push_str("<p><strong>At a Glance</strong> &mdash; Displays a summary of the content on your site and identifies which theme and version of WordPress you are using.</p>");
    }
    help.push_str("<p><strong>Activity</strong> &mdash; Shows the upcoming scheduled posts, recently published posts, and the most recent comments on your posts and allows you to moderate them.</p>");
    if site.can("edit_posts") {
        help.push_str("<p><strong>Quick Draft</strong> &mdash; Allows you to create a new post and save it as a draft. Also displays links to the 3 most recent draft posts you've started.</p>");
    }
    help.push_str("<p><strong>WordPress Events and News</strong> &mdash; Upcoming events near you as well as the latest news from the official WordPress project and the <a href=\"https://planet.wordpress.org/\">WordPress Planet</a>.</p>");
    help
}

/// The `site-health` script's `SiteHealth` on `screen`, with no test to run.
pub(crate) fn site_health_data(site: &dyn DashboardSite, screen: &str) -> String {
    let health = object(vec![
        ("screen", text(screen)),
        (
            "nonce",
            object(vec![
                ("site_status", text(&site.nonce("health-check-site-status"))),
                (
                    "site_status_result",
                    text(&site.nonce("health-check-site-status-result")),
                ),
            ]),
        ),
        (
            "site_status",
            object(vec![
                ("direct", Value::Array(Vec::new())),
                ("async", Value::Array(Vec::new())),
                ("issues", issue_counts(site)),
            ]),
        ),
    ]);
    format!("var SiteHealth = {};", json(&health))
}

/// `SiteHealth.site_status.issues`: the last check's cached counts, or
/// none of each.
fn issue_counts(site: &dyn DashboardSite) -> Value {
    match site.transient("health-check-site-status-result") {
        Some(stored) => json_decode(&stored).unwrap_or(Value::Null),
        None => object(vec![
            ("good", Value::Int(0)),
            ("recommended", Value::Int(0)),
            ("critical", Value::Int(0)),
        ]),
    }
}

/// `wp_get_tooltip()` around one of a meta box's buttons.
fn tooltip(id: &str, label: &str, button: &str) -> String {
    format!(
        "<span class=\"wp-tooltip wp-is-tooltip\">\n\t\t\t\t{button}\n\t\t\t\t<span popover=\"hint\" id=\"{id}\" class=\"wp-tooltip__bubble\" role=\"tooltip\"><span id=\"{id}-text\" class=\"wp-tooltip__text\">{label}</span></span></span>"
    )
}

/// A meta box as `do_meta_boxes()` prints it, `hidden` by the user's
/// screen options.
pub(crate) fn postbox(id: &str, title: &str, closed: bool, hidden: bool, inside: &str) -> String {
    let closed = if closed { "closed" } else { "" };
    let hidden = if hidden { " hide-if-js" } else { "" };
    let label = esc_attr(&wp_strip_all_tags(title.as_bytes(), false));
    let button = |class: &str, label: &str, indicator: &str, expanded: &str| {
        format!(
            "<button aria-label=\"{label}\" type=\"button\" class=\"{class} wp-tooltip__toggle\"{expanded} aria-describedby=\"{id}-title\">\n\t\t\t\t\t\t\t<span class=\"screen-reader-text\">{label}</span>\n\t\t\t\t\t\t\t<span class=\"{indicator}\" aria-hidden=\"true\"></span>\n\t\t\t\t\t\t</button>"
        )
    };
    let up = tooltip(
        &format!("{id}-handle-order-higher-description"),
        "Move up",
        &button(
            "handle-order-higher",
            "Move up",
            "order-higher-indicator",
            "",
        ),
    );
    let down = tooltip(
        &format!("{id}-handle-order-lower-description"),
        "Move down",
        &button(
            "handle-order-lower",
            "Move down",
            "order-lower-indicator",
            "",
        ),
    );
    let toggle = tooltip(
        &format!("{id}-handlediv"),
        "Show or hide panel",
        &button(
            "handlediv",
            "Show or hide panel",
            "toggle-indicator",
            " aria-expanded=\"true\"",
        ),
    );
    format!(
        "<div id=\"{id}\" class=\"postbox {closed}{hidden}\"  role=\"region\" aria-label=\"{label}\">\n<div class=\"postbox-header\"><h2 class=\"hndle\" id=\"{id}-title\">{title}</h2>\n<div class=\"handle-actions hide-if-no-js\">{up}{down}{toggle}</div></div><div class=\"inside\">\n{inside}</div>\n</div>\n"
    )
}

#[cfg(test)]
pub(crate) mod tests;
