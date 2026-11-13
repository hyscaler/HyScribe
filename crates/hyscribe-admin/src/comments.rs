// SPDX-License-Identifier: AGPL-3.0-or-later

//! `edit-comments.php`: the comments list, `WP_Comments_List_Table`, and
//! the comment parts the dashboard shares with it.

use hyscribe_kses::{esc_attr, esc_html, esc_textarea, esc_url};
use hyscribe_model::{Comment, User};
use hyscribe_phpser::{Value, add_query_arg, is_empty_string, remove_query_arg};
use hyscribe_render::date::{self, Zone};
use hyscribe_render::formatting::CORE_SHORTCODE_TAGS;
use hyscribe_render::formatting::{antispambot, convert_chars, wptexturize};
use hyscribe_render::post::{admin_title, sanitize_html_class};
use hyscribe_render::{
    admin_comment_text, comment_author_of, comment_author_url, comment_avatar, number_format,
};

use crate::admin_bar::{BarSite, Browser, html_excerpt};
use crate::dashboard::{DashboardSite, RecentComment, approved_of};
use crate::list_table::{Column, Order, Sort, View, bulk_actions, column_headers, pagination};
use crate::list_table::{pagenum, screen_options, search_box, views};
use crate::menu::{Screen, esc_link, link};
use crate::page::{HelpTab, Page, admin_page};
use crate::posts::{
    Answer, comments_bubble, edit_link, hidden_columns, per_page, user_setting, view_mode_fields,
};

/// What the comments list asks of the site beyond the dashboard's.
pub trait CommentsSite: DashboardSite {
    /// The comments `query` finds, newest first, and how many there are.
    fn list_comments(&self, query: &CommentQuery<'_>) -> (Vec<RecentComment>, u64);
    /// Whether any comment, notes aside, is of `kind`: `comment` or `pings`.
    fn has_comments(&self, kind: &str) -> bool;
    /// `get_comment_link()` and `get_comment_author()` of comment `id`,
    /// if there is one.
    fn parent_comment(&self, id: u64) -> Option<(String, Vec<u8>)>;
    /// Whether comment `id`'s `_wp_trash_meta_status` is `1`: approved
    /// before it was trashed or marked as spam.
    fn approved_before(&self, id: u64) -> bool;
}

/// The comments the list asks for, notes aside.
pub struct CommentQuery<'a> {
    /// Their `comment_approved` values.
    pub approved: &'a [&'a str],
    /// `user_id = %d`, when the request gives a user.
    pub user: Option<i64>,
    /// How many to skip.
    pub offset: u64,
    /// How many to list.
    pub number: u64,
}

/// The arguments of `edit-comments.php` ported so far, as requested.
#[derive(Default)]
pub struct CommentArgs {
    /// `comment_status`.
    pub status: Option<String>,
    /// `user_id`.
    pub user_id: Option<String>,
    /// `paged`.
    pub paged: Option<String>,
}

/// The views of `prepare_items()`, and the `comment_approved` each lists.
const VIEWS: [(&str, &[&str]); 6] = [
    ("all", &["0", "1"]),
    ("mine", &["0", "1"]),
    ("moderated", &["0"]),
    ("approved", &["1"]),
    ("spam", &["spam"]),
    ("trash", &["trash"]),
];

const REPLY: &str = include_str!("comments/reply.html");
const EDITOR: &str = include_str!("comments/editor.html");

/// `_WP_Editors::editor_js()` and the link dialog of the reply form.
pub(crate) fn editor_footer(site: &dyn DashboardSite) -> String {
    EDITOR
        .replace("%site_url%", &site.site_url())
        .replace("%nonce%", &site.nonce("internal-linking"))
}

/// `wp_comment_reply( -1, $checkbox, $mode )` and
/// `wp_comment_trashnotice()`: in a table row of `colspan` columns, or in
/// the dashboard's `<div>`s without one.
pub(crate) fn comment_reply(
    site: &dyn DashboardSite,
    colspan: Option<usize>,
    checkbox: bool,
    mode: &str,
) -> String {
    let (open, close) = match colspan {
        Some(columns) => (
            format!(
                "<table style=\"display:none;\"><tbody id=\"com-reply\"><tr id=\"replyrow\" class=\"inline-edit-row\" style=\"display:none;\"><td colspan=\"{columns}\" class=\"colspanchange\">"
            ),
            "</td></tr></tbody></table>",
        ),
        None => (
            "<div id=\"com-reply\" style=\"display:none;\"><div id=\"replyrow\" style=\"display:none;\">".into(),
            "</div></div>",
        ),
    };
    let editor_css = format!(
        "{}/wp-includes/css/editor.min.css?ver={}",
        site.site_url(),
        crate::dashboard::VERSION.trim()
    );
    crate::list_table::fill(
        REPLY,
        &[
            ("open", open),
            ("close", close.into()),
            (
                "editor_css",
                String::from_utf8_lossy(&esc_url(editor_css.as_bytes())).into_owned(),
            ),
            ("checkbox", u8::from(checkbox).to_string()),
            ("mode", mode.into()),
            ("nonce", site.nonce("replyto-comment")),
        ],
    )
}

/// `get_comment_class( $extra, $comment )` of the comment printed `index`th
/// on the page, `user` its author's account, `post_author` its post's.
pub(crate) fn comment_classes(
    comment: &Comment,
    user: Option<&User>,
    post_author: Option<u64>,
    index: usize,
    extra: &[&str],
) -> String {
    let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
    let mut classes = vec![if comment.comment_type.is_empty() {
        "comment".to_string()
    } else {
        text(&comment.comment_type)
    }];
    if let Some(user) = user {
        classes.push("byuser".into());
        let fallback = comment.user_id.to_string();
        classes.push(format!(
            "comment-author-{}",
            text(&sanitize_html_class(&user.nicename, fallback.as_bytes()))
        ));
        if post_author == Some(comment.user_id) {
            classes.push("bypostauthor".into());
        }
    }
    let odd = index % 2 == 1;
    classes.push(if odd { "odd alt" } else { "even" }.into());
    classes.push(
        if odd {
            "thread-odd thread-alt"
        } else {
            "thread-even"
        }
        .into(),
    );
    classes.push("depth-1".into());
    classes.extend(extra.iter().map(|class| (*class).to_string()));
    esc_attr(classes.join(" ").as_bytes())
}

/// The list being printed.
struct Listing<'a> {
    site: &'a dyn CommentsSite,
    columns: &'a [Column],
    hidden: &'a [&'a str],
    /// The extended view, whose row actions always show.
    excerpt: bool,
    /// `$comment_status`, the view listed.
    view: &'a str,
}

impl Listing<'_> {
    /// `single_row()` of `row`, printed `index`th, unless its post is one
    /// the user may not see.
    fn row(&self, row: &RecentComment, index: &mut usize) -> String {
        let site = self.site;
        let visible = match &row.post {
            Some(post) => {
                site.can_post("edit_post", post)
                    || post.password.is_empty() && site.can_post("read_post", post)
            }
            None => site.can("edit_posts"),
        };
        if !visible {
            return String::new();
        }
        let comment = &row.comment;
        let classes = comment_classes(
            comment,
            row.user.as_ref(),
            row.post.as_ref().map(|post| post.author),
            *index,
            &[row.status],
        );
        *index += 1;
        let mut out = format!("<tr id='comment-{}' class='{classes}'>", comment.id);
        for column in self.columns {
            let key = column.key;
            let label = hyscribe_kses::wp_strip_all_tags(column.label.as_bytes(), false);
            let hidden = if self.hidden.contains(&key) {
                " hidden"
            } else {
                ""
            };
            let attributes = format!("class='{key} column-{key}");
            let colname = format!("data-colname=\"{}\"", esc_attr(&label));
            out.push_str(&match key {
                "cb" => format!("<td class=\"check-column\">{}</td>", self.column_cb(row)),
                "comment" => format!(
                    "<th {attributes} has-row-actions column-primary{hidden}' {colname} scope=\"row\">{}{}</th>",
                    self.column_comment(row),
                    self.actions(row)
                ),
                "author" => format!("<td {attributes}{hidden}' {colname}>{}</td>", self.column_author(row)),
                "response" => format!("<td {attributes}{hidden}' {colname}>{}</td>", self.column_response(row)),
                _ => format!("<td {attributes}{hidden}' {colname}>{}</td>", column_date(row, self.site.clock().0)),
            });
        }
        out.push_str("</tr>\n");
        out
    }

    /// `column_cb()`: a box for a comment the user may edit.
    fn column_cb(&self, row: &RecentComment) -> String {
        if !row.can_edit {
            return String::new();
        }
        let id = row.comment.id;
        format!(
            "\t\t<input id=\"cb-select-{id}\" type=\"checkbox\" name=\"delete_comments[]\" value=\"{id}\" />\n\t\t<label for=\"cb-select-{id}\">\n\t\t\t<span class=\"screen-reader-text\">\n\t\t\tSelect comment\t\t\t</span>\n\t\t</label>\n\t\t\t"
        )
    }

    /// `column_author()`: the author with their avatar, site, and for those
    /// who may edit, their email and IP.
    fn column_author(&self, row: &RecentComment) -> String {
        let site = self.site;
        let comment = &row.comment;
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        let name = comment_author_of(comment, row.user.as_ref());
        let texturized = wptexturize(&name, &CORE_SHORTCODE_TAGS);
        let mut author = esc_html(&convert_chars(&texturized));
        // `floated_admin_avatar()`, when the site shows avatars.
        if site
            .option("show_avatars")
            .is_some_and(|v| !is_empty_string(&v))
        {
            let email = row
                .user
                .as_ref()
                .map_or(&comment.author_email, |user| &user.email);
            let avatar = if matches!(&comment.comment_type[..], b"" | b"comment") {
                comment_avatar(&|name| site.option(name), email, 32)
            } else {
                Vec::new()
            };
            author = format!("{} {author}", text(&avatar));
        }
        let mut out = format!("<strong>{author}</strong><br />");
        let url = comment_author_url(comment);
        let mut shown = String::from_utf8_lossy(&strip_scheme(&url)).into_owned();
        if shown.len() > 50 {
            shown = html_excerpt(&shown, 49, "&hellip;");
        }
        if !is_empty_string(shown.as_bytes()) {
            out.push_str(&format!(
                "<a href=\"{}\" rel=\"noopener noreferrer\">{}</a><br />",
                text(&esc_url(&url)),
                esc_html(shown.as_bytes())
            ));
        }
        if row.can_edit {
            if !is_empty_string(&comment.author_email) {
                // `antispambot()` encodes `@`, so the address is never `@`.
                let email = antispambot(&comment.author_email);
                let mailto = [&b"mailto:"[..], &email].concat();
                out.push_str(&format!(
                    "<a href=\"{}\">{}</a><br />",
                    text(&esc_url(&mailto)),
                    esc_html(&email)
                ));
            }
            if !is_empty_string(&comment.author_ip) {
                let mut url = add_query_arg(
                    &[(b"s", Some(&comment.author_ip)), (b"mode", Some(b"detail"))],
                    BarSite::admin_url(site, "edit-comments.php").as_bytes(),
                );
                if self.view == "spam" {
                    url = add_query_arg(&[(b"comment_status", Some(b"spam"))], &url);
                }
                out.push_str(&format!(
                    "<a href=\"{}\">{}</a>",
                    text(&esc_url(&url)),
                    esc_html(&comment.author_ip)
                ));
            }
        }
        out
    }

    /// `column_comment()`: the author again, the text, and the Quick Edit
    /// data for those who may edit.
    fn column_comment(&self, row: &RecentComment) -> String {
        let site = self.site;
        let comment = &row.comment;
        let text = admin_comment_text(
            &|name| site.option(name),
            site.home_url().as_bytes(),
            &comment.content,
        );
        let mut out = format!(
            "<div class=\"comment-author\">{}</div>",
            self.column_author(row)
        );
        let parent = (comment.parent != 0)
            .then(|| site.parent_comment(comment.parent))
            .flatten();
        if let Some((link, author)) = parent {
            out.push_str(&format!(
                "In reply to <a href=\"{}\">{}</a>.",
                String::from_utf8_lossy(&esc_url(link.as_bytes())),
                String::from_utf8_lossy(&author)
            ));
        }
        out.push_str(&String::from_utf8_lossy(&text));
        if row.can_edit {
            out.push_str(&format!(
                "\t\t<div id=\"inline-{}\" class=\"hidden\">\n\t\t\t<textarea class=\"comment\" rows=\"1\" cols=\"1\">{}</textarea>\n\t\t\t<div class=\"author-email\">{}</div>\n\t\t\t<div class=\"author\">{}</div>\n\t\t\t<div class=\"author-url\">{}</div>\n\t\t\t<div class=\"comment_status\">{}</div>\n\t\t</div>\n\t\t\t",
                comment.id,
                String::from_utf8_lossy(&esc_textarea(&comment.content)),
                esc_html(&comment.author_email),
                esc_html(&comment.author),
                String::from_utf8_lossy(&esc_url(&comment.author_url)),
                approved_of(row.status)
            ));
        }
        out
    }

    /// `handle_row_actions()`: what the user may do to the comment in this
    /// view.
    fn actions(&self, row: &RecentComment) -> String {
        if !row.can_edit {
            return String::new();
        }
        let site = self.site;
        let id = row.comment.id;
        let status = row.status;
        // The nonce, hex digits, escapes to itself.
        let url = |action: &str, nonce: &str| {
            let url = esc_link(&format!("comment.php?action={action}&c={id}&_wpnonce="));
            format!("{url}{}", site.nonce(&format!("{nonce}-comment_{id}")))
        };
        let anchor = |href: String, lists: String, class: &str, label: &str, text: &str| {
            format!(
                "<a href=\"{href}\" data-wp-lists=\"{lists}\" class=\"{class} aria-button-if-js\" aria-label=\"{label}\">{text}</a>"
            )
        };
        let button = |action: &str, class: &str, label: &str, text: &str| {
            format!(
                "<button type=\"button\" data-comment-id=\"{id}\" data-post-id=\"{}\" data-action=\"{action}\" class=\"{class} comment-inline button-link\" aria-expanded=\"false\" aria-label=\"{label}\">{text}</button>",
                row.comment.post_id
            )
        };
        let all = self.view == "all";
        let approve = |new: &str, class: &str, label: &str, text: &str| {
            let action = if new == "approved" {
                "approvecomment"
            } else {
                "unapprovecomment"
            };
            let lists = if all {
                format!("dim:the-comment-list:comment-{id}:unapproved:e7e7d3:e7e7d3:new={new}")
            } else {
                format!(
                    "delete:the-comment-list:comment-{id}:e7e7d3:action=dim-comment&amp;new={new}"
                )
            };
            let class = if all {
                class.to_string()
            } else {
                format!("{class} vim-destructive")
            };
            anchor(url(action, "approve"), lists, &class, label, text)
        };
        let shown = !matches!(status, "spam" | "trash");
        let destroy = |action: &str, lists: &str, class: &str, label: &str, text: &str| {
            anchor(
                url(action, "delete"),
                format!("delete:the-comment-list:comment-{id}:{lists}"),
                class,
                label,
                text,
            )
        };
        let actions = [
            (
                "approve",
                (all || status == "unapproved")
                    .then(|| approve("approved", "vim-a", "Approve this comment", "Approve")),
            ),
            (
                "unapprove",
                (all || status == "approved")
                    .then(|| approve("unapproved", "vim-u", "Unapprove this comment", "Unapprove")),
            ),
            (
                "reply",
                shown.then(|| button("replyto", "vim-r", "Reply to this comment", "Reply")),
            ),
            (
                "quickedit",
                shown.then(|| {
                    button("edit", "vim-q", "Quick edit this comment inline", "Quick&nbsp;Edit")
                }),
            ),
            (
                "edit",
                shown.then(|| {
                    format!(
                        "<a href=\"{}?action=editcomment&amp;c={id}\" aria-label=\"Edit this comment\">Edit</a>",
                        link("comment.php")
                    )
                }),
            ),
            (
                "spam",
                (status != "spam")
                    .then(|| {
                        destroy("spamcomment", ":spam=1", "vim-s vim-destructive", "Mark this comment as spam", "Spam")
                    }),
            ),
            (
                "unspam",
                (status == "spam")
                    .then(|| {
                        destroy("unspamcomment", "66cc66:unspam=1", "vim-z vim-destructive", "Restore this comment from the spam", "Not Spam")
                    }),
            ),
            (
                "trash",
                shown
                    .then(|| {
                        destroy("trashcomment", ":trash=1", "delete vim-d vim-destructive", "Move this comment to the Trash", "Trash")
                    }),
            ),
            (
                "untrash",
                (status == "trash")
                    .then(|| {
                        destroy("untrashcomment", "66cc66:untrash=1", "vim-z vim-destructive", "Restore this comment from the Trash", "Restore")
                    }),
            ),
            (
                "delete",
                (!shown)
                    .then(|| {
                        destroy("deletecomment", ":delete=1", "delete vim-d vim-destructive", "Delete this comment permanently", "Delete Permanently")
                    }),
            ),
        ];
        let spans: String = actions
            .into_iter()
            .filter_map(|(action, markup)| Some((action, markup?)))
            .enumerate()
            .map(|(i, (action, markup))| {
                // Approve and Unapprove both open the list, one of them hidden.
                let first = i == 0 || i == 1 && matches!(action, "approve" | "unapprove");
                let separator = if first { "" } else { " | " };
                let class = match action {
                    "reply" | "quickedit" => format!("{action} hide-if-no-js"),
                    "untrash" | "unspam" if site.approved_before(id) => format!("{action} approve"),
                    "untrash" | "unspam" => format!("{action} unapprove"),
                    _ => action.to_string(),
                };
                format!("<span class='{class}'>{separator}{markup}</span>")
            })
            .collect();
        format!(
            "<div class=\"row-actions{}\">{spans}</div><button type=\"button\" class=\"toggle-row\"><span class=\"screen-reader-text\">Show more details</span></button>",
            if self.excerpt { " visible" } else { "" }
        )
    }

    /// `column_response()`: the post, linked to its editor for those who may
    /// edit it, its page and its comment counts.
    fn column_response(&self, row: &RecentComment) -> String {
        let site = self.site;
        let Some(post) = &row.post else {
            return String::new();
        };
        let title = esc_html(&admin_title(post));
        let title = if site.can_post("edit_post", post) {
            format!(
                "<a href='{}' class='comments-edit-item-link'>{title}</a>",
                edit_link(site, post)
            )
        } else {
            title
        };
        let view = if post.post_type == "page" {
            "View Page"
        } else {
            "View Post"
        };
        format!(
            "<div class=\"response-links\">{title}<a href='{}' class='comments-view-item-link'>{view}</a><span class=\"post-com-count-wrapper post-com-count-{}\">{}</span> </div>",
            row.permalink,
            post.id,
            comments_bubble(site, post, site.pending_comments(post))
        )
    }
}

/// The author URL without its scheme or `www.`, nor a trailing slash, cut
/// to 49 characters as `wp_html_excerpt()` cuts it.
fn strip_scheme(url: &[u8]) -> Vec<u8> {
    let lower = url.to_ascii_lowercase();
    let start = [&b"https://"[..], b"http://"]
        .iter()
        .find(|prefix| lower.starts_with(prefix))
        .map_or(0, |prefix| prefix.len());
    let www = start > 0
        && lower
            .get(start..)
            .is_some_and(|rest| rest.starts_with(b"www."));
    let start = if www { start + 4 } else { start };
    let mut shown = url.get(start..).unwrap_or_default().to_vec();
    while shown.last().is_some_and(|b| matches!(b, b'/' | b'\\')) {
        shown.pop();
    }
    shown
}

/// `column_date()`: when the comment was left, linked to it once approved.
fn column_date(row: &RecentComment, zone: Zone) -> String {
    let comment = &row.comment;
    let stored = |format: &str| date::format(format, &comment.date, zone);
    let submitted = format!("{} at {}", stored("Y/m/d"), stored("g:i a"));
    let shown = if row.status == "approved" && comment.post_id != 0 {
        format!(
            "<a href=\"{}\">{submitted}</a>",
            String::from_utf8_lossy(&esc_url(row.link.as_bytes()))
        )
    } else {
        submitted
    };
    format!("<div class=\"submitted-on\">{shown}</div>")
}

/// The list's columns.
fn columns() -> Vec<Column> {
    let column = |key, label: &str, sort| Column {
        key,
        label: label.into(),
        sort,
        primary: key == "comment",
    };
    let sort = |orderby, abbr, caption| {
        Some(Sort {
            orderby,
            desc_first: false,
            abbr,
            caption,
            initial: None,
        })
    };
    vec![
        column("cb", "<input type=\"checkbox\" />", None),
        column(
            "author",
            "Author",
            sort(
                "comment_author",
                "Author",
                "Table ordered by Comment Author.",
            ),
        ),
        column("comment", "Comment", None),
        column(
            "response",
            "In response to",
            sort(
                "comment_post_ID",
                "In Response To",
                "Table ordered by Post Replied To.",
            ),
        ),
        column("date", "Submitted on", sort("comment_date", "", "")),
    ]
}

/// `get_views()`: each status with its count, `current` the one shown.
pub(crate) fn comment_views(site: &dyn DashboardSite, current: &str) -> Vec<View> {
    let counts = site.comment_counts();
    let user = site.user_id();
    [
        ("all", "All", "all", counts.all()),
        ("mine", "Mine", "mine", counts.mine),
        ("moderated", "Pending", "pending", counts.moderated),
        ("approved", "Approved", "approved", counts.approved),
        ("spam", "Spam", "spam", counts.spam),
        ("trash", "Trash", "trash", counts.trash),
    ]
    .iter()
    .map(|(status, label, class, count)| {
        let mut url = format!("edit-comments.php?comment_status={status}");
        if *status == "mine" {
            url = format!("edit-comments.php?comment_status=mine&user_id={user}");
        }
        View {
            class: (*status).into(),
            url: BarSite::admin_url(site, &url),
            label: format!(
                "{label} <span class=\"count\">(<span class=\"{class}-count\">{}</span>)</span>",
                number_format(*count)
            ),
            current: *status == current,
        }
    })
    .collect()
}

const HELP_OVERVIEW: &str = "<p>You can manage comments made on your site similar to the way you manage posts and other content. This screen is customizable in the same ways as other management screens, and you can act on comments using the on-hover action links or the bulk actions.</p>";

const HELP_MODERATING: &str = "<p>A red bar on the left means the comment is waiting for you to moderate it.</p><p>In the <strong>Author</strong> column, in addition to the author&#8217;s name, email address, and site URL, the commenter&#8217;s IP address is shown. Clicking on this link will show you all the comments made from this IP address.</p><p>In the <strong>Comment</strong> column, hovering over any comment gives you options to approve, reply (and approve), quick edit, edit, spam mark, or trash that comment.</p><p>In the <strong>In response to</strong> column, there are three elements. The text is the name of the post that inspired the comment, and links to the post editor for that entry. The View Post link leads to that post on your live site. The small bubble with the number in it shows the number of approved comments that post has received. If there are pending comments, a red notification circle with the number of pending comments is displayed. Clicking the notification circle will filter the comments screen to show only pending comments on that post.</p><p>In the <strong>Submitted on</strong> column, the date and time the comment was left on your site appears. Clicking on the date/time link will take you to that comment on your live site.</p><p>Many people take advantage of keyboard shortcuts to moderate their comments more quickly. Use the link to the side to learn more.</p>";

const HELP_SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/comments-screen/\">Documentation on Comments</a></p><p><a href=\"https://wordpress.org/documentation/article/understand-comment-spam/\">Documentation on Comment Spam</a></p><p><a href=\"https://wordpress.org/documentation/article/keyboard-shortcuts-classic-editor/#keyboard-shortcuts-for-comments\">Documentation on Keyboard Shortcuts</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// `edit-comments.php`: the comments of the view and page `args` ask
/// for, in the view mode of the user's `settings` cookie or stored
/// settings; the menu refuses those who may not edit posts.
pub fn edit_comments(
    site: &dyn CommentsSite,
    args: &CommentArgs,
    settings: Option<&str>,
    canonical: &str,
    origin: &str,
    request_uri: &str,
    browser: &Browser,
) -> Answer {
    let (view, approved) = VIEWS
        .into_iter()
        .find(|(view, _)| args.status.as_deref() == Some(*view))
        .unwrap_or(VIEWS[0]);
    let mode = user_setting(site, settings, "posts_list_mode").unwrap_or_else(|| "list".into());
    let per_page = per_page(site, "edit_comments_per_page");
    let page = pagenum(args.paged.as_deref());
    // A few more than a page, for the rows that replace those acted on.
    let (found, total) = site.list_comments(&CommentQuery {
        approved,
        user: args
            .user_id
            .as_deref()
            .filter(|user| !user.is_empty())
            .map(|user| Value::String(user.into()).to_int()),
        offset: (page - 1).saturating_mul(per_page),
        number: per_page + per_page.min(8),
    });
    let pages = total.div_ceil(per_page);
    if pages > 0 && page > pages {
        let last = pages.to_string();
        let to = add_query_arg(&[(b"paged", Some(last.as_bytes()))], request_uri.as_bytes());
        return Answer::Redirect(String::from_utf8_lossy(&to).into_owned());
    }
    let split = found
        .len()
        .min(usize::try_from(per_page).unwrap_or(usize::MAX));
    let (items, extra) = found.split_at(split);
    let columns = columns();
    let hidden = hidden_columns(site, "edit-comments");
    let hidden_keys: Vec<&str> = hidden.iter().map(String::as_str).collect();
    // `get_column_count()`: the columns not hidden.
    let visible = columns
        .iter()
        .filter(|column| !hidden_keys.contains(&column.key))
        .count();
    let listing = Listing {
        site,
        columns: &columns,
        hidden: &hidden_keys,
        excerpt: mode == "excerpt",
        view,
    };
    let mut index = 0;
    let rows: String = items
        .iter()
        .map(|row| listing.row(row, &mut index))
        .collect();
    let extra_rows: String = extra
        .iter()
        .map(|row| listing.row(row, &mut index))
        .collect();
    let none = format!(
        "<tr class=\"no-items\"><td class=\"colspanchange\" colspan=\"{visible}\">{}</td></tr>",
        match view {
            "moderated" => "No comments awaiting moderation.",
            "trash" => "No comments found in Trash.",
            _ => "No comments found.",
        }
    );
    let empty = items.is_empty();
    let rows = if empty { none.clone() } else { rows };
    let extra_rows = if extra.is_empty() { none } else { extra_rows };
    let moderate = site.can("moderate_comments");
    // `get_bulk_actions()`, none for those who may not moderate.
    let actions: Vec<(&str, &str)> = [
        ("unapprove", "Unapprove", matches!(view, "all" | "approved")),
        ("approve", "Approve", matches!(view, "all" | "moderated")),
        ("spam", "Mark as spam", !matches!(view, "mine" | "spam")),
        ("untrash", "Restore", view == "trash"),
        ("unspam", "Not spam", view == "spam"),
        (
            "delete",
            "Delete permanently",
            matches!(view, "spam" | "trash"),
        ),
        ("trash", "Move to Trash", !matches!(view, "spam" | "trash")),
    ]
    .into_iter()
    .filter(|(_, _, offered)| moderate && *offered)
    .map(|(action, label, _)| (action, label))
    .collect();
    let types: String = [("comment", "Comments"), ("pings", "Pings")]
        .iter()
        .filter(|(kind, _)| site.has_comments(kind))
        .map(|(kind, label)| format!("\t<option value='{kind}'>{label}</option>\n"))
        .collect();
    let referer = String::from_utf8_lossy(&esc_url(&remove_query_arg(
        &[b"_wp_http_referer"],
        request_uri.as_bytes(),
    )))
    .into_owned();
    let field = |id: &str, action: &str| {
        format!(
            "<input type=\"hidden\" id=\"{id}\" name=\"{id}\" value=\"{}\" /><input type=\"hidden\" name=\"_wp_http_referer\" value=\"{referer}\" />",
            site.nonce(action)
        )
    };
    // `extra_tablenav()`: a spam or trashed list can be emptied.
    let destroy = if moderate && matches!(view, "spam" | "trash") {
        format!(
            "{}<input type=\"submit\" name=\"delete_all\" id=\"delete_all\" class=\"button apply\" value=\"{}\"  />",
            field("_destroy_nonce", "bulk-destroy"),
            if view == "spam" {
                "Empty Spam"
            } else {
                "Empty Trash"
            }
        )
    } else {
        String::new()
    };
    let tablenav = |which: &str| {
        if empty {
            return String::new();
        }
        let extra = if which == "top" {
            format!(
                "<label class=\"screen-reader-text\" for=\"filter-by-comment-type\">Filter by comment type</label><select id=\"filter-by-comment-type\" name=\"comment_type\">\t<option value=''>All comment types</option>{types}</select><input type=\"submit\" name=\"filter_action\" id=\"post-query-submit\" class=\"button button-compact\" value=\"Filter\"  />"
            )
        } else {
            String::new()
        };
        format!(
            "\t<div class=\"tablenav {which}\">\n\n\t\t\t\t<div class=\"alignleft actions bulkactions\">\n\t\t\t{}\t\t</div>\n\t\t<div class=\"alignleft actions\">{extra}{destroy}</div>{}\n\t\t<br class=\"clear\" />\n\t</div>\n\t\t",
            bulk_actions(which, &actions),
            pagination(
                which,
                "Comments list navigation",
                total,
                pages,
                page,
                canonical
            )
        )
    };
    let order = Order {
        orderby: None,
        desc: false,
    };
    let unpaged =
        String::from_utf8_lossy(&remove_query_arg(&[b"paged"], canonical.as_bytes())).into_owned();
    let list = format!(
        "{}{}{}<h2 class='screen-reader-text'>Comments list</h2><table class=\"wp-list-table widefat fixed striped {} comments\">\n\t\t<caption class=\"screen-reader-text\">Ordered by Comment Date, descending.</caption>\t<thead>\n\t<tr>\n\t\t{}\t</tr>\n\t</thead>\n\n\t<tbody id=\"the-comment-list\" data-wp-lists=\"list:comment\">\n\t\t{rows}\t</tbody>\n\n\t<tbody id=\"the-extra-comment-list\" data-wp-lists=\"list:comment\" style=\"display: none;\">\n\t\t{extra_rows}\t</tbody>\n\n\t<tfoot>\n\t<tr>\n\t\t{}\t</tr>\n\t</tfoot>\n\n</table>\n\t\t{}",
        field(
            "_ajax_fetch_list_nonce",
            "fetch-list-WP_Comments_List_Table"
        ),
        if empty {
            String::new()
        } else {
            field("_wpnonce", "bulk-comments")
        },
        tablenav("top"),
        esc_attr(format!("table-view-{mode}").as_bytes()),
        column_headers(&columns, &hidden_keys, &order, &unpaged, 1, true),
        column_headers(&columns, &hidden_keys, &order, &unpaged, 2, false),
        tablenav("bottom")
    );
    // `current_time( 'mysql', true )`.
    let now = site.clock().1;
    let search = if empty {
        String::new()
    } else {
        search_box("Search Comments", "comment", "", &[])
    };
    // `absint()` of the page asked for, which may be 0.
    let paged = args.paged.as_deref().map_or_else(String::new, |paged| {
        format!(
            "\t<input type=\"hidden\" name=\"paged\" value=\"{}\" />\n",
            Value::String(paged.into()).to_int().unsigned_abs()
        )
    });
    let content = format!(
        "\n<div class=\"wrap\">\n<h1 class=\"wp-heading-inline\">\nComments</h1>\n\n\n<hr class=\"wp-header-end\">\n\n\n{}\n<form id=\"comments-form\" method=\"get\">\n\n{}\n<input type=\"hidden\" name=\"comment_status\" value=\"{view}\" />\n<input type=\"hidden\" name=\"pagegen_timestamp\" value=\"{}\" />\n\n<input type=\"hidden\" name=\"_total\" value=\"{total}\" />\n<input type=\"hidden\" name=\"_per_page\" value=\"{per_page}\" />\n<input type=\"hidden\" name=\"_page\" value=\"{page}\" />\n\n{paged}\n{list}</form>\n</div>\n\n<div id=\"ajax-response\"></div>\n\n{}\t",
        views("Filter comments list", &comment_views(site, view)),
        search,
        date::format_unix("Y-m-d H:i:s", now, Zone::Utc),
        comment_reply(site, Some(visible), true, "detail")
    );
    let screen_options = screen_options(
        &columns,
        &hidden,
        "edit_comments_per_page",
        per_page,
        &view_mode_fields(&mode),
        site.nonce("screen-options-nonce"),
    );
    let help = [
        HelpTab {
            id: "overview",
            title: "Overview",
            content: HELP_OVERVIEW,
        },
        HelpTab {
            id: "moderating-comments",
            title: "Moderating Comments",
            content: HELP_MODERATING,
        },
    ];
    let moderated = site.comment_counts().moderated;
    let title = if moderated > 0 {
        format!("Comments ({})", number_format(moderated))
    } else {
        "Comments".into()
    };
    let page = Page {
        screen: Screen {
            pagenow: "edit-comments.php",
            typenow: "",
            taxonomy: "",
            parent_file: None,
            submenu_file: None,
        },
        id: "edit-comments",
        title: &title,
        help: &help,
        sidebar: HELP_SIDEBAR,
        screen_options: &screen_options,
        styles: &[],
        scripts: &["admin-comments"],
        late_scripts: &["quicktags", "wplink", "jquery-ui-autocomplete"],
        script_data: &[],
        // `_js_vars()` of the list table, which loads its pages by Ajax.
        admin_footer: "<script>list_args = {\"class\":\"WP_Comments_List_Table\",\"screen\":{\"id\":\"edit-comments\",\"base\":\"edit-comments\"}};</script>\n",
        footer_scripts: &editor_footer(site),
        content: &content,
        canonical,
        origin,
    };
    admin_page(site, &page, browser).map_or(
        Answer::Denied("Sorry, you are not allowed to access this page."),
        Answer::Page,
    )
}

#[cfg(test)]
pub(crate) mod content;
#[cfg(test)]
mod tests;
