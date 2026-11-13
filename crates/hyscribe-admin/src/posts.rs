// SPDX-License-Identifier: AGPL-3.0-or-later

//! `edit.php`: the posts list, `WP_Posts_List_Table`.

use hyscribe_kses::{esc_attr, esc_html, esc_textarea, esc_url};
use hyscribe_model::{Post, PostQuery, StatusFilter, Term, User};
use hyscribe_phpser::{Maybe, Value, add_query_arg, is_empty_string, maybe_unserialize, urldecode};
use hyscribe_render::date::{self, Zone};
use hyscribe_render::formatting::{convert_chars, ent2ncr, sanitize_text_field, wptexturize};
use hyscribe_render::post::admin_title;
use hyscribe_render::post::post_format_string;
use hyscribe_render::{Walk, number_format, sticky_posts, walk, wp_trim_words};

use crate::admin_bar::BarSite;
use crate::admin_bar::Browser;
use crate::list_table::{
    Column, Sort, View, bulk_actions, column_headers, months_dropdown, pagination, row_actions,
    search_box, table_description, views,
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
    /// The years and months with posts of `post_type` not trashed, newest
    /// first.
    fn months(&self, post_type: &str) -> Vec<(u32, u32)>;
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

/// `get_post_states()` of a post in the unfiltered posts list; the pages'
/// states (front page, posts page, privacy policy) come with Pages.
fn post_states(site: &dyn PostsSite, post: &Post) -> Vec<&'static str> {
    let mut states = Vec::new();
    if !post.password.is_empty() {
        states.push("Password protected");
    }
    match post.status.as_str() {
        "private" => states.push("Private"),
        "draft"
            if site
                .meta(post, "_customize_changeset_uuid")
                .is_some_and(|v| !v.is_empty()) =>
        {
            states.push("Customization Draft");
        }
        "draft" => states.push("Draft"),
        "pending" => states.push("Pending"),
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
fn post_states_html(site: &dyn PostsSite, post: &Post) -> String {
    let states = post_states(site, post);
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
fn column_title(site: &dyn PostsSite, post: &Post, classes: &str) -> String {
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
    out.push_str(&post_states_html(site, post));
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
    // The Trash, with its restore and delete links, is not listed yet.
    if site.can_post("delete_post", post) {
        let id = post.id;
        let nonce = site.nonce(&format!("trash-post_{id}"));
        let link = format!("post.php?post={id}&amp;action=trash&amp;_wpnonce={nonce}");
        actions.push((
            "trash",
            format!(
                "<a href=\"{}\" class=\"submitdelete\" aria-label=\"{}\">Trash</a>",
                BarSite::admin_url(site, &link),
                esc_attr(format!("Move &#8220;{title}&#8221; to the Trash").as_bytes())
            ),
        ));
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
    } else {
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
        format!(
            "<a href=\"{}\" class=\"post-com-count post-com-count-approved\"><span class=\"comment-count-approved\" aria-hidden=\"true\">{}</span><span class=\"screen-reader-text\">{approved_phrase}</span></a>",
            link("approved"),
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
pub fn row(site: &dyn PostsSite, post: &Post, columns: &[Column], hidden: &[&str]) -> String {
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
                out.push_str(&column_title(site, post, &classes));
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

/// The posts list's columns.
pub fn columns() -> Vec<Column> {
    let sort = |orderby, abbr, caption, initial| {
        Some(Sort {
            orderby,
            abbr,
            caption,
            initial,
        })
    };
    vec![
        Column {
            key: "cb",
            label: "<input type=\"checkbox\" />".into(),
            sort: None,
        },
        Column {
            key: "title",
            label: "Title".into(),
            sort: sort("title", "Title", "Table ordered by Title.", false),
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
            key: "comments",
            label: "<span class=\"vers comment-grey-bubble\" title=\"Comments\" aria-hidden=\"true\"></span><span class=\"screen-reader-text\">Comments</span>".into(),
            sort: sort("comment_count", "Comments", "Table ordered by Comments.", false),
        },
        Column {
            key: "date",
            label: "Date".into(),
            sort: sort("date", "Date", "Table ordered by Date.", true),
        },
    ]
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
}

/// `WP_Posts_List_Table::get_views()` for the unfiltered list.
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
        current: !listing.own_only,
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
            current: false,
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

/// `display_tablenav( $which )`, its bulk actions hidden for an `empty` list.
fn tablenav(
    site: &dyn PostsSite,
    which: &str,
    empty: bool,
    months: &[(u32, u32)],
    pages: String,
) -> String {
    let mut actions = vec![("edit", "Bulk edit")];
    if site.can("delete_posts") {
        actions.push(("trash", "Move to Trash"));
    }
    let hidden = if empty { " hidden" } else { "" };
    let extra = if which == "top" {
        format!(
            "{}{}{}<input type=\"submit\" name=\"filter_action\" id=\"post-query-submit\" class=\"button button-compact\" value=\"Filter\"  />",
            months_dropdown(months, "Filter by date"),
            categories_dropdown(site),
            formats_dropdown(site)
        )
    } else {
        String::new()
    };
    format!(
        "\t<div class=\"tablenav {which}\">\n\n\t\t\t\t<div class=\"alignleft actions bulkactions{hidden}\">\n\t\t\t{}\t\t</div>\n\t\t\t\t<div class=\"alignleft actions\">\n\t\t{extra}\t\t</div>\n\t\t{pages}\n\t\t<br class=\"clear\" />\n\t</div>\n\t\t",
        bulk_actions(which, &actions)
    )
}

/// The list's markup, from its heading to the inline editors.
fn list_content(
    site: &dyn PostsSite,
    listing: &Listing,
    hidden: &[&str],
    request_uri: &str,
) -> String {
    let columns = columns();
    let url = BarSite::admin_url(site, "edit.php");
    let months = site.months("post");
    let rows: String = listing
        .posts
        .iter()
        .map(|post| row(site, post, &columns, hidden))
        .collect();
    let visible = columns
        .iter()
        .filter(|column| !hidden.contains(&column.key))
        .count();
    let per_page = per_page(site);
    let pages = listing.total.div_ceil(per_page);
    let links = status_views(listing);
    let mut out = format!(
        "<div class=\"wrap\">\n<h1 class=\"wp-heading-inline\">\nPosts</h1>\n\n <a href=\"{}\" class=\"page-title-action\">Add Post</a>\n<hr class=\"wp-header-end\">\n\n\n{}\n<form id=\"posts-filter\" method=\"get\">\n\n{}\n<input type=\"hidden\" name=\"post_status\" class=\"post_status_page\" value=\"all\" />\n<input type=\"hidden\" name=\"post_type\" class=\"post_type_page\" value=\"post\" />\n\n\n\n",
        text(&esc_url(
            BarSite::admin_url(site, "post-new.php").as_bytes()
        )),
        views("Filter posts list", &links),
        if listing.posts.is_empty() {
            String::new()
        } else {
            search_box("Search Posts", "post")
        }
    );
    out.push_str(&format!(
        "<input type=\"hidden\" id=\"_wpnonce\" name=\"_wpnonce\" value=\"{}\" /><input type=\"hidden\" name=\"_wp_http_referer\" value=\"{}\" />",
        site.nonce("bulk-posts"),
        esc_attr(request_uri.as_bytes())
    ));
    let empty = listing.posts.is_empty();
    out.push_str(&tablenav(
        site,
        "top",
        empty,
        &months,
        pagination("top", listing.total, pages, &url),
    ));
    let rows = if empty {
        format!(
            "<tr class=\"no-items\"><td class=\"colspanchange\" colspan=\"{visible}\">No posts found.</td></tr>"
        )
    } else {
        rows
    };
    out.push_str(&format!(
        "<h2 class='screen-reader-text'>Posts list</h2><table class=\"wp-list-table widefat fixed striped table-view-list posts\">\n\t\t{}\t<thead>\n\t<tr>\n\t\t{}\t</tr>\n\t</thead>\n\n\t<tbody id=\"the-list\"\n\t\t\t\t>\n\t\t{rows}\t</tbody>\n\n\t<tfoot>\n\t<tr>\n\t\t{}\t</tr>\n\t</tfoot>\n\n</table>\n\t\t",
        table_description(&columns),
        column_headers(&columns, hidden, &url, 1, true),
        column_headers(&columns, hidden, &url, 2, false)
    ));
    if !empty {
        out.push_str(&tablenav(
            site,
            "bottom",
            false,
            &months,
            pagination("bottom", listing.total, pages, &url),
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
fn screen_options(site: &dyn PostsSite, columns: &[Column], hidden: &[String]) -> String {
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
        .replace("%per_page%", &per_page(site).to_string())
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

/// `edit.php`: the posts list, `None` for a user who may not edit posts.
pub fn edit(
    site: &dyn PostsSite,
    canonical: &str,
    origin: &str,
    request_uri: &str,
    browser: &Browser,
) -> Option<String> {
    if !site.can("edit_posts") {
        return None;
    }
    let own = site.own_posts("post");
    let own_only = own > 0 && !site.can("edit_others_posts");
    let query = PostQuery {
        post_types: vec!["post".into()],
        statuses: all_statuses(site),
        authors: if own_only {
            vec![site.user_id()]
        } else {
            Vec::new()
        },
        limit: u32::try_from(per_page(site)).unwrap_or(u32::MAX),
        ..PostQuery::default()
    };
    let (posts, total) = site.list_posts(&query);
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
    };
    let hidden = hidden_columns(site);
    let hidden_keys: Vec<&str> = hidden.iter().map(String::as_str).collect();
    let content = list_content(site, &listing, &hidden_keys, request_uri);
    let screen_options = screen_options(site, &columns(), &hidden);
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
    admin_page(site, &page, browser)
}

#[cfg(test)]
pub(crate) mod content;
#[cfg(test)]
pub(crate) mod tests;
