// SPDX-License-Identifier: AGPL-3.0-or-later

//! `edit.php`: the posts list, `WP_Posts_List_Table`.

use hyscribe_kses::{esc_attr, esc_html, esc_textarea, esc_url};
use hyscribe_model::{OrderBy, Post, PostQuery, StatusFilter, Term, User};
use hyscribe_phpser::{
    Maybe, Value, add_query_arg, is_empty_string, maybe_unserialize, remove_query_arg, urldecode,
};
use hyscribe_render::date::{self, Zone};
use hyscribe_render::formatting::{convert_chars, ent2ncr, sanitize_text_field, wptexturize};
use hyscribe_render::post::admin_title;
use hyscribe_render::post::post_format_string;
use hyscribe_render::{Walk, number_format, order_by, sticky_posts, walk, wp_trim_words};

use crate::admin_bar::BarSite;
use crate::admin_bar::Browser;
use crate::list_table::{
    Column, Order, Sort, View, bulk_actions, column_headers, months_dropdown, pagination,
    row_actions, search_box, table_description, views,
};
use crate::menu::Screen;
use crate::menu::esc_link;
use crate::page::PageSite;
use crate::page::{HelpTab, Page, admin_page};

/// What the posts list asks of the site about its posts.
pub trait PostsSite: PageSite {
    /// `current_user_can( $cap, $post->ID )`.
    fn can_post(&self, cap: &str, post: &Post) -> bool;
    /// `get_permalink( $post )`.
    fn permalink(&self, post: &Post) -> String;
    /// The site's time zone and the time now.
    fn clock(&self) -> (Zone, i64);
    /// `get_option( $name )`, as stored.
    fn option(&self, name: &str) -> Option<Vec<u8>>;
    /// `get_post_class( $class, $post )` in the admin, joined by spaces.
    fn post_class(&self, class: &str, post: &Post) -> String;
    /// `get_the_terms( $post, $taxonomy )`.
    fn post_terms(&self, post: &Post, taxonomy: &str) -> Vec<Term>;
    /// `get_post_meta( $post->ID, $key, true )`, as stored.
    fn meta(&self, post: &Post, key: &str) -> Option<Vec<u8>>;
    /// The display name of user `id`, if there is one.
    fn display_name(&self, id: u64) -> Option<Vec<u8>>;
    /// `get_pending_comments_num( $post->ID )`.
    fn pending_comments(&self, post: &Post) -> u64;
    /// `get_the_excerpt( $post )`.
    fn excerpt(&self, post: &Post) -> Vec<u8>;
    /// Every term of `taxonomy`, by name.
    fn terms(&self, taxonomy: &str) -> Vec<Term>;
    /// The IDs of the ten terms of `taxonomy` with the most posts.
    fn popular_terms(&self, taxonomy: &str) -> Vec<u64>;
    /// The users who may `edit_posts`, by display name.
    fn authors(&self) -> Vec<User>;
    /// The post formats the theme supports.
    fn post_formats(&self) -> &'static [&'static str];
    /// The posts `query` selects and how many it matches on every page.
    fn list_posts(&self, query: &PostQuery) -> (Vec<Post>, u64);
    /// `wp_count_posts( $post_type, 'readable' )`: the posts of each status,
    /// private ones only by `private_author` when given.
    fn status_counts(&self, post_type: &str, private_author: Option<u64>) -> Vec<(String, u64)>;
    /// The user's posts of `post_type` the All list shows.
    fn own_posts(&self, post_type: &str) -> u64;
    /// The posts of `post_type` among `ids` neither trashed nor auto-drafts.
    fn sticky_count(&self, post_type: &str, ids: &[u64]) -> u64;
    /// The years and months with posts of `post_type`, newest first: of the
    /// trashed ones when `trash`, else of the others.
    fn months(&self, post_type: &str, trash: bool) -> Vec<(u32, u32)>;
    /// `get_user_option( $name )`, as stored.
    fn user_option(&self, name: &str) -> Option<Vec<u8>>;
}

/// `_draft_or_post_title()`: the title, escaped, or `(no title)`.
pub(crate) fn draft_or_post_title(post: &Post) -> String {
    let title = admin_title(post);
    if is_empty_string(&title) {
        "(no title)".to_string()
    } else {
        esc_html(&title)
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// `is_sticky( $post->ID )`.
fn is_sticky(site: &dyn PostsSite, post: &Post) -> bool {
    sticky_posts(&site.option("sticky_posts").unwrap_or_default()).contains(&post.id)
}

/// `get_the_excerpt()` trimmed to 15 words, for a post without a title the
/// user may read, in list mode.
fn no_title_excerpt(site: &dyn PostsSite, post: &Post) -> String {
    if !admin_title(post).is_empty()
        || !post.password.is_empty()
        || !site.can_post("read_post", post)
    {
        return String::new();
    }
    esc_html(&wp_trim_words(&site.excerpt(post), 15, b"&hellip;"))
}

/// The post's title for its labels: with part of its excerpt if it has
/// none, the excerpt wrapped as `column_title()` shows it.
fn labelled_title(site: &dyn PostsSite, post: &Post, wrap: bool) -> String {
    let mut title = draft_or_post_title(post);
    let excerpt = no_title_excerpt(site, post);
    if !excerpt.is_empty() {
        title.push(' ');
        if wrap {
            title.push_str(&format!(
                "<span class=\"trimmed-post-excerpt\">{excerpt}</span>"
            ));
        } else {
            title.push_str(&excerpt);
        }
    }
    title
}

/// `column_cb()`.
fn column_cb(site: &dyn PostsSite, post: &Post) -> String {
    if !site.can_post("edit_post", post) {
        return String::new();
    }
    let id = post.id;
    let title = labelled_title(site, post, false);
    format!(
        "\t\t\t<input id=\"cb-select-{id}\" type=\"checkbox\" name=\"post[]\" value=\"{id}\" />\n\t\t\t<label for=\"cb-select-{id}\">\n\t\t\t\t<span class=\"screen-reader-text\">\n\t\t\t\tSelect {title}\t\t\t\t</span>\n\t\t\t</label>\n\t\t\t<div class=\"locked-indicator\">\n\t\t\t\t<span class=\"locked-indicator-icon\" aria-hidden=\"true\"></span>\n\t\t\t\t<span class=\"screen-reader-text\">\n\t\t\t\t&#8220;{title}&#8221; is locked\t\t\t\t</span>\n\t\t\t</div>\n\t\t\t"
    )
}

/// `get_post_states()` in the posts list filtered by `filter`, whose own
/// state goes unsaid; the pages' states come with Pages.
fn post_states(site: &dyn PostsSite, post: &Post, filter: &str) -> Vec<&'static str> {
    let mut states = Vec::new();
    if !post.password.is_empty() {
        states.push("Password protected");
    }
    let customized = || {
        site.meta(post, "_customize_changeset_uuid")
            .is_some_and(|v| !v.is_empty())
    };
    match post.status.as_str() {
        "private" if filter != "private" => states.push("Private"),
        "draft" | "trash" if customized() => states.push("Customization Draft"),
        "draft" if filter != "draft" => states.push("Draft"),
        "pending" if filter != "pending" => states.push("Pending"),
        _ => {}
    }
    if is_sticky(site, post) {
        states.push("Sticky");
    }
    if post.status == "future" {
        states.push("Scheduled");
    }
    states
}

/// `_post_states( $post )`.
fn post_states_html(site: &dyn PostsSite, post: &Post, filter: &str) -> String {
    let states = post_states(site, post, filter);
    if states.is_empty() {
        return String::new();
    }
    let count = states.len();
    let spans: String = states
        .iter()
        .enumerate()
        .map(|(i, state)| {
            let suffix = if i + 1 < count { ", " } else { "" };
            format!("<span class='post-state'>{state}{suffix}</span>")
        })
        .collect();
    format!(" &mdash; {spans}")
}

/// `get_edit_post_link( $post->ID )`.
fn edit_link(site: &dyn PostsSite, post: &Post) -> String {
    BarSite::admin_url(site, &format!("post.php?post={}&amp;action=edit", post.id))
}

/// `get_inline_data( $post )`: the Quick Edit form's values, for a post
/// the user may edit.
fn inline_data(site: &dyn PostsSite, post: &Post) -> String {
    let id = post.id;
    let stored = |format: &str| date::format(format, &post.date, Zone::Utc);
    let template = site.meta(post, "_wp_page_template").unwrap_or_default();
    let mut out = format!(
        "\n<div class=\"hidden\" id=\"inline_{id}\">\n\t<div class=\"post_title\">{}</div>\n\t<div class=\"post_name\">{}</div>\n\t<div class=\"post_author\">{}</div>\n\t<div class=\"comment_status\">{}</div>\n\t<div class=\"ping_status\">{}</div>\n\t<div class=\"_status\">{}</div>\n\t<div class=\"jj\">{}</div>\n\t<div class=\"mm\">{}</div>\n\t<div class=\"aa\">{}</div>\n\t<div class=\"hh\">{}</div>\n\t<div class=\"mn\">{}</div>\n\t<div class=\"ss\">{}</div>\n\t<div class=\"post_password\">{}</div><div class=\"page_template\">{}</div>",
        text(&esc_textarea(hyscribe_phpser::php_trim(&post.title))),
        text(&esc_textarea(&urldecode(&post.name))),
        post.author,
        esc_html(post.comment_status.as_bytes()),
        esc_html(post.ping_status.as_bytes()),
        esc_html(post.status.as_bytes()),
        stored("d"),
        stored("m"),
        stored("Y"),
        stored("H"),
        stored("i"),
        stored("s"),
        esc_html(&post.password),
        if template.is_empty() {
            "default".to_string()
        } else {
            esc_html(&template)
        },
    );
    let categories = site.post_terms(post, "category");
    let ids: Vec<String> = categories.iter().map(|term| term.id.to_string()).collect();
    out.push_str(&format!(
        "<div class=\"post_category\" id=\"category_{id}\">{}</div>",
        ids.join(",")
    ));
    // `get_terms_to_edit()`: the names, `esc_attr()`ed and comma separated.
    let tags = site.post_terms(post, "post_tag");
    let names: Vec<&[u8]> = tags.iter().map(|term| term.name.as_slice()).collect();
    let tags = esc_attr(&names.join(&b","[..]));
    out.push_str(&format!(
        "<div class=\"tags_input\" id=\"post_tag_{id}\">{}</div>",
        esc_html(tags.replace(',', ", ").as_bytes())
    ));
    let format = site
        .post_terms(post, "post_format")
        .first()
        .map(|term| text(&term.slug).replacen("post-format-", "", 1))
        .unwrap_or_default();
    out.push_str(&format!(
        "<div class=\"sticky\">{}</div><div class=\"post_format\">{}</div></div>",
        if is_sticky(site, post) { "sticky" } else { "" },
        esc_html(format.as_bytes())
    ));
    out
}

/// `_column_title()` and `column_title()`, with the row actions.
fn column_title(site: &dyn PostsSite, post: &Post, classes: &str, filter: &str) -> String {
    let aria_label = if is_empty_string(&post.title) {
        "no title".to_string()
    } else {
        esc_attr(&post.title)
    };
    let mut out = format!(
        "<th scope=\"row\" class=\"{classes} page-title\" data-colname=\"Title\" aria-label=\"{aria_label}\">"
    );
    let editable = site.can_post("edit_post", post) && post.status != "trash";
    if editable {
        out.push_str("<div class=\"locked-info\"><span class=\"locked-avatar\"></span> <span class=\"locked-text\"></span></div>\n");
    }
    let title = labelled_title(site, post, true);
    out.push_str("<strong>");
    if editable {
        out.push_str(&format!(
            "<a class=\"row-title\" href=\"{}\">{title}</a>",
            edit_link(site, post)
        ));
    } else {
        out.push_str(&format!("<span>{title}</span>"));
    }
    out.push_str(&post_states_html(site, post, filter));
    out.push_str("</strong>\n");
    if site.can_post("edit_post", post) {
        out.push_str(&inline_data(site, post));
    }
    out.push_str(&title_actions(site, post));
    out.push_str("</th>");
    out
}

/// `handle_row_actions()` for the title column.
fn title_actions(site: &dyn PostsSite, post: &Post) -> String {
    let title = draft_or_post_title(post);
    let can_edit = site.can_post("edit_post", post);
    let mut actions: Vec<(&str, String)> = Vec::new();
    if can_edit && post.status != "trash" {
        actions.push((
            "edit",
            format!(
                "<a href=\"{}\" aria-label=\"{}\">Edit</a>",
                edit_link(site, post),
                esc_attr(format!("Edit &#8220;{title}&#8221;").as_bytes())
            ),
        ));
        actions.push((
            "inline hide-if-no-js",
            format!(
                "<button type=\"button\" class=\"button-link editinline\" aria-label=\"{}\" aria-expanded=\"false\">Quick&nbsp;Edit</button>",
                esc_attr(format!("Quick edit &#8220;{title}&#8221; inline").as_bytes())
            ),
        ));
    }
    if site.can_post("delete_post", post) {
        let id = post.id;
        let link = |action: &str| {
            let nonce = site.nonce(&format!("{action}-post_{id}"));
            let link = format!("post.php?post={id}&amp;action={action}&amp;_wpnonce={nonce}");
            BarSite::admin_url(site, &link)
        };
        if post.status == "trash" {
            actions.push((
                "untrash",
                format!(
                    "<a href=\"{}\" aria-label=\"{}\">Restore</a>",
                    link("untrash"),
                    esc_attr(format!("Restore &#8220;{title}&#8221; from the Trash").as_bytes())
                ),
            ));
            actions.push((
                "delete",
                format!(
                    "<a href=\"{}\" class=\"submitdelete\" aria-label=\"{}\">Delete Permanently</a>",
                    link("delete"),
                    esc_attr(format!("Delete &#8220;{title}&#8221; permanently").as_bytes())
                ),
            ));
        } else {
            actions.push((
                "trash",
                format!(
                    "<a href=\"{}\" class=\"submitdelete\" aria-label=\"{}\">Trash</a>",
                    link("trash"),
                    esc_attr(format!("Move &#8220;{title}&#8221; to the Trash").as_bytes())
                ),
            ));
        }
    }
    if ["pending", "draft", "future"].contains(&post.status.as_str()) {
        if can_edit {
            let preview = add_query_arg(
                &[(b"preview", Some(b"true"))],
                site.permalink(post).as_bytes(),
            );
            actions.push((
                "view",
                format!(
                    "<a href=\"{}\" rel=\"bookmark\" aria-label=\"{}\">Preview</a>",
                    text(&esc_url(&preview)),
                    esc_attr(format!("Preview &#8220;{title}&#8221;").as_bytes())
                ),
            ));
        }
    } else if post.status != "trash" {
        actions.push((
            "view",
            format!(
                "<a href=\"{}\" rel=\"bookmark\" aria-label=\"{}\">View</a>",
                site.permalink(post),
                esc_attr(format!("View &#8220;{title}&#8221;").as_bytes())
            ),
        ));
    }
    row_actions(&actions)
}

/// `get_edit_link( $args, $text )`: a link to the list filtered by `args`.
fn list_link(args: &[(&[u8], Option<&[u8]>)], label: &str) -> String {
    let url = text(&add_query_arg(args, b"edit.php"));
    format!("<a href=\"{}\">{label}</a>", esc_link(&url))
}

/// The dash of an empty cell, with its screen reader text.
fn none(label: &str) -> String {
    format!(
        "<span aria-hidden=\"true\">&#8212;</span><span class=\"screen-reader-text\">{label}</span>"
    )
}

/// `column_author()`.
fn column_author(site: &dyn PostsSite, post: &Post) -> String {
    match site
        .display_name(post.author)
        .filter(|name| !name.is_empty())
    {
        Some(name) => {
            let author = post.author.to_string();
            list_link(
                &[
                    (b"post_type", Some(post.post_type.as_bytes())),
                    (b"author", Some(author.as_bytes())),
                ],
                &esc_html(&ent2ncr(&name)),
            )
        }
        None => none("(no author)"),
    }
}

/// A term's name through the admin's `term_name` display filters, short
/// of `wp_kses_data()`; their `esc_html()` covers `column_default()`'s.
fn term_name(name: &[u8]) -> String {
    esc_html(&convert_chars(&wptexturize(
        &sanitize_text_field(name),
        &[],
    )))
}

/// `column_default()` of a taxonomy column: links to the list of each term.
fn column_terms(site: &dyn PostsSite, post: &Post, taxonomy: &str) -> String {
    let (query_var, no_terms) = if taxonomy == "category" {
        (&b"category_name"[..], "No categories")
    } else {
        (&b"tag"[..], "No tags")
    };
    let terms = site.post_terms(post, taxonomy);
    if terms.is_empty() {
        return none(no_terms);
    }
    let links: Vec<String> = terms
        .iter()
        .map(|term| list_link(&[(query_var, Some(&term.slug))], &term_name(&term.name)))
        .collect();
    links.join(", ")
}

/// `comments_bubble()`: the approved and pending comment counts.
fn comments_bubble(site: &dyn PostsSite, post: &Post) -> String {
    if !site.can_post("edit_post", post)
        && (!post.password.is_empty() || !site.can_post("read_post", post))
    {
        return String::new();
    }
    let approved = u64::try_from(post.comment_count).unwrap_or_default();
    let pending = site.pending_comments(post);
    let plural = |n: u64, one: &str, many: &str| {
        format!("{} {}", number_format(n), if n == 1 { one } else { many })
    };
    let approved_phrase = if pending > 0 {
        plural(approved, "approved comment", "approved comments")
    } else {
        plural(approved, "comment", "comments")
    };
    let id = post.id.to_string();
    let link = |status: &str| {
        let url = BarSite::admin_url(site, "edit-comments.php");
        let args: [(&[u8], Option<&[u8]>); 2] = [
            (b"p", Some(id.as_bytes())),
            (b"comment_status", Some(status.as_bytes())),
        ];
        text(&esc_url(&add_query_arg(&args, url.as_bytes())))
    };
    let mut out = if approved == 0 && pending == 0 {
        none("No comments")
    } else if approved > 0 {
        // A trashed post's bubble links nowhere.
        let (open, close) = if post.status == "trash" {
            ("<span".to_string(), "</span>")
        } else {
            (format!("<a href=\"{}\"", link("approved")), "</a>")
        };
        format!(
            "{open} class=\"post-com-count post-com-count-approved\"><span class=\"comment-count-approved\" aria-hidden=\"true\">{}</span><span class=\"screen-reader-text\">{approved_phrase}</span>{close}",
            number_format(approved)
        )
    } else {
        "<span class=\"post-com-count post-com-count-no-comments\"><span class=\"comment-count comment-count-no-comments\" aria-hidden=\"true\">0</span><span class=\"screen-reader-text\">No approved comments</span></span>".to_string()
    };
    if pending > 0 {
        out.push_str(&format!(
            "<a href=\"{}\" class=\"post-com-count post-com-count-pending\"><span class=\"comment-count-pending\" aria-hidden=\"true\">{}</span><span class=\"screen-reader-text\">{}</span></a>",
            link("moderated"),
            number_format(pending),
            plural(pending, "pending comment", "pending comments")
        ));
    } else {
        out.push_str(&format!(
            "<span class=\"post-com-count post-com-count-pending post-com-count-no-pending\"><span class=\"comment-count comment-count-no-pending\" aria-hidden=\"true\">0</span><span class=\"screen-reader-text\">{}</span></span>",
            if approved > 0 {
                "No pending comments"
            } else {
                "No comments"
            }
        ));
    }
    out
}

/// `column_date()`.
fn column_date(site: &dyn PostsSite, post: &Post) -> String {
    let (zone, now) = site.clock();
    let (time, missed) = if post.date == "0000-00-00 00:00:00" {
        ("Unpublished".to_string(), false)
    } else {
        (
            format!(
                "{} at {}",
                date::format("Y/m/d", &post.date, zone),
                date::format("g:i a", &post.date, zone)
            ),
            now > date::stored_unix(&post.date, zone),
        )
    };
    let status = match post.status.as_str() {
        "publish" => "Published",
        "future" if missed => "<strong class=\"error-message\">Missed schedule</strong>",
        "future" => "Scheduled",
        _ => "Last Modified",
    };
    format!("{status}<br />{time}")
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// `touch_time( 1, 1, 0, 1 )` for `post`, the global post: the fields of
/// its date, or of the time now for a draft never dated.
fn touch_time(post: &Post, zone: Zone, now: i64) -> String {
    let undated = ["draft", "pending"].contains(&post.status.as_str())
        && (post.date_gmt.is_empty() || post.date_gmt == "0000-00-00 00:00:00");
    let field = |format: &str| {
        if undated {
            date::format_unix(format, now, zone)
        } else {
            date::format(format, &post.date, Zone::Utc)
        }
    };
    let mm = field("m");
    let options: String = MONTHS
        .iter()
        .enumerate()
        .map(|(i, text)| {
            let number = format!("{:02}", i + 1);
            let selected = if number == mm {
                " selected='selected'"
            } else {
                ""
            };
            format!("\t\t\t<option value=\"{number}\" data-text=\"{text}\" {selected}>{number}-{text}</option>\n")
        })
        .collect();
    let input = |label: &str, name: &str, size: u8| {
        format!(
            "<label><span class=\"screen-reader-text\">{label}</span><input type=\"text\" name=\"{name}\" value=\"{}\" size=\"{size}\" maxlength=\"{size}\" autocomplete=\"off\" class=\"form-required\" inputmode=\"numeric\" /></label>",
            field(match name {
                "jj" => "d",
                "aa" => "Y",
                "hh" => "H",
                _ => "i",
            })
        )
    };
    format!(
        "<div class=\"timestamp-wrap\"><label><span class=\"screen-reader-text\">Month</span><select class=\"form-required\" name=\"mm\">\n{options}</select></label> {}, {} at {}:{}</div><input type=\"hidden\" id=\"ss\" name=\"ss\" value=\"{}\" />",
        input("Day", "jj", 2),
        input("Year", "aa", 4),
        input("Hour", "hh", 2),
        input("Minute", "mn", 2),
        field("s")
    )
}

/// `wp_dropdown_users()` of `users` in the inline editors, with the bulk
/// editor's "no change".
fn authors_dropdown(users: &[User], bulk: bool) -> String {
    let mut out = String::from(
        "<label class=\"inline-edit-author\"><span class=\"title\">Author</span><select name='post_author' class='authors'>\n",
    );
    if bulk {
        out.push_str("\t<option value='-1'>&mdash; No Change &mdash;</option>\n");
    }
    for user in users {
        let label = [&user.display_name[..], b" (", &user.login, b")"].concat();
        out.push_str(&format!(
            "\t<option value='{}'>{}</option>\n",
            user.id,
            esc_html(&label)
        ));
    }
    out.push_str("</select></label>");
    out
}

/// `wp_terms_checklist( 0 )` of the categories, the popular ones marked;
/// the page's only checklist, so its IDs are the first of their prefix.
fn categories_checklist(site: &dyn PostsSite) -> String {
    let terms = site.terms("category");
    let popular = site.popular_terms("category");
    let mut out = String::new();
    walk(&terms, true, &mut |event| match event {
        Walk::Start(term) => {
            let class = if popular.contains(&term.id) {
                " class=\"popular-category\""
            } else {
                ""
            };
            let id = term.id;
            out.push_str(&format!(
                "\n<li id='in-category-{id}-1'{class}><label class=\"selectit\"><input value=\"{id}\" type=\"checkbox\" name=\"post_category[]\" id=\"in-category-{id}-2\" /> {}</label>",
                esc_html(&term.name)
            ));
        }
        Walk::Level(depth, open) => {
            let tabs = "\t".repeat(depth);
            out.push_str(&if open {
                format!("{tabs}<ul class='children'>\n")
            } else {
                format!("{tabs}</ul>\n")
            });
        }
        Walk::End => out.push_str("</li>\n"),
    });
    out
}

/// The status options of an inline editor, as its loop indents them.
fn status_options(can_publish: bool, bulk: bool) -> String {
    let mut statuses = Vec::new();
    if bulk {
        statuses.push(("-1", "&mdash; No Change &mdash;"));
    }
    if can_publish {
        statuses.push(("publish", "Published"));
        statuses.push(("future", "Scheduled"));
        if bulk {
            statuses.push(("private", "Private"));
        }
    }
    statuses.push(("pending", "Pending Review"));
    statuses.push(("draft", "Draft"));
    let tabs = "\t".repeat(16);
    let options: String = statuses
        .iter()
        .map(|(value, text)| {
            format!(
                "\t\t<option value=\"{value}\">{}</option>\n{tabs}",
                esc_attr(text.as_bytes())
            )
        })
        .collect();
    format!("{}{options}</select>", "\t".repeat(15))
}

/// The bulk editor's post format select.
fn formats_select(formats: &[&str]) -> String {
    let options: String = formats
        .iter()
        .map(|format| {
            format!(
                "{}<option value=\"{}\">{}</option>\n{}",
                "\t".repeat(9),
                esc_attr(format.as_bytes()),
                esc_html(post_format_string(format.as_bytes()).as_bytes()),
                "\t".repeat(8)
            )
        })
        .collect();
    format!(
        "\t\t\t\t\t\n\t\t\t\t\t<label class=\"alignleft\">\n\t\t\t\t\t\t<span class=\"title\">Format</span>\n\t\t\t\t\t\t<select name=\"post_format\">\n\t\t\t\t\t\t\t<option value=\"-1\">&mdash; No Change &mdash;</option>\n\t\t\t\t\t\t\t<option value=\"0\">Standard</option>\n{}{options}{}</select>\n\t\t\t\t\t</label>\n\n\t\t\t\t",
        "\t".repeat(15),
        "\t".repeat(13)
    )
}

const INLINE_EDIT: &str = include_str!("posts/inline-edit.html");

const PASSWORD: &str = "\n\t\t\t\t\t<div class=\"inline-edit-group wp-clearfix\">\n\t\t\t\t\t\t<label class=\"alignleft\">\n\t\t\t\t\t\t\t<span class=\"title\">Password</span>\n\t\t\t\t\t\t\t<span class=\"input-text-wrap\"><input type=\"text\" name=\"post_password\" class=\"inline-edit-password-input ltr\" value=\"\" /></span>\n\t\t\t\t\t\t</label>\n\n\t\t\t\t\t\t<span class=\"alignleft inline-edit-or\">\n\t\t\t\t\t\t\t&ndash;OR&ndash;\t\t\t\t\t\t</span>\n\t\t\t\t\t\t<label class=\"alignleft inline-edit-private\">\n\t\t\t\t\t\t\t<input type=\"checkbox\" name=\"keep_private\" value=\"private\" />\n\t\t\t\t\t\t\t<span class=\"checkbox-title\">Private</span>\n\t\t\t\t\t\t</label>\n\t\t\t\t\t</div>\n\n\t\t\t\t";

const STICKY: &str = "\n\t\t\t\t\t\t\t\n\t\t\t\t\t\t\t\t<label class=\"alignleft\">\n\t\t\t\t\t\t\t\t\t<input type=\"checkbox\" name=\"sticky\" value=\"sticky\" />\n\t\t\t\t\t\t\t\t\t<span class=\"checkbox-title\">Make this post sticky</span>\n\t\t\t\t\t\t\t\t</label>\n\n\t\t\t\t\t\t\t\n\t\t\t\t\t\t";

const BULK_STICKY: &str = "\n\t\t\t\t\t\t\t\n\t\t\t\t\t\t\t\t<label class=\"alignright\">\n\t\t\t\t\t\t\t\t\t<span class=\"title\">Sticky</span>\n\t\t\t\t\t\t\t\t\t<select name=\"sticky\">\n\t\t\t\t\t\t\t\t\t\t<option value=\"-1\">&mdash; No Change &mdash;</option>\n\t\t\t\t\t\t\t\t\t\t<option value=\"sticky\">Sticky</option>\n\t\t\t\t\t\t\t\t\t\t<option value=\"unsticky\">Not Sticky</option>\n\t\t\t\t\t\t\t\t\t</select>\n\t\t\t\t\t\t\t\t</label>\n\n\t\t\t\t\t\t\t\n\t\t\t\t\t\t";

/// `inline_edit()`: the Quick Edit and Bulk Edit rows of a table of
/// `columns` visible columns, the date fields from `first`, the global post.
fn inline_edit(site: &dyn PostsSite, first: &Post, columns: usize) -> String {
    let can_publish = site.can("publish_posts");
    let others = site.can("edit_others_posts");
    let (zone, now) = site.clock();
    let when = |shown: bool, text: &str| {
        if shown {
            text.to_string()
        } else {
            String::new()
        }
    };
    let (authors, bulk_authors) = if others {
        let users = site.authors();
        (
            authors_dropdown(&users, false),
            authors_dropdown(&users, true),
        )
    } else {
        Default::default()
    };
    let formats = site.post_formats();
    INLINE_EDIT
        .replace("%columns%", &columns.to_string())
        .replace("%date%", &touch_time(first, zone, now))
        .replace("%authors%", &authors)
        .replace("%bulk_authors%", &bulk_authors)
        .replace("%password%", &when(can_publish, PASSWORD))
        .replace("%categories%", &categories_checklist(site))
        .replace("%statuses%", &status_options(can_publish, false))
        .replace("%bulk_statuses%", &status_options(can_publish, true))
        .replace("%sticky%", &when(can_publish && others, STICKY))
        .replace("%bulk_sticky%", &when(can_publish && others, BULK_STICKY))
        .replace(
            "%formats%",
            &when(!formats.is_empty(), &formats_select(formats)),
        )
        .replace("%nonce%", &site.nonce("inlineeditnonce"))
}

/// `single_row( $post )`: the post's row of `columns`, `hidden` ones
/// hidden.
pub fn row(
    site: &dyn PostsSite,
    post: &Post,
    columns: &[Column],
    hidden: &[&str],
    filter: &str,
) -> String {
    let author = if site.user_id() == post.author {
        "self"
    } else {
        "other"
    };
    let class = site.post_class(&format!("iedit author-{author} level-0"), post);
    let mut out = format!("\t\t<tr id=\"post-{}\" class=\"{class}\">\n\t\t\t", post.id);
    for column in columns {
        let key = column.key;
        let mut classes = format!("{key} column-{key}");
        if key == "title" {
            classes.push_str(" has-row-actions column-primary");
        }
        if hidden.contains(&key) {
            classes.push_str(" hidden");
        }
        let cell = match key {
            "cb" => {
                out.push_str(&format!(
                    "<td class=\"check-column\">{}</td>",
                    column_cb(site, post)
                ));
                continue;
            }
            "title" => {
                out.push_str(&column_title(site, post, &classes, filter));
                continue;
            }
            "author" => column_author(site, post),
            "categories" => column_terms(site, post, "category"),
            "tags" => column_terms(site, post, "post_tag"),
            "comments" => format!(
                "\t\t<div class=\"post-com-count-wrapper\">\n\t\t{}\t\t</div>\n\t\t",
                comments_bubble(site, post)
            ),
            _ => column_date(site, post),
        };
        let label = hyscribe_kses::wp_strip_all_tags(column.label.as_bytes(), false);
        out.push_str(&format!(
            "<td class='{classes}' data-colname=\"{}\">{cell}</td>",
            esc_attr(&label)
        ));
    }
    out.push_str("\t\t</tr>\n\t\t");
    out
}

/// The posts list's columns in the view of `filter`: no comments for posts
/// not published yet.
pub fn columns(filter: &str) -> Vec<Column> {
    let sort = |orderby, desc_first, abbr, caption, initial| {
        Some(Sort {
            orderby,
            desc_first,
            abbr,
            caption,
            initial,
        })
    };
    let mut columns = vec![
        Column {
            key: "cb",
            label: "<input type=\"checkbox\" />".into(),
            sort: None,
        },
        Column {
            key: "title",
            label: "Title".into(),
            sort: sort("title", false, "Title", "Table ordered by Title.", None),
        },
        Column {
            key: "author",
            label: "Author".into(),
            sort: None,
        },
        Column {
            key: "categories",
            label: "Categories".into(),
            sort: None,
        },
        Column {
            key: "tags",
            label: "Tags".into(),
            sort: None,
        },
        Column {
            key: "date",
            label: "Date".into(),
            sort: sort("date", true, "Date", "Table ordered by Date.", Some("desc")),
        },
    ];
    if !["pending", "draft", "future"].contains(&filter) {
        columns.insert(
            5,
            Column {
                key: "comments",
                label: "<span class=\"vers comment-grey-bubble\" title=\"Comments\" aria-hidden=\"true\"></span><span class=\"screen-reader-text\">Comments</span>".into(),
                sort: sort("comment_count", false, "Comments", "Table ordered by Comments.", None),
            },
        );
    }
    columns
}

/// The statuses `get_post_stati()` registers that the status links list,
/// with their labels for one and for more.
const STATUSES: [(&str, &str, &str); 6] = [
    ("publish", "Published", "Published"),
    ("future", "Scheduled", "Scheduled"),
    ("draft", "Draft", "Drafts"),
    ("pending", "Pending", "Pending"),
    ("private", "Private", "Private"),
    ("trash", "Trash", "Trash"),
];

/// The statuses left out of the All list.
const NOT_IN_ALL: [&str; 7] = [
    "trash",
    "auto-draft",
    "inherit",
    "request-pending",
    "request-confirmed",
    "request-failed",
    "request-completed",
];

/// What the list shows: the counts its links and pages read.
struct Listing {
    posts: Vec<Post>,
    total: u64,
    counts: Vec<(String, u64)>,
    own: u64,
    sticky: u64,
    /// `$_GET['author']` set to the user, who may only list their own.
    own_only: bool,
    user: u64,
    /// `$_REQUEST['post_status']`, empty for none.
    filter: String,
    /// `is_base_request()`.
    base: bool,
    /// The page shown, from 1, of how many.
    page: u64,
    pages: u64,
    /// `$_GET['orderby']` and `$_GET['order']`, if given.
    orderby: Option<String>,
    order: Option<String>,
}

impl Listing {
    /// The Trash is listed.
    fn trash(&self) -> bool {
        self.filter == "trash"
    }
}

/// `WP_Posts_List_Table::get_views()`.
fn status_views(listing: &Listing) -> Vec<View> {
    let count = |status: &str| {
        listing
            .counts
            .iter()
            .find(|(name, _)| name == status)
            .map_or(0, |(_, count)| *count)
    };
    let all_total: u64 = listing
        .counts
        .iter()
        .filter(|(status, _)| !NOT_IN_ALL.contains(&status.as_str()))
        .map(|(_, count)| count)
        .sum();
    let label =
        |text: &str, n: u64| format!("{text} <span class=\"count\">({})</span>", number_format(n));
    let mine = listing.own > 0 && listing.own != all_total;
    let mut links = vec![View {
        class: "all".into(),
        url: if mine {
            "edit.php?post_type=post&all_posts=1".into()
        } else {
            "edit.php?post_type=post".into()
        },
        label: label("All", all_total),
        current: !listing.own_only && listing.base,
    }];
    if mine {
        links.push(View {
            class: "mine".into(),
            url: format!("edit.php?post_type=post&author={}", listing.user),
            label: label("Mine", listing.own),
            current: listing.own_only,
        });
    }
    for (status, one, many) in STATUSES {
        let n = count(status);
        if n == 0 {
            continue;
        }
        links.push(View {
            class: status.into(),
            url: format!("edit.php?post_status={status}&post_type=post"),
            label: label(if n == 1 { one } else { many }, n),
            current: listing.filter == status,
        });
    }
    if listing.sticky > 0 {
        let after = links
            .iter()
            .position(|view| view.class == "publish")
            .unwrap_or(0);
        links.insert(
            after + 1,
            View {
                class: "sticky".into(),
                url: "edit.php?post_type=post&show_sticky=1".into(),
                label: label("Sticky", listing.sticky),
                current: false,
            },
        );
    }
    links
}

/// `wp_dropdown_categories()` of the category filter, all of them by name.
fn categories_dropdown(site: &dyn PostsSite) -> String {
    let terms = site.terms("category");
    let mut out = String::from(
        "<label class=\"screen-reader-text\" for=\"cat\">Filter by category</label><select  name='cat' id='cat' class='postform'>\n",
    );
    // The default category is always there.
    out.push_str("\t<option value='0'>All Categories</option>\n");
    let mut depth = 0;
    walk(&terms, true, &mut |event| match event {
        Walk::Start(term) => out.push_str(&format!(
            "\t<option class=\"level-{depth}\" value=\"{}\">{}{}</option>\n",
            term.id,
            "&nbsp;".repeat(depth * 3),
            text(&wptexturize(&term.name, &[]))
        )),
        Walk::Level(_, open) => {
            if open {
                depth += 1;
            } else {
                depth -= 1;
            }
        }
        Walk::End => {}
    });
    out.push_str("</select>\n");
    out
}

/// `formats_dropdown()`: the formats some post has, standard left out.
fn formats_dropdown(site: &dyn PostsSite) -> String {
    let used: Vec<Term> = site
        .terms("post_format")
        .into_iter()
        .filter(|term| term.count > 0)
        .collect();
    if used.is_empty() {
        return String::new();
    }
    let options: String = used
        .iter()
        .map(|term| text(&term.slug).replacen("post-format-", "", 1))
        .filter(|slug| slug != "standard")
        .map(|slug| {
            format!(
                "\t\t\t\t<option value=\"{}\">{}</option>\n\t\t\t\t",
                esc_attr(slug.as_bytes()),
                esc_html(post_format_string(slug.as_bytes()).as_bytes())
            )
        })
        .collect();
    format!(
        "\t\t<label for=\"filter-by-format\" class=\"screen-reader-text\">\n\t\t\tFilter by post format\t\t</label>\n\t\t<select name=\"post_format\" id=\"filter-by-format\">\n\t\t\t<option selected='selected' value=\"\">All formats</option>\n\t\t\t{options}\t\t</select>\n\t\t"
    )
}

/// `display_tablenav( $which )` of `listing`, its bulk actions hidden when
/// it is empty.
fn tablenav(
    site: &dyn PostsSite,
    listing: &Listing,
    which: &str,
    months: &[(u32, u32)],
    pages: String,
) -> String {
    let mut actions = vec![if listing.trash() {
        ("untrash", "Restore")
    } else {
        ("edit", "Bulk edit")
    }];
    if site.can("delete_posts") {
        actions.push(if listing.trash() {
            ("delete", "Delete permanently")
        } else {
            ("trash", "Move to Trash")
        });
    }
    let empty = listing.posts.is_empty();
    let hidden = if empty { " hidden" } else { "" };
    let mut extra = if which == "top" {
        format!(
            "{}{}{}<input type=\"submit\" name=\"filter_action\" id=\"post-query-submit\" class=\"button button-compact\" value=\"Filter\"  />",
            months_dropdown(months, "Filter by date"),
            categories_dropdown(site),
            if listing.trash() {
                String::new()
            } else {
                formats_dropdown(site)
            }
        )
    } else {
        String::new()
    };
    if listing.trash() && !empty && site.can("edit_others_posts") {
        extra.push_str("<input type=\"submit\" name=\"delete_all\" id=\"delete_all\" class=\"button apply\" value=\"Empty Trash\"  />");
    }
    format!(
        "\t<div class=\"tablenav {which}\">\n\n\t\t\t\t<div class=\"alignleft actions bulkactions{hidden}\">\n\t\t\t{}\t\t</div>\n\t\t\t\t<div class=\"alignleft actions\">\n\t\t{extra}\t\t</div>\n\t\t{pages}\n\t\t<br class=\"clear\" />\n\t</div>\n\t\t",
        bulk_actions(which, &actions)
    )
}

/// The list's markup, from its heading to the inline editors.
/// `url` is the page's and `request_uri` its path and query.
fn list_content(
    site: &dyn PostsSite,
    listing: &Listing,
    hidden: &[&str],
    url: &str,
    request_uri: &str,
) -> String {
    let columns = columns(&listing.filter);
    let months = site.months("post", listing.trash());
    let rows: String = listing
        .posts
        .iter()
        .map(|post| row(site, post, &columns, hidden, &listing.filter))
        .collect();
    let visible = columns
        .iter()
        .filter(|column| !hidden.contains(&column.key))
        .count();
    let links = status_views(listing);
    let order = Order {
        orderby: listing.orderby.as_deref(),
        desc: listing.order.as_deref() == Some("desc"),
    };
    let unpaged = text(&remove_query_arg(&[b"paged"], url.as_bytes()));
    let kept = |value: &Option<String>| addslashes(value.as_deref().unwrap_or_default());
    let order_fields = [
        ("orderby", kept(&listing.orderby)),
        ("order", kept(&listing.order)),
    ];
    let order_fields: Vec<(&str, &str)> = order_fields
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let mut out = format!(
        "<div class=\"wrap\">\n<h1 class=\"wp-heading-inline\">\nPosts</h1>\n\n <a href=\"{}\" class=\"page-title-action\">Add Post</a>\n<hr class=\"wp-header-end\">\n\n\n{}\n<form id=\"posts-filter\" method=\"get\">\n\n{}\n<input type=\"hidden\" name=\"post_status\" class=\"post_status_page\" value=\"{}\" />\n<input type=\"hidden\" name=\"post_type\" class=\"post_type_page\" value=\"post\" />\n\n\n\n",
        text(&esc_url(
            BarSite::admin_url(site, "post-new.php").as_bytes()
        )),
        views("Filter posts list", &links),
        if listing.posts.is_empty() {
            String::new()
        } else {
            search_box("Search Posts", "post", &order_fields)
        },
        if is_empty_string(listing.filter.as_bytes()) {
            "all".to_string()
        } else {
            esc_attr(addslashes(&listing.filter).as_bytes())
        }
    );
    out.push_str(&format!(
        "<input type=\"hidden\" id=\"_wpnonce\" name=\"_wpnonce\" value=\"{}\" /><input type=\"hidden\" name=\"_wp_http_referer\" value=\"{}\" />",
        site.nonce("bulk-posts"),
        text(&esc_url(request_uri.as_bytes()))
    ));
    let empty = listing.posts.is_empty();
    out.push_str(&tablenav(
        site,
        listing,
        "top",
        &months,
        pagination("top", listing.total, listing.pages, listing.page, url),
    ));
    let rows = if empty {
        let none = if listing.trash() {
            "No posts found in Trash."
        } else {
            "No posts found."
        };
        format!(
            "<tr class=\"no-items\"><td class=\"colspanchange\" colspan=\"{visible}\">{none}</td></tr>"
        )
    } else {
        rows
    };
    out.push_str(&format!(
        "<h2 class='screen-reader-text'>Posts list</h2><table class=\"wp-list-table widefat fixed striped table-view-list posts\">\n\t\t{}\t<thead>\n\t<tr>\n\t\t{}\t</tr>\n\t</thead>\n\n\t<tbody id=\"the-list\"\n\t\t\t\t>\n\t\t{rows}\t</tbody>\n\n\t<tfoot>\n\t<tr>\n\t\t{}\t</tr>\n\t</tfoot>\n\n</table>\n\t\t",
        table_description(&columns, &order),
        column_headers(&columns, hidden, &order, &unpaged, 1, true),
        column_headers(&columns, hidden, &order, &unpaged, 2, false)
    ));
    if !empty {
        out.push_str(&tablenav(
            site,
            listing,
            "bottom",
            &months,
            pagination("bottom", listing.total, listing.pages, listing.page, url),
        ));
    }
    out.push_str("\n</form>\n\n");
    if let Some(first) = listing.posts.first() {
        out.push_str(&format!("\n{}\t\t", inline_edit(site, first, visible)));
    }
    out.push_str("\n<div id=\"ajax-response\"></div>\n<div class=\"clear\"></div>\n</div>\n\n");
    out
}

const HELP_OVERVIEW: &str = "<p>This screen provides access to all of your posts. You can customize the display of this screen to suit your workflow.</p>";

const HELP_SCREEN_CONTENT: &str = "<p>You can customize the display of this screen&#8217;s contents in a number of ways:</p><ul><li>You can hide/display columns based on your needs and decide how many posts to list per screen using the Screen Options tab.</li><li>You can filter the list of posts by post status using the text links above the posts list to only show posts with that status. The default view is to show all posts.</li><li>You can view posts in a simple title list or with an excerpt using the Screen Options tab.</li><li>You can refine the list to show only posts in a specific category or from a specific month by using the dropdown menus above the posts list. Click the Filter button after making your selection. You also can refine the list by clicking on the post author, category or tag in the posts list.</li></ul>";

const HELP_ACTION_LINKS: &str = "<p>Hovering over a row in the posts list will display action links that allow you to manage your post. You can perform the following actions:</p><ul><li><strong>Edit</strong> takes you to the editing screen for that post. You can also reach that screen by clicking on the post title.</li><li><strong>Quick Edit</strong> provides inline access to the metadata of your post, allowing you to update post details without leaving this screen.</li><li><strong>Trash</strong> removes your post from this list and places it in the Trash, from which you can permanently delete it.</li><li><strong>Preview</strong> will show you what your draft post will look like if you publish it. View will take you to your live site to view the post. Which link is available depends on your post&#8217;s status.</li></ul>";

const HELP_BULK_ACTIONS: &str = "<p>You can also edit or move multiple posts to the Trash at once. Select the posts you want to act on using the checkboxes, then select the action you want to take from the Bulk actions menu and click Apply.</p><p>When using Bulk Edit, you can change the metadata (categories, author, etc.) for all selected posts at once. To remove a post from the grouping, just click the <span class=\"dashicons dashicons-dismiss\" aria-hidden=\"true\" style=\"font-size: 16px; width: 16px; vertical-align: middle;\"></span><span class=\"screen-reader-text\">remove</span> button next to its name in the Bulk Edit area that appears.</p>";

const HELP_SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/posts-screen/\">Documentation on Managing Posts</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

const SCREEN_OPTIONS: &str = include_str!("posts/screen-options.html");

/// `render_screen_options()`: the columns the user may hide, `hidden` ones
/// unchecked, and the number of posts per page.
fn screen_options(
    site: &dyn PostsSite,
    columns: &[Column],
    hidden: &[String],
    per_page: u64,
) -> String {
    let labels: String = columns
        .iter()
        .filter(|column| !["cb", "title"].contains(&column.key))
        .map(|column| {
            let key = column.key;
            let checked = if hidden.iter().any(|h| h == key) {
                ""
            } else {
                " checked='checked'"
            };
            format!(
                "<label><input class=\"hide-column-tog\" name=\"{key}-hide\" type=\"checkbox\" id=\"{key}-hide\" value=\"{key}\"{checked} />{}</label>\n",
                text(&hyscribe_kses::wp_strip_all_tags(column.label.as_bytes(), false))
            )
        })
        .collect();
    SCREEN_OPTIONS
        .replace("%labels%", &labels)
        .replace("%per_page%", &per_page.to_string())
        .replace("%nonce%", &site.nonce("screen-options-nonce"))
}

/// `get_hidden_columns()`: the columns the user hid, none by default.
fn hidden_columns(site: &dyn PostsSite) -> Vec<String> {
    let stored = site
        .user_option("manageedit-postcolumnshidden")
        .unwrap_or_default();
    match maybe_unserialize(&stored) {
        Maybe::Value(Value::Array(items)) => items
            .iter()
            .filter_map(|(_, value)| value.as_bytes().map(text))
            .collect(),
        _ => Vec::new(),
    }
}

/// `edit_post_per_page`, 20 unless the user chose a number of posts.
fn per_page(site: &dyn PostsSite) -> u64 {
    let stored = Value::String(site.user_option("edit_post_per_page").unwrap_or_default()).to_int();
    u64::try_from(stored).ok().filter(|n| *n > 0).unwrap_or(20)
}

/// The only author whose private posts the user may read, unless they may
/// read everyone's.
fn private_author(site: &dyn PostsSite) -> Option<u64> {
    (!site.can("read_private_posts")).then(|| site.user_id())
}

/// The statuses the All list shows the user: private ones only their own
/// unless they may read others'.
fn all_statuses(site: &dyn PostsSite) -> Vec<StatusFilter> {
    let private_author = private_author(site);
    ["publish", "future", "draft", "pending", "private"]
        .iter()
        .map(|status| StatusFilter {
            status: (*status).into(),
            author: if *status == "private" {
                private_author
            } else {
                None
            },
        })
        .collect()
}

/// `wp_edit_posts_query()`'s statuses for the requested `filter`: one
/// registered status, readable, or the All list's.
fn filtered(site: &dyn PostsSite, filter: &str) -> Vec<StatusFilter> {
    let registered =
        STATUSES.iter().any(|(status, ..)| *status == filter) || NOT_IN_ALL.contains(&filter);
    if !registered {
        return all_statuses(site);
    }
    vec![StatusFilter {
        status: filter.into(),
        author: private_author(site).filter(|_| filter == "private"),
    }]
}

/// `wp_edit_posts_query()`'s order through `WP_Query`: the requested one,
/// drafts and pending posts by their last change, pending ones oldest first.
fn sort(args: &ListArgs, query: &PostQuery) -> Vec<(OrderBy, bool)> {
    let changed = ["draft", "pending"].contains(&args.status.as_str());
    let orderby = match &args.orderby {
        Some(orderby) => orderby.as_str(),
        None if changed => "modified",
        None => "",
    };
    let ascending = match &args.order {
        Some(order) => order.eq_ignore_ascii_case("asc"),
        None => args.status == "pending",
    };
    order_by(&Value::String(orderby.into()), ascending, query)
}

/// The posts list's query arguments this port reads.
#[derive(Default)]
pub struct ListArgs {
    /// `$_GET['post_status']`, empty for none.
    pub status: String,
    /// `$_GET['orderby']`, if given.
    pub orderby: Option<String>,
    /// `$_GET['order']`, if given.
    pub order: Option<String>,
    /// `$_GET['paged']`, if given.
    pub paged: Option<String>,
    /// `is_base_request()`: no arguments but `paged` and the post type.
    pub base: bool,
}

/// What a screen answers: its page, or a redirect to the URL given.
pub enum Answer {
    /// The page.
    Page(String),
    /// A `302` to this URL.
    Redirect(String),
}

/// `addslashes()`, which `wp_magic_quotes()` applies to the request.
fn addslashes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\'' | '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            '\0' => out.push_str("\\0"),
            c => out.push(c),
        }
    }
    out
}

/// `edit.php`: the posts list for `args`, or a redirect from a page past
/// the last; `None` for a user who may not edit posts.
pub fn edit(
    site: &dyn PostsSite,
    args: &ListArgs,
    canonical: &str,
    origin: &str,
    request_uri: &str,
    browser: &Browser,
) -> Option<Answer> {
    if !site.can("edit_posts") {
        return None;
    }
    let own = site.own_posts("post");
    let filter = args.status.as_str();
    let own_only = own > 0 && !site.can("edit_others_posts") && is_empty_string(filter.as_bytes());
    let per_page = per_page(site);
    let mut query = PostQuery {
        post_types: vec!["post".into()],
        statuses: filtered(site, filter),
        authors: if own_only {
            vec![site.user_id()]
        } else {
            Vec::new()
        },
        limit: u32::try_from(per_page).unwrap_or(u32::MAX),
        ..PostQuery::default()
    };
    query.order = sort(args, &query);
    // `absint( $_REQUEST['paged'] )`, the first page for none.
    let requested = Value::String(args.paged.clone().unwrap_or_default().into_bytes())
        .to_int()
        .unsigned_abs()
        .max(1);
    query.offset = u32::try_from((requested - 1).saturating_mul(per_page)).unwrap_or(u32::MAX);
    let (posts, total) = site.list_posts(&query);
    let pages = total.div_ceil(per_page);
    if pages > 0 && requested > pages {
        let page = pages.to_string();
        let to = add_query_arg(&[(b"paged", Some(page.as_bytes()))], request_uri.as_bytes());
        return Some(Answer::Redirect(text(&to)));
    }
    let sticky_ids = sticky_posts(&site.option("sticky_posts").unwrap_or_default());
    let listing = Listing {
        posts,
        total,
        counts: site.status_counts("post", private_author(site)),
        own,
        sticky: if sticky_ids.is_empty() {
            0
        } else {
            site.sticky_count("post", &sticky_ids)
        },
        own_only,
        user: site.user_id(),
        filter: filter.into(),
        base: args.base,
        page: requested,
        pages,
        orderby: args.orderby.clone(),
        order: args.order.clone(),
    };
    let hidden = hidden_columns(site);
    let hidden_keys: Vec<&str> = hidden.iter().map(String::as_str).collect();
    let content = list_content(site, &listing, &hidden_keys, canonical, request_uri);
    let screen_options = screen_options(site, &columns(filter), &hidden, per_page);
    let help = [
        HelpTab {
            id: "overview",
            title: "Overview",
            content: HELP_OVERVIEW,
        },
        HelpTab {
            id: "screen-content",
            title: "Screen Content",
            content: HELP_SCREEN_CONTENT,
        },
        HelpTab {
            id: "action-links",
            title: "Available Actions",
            content: HELP_ACTION_LINKS,
        },
        HelpTab {
            id: "bulk-actions",
            title: "Bulk actions",
            content: HELP_BULK_ACTIONS,
        },
    ];
    let page = Page {
        screen: Screen {
            pagenow: "edit.php",
            typenow: "post",
            parent_file: Some("edit.php"),
            submenu_file: Some("edit.php"),
        },
        id: "edit-post",
        title: "Posts",
        help: &help,
        sidebar: HELP_SIDEBAR,
        screen_options: &screen_options,
        styles: &[],
        scripts: &["inline-edit-post", "heartbeat"],
        late_scripts: &[],
        script_data: &[],
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical,
        origin,
    };
    admin_page(site, &page, browser).map(Answer::Page)
}

#[cfg(test)]
pub(crate) mod content;
#[cfg(test)]
pub(crate) mod tests;
