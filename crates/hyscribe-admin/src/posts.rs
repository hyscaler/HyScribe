// SPDX-License-Identifier: AGPL-3.0-or-later

//! `edit.php`: the posts list, `WP_Posts_List_Table`.

use std::collections::HashMap;

use hyscribe_kses::{esc_attr, esc_html, esc_textarea, esc_url};
use hyscribe_model::{OrderBy, Post, PostQuery, StatusFilter, Term, User};
use hyscribe_phpser::{
    Maybe, Value, absint, add_query_arg, addslashes, is_empty_string, maybe_unserialize,
    remove_query_arg, urldecode,
};
use hyscribe_render::date::{self, Zone};
use hyscribe_render::formatting::{convert_chars, ent2ncr, sanitize_text_field, wptexturize};
use hyscribe_render::kses::wp_kses_data;
use hyscribe_render::post::admin_title;
use hyscribe_render::post::post_format_string;
use hyscribe_render::{
    Walk, number_format, order_by, sticky_posts, user_avatar, walk, wp_trim_words,
};

use crate::admin_bar::BarSite;
use crate::admin_bar::Browser;
use crate::assets::Urls;
use crate::list_table::{
    Column, Order, Sort, View, bulk_actions, column_headers, fill, months_dropdown, pagination,
    row_actions, screen_options as list_screen_options, search_box, selected as selected_if,
    table_description, views, wp_nonce_field,
};
use crate::login::set_url_scheme;
use crate::menu::Screen;
use crate::menu::esc_link;
use crate::page::{HelpTab, Page, admin_page, wp_admin_notice};
use crate::page::{PageSite, user_setting};
use crate::text;

/// What the posts list asks of the site about its posts.
pub trait PostsSite: PageSite {
    /// `current_user_can( $cap, $post->ID )`.
    fn can_post(&self, cap: &str, post: &Post) -> bool;
    /// `current_user_can( $cap, $id )` of a user or a term.
    fn can_for(&self, cap: &str, id: u64) -> bool;
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
    /// `get_userdata( $id )`.
    fn userdata(&self, id: u64) -> Option<User>;
    /// `get_pending_comments_num( $post->ID )`.
    fn pending_comments(&self, post: &Post) -> u64;
    /// `get_the_excerpt( $post )`.
    fn excerpt(&self, post: &Post) -> Vec<u8>;
    /// Every term of `taxonomy`, by name.
    fn terms(&self, taxonomy: &str) -> Vec<Term>;
    /// The IDs of the ten terms of `taxonomy` with the most posts.
    fn popular_terms(&self, taxonomy: &str) -> Vec<u64>;
    /// The users who have `cap`, by display name.
    fn authors(&self, cap: &str) -> Vec<User>;
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
    /// `get_post( $id )`, of any type and status.
    fn post(&self, id: u64) -> Option<Post>;
    /// `get_page_templates( null, $post_type )`: each template's slug and
    /// title, by title.
    fn page_templates(&self, post_type: &str) -> Vec<(String, String)>;
}

/// A post type the list shows, as `register_post_type()` labels it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PostType {
    /// Its name.
    pub name: &'static str,
    /// `labels->name`, which in lower case names its capabilities.
    plural: &'static str,
    /// `labels->singular_name`.
    singular: &'static str,
}

/// Posts.
pub const POST: PostType = PostType {
    name: "post",
    plural: "Posts",
    singular: "Post",
};

/// Pages, which nest and have an order.
pub const PAGE: PostType = PostType {
    name: "page",
    plural: "Pages",
    singular: "Page",
};

impl Default for PostType {
    fn default() -> Self {
        POST
    }
}

impl PostType {
    /// The type the list shows by `name`, if any.
    pub fn named(name: &[u8]) -> Option<Self> {
        [POST, PAGE]
            .into_iter()
            .find(|kind| kind.name.as_bytes() == name)
    }

    fn hierarchical(self) -> bool {
        self == PAGE
    }

    fn lower(self) -> String {
        self.plural.to_ascii_lowercase()
    }

    /// The type's own `*_posts` capability.
    fn cap(self, cap: &str) -> String {
        cap.replacen("posts", &self.lower(), 1)
    }
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

/// `is_sticky( $post->ID )`.
fn is_sticky(site: &dyn PostsSite, post: &Post) -> bool {
    sticky_posts(&site.option("sticky_posts").unwrap_or_default()).contains(&post.id)
}

/// `get_the_excerpt()` trimmed to 15 words, for a post without a title the
/// user may read, in the compact view.
fn no_title_excerpt(site: &dyn PostsSite, post: &Post, excerpt: bool) -> String {
    if excerpt
        || !admin_title(post).is_empty()
        || !post.password.is_empty()
        || !site.can_post("read_post", post)
    {
        return String::new();
    }
    esc_html(&wp_trim_words(&site.excerpt(post), 15, b"&hellip;"))
}

/// The post's title for its labels: with part of its excerpt if it has
/// none, the excerpt wrapped as `column_title()` shows it.
fn labelled_title(site: &dyn PostsSite, post: &Post, wrap: bool, extended: bool) -> String {
    let mut title = draft_or_post_title(post);
    let excerpt = no_title_excerpt(site, post, extended);
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
fn column_cb(site: &dyn PostsSite, post: &Post, excerpt: bool) -> String {
    if !site.can_post("edit_post", post) {
        return String::new();
    }
    let id = post.id;
    let title = labelled_title(site, post, false, excerpt);
    format!(
        "\t\t\t<input id=\"cb-select-{id}\" type=\"checkbox\" name=\"post[]\" value=\"{id}\" />\n\t\t\t<label for=\"cb-select-{id}\">\n\t\t\t\t<span class=\"screen-reader-text\">\n\t\t\t\tSelect {title}\t\t\t\t</span>\n\t\t\t</label>\n\t\t\t<div class=\"locked-indicator\">\n\t\t\t\t<span class=\"locked-indicator-icon\" aria-hidden=\"true\"></span>\n\t\t\t\t<span class=\"screen-reader-text\">\n\t\t\t\t&#8220;{title}&#8221; is locked\t\t\t\t</span>\n\t\t\t</div>\n\t\t\t"
    )
}

/// `get_post_states()` in the posts list filtered by `filter`, whose own
/// state goes unsaid.
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
    let is = |option: &str| {
        Value::String(site.option(option).unwrap_or_default()).to_int() == post.id as i64
    };
    if site.option("show_on_front").as_deref() == Some(b"page") {
        if is("page_on_front") {
            states.push("Front Page");
        }
        if is("page_for_posts") {
            states.push("Posts Page");
        }
    }
    if is("wp_page_for_privacy_policy") {
        states.push("Privacy Policy Page");
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
pub(crate) fn edit_link(site: &dyn PostsSite, post: &Post) -> String {
    BarSite::admin_url(site, &format!("post.php?post={}&amp;action=edit", post.id))
}

/// `get_inline_data( $post )`: the Quick Edit form's values, for a post
/// the user may edit.
fn inline_data(site: &dyn PostsSite, post: &Post) -> String {
    let id = post.id;
    let stored = |format: &str| date::format(format, &post.date, Zone::Utc);
    let template = site.meta(post, "_wp_page_template").unwrap_or_default();
    let mut out = format!(
        "\n<div class=\"hidden\" id=\"inline_{id}\">\n\t<div class=\"post_title\">{}</div>\n\t<div class=\"post_name\">{}</div>\n\t<div class=\"post_author\">{}</div>\n\t<div class=\"comment_status\">{}</div>\n\t<div class=\"ping_status\">{}</div>\n\t<div class=\"_status\">{}</div>\n\t<div class=\"jj\">{}</div>\n\t<div class=\"mm\">{}</div>\n\t<div class=\"aa\">{}</div>\n\t<div class=\"hh\">{}</div>\n\t<div class=\"mn\">{}</div>\n\t<div class=\"ss\">{}</div>\n\t<div class=\"post_password\">{}</div>{}<div class=\"page_template\">{}</div>",
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
        if post.post_type == "page" {
            format!("<div class=\"post_parent\">{}</div>", post.parent)
        } else {
            String::new()
        },
        if template.is_empty() {
            "default".to_string()
        } else {
            esc_html(&template)
        },
    );
    if post.post_type == "page" {
        out.push_str(&format!(
            "<div class=\"menu_order\">{}</div></div>",
            post.menu_order
        ));
        return out;
    }
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

/// `get_post_ancestors( $post )`: its parent's ID, then theirs, until one
/// is missing or repeats.
fn ancestors(lookup: &dyn Fn(u64) -> Option<Post>, post: &Post) -> Vec<u64> {
    if post.parent == 0 || post.parent == post.id {
        return Vec::new();
    }
    let mut ids = vec![post.parent];
    while let Some(ancestor) = ids.last().and_then(|id| lookup(*id)) {
        let up = ancestor.parent;
        if up == 0 || up == post.id || ids.contains(&up) {
            break;
        }
        ids.push(up);
    }
    ids
}

/// `wp_check_post_lock()`: the other user whose edit lock on `post` is
/// fresh.
pub(crate) fn lock_holder(site: &dyn PostsSite, post: &Post) -> Option<User> {
    let lock = site.meta(post, "_edit_lock").unwrap_or_default();
    let int = |text: &[u8]| Value::String(text.to_vec()).to_int();
    let mut parts = lock.split(|b| *b == b':');
    let time = int(parts.next().unwrap_or_default());
    let user = match parts.next() {
        Some(user) => int(user),
        None => int(&site.meta(post, "_edit_last").unwrap_or_default()),
    };
    let user = site.userdata(u64::try_from(user).unwrap_or_default())?;
    let now = site.clock().1;
    (time != 0 && time > now - 150 && user.id != site.user_id()).then_some(user)
}

/// `get_preview_post_link( $post, $args )`: the permalink in the admin's
/// scheme, which is HTTPS with an HTTPS site URL.
pub(crate) fn preview_link(
    site: &dyn PostsSite,
    post: &Post,
    args: &[(&[u8], Option<&[u8]>)],
) -> Vec<u8> {
    let ssl = Urls::site_url(site)
        .to_ascii_lowercase()
        .starts_with("https:");
    let link = set_url_scheme(&site.permalink(post), if ssl { "https" } else { "http" });
    let args = [args, &[(b"preview", Some(b"true"))]].concat();
    add_query_arg(&args, link.as_bytes())
}

/// `the_title` of `post` with the list's `esc_html` filter.
fn listed_title(post: &Post) -> String {
    esc_html(&admin_title(post))
}

/// `_column_title()` and `column_title()` with the row actions; a tree's
/// page at `level` 0 with a parent is padded to its depth and names it.
fn column_title(
    site: &dyn PostsSite,
    post: &Post,
    classes: &str,
    filter: &str,
    excerpt: bool,
    (level, lock): (Option<usize>, Option<&User>),
) -> String {
    let aria_label = if is_empty_string(&post.title) {
        "no title".to_string()
    } else {
        esc_attr(&post.title)
    };
    let mut out = format!(
        "<th scope=\"row\" class=\"{classes} page-title\" data-colname=\"Title\" aria-label=\"{aria_label}\">"
    );
    let mut depth = level.unwrap_or_default();
    let mut parent_name = None;
    if level == Some(0) {
        // The ancestors there are, which PHP walks forever round a loop.
        let found: Vec<Post> = ancestors(&|id| site.post(id), post)
            .into_iter()
            .map_while(|id| site.post(id))
            .collect();
        depth += found.len();
        parent_name = found.first().map(listed_title);
    }
    let editable = site.can_post("edit_post", post) && post.status != "trash";
    if editable {
        let (avatar, text) = lock.map_or_else(Default::default, |holder| {
            let avatar = user_avatar(&|name| site.option(name), holder, 18);
            let name = String::from_utf8_lossy(&holder.display_name);
            (
                avatar,
                esc_html(format!("{name} is currently editing").as_bytes()),
            )
        });
        out.push_str(&format!(
            "<div class=\"locked-info\"><span class=\"locked-avatar\">{}</span> <span class=\"locked-text\">{text}</span></div>\n",
            String::from_utf8_lossy(&avatar)
        ));
    }
    let pad = "<span aria-hidden=\"true\">&#8212;</span> ".repeat(depth);
    let (described_by, linked, unlinked) = match site.post(post.parent) {
        Some(parent) => {
            let id = format!("post-hierarchy-{}", post.id);
            let child_of = format!(
                "Child of {}",
                text(&hyscribe_kses::wp_strip_all_tags(
                    listed_title(&parent).as_bytes(),
                    false
                ))
            );
            let child_of = esc_html(child_of.as_bytes());
            (
                format!(" aria-describedby=\"{id}\""),
                format!("<span id=\"{id}\" class=\"hidden\">{child_of}</span>"),
                format!("<span id=\"{id}\" class=\"screen-reader-text\"> ({child_of})</span>"),
            )
        }
        None => Default::default(),
    };
    let title = labelled_title(site, post, true, excerpt);
    out.push_str("<strong>");
    if editable {
        out.push_str(&format!(
            "{pad}<a class=\"row-title\" href=\"{}\"{described_by}>{title}</a>{linked}",
            edit_link(site, post)
        ));
    } else {
        out.push_str(&format!("{pad}<span>{title}{unlinked}</span>"));
    }
    out.push_str(&post_states_html(site, post, filter));
    if let Some(name) = parent_name {
        out.push_str(&format!(" | Parent Page: {}", esc_html(name.as_bytes())));
    }
    out.push_str("</strong>\n");
    if excerpt && post.post_type != "page" && site.can_post("read_post", post) {
        let text = esc_html(&site.excerpt(post));
        if post.password.is_empty() {
            out.push_str(&text);
        } else {
            out.push_str(&format!(
                "<span class=\"protected-post-excerpt\">{text}</span>"
            ));
        }
    }
    if site.can_post("edit_post", post) {
        out.push_str(&inline_data(site, post));
    }
    out.push_str(&title_actions(site, post, excerpt));
    out.push_str("</th>");
    out
}

/// `handle_row_actions()` for the title column.
fn title_actions(site: &dyn PostsSite, post: &Post, excerpt: bool) -> String {
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
            let preview = preview_link(site, post, &[]);
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
    row_actions(&actions, excerpt)
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
        .userdata(post.author)
        .map(|user| user.display_name)
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

/// A term's name through the admin's `term_name` display filters; their
/// `esc_html()` covers `column_default()`'s.
pub(crate) fn term_name(name: &[u8]) -> String {
    esc_html(&convert_chars(&wptexturize(
        &wp_kses_data(&sanitize_text_field(name)),
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

/// `comments_bubble()`: the approved and `pending` comment counts.
pub(crate) fn comments_bubble(site: &dyn PostsSite, post: &Post, pending: u64) -> String {
    if !site.can_post("edit_post", post)
        && (!post.password.is_empty() || !site.can_post("read_post", post))
    {
        return String::new();
    }
    let approved = u64::try_from(post.comment_count).unwrap_or_default();
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

/// `wp_dropdown_users()` of `users` in the inline editors, `selected` the
/// author query's; with the bulk editor's "no change".
fn authors_dropdown(users: &[User], bulk: bool, chosen_author: &str) -> String {
    let mut out = String::from(
        "<label class=\"inline-edit-author\"><span class=\"title\">Author</span><select name='post_author' class='authors'>\n",
    );
    if bulk {
        out.push_str(&format!(
            "\t<option value='-1'{}>&mdash; No Change &mdash;</option>\n",
            selected_if("-1" == chosen_author)
        ));
    }
    for user in users {
        let label = [&user.display_name[..], b" (", &user.login, b")"].concat();
        out.push_str(&format!(
            "\t<option value='{}'{}>{}</option>\n",
            user.id,
            selected_if(user.id.to_string() == chosen_author),
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

/// The dropdown walkers' attribute of the chosen option.
fn walker_selected(chosen: bool) -> &'static str {
    if chosen { " selected=\"selected\"" } else { "" }
}

/// The published posts of `kind` by order and title, as `wp_dropdown_pages()` asks.
pub(crate) fn published(site: &dyn PostsSite, kind: PostType) -> Vec<Post> {
    let query = PostQuery {
        post_types: vec![kind.name.into()],
        statuses: vec![StatusFilter {
            status: "publish".into(),
            author: None,
        }],
        order: vec![(OrderBy::MenuOrder, true), (OrderBy::Title, true)],
        limit: u32::MAX,
        ..PostQuery::default()
    };
    site.list_posts(&query).0
}

/// `wp_dropdown_pages()`'s options for `pages` as a tree, without pages
/// whose parents are not listed, the page `selected` marked.
pub(crate) fn page_options(pages: &[Post], selected: &str) -> String {
    let mut children: HashMap<u64, Vec<&Post>> = HashMap::new();
    for page in pages {
        children.entry(page.parent).or_default().push(page);
    }
    let mut below = |parent: u64, depth: usize| {
        let pages = children.remove(&parent).unwrap_or_default();
        pages.into_iter().rev().map(move |page| (page, depth))
    };
    let mut stack: Vec<(&Post, usize)> = below(0, 0).collect();
    let mut out = String::new();
    while let Some((page, depth)) = stack.pop() {
        let title = if page.title.is_empty() {
            format!("#{} (no title)", page.id)
        } else {
            text(&page.title)
        };
        out.push_str(&format!(
            "\t<option class=\"level-{depth}\" value=\"{}\"{}>{}{}</option>\n",
            page.id,
            walker_selected(page.id.to_string() == selected),
            "&nbsp;".repeat(depth * 3),
            esc_html(title.as_bytes())
        ));
        stack.extend(below(page.id, depth + 1));
    }
    out
}

/// The parent select of an inline editor over `options`, with the bulk
/// editor's "no change"; none without options.
fn parent_dropdown(options: &str, bulk: bool) -> String {
    if options.is_empty() {
        return String::new();
    }
    let (id, no_change) = if bulk {
        (
            "bulk_edit_post_parent",
            "\t<option value=\"-1\">&mdash; No Change &mdash;</option>\n",
        )
    } else {
        ("post_parent", "")
    };
    format!(
        "<select name='post_parent' id='{id}'>\n{no_change}\t<option value=\"0\">Main Page (no parent)</option>\n{options}</select>\n"
    )
}

/// The template select of an inline editor over `templates`, if any.
fn templates_select(templates: &[(String, String)], bulk: bool) -> String {
    if templates.is_empty() {
        return String::new();
    }
    let options: String = templates
        .iter()
        .map(|(slug, title)| {
            format!(
                "\n\t<option value='{}' >{}</option>",
                esc_attr(slug.as_bytes()),
                esc_html(title.as_bytes())
            )
        })
        .collect();
    format!(
        "\t\t\t\t\t<label>\n\t\t\t\t\t\t<span class=\"title\">Template</span>\n\t\t\t\t\t\t<select name=\"page_template\">\n{}{}<option value=\"default\">Default template</option>\n\t\t\t\t\t\t\t{options}\t\t\t\t\t\t</select>\n\t\t\t\t\t</label>\n\n\t\t\t\t\n",
        if bulk {
            format!(
                "{}<option value=\"-1\">&mdash; No Change &mdash;</option>\n",
                "\t".repeat(14)
            )
        } else {
            String::new()
        },
        "\t".repeat(21)
    )
}

const INLINE_CATEGORIES: &str = include_str!("posts/inline/categories.html");
const INLINE_TAGS: &str = include_str!("posts/inline/tags.html");
const INLINE_PINGS: &str = include_str!("posts/inline/pings.html");
const INLINE_BULK_PINGS: &str = include_str!("posts/inline/bulk-pings.html");
const INLINE_ATTRIBUTES: &str = include_str!("posts/inline/attributes.html");
const INLINE_BULK_ATTRIBUTES: &str = include_str!("posts/inline/bulk-attributes.html");

/// `inline_edit()`: the Quick and Bulk Edit rows of a list of `kind` with
/// `columns` visible columns, dated from `first`, the global post.
fn inline_edit(
    site: &dyn PostsSite,
    kind: PostType,
    first: &Post,
    columns: usize,
    author: &str,
    excerpt: bool,
) -> String {
    let can_publish = site.can(&kind.cap("publish_posts"));
    let others = site.can(&kind.cap("edit_others_posts"));
    let (zone, now) = site.clock();
    let when = |shown: bool, text: &str| {
        if shown {
            text.to_string()
        } else {
            String::new()
        }
    };
    let (authors, bulk_authors) = if others {
        let users = site.authors(&kind.cap("edit_posts"));
        (
            authors_dropdown(&users, false, author),
            authors_dropdown(&users, true, author),
        )
    } else {
        Default::default()
    };
    let posts = kind == POST;
    let formats = if posts { site.post_formats() } else { &[] };
    let (attributes, bulk_attributes) = if kind.hierarchical() {
        let options = page_options(&published(site, kind), "");
        (
            INLINE_ATTRIBUTES.replace("%parent%", &parent_dropdown(&options, false)),
            INLINE_BULK_ATTRIBUTES.replace("%parent%", &parent_dropdown(&options, true)),
        )
    } else {
        Default::default()
    };
    let categories = if posts {
        INLINE_CATEGORIES.replace("%checklist%", &categories_checklist(site))
    } else {
        String::new()
    };
    let templates = site.page_templates(kind.name);
    let sticky = posts && can_publish && others;
    fill(
        INLINE_EDIT,
        &[
            ("type", kind.name.into()),
            ("view", if excerpt { "excerpt" } else { "list" }.into()),
            ("columns", columns.to_string()),
            ("date", touch_time(first, zone, now)),
            ("authors", authors),
            ("bulk_authors", bulk_authors),
            ("password", when(can_publish, PASSWORD)),
            ("categories", categories),
            ("attributes", attributes),
            ("bulk_attributes", bulk_attributes),
            ("templates", templates_select(&templates, false)),
            ("bulk_templates", templates_select(&templates, true)),
            ("tags", when(posts, INLINE_TAGS)),
            ("pings", when(posts, INLINE_PINGS)),
            ("bulk_pings", when(posts, INLINE_BULK_PINGS)),
            ("statuses", status_options(can_publish, false)),
            ("bulk_statuses", status_options(can_publish, true)),
            ("sticky", when(sticky, STICKY)),
            ("bulk_sticky", when(sticky, BULK_STICKY)),
            (
                "formats",
                when(!formats.is_empty(), &formats_select(formats)),
            ),
            ("nonce", site.nonce("inlineeditnonce")),
        ],
    )
}

/// `single_row( $post, $level )` of `columns` but `hidden`, in the view of
/// `filter` and `excerpt`; a tree's rows have a `level`, no pending count.
pub fn row(
    site: &dyn PostsSite,
    post: &Post,
    columns: &[Column],
    hidden: &[&str],
    filter: &str,
    excerpt: bool,
    level: Option<usize>,
) -> String {
    let author = if site.user_id() == post.author {
        "self"
    } else {
        "other"
    };
    let lock = lock_holder(site, post);
    let class = format!(
        "iedit author-{author}{} level-{}",
        if lock.is_some() { " wp-locked" } else { "" },
        ancestors(&|id| site.post(id), post).len()
    );
    let class = site.post_class(&class, post);
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
                    column_cb(site, post, excerpt)
                ));
                continue;
            }
            "title" => {
                out.push_str(&column_title(
                    site,
                    post,
                    &classes,
                    filter,
                    excerpt,
                    (level, lock.as_ref()),
                ));
                continue;
            }
            "author" => column_author(site, post),
            "categories" => column_terms(site, post, "category"),
            "tags" => column_terms(site, post, "post_tag"),
            "comments" => format!(
                "\t\t<div class=\"post-com-count-wrapper\">\n\t\t{}\t\t</div>\n\t\t",
                comments_bubble(
                    site,
                    post,
                    level.map_or_else(|| site.pending_comments(post), |_| 0)
                )
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

/// The columns of a list of `kind` in the view of `filter`, without
/// comments before publishing; a page title's caption says if `ordered`.
pub fn columns(kind: PostType, filter: &str, ordered: bool) -> Vec<Column> {
    let sort = |orderby, desc_first, abbr, caption, initial| {
        Some(Sort {
            orderby,
            desc_first,
            abbr,
            caption,
            initial,
        })
    };
    let column = |key, label: &str, sort| Column {
        key,
        label: label.into(),
        sort,
        primary: key == "title",
    };
    let pages = kind.hierarchical();
    let caption = if pages && !ordered {
        "Table ordered by Hierarchical Menu Order and Title."
    } else {
        "Table ordered by Title."
    };
    let mut columns = vec![
        column("cb", "<input type=\"checkbox\" />", None),
        column(
            "title",
            "Title",
            sort("title", false, "Title", caption, pages.then_some("asc")),
        ),
        column("author", "Author", None),
    ];
    if !pages {
        columns.push(column("categories", "Categories", None));
        columns.push(column("tags", "Tags", None));
    }
    if !["pending", "draft", "future"].contains(&filter) {
        columns.push(column(
            "comments",
            "<span class=\"vers comment-grey-bubble\" title=\"Comments\" aria-hidden=\"true\"></span><span class=\"screen-reader-text\">Comments</span>",
            sort("comment_count", false, "Comments", "Table ordered by Comments.", None),
        ));
    }
    columns.push(column(
        "date",
        "Date",
        sort(
            "date",
            true,
            "Date",
            "Table ordered by Date.",
            (!pages).then_some("desc"),
        ),
    ));
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
#[derive(Default)]
struct Listing {
    kind: PostType,
    posts: Vec<Post>,
    /// The level of each post of a tree, none for a flat list.
    levels: Option<Vec<usize>>,
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
    /// The user's own posts are listed.
    mine: bool,
    /// `$_GET['all_posts']`, `$_GET['show_sticky']` and `$_GET['author']`.
    all_posts: bool,
    show_sticky: bool,
    author: String,
    author_query: String,
    /// `$_GET['s']`, if given.
    search: Option<String>,
    /// `get_user_setting( 'posts_list_mode', 'list' )`.
    mode: String,
    /// The filters' choices: `m`, `post_format` and `cat`.
    m: String,
    post_format: String,
    cat: String,
    /// The notice of a bulk action, empty for none.
    notice: String,
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
    let kind = listing.kind.name;
    let mut links = vec![View {
        class: "all".into(),
        url: if mine {
            format!("edit.php?post_type={kind}&all_posts=1")
        } else {
            format!("edit.php?post_type={kind}")
        },
        label: label("All", all_total),
        current: listing.base && !listing.own_only || listing.all_posts,
    }];
    if mine {
        links.push(View {
            class: "mine".into(),
            url: format!("edit.php?post_type={kind}&author={}", listing.user),
            label: label("Mine", listing.own),
            current: listing.mine,
        });
    }
    for (status, one, many) in STATUSES {
        let n = count(status);
        if n == 0 {
            continue;
        }
        links.push(View {
            class: status.into(),
            url: format!("edit.php?post_status={status}&post_type={kind}"),
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
                url: format!("edit.php?post_type={kind}&show_sticky=1"),
                label: label("Sticky", listing.sticky),
                current: listing.show_sticky,
            },
        );
    }
    links
}

/// The options of `wp_dropdown_categories()` for `terms` as a tree, the
/// term `selected` marked.
pub(crate) fn term_options(terms: &[Term], selected: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    walk(terms, true, &mut |event| match event {
        Walk::Start(term) => out.push_str(&format!(
            "\t<option class=\"level-{depth}\" value=\"{}\"{}>{}{}</option>\n",
            term.id,
            walker_selected(term.id.to_string() == selected),
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
    out
}

/// `wp_dropdown_categories()` of the category filter, all of them by name,
/// `selected` the `cat` query var.
fn categories_dropdown(site: &dyn PostsSite, selected: &str) -> String {
    let terms = site.terms("category");
    let mut out = String::from(
        "<label class=\"screen-reader-text\" for=\"cat\">Filter by category</label><select  name='cat' id='cat' class='postform'>\n",
    );
    // The default category is always there.
    let all = selected_if(selected == "0");
    out.push_str(&format!(
        "\t<option value='0'{all}>All Categories</option>\n"
    ));
    out.push_str(&term_options(&terms, selected));
    out.push_str("</select>\n");
    out
}

/// `formats_dropdown()`: the formats some post has, standard left out,
/// `selected` the requested one.
fn formats_dropdown(site: &dyn PostsSite, chosen_format: &str) -> String {
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
                "\t\t\t\t<option{} value=\"{}\">{}</option>\n\t\t\t\t",
                selected_if(slug == chosen_format),
                esc_attr(slug.as_bytes()),
                esc_html(post_format_string(slug.as_bytes()).as_bytes())
            )
        })
        .collect();
    format!(
        "\t\t<label for=\"filter-by-format\" class=\"screen-reader-text\">\n\t\t\tFilter by post format\t\t</label>\n\t\t<select name=\"post_format\" id=\"filter-by-format\">\n\t\t\t<option{} value=\"\">All formats</option>\n\t\t\t{options}\t\t</select>\n\t\t",
        selected_if(chosen_format.is_empty())
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
    let kind = listing.kind;
    if site.can(&kind.cap("delete_posts")) {
        actions.push(if listing.trash() {
            ("delete", "Delete permanently")
        } else {
            ("trash", "Move to Trash")
        });
    }
    let empty = listing.posts.is_empty();
    let hidden = if empty { " hidden" } else { "" };
    let mut extra = String::new();
    if which == "top" {
        extra.push_str(&months_dropdown(months, &listing.m, "Filter by date"));
        if kind == POST {
            extra.push_str(&categories_dropdown(site, &listing.cat));
            if !listing.trash() {
                extra.push_str(&formats_dropdown(site, &listing.post_format));
            }
        }
        if !extra.is_empty() {
            extra.push_str("<input type=\"submit\" name=\"filter_action\" id=\"post-query-submit\" class=\"button button-compact\" value=\"Filter\"  />");
        }
    }
    if listing.trash() && !empty && site.can(&kind.cap("edit_others_posts")) {
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
    (url, origin): (&str, &str),
    request_uri: &str,
) -> String {
    let kind = listing.kind;
    let columns = columns(kind, &listing.filter, listing.orderby.is_some());
    let months = site.months(kind.name, listing.trash());
    let extended = listing.mode == "excerpt";
    let rows: String = listing
        .posts
        .iter()
        .enumerate()
        .map(|(i, post)| {
            let level = listing
                .levels
                .as_ref()
                .and_then(|levels| levels.get(i).copied());
            let row = row(
                site,
                post,
                &columns,
                hidden,
                &listing.filter,
                extended,
                level,
            );
            match level {
                Some(_) => format!("\t{row}"),
                None => row,
            }
        })
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
    // `edit.php` removes its notices' arguments from the request first.
    let request_uri = text(&remove_query_arg(&NOTICE_ARGS, request_uri.as_bytes()));
    let unpaged = crate::list_table::columns_url(origin, &request_uri);
    let kept =
        |value: &Option<String>| text(&addslashes(value.as_deref().unwrap_or_default().as_bytes()));
    let order_fields = [
        ("orderby", kept(&listing.orderby)),
        ("order", kept(&listing.order)),
    ];
    let order_fields: Vec<(&str, &str)> = order_fields
        .iter()
        .map(|(name, value)| (*name, value.as_str()))
        .collect();
    let new_post = if kind == POST {
        "post-new.php".to_string()
    } else {
        format!("post-new.php?post_type={}", kind.name)
    };
    let mut out = format!(
        "<div class=\"wrap\">\n<h1 class=\"wp-heading-inline\">\n{}</h1>\n\n <a href=\"{}\" class=\"page-title-action\">Add {}</a>{}\n<hr class=\"wp-header-end\">\n\n{}\n{}\n<form id=\"posts-filter\" method=\"get\">\n\n{}\n<input type=\"hidden\" name=\"post_status\" class=\"post_status_page\" value=\"{}\" />\n<input type=\"hidden\" name=\"post_type\" class=\"post_type_page\" value=\"{}\" />\n\n{}\n{}\n",
        kind.plural,
        text(&esc_url(BarSite::admin_url(site, &new_post).as_bytes())),
        kind.singular,
        // `get_search_query()`, the query var without line breaks.
        match listing.search.as_deref().filter(|s| !s.is_empty()) {
            Some(search) => format!(
                "<span class=\"subtitle\">Search results for: <strong>{}</strong></span>",
                // `parse_query()` empties a search over 1600 bytes.
                if search.len() > 1600 {
                    String::new()
                } else {
                    esc_attr(search.replace(['\r', '\n'], "").as_bytes())
                }
            ),
            None => String::new(),
        },
        listing.notice,
        views(&format!("Filter {} list", kind.lower()), &links),
        if listing.posts.is_empty()
            && listing
                .search
                .as_deref()
                .is_none_or(|s| is_empty_string(s.as_bytes()))
        {
            String::new()
        } else {
            let search = listing.search.as_deref().unwrap_or_default();
            let label = format!("Search {}", kind.plural);
            search_box(&label, "post", search, &order_fields)
        },
        if is_empty_string(listing.filter.as_bytes()) {
            "all".to_string()
        } else {
            esc_attr(&addslashes(listing.filter.as_bytes()))
        },
        kind.name,
        if is_empty_string(listing.author.as_bytes()) {
            String::new()
        } else {
            format!(
                "<input type=\"hidden\" name=\"author\" value=\"{}\" />\n",
                esc_attr(&addslashes(listing.author.as_bytes()))
            )
        },
        if listing.show_sticky {
            "<input type=\"hidden\" name=\"show_sticky\" value=\"1\" />\n"
        } else {
            ""
        }
    );
    out.push_str(&wp_nonce_field(
        site,
        "bulk-posts",
        "_wpnonce",
        &request_uri,
    ));
    let empty = listing.posts.is_empty();
    let heading = format!("{} list navigation", kind.plural);
    let pages = |which| {
        pagination(
            which,
            &heading,
            listing.total,
            listing.pages,
            listing.page,
            url,
        )
    };
    out.push_str(&tablenav(site, listing, "top", &months, pages("top")));
    let rows = if empty {
        let in_trash = if listing.trash() { " in Trash" } else { "" };
        format!(
            "<tr class=\"no-items\"><td class=\"colspanchange\" colspan=\"{visible}\">No {} found{in_trash}.</td></tr>",
            kind.lower()
        )
    } else {
        rows
    };
    out.push_str(&format!(
        "<h2 class='screen-reader-text'>{} list</h2><table class=\"wp-list-table widefat fixed striped {} {}\">\n\t\t{}\t<thead>\n\t<tr>\n\t\t{}\t</tr>\n\t</thead>\n\n\t<tbody id=\"the-list\"\n\t\t\t\t>\n\t\t{rows}\t</tbody>\n\n\t<tfoot>\n\t<tr>\n\t\t{}\t</tr>\n\t</tfoot>\n\n</table>\n\t\t",
        kind.plural,
        esc_attr(format!("table-view-{}", listing.mode).as_bytes()),
        kind.lower(),
        table_description(&columns, &order),
        column_headers(&columns, hidden, &order, &unpaged, 1, true),
        column_headers(&columns, hidden, &order, &unpaged, 2, false)
    ));
    if !empty {
        out.push_str(&tablenav(site, listing, "bottom", &months, pages("bottom")));
    }
    out.push_str("\n</form>\n\n");
    if let Some(first) = listing.posts.first() {
        out.push_str(&format!(
            "\n{}\t\t",
            inline_edit(site, kind, first, visible, &listing.author_query, extended)
        ));
    }
    out.push_str("\n<div id=\"ajax-response\"></div>\n<div class=\"clear\"></div>\n</div>\n\n");
    out
}

const HELP_OVERVIEW: &str = "<p>This screen provides access to all of your posts. You can customize the display of this screen to suit your workflow.</p>";

const HELP_SCREEN_CONTENT: &str = "<p>You can customize the display of this screen&#8217;s contents in a number of ways:</p><ul><li>You can hide/display columns based on your needs and decide how many posts to list per screen using the Screen Options tab.</li><li>You can filter the list of posts by post status using the text links above the posts list to only show posts with that status. The default view is to show all posts.</li><li>You can view posts in a simple title list or with an excerpt using the Screen Options tab.</li><li>You can refine the list to show only posts in a specific category or from a specific month by using the dropdown menus above the posts list. Click the Filter button after making your selection. You also can refine the list by clicking on the post author, category or tag in the posts list.</li></ul>";

const HELP_ACTION_LINKS: &str = "<p>Hovering over a row in the posts list will display action links that allow you to manage your post. You can perform the following actions:</p><ul><li><strong>Edit</strong> takes you to the editing screen for that post. You can also reach that screen by clicking on the post title.</li><li><strong>Quick Edit</strong> provides inline access to the metadata of your post, allowing you to update post details without leaving this screen.</li><li><strong>Trash</strong> removes your post from this list and places it in the Trash, from which you can permanently delete it.</li><li><strong>Preview</strong> will show you what your draft post will look like if you publish it. View will take you to your live site to view the post. Which link is available depends on your post&#8217;s status.</li></ul>";

const HELP_BULK_ACTIONS: &str = "<p>You can also edit or move multiple posts to the Trash at once. Select the posts you want to act on using the checkboxes, then select the action you want to take from the Bulk actions menu and click Apply.</p><p>When using Bulk Edit, you can change the metadata (categories, author, etc.) for all selected posts at once. To remove a post from the grouping, just click the <span class=\"dashicons dashicons-dismiss\" aria-hidden=\"true\" style=\"font-size: 16px; width: 16px; vertical-align: middle;\"></span><span class=\"screen-reader-text\">remove</span> button next to its name in the Bulk Edit area that appears.</p>";

const HELP_SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/posts-screen/\">Documentation on Managing Posts</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

const HELP_PAGES_OVERVIEW: &str = "<p>Pages are similar to posts in that they have a title, body text, and associated metadata, but they are different in that they are not part of the chronological blog stream, kind of like permanent posts. Pages are not categorized or tagged, but can have a hierarchy. You can nest pages under other pages by making one the &#8220;Parent&#8221; of the other, creating a group of pages.</p>";

const HELP_MANAGING_PAGES: &str = "<p>Managing pages is very similar to managing posts, and the screens can be customized in the same way.</p><p>You can also perform the same types of actions, including narrowing the list by using the filters, acting on a page using the action links that appear when you hover over a row, or using the Bulk actions menu to edit the metadata for multiple pages at once.</p>";

const HELP_PAGES_SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/pages-screen/\">Documentation on Managing Pages</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// `edit.php`'s help tabs for `kind`, and its sidebar.
fn help(kind: PostType) -> (Vec<HelpTab<'static>>, &'static str) {
    let tab = |id, title, content| HelpTab { id, title, content };
    if kind.hierarchical() {
        return (
            vec![
                tab("overview", "Overview", HELP_PAGES_OVERVIEW),
                tab("managing-pages", "Managing Pages", HELP_MANAGING_PAGES),
            ],
            HELP_PAGES_SIDEBAR,
        );
    }
    (
        vec![
            tab("overview", "Overview", HELP_OVERVIEW),
            tab("screen-content", "Screen Content", HELP_SCREEN_CONTENT),
            tab("action-links", "Available Actions", HELP_ACTION_LINKS),
            tab("bulk-actions", "Bulk actions", HELP_BULK_ACTIONS),
        ],
        HELP_SIDEBAR,
    )
}

const VIEW_MODE: &str = include_str!("posts/view-mode.html");

/// The screen options' view mode fields, `mode` checked.
pub(crate) fn view_mode_fields(mode: &str) -> String {
    let checked = |value: &str| {
        if value == mode {
            " checked='checked'"
        } else {
            ""
        }
    };
    VIEW_MODE
        .replace("%list%", checked("list"))
        .replace("%excerpt%", checked("excerpt"))
}

/// `render_screen_options()`: the columns, `hidden` ones unchecked, the
/// number of posts of `kind` per page and the view `mode`.
fn screen_options(
    site: &dyn PostsSite,
    kind: PostType,
    columns: &[Column],
    hidden: &[String],
    per_page: u64,
    mode: &str,
) -> String {
    let view_mode = view_mode_fields(mode);
    list_screen_options(
        columns,
        hidden,
        &format!("edit_{}_per_page", kind.name),
        per_page,
        &view_mode,
        site.nonce("screen-options-nonce"),
    )
}

/// `get_hidden_columns()`: the columns of screen `screen` the user hid,
/// none by default.
pub(crate) fn hidden_columns(site: &dyn PostsSite, screen: &str) -> Vec<String> {
    let stored = site
        .user_option(&format!("manage{screen}columnshidden"))
        .unwrap_or_default();
    match maybe_unserialize(&stored) {
        Maybe::Value(Value::Array(items)) => items
            .iter()
            .filter_map(|(_, value)| value.as_bytes().map(text))
            .collect(),
        _ => Vec::new(),
    }
}

/// The number per page the user chose as `option`, else `default`.
pub(crate) fn per_page(site: &dyn PostsSite, option: &str, default: u64) -> u64 {
    let stored = Value::String(site.user_option(option).unwrap_or_default()).to_int();
    u64::try_from(stored)
        .ok()
        .filter(|n| *n > 0)
        .unwrap_or(default)
}

/// The only author whose private posts of `kind` the user may read, unless
/// they may read everyone's.
fn private_author(site: &dyn PostsSite, kind: PostType) -> Option<u64> {
    (!site.can(&kind.cap("read_private_posts"))).then(|| site.user_id())
}

/// The statuses the All list shows the user: private ones only their own
/// unless they may read others'.
fn all_statuses(site: &dyn PostsSite, kind: PostType) -> Vec<StatusFilter> {
    let private_author = private_author(site, kind);
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
fn filtered(site: &dyn PostsSite, kind: PostType, filter: &str) -> Vec<StatusFilter> {
    let registered =
        STATUSES.iter().any(|(status, ..)| *status == filter) || NOT_IN_ALL.contains(&filter);
    if !registered {
        return all_statuses(site, kind);
    }
    vec![StatusFilter {
        status: filter.into(),
        author: private_author(site, kind).filter(|_| filter == "private"),
    }]
}

/// `wp_edit_posts_query()`'s `orderby`, drafts and pending by their change,
/// else a tree of all pages by order and title, which it says.
fn list_orderby(kind: PostType, args: &ListArgs) -> (String, bool) {
    let changed = ["draft", "pending"].contains(&args.status.as_str());
    let orderby = match &args.orderby {
        Some(orderby) => orderby.as_str(),
        None if changed => "modified",
        None => "",
    };
    if kind.hierarchical() && is_empty_string(orderby.as_bytes()) {
        return ("menu_order title".into(), true);
    }
    (orderby.into(), false)
}

/// The order of `orderby` through `WP_Query`: pending posts oldest first
/// and a `tree` ascending unless asked otherwise.
fn sort(args: &ListArgs, query: &PostQuery, orderby: &str, tree: bool) -> Vec<(OrderBy, bool)> {
    let ascending = match &args.order {
        _ if tree => true,
        Some(order) => order.eq_ignore_ascii_case("asc"),
        None => args.status == "pending",
    };
    let mut order = order_by(&Value::String(orderby.into()), ascending, query);
    // A search with a term not left out is ordered by relevance first,
    // unless another order is asked.
    let titled = query.search.iter().any(|term| !term.starts_with('-'));
    if titled && !query.exact && (is_empty_string(orderby.as_bytes()) || orderby == "relevance") {
        order.insert(0, (OrderBy::Relevance, false));
    }
    order
}

/// `_display_rows_hierarchical()`: page `page` of a tree of `pages`, a
/// subtree after its parents, orphans last; flat for a search.
fn tree_rows(
    site: &dyn PostsSite,
    pages: Vec<Post>,
    page: u64,
    per_page: u64,
    search: bool,
) -> Vec<(Post, usize)> {
    let start = (page - 1).saturating_mul(per_page);
    let mut rows = Tree {
        site,
        start,
        end: start.saturating_add(per_page),
        count: 0,
        rows: Vec::new(),
        children: HashMap::new(),
    };
    if search {
        for page in pages {
            rows.add(page, 0);
        }
        return rows.rows;
    }
    let mut parents = Vec::new();
    let mut top = Vec::new();
    for page in pages.into_iter().map(repaired) {
        if page.parent > 0 {
            if !rows.children.contains_key(&page.parent) {
                parents.push(page.parent);
            }
            rows.children.entry(page.parent).or_default().push(page);
        } else {
            top.push(page);
        }
    }
    for page in top {
        let id = page.id;
        if !rows.add(page, 0) {
            break;
        }
        rows.children_of(id, 1);
    }
    for parent in parents {
        for orphan in rows.children.remove(&parent).unwrap_or_default() {
            if !rows.add(orphan, 0) {
                break;
            }
        }
    }
    rows.rows
}

/// `page` as a tree shows it: a page that is its own parent is a top one,
/// which PHP also writes.
fn repaired(mut page: Post) -> Post {
    if page.parent == page.id {
        page.parent = 0;
    }
    page
}

/// A tree of pages being paged: the rows on the page and the pages left.
struct Tree<'a> {
    site: &'a dyn PostsSite,
    start: u64,
    end: u64,
    count: u64,
    rows: Vec<(Post, usize)>,
    children: HashMap<u64, Vec<Post>>,
}

impl Tree<'_> {
    /// Counts `page` at `level`, a row if on the page; false past its end.
    fn add(&mut self, page: Post, level: usize) -> bool {
        if self.count >= self.end {
            return false;
        }
        if self.count >= self.start {
            self.rows.push((page, level));
        }
        self.count += 1;
        true
    }

    /// `_page_rows()`: the children of `parent` at `level`, and theirs,
    /// depth first without recursing.
    fn children_of(&mut self, parent: u64, level: usize) {
        let mut stack = vec![(self.take(parent), level)];
        while let Some((siblings, level)) = stack.last_mut() {
            let level = *level;
            let Some(page) = siblings.next() else {
                stack.pop();
                continue;
            };
            if self.count == self.start {
                let ancestors = ancestors(&|id| self.site.post(id), &page);
                let above = ancestors.len();
                for (i, id) in ancestors.iter().rev().enumerate() {
                    if let Some(ancestor) = self.site.post(*id).map(repaired) {
                        self.rows
                            .push((ancestor, (level + i).saturating_sub(above)));
                    }
                }
            }
            let id = page.id;
            if !self.add(page, level) {
                // Past the page's end nothing more is listed.
                break;
            }
            stack.push((self.take(id), level + 1));
        }
    }

    /// The children of `parent` not yet listed, which are then no orphans.
    fn take(&mut self, parent: u64) -> std::vec::IntoIter<Post> {
        self.children
            .remove(&parent)
            .unwrap_or_default()
            .into_iter()
    }
}

/// The posts list's query arguments this port reads.
#[derive(Default)]
pub struct ListArgs {
    /// `$_GET['post_type']`, posts for none.
    pub post_type: PostType,
    /// `$_GET['post_status']`, empty for none.
    pub status: String,
    /// `$_GET['orderby']`, if given.
    pub orderby: Option<String>,
    /// `$_GET['order']`, if given.
    pub order: Option<String>,
    /// `$_GET['paged']`, if given.
    pub paged: Option<String>,
    /// `$_GET['s']`, if given.
    pub search: Option<String>,
    /// `$_GET['mode']`, if given.
    pub mode: Option<String>,
    /// The user's `wp-settings-{ID}` cookie, if sent.
    pub settings: Option<String>,
    /// `$_GET['author']`, if given.
    pub author: Option<String>,
    /// `$_GET['all_posts']`, if given.
    pub all_posts: Option<String>,
    /// `$_GET['show_sticky']` is not empty.
    pub show_sticky: bool,
    /// `$_GET['m']`, empty for none.
    pub m: String,
    /// `$_GET['post_format']`, empty for none.
    pub post_format: String,
    /// The `cat` query var after `WP_Query` cleaned it, empty for none.
    pub cat: String,
    /// The term and date filters of `WP_Query` for the request.
    pub filters: PostQuery,
    /// `is_base_request()`: no arguments but `paged` and the post type.
    pub base: bool,
    /// The counts of a bulk action, as `BULK` names them, if given.
    pub counts: [Option<String>; 5],
    /// `$_REQUEST['ids']`, the posts acted on, if given.
    pub ids: Option<String>,
}

/// The counts `edit.php` is sent back with after a bulk action, in the
/// order its notice reports them.
pub const BULK: [&str; 5] = ["updated", "locked", "deleted", "trashed", "untrashed"];

/// The arguments `edit.php` removes from the request after its notice.
const NOTICE_ARGS: [&[u8]; 6] = [
    b"locked",
    b"skipped",
    b"updated",
    b"deleted",
    b"trashed",
    b"untrashed",
];

/// `edit.php`'s bulk action notice, empty for none, `first` the global
/// post; `None` for an Edit link to a post type not ported.
fn bulk_notice(site: &dyn PostsSite, args: &ListArgs, first: Option<&Post>) -> Option<String> {
    let kind = args.post_type;
    let one = kind.singular.to_ascii_lowercase();
    let mut messages = Vec::new();
    for (name, count) in BULK.into_iter().zip(&args.counts) {
        let count = count
            .as_deref()
            .map_or(0, |count| absint(&Value::String(count.into())));
        if count == 0 {
            continue;
        }
        let noun = if count == 1 {
            one.clone()
        } else {
            kind.lower()
        };
        let n = number_format(count);
        messages.push(match name {
            "updated" => format!("{n} {noun} updated."),
            "locked" if count == 1 => format!("{n} {noun} not updated, somebody is editing it."),
            "locked" => format!("{n} {noun} not updated, somebody is editing them."),
            "deleted" => format!("{n} {noun} permanently deleted."),
            "trashed" => format!("{n} {noun} moved to the Trash."),
            _ => format!("{n} {noun} restored from the Trash."),
        });
        let Some(ids) = args.ids.as_deref() else {
            continue;
        };
        if name == "trashed" {
            let ids: String = ids
                .chars()
                .filter(|c| c.is_ascii_digit() || *c == ',')
                .collect();
            let url = format!(
                "edit.php?post_type={}&doaction=undo&action=untrash&ids={ids}",
                kind.name
            );
            let url = crate::list_table::nonce_url(&url);
            messages.push(format!("<a href=\"{url}\">Undo</a>"));
        } else if name == "untrashed" && !ids.contains(',') {
            // `get_post( $ids[0] )`: the global post for an empty ID.
            let id = Value::String(ids.into());
            let post = if id.is_empty() {
                first.cloned()
            } else if id.is_numeric() {
                // No post has an ID below 1.
                u64::try_from(id.to_int()).ok().and_then(|id| site.post(id))
            } else {
                None
            };
            if let Some(post) = post.filter(|post| site.can_post("edit_post", post)) {
                let label = PostType::named(post.post_type.as_bytes())?.singular;
                let link = format!("post.php?post={}&amp;action=edit", post.id);
                let link = text(&esc_url(BarSite::admin_url(site, &link).as_bytes()));
                messages.push(format!("<a href=\"{link}\">Edit {label}</a>"));
            }
        }
    }
    if messages.is_empty() {
        return Some(String::new());
    }
    let message = messages.join(" ");
    let nonce = site.nonce("bulk-posts");
    Some(wp_admin_notice(
        &message,
        "message",
        "is-dismissible updated",
        true,
        &nonce,
    ))
}

/// `WP_Query`'s cleaning of `author` and `cat`: their digits, commas and
/// minus signs.
pub fn kept_ids(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_ascii_digit() || *c == ',' || *c == '-')
        .collect()
}

/// The sorted integers of a cleaned `author` or `cat`.
pub fn sorted_ids(kept: &str) -> Vec<i64> {
    let mut ids: Vec<i64> = kept
        .split(',')
        .map(|id| Value::String(id.as_bytes().to_vec()).to_int())
        .collect();
    ids.sort_unstable();
    ids
}

/// `author__in` and `author__not_in` of `ids`: any ID not above 0 leaves
/// out its posts, and then no others are listed alone.
pub(crate) fn author_lists(ids: &[i64]) -> (Vec<u64>, Vec<u64>) {
    let list = |positive: bool| -> Vec<u64> {
        ids.iter()
            .filter(|id| (**id > 0) == positive)
            .map(|id| id.unsigned_abs())
            .collect()
    };
    match list(false) {
        excluded if excluded.is_empty() => (list(true), Vec::new()),
        excluded => (Vec::new(), excluded),
    }
}

/// `WP_Query`'s `author` IDs, each once: none for an empty or zero value.
pub(crate) fn author_ids(raw: &str) -> Option<Vec<i64>> {
    let kept = kept_ids(raw);
    let value = Value::String(kept.clone().into_bytes());
    if is_empty_string(kept.as_bytes()) || value.is_numeric() && value.to_float() == 0.0 {
        return None;
    }
    let mut ids = sorted_ids(&kept);
    ids.dedup();
    Some(ids)
}

/// What a screen answers: its page, or a redirect to the URL given.
pub enum Answer {
    /// The page.
    Page(String),
    /// A `302` to this URL.
    Redirect(String),
    /// `wp_die()`'s `403` with this message.
    Denied(&'static str),
    /// `wp_die()`'s default `500` with this message.
    Died(&'static str),
    /// `wp_die()`'s `404` with this message.
    Missing(&'static str),
}

impl Answer {
    /// A screen's page, or `wp_die()`'s refusal from
    /// `wp-admin/includes/menu.php` when the menu turns the user away.
    pub fn from_page(page: Option<String>) -> Answer {
        page.map_or(
            Answer::Denied("Sorry, you are not allowed to access this page."),
            Answer::Page,
        )
    }
}

/// `prepare_items()`'s view mode: a non-empty request's, which PHP saves
///, else the user's setting.
pub(crate) fn view_mode(
    site: &dyn PostsSite,
    mode: Option<&str>,
    settings: Option<&str>,
) -> String {
    match mode.filter(|mode| !is_empty_string(mode.as_bytes())) {
        Some("excerpt") => "excerpt".into(),
        Some(_) => "list".into(),
        None => user_setting(site, settings, "posts_list_mode").unwrap_or_else(|| "list".into()),
    }
}

/// `edit.php` for `args`: the list, a redirect from a page past the last or
/// a refusal; `None` for an Edit link in its notice not ported.
pub fn edit(
    site: &dyn PostsSite,
    args: &ListArgs,
    canonical: &str,
    origin: &str,
    request_uri: &str,
    browser: &Browser,
) -> Option<Answer> {
    let kind = args.post_type;
    if !site.can(&kind.cap("edit_posts")) {
        return Some(Answer::from_page(None));
    }
    let own = site.own_posts(kind.name);
    let filter = args.status.as_str();
    let author = args.author.as_deref().unwrap_or_default();
    let own_only = own > 0
        && !site.can(&kind.cap("edit_others_posts"))
        && [filter, author]
            .iter()
            .all(|arg| is_empty_string(arg.as_bytes()))
        && args
            .all_posts
            .as_deref()
            .is_none_or(|all| is_empty_string(all.as_bytes()))
        && !args.show_sticky;
    let per_page = per_page(site, &format!("edit_{}_per_page", kind.name), 20);
    let author_ids = author_ids(author);
    let (authors, excluded_authors) = if own_only {
        (vec![site.user_id()], Vec::new())
    } else {
        author_lists(&author_ids.clone().unwrap_or_default())
    };
    let sticky_ids = sticky_posts(&site.option("sticky_posts").unwrap_or_default());
    let (orderby, tree) = list_orderby(kind, args);
    let mut query = PostQuery {
        post_types: vec![kind.name.into()],
        statuses: filtered(site, kind, filter),
        authors,
        excluded_authors,
        limit: if tree {
            u32::MAX
        } else {
            u32::try_from(per_page).unwrap_or(u32::MAX)
        },
        ..args.filters.clone()
    };
    if args.show_sticky && !sticky_ids.is_empty() {
        query.include = Some(sticky_ids.clone());
    }
    query.order = sort(args, &query, &orderby, tree);
    let requested = crate::list_table::pagenum(args.paged.as_deref());
    if !tree {
        query.offset = u32::try_from((requested - 1).saturating_mul(per_page)).unwrap_or(u32::MAX);
    }
    let (posts, found) = site.list_posts(&query);
    // A tree's pages are counted as listed.
    let display = kind.hierarchical() && orderby == "menu_order title";
    let total = if display { posts.len() as u64 } else { found };
    let pages = total.div_ceil(per_page);
    if pages > 0 && requested > pages {
        let page = pages.to_string();
        let to = add_query_arg(&[(b"paged", Some(page.as_bytes()))], request_uri.as_bytes());
        return Some(Answer::Redirect(text(&to)));
    }
    let notice = bulk_notice(site, args, posts.first())?;
    let (posts, levels) = if display {
        let searched = args
            .search
            .as_deref()
            .is_some_and(|s| !is_empty_string(s.as_bytes()));
        let rows = tree_rows(site, posts, requested, per_page, searched);
        let (posts, levels) = rows.into_iter().unzip();
        (posts, Some(levels))
    } else {
        (posts, None)
    };
    let listing = Listing {
        kind,
        posts,
        levels,
        total,
        counts: site.status_counts(kind.name, private_author(site, kind)),
        own,
        sticky: if sticky_ids.is_empty() || kind != POST {
            0
        } else {
            site.sticky_count(kind.name, &sticky_ids)
        },
        own_only,
        user: site.user_id(),
        filter: filter.into(),
        base: args.base,
        page: requested,
        pages,
        orderby: args.orderby.clone(),
        order: args.order.clone(),
        mine: own_only || Value::String(author.into()).to_int() == site.user_id() as i64,
        all_posts: args.all_posts.is_some(),
        show_sticky: args.show_sticky,
        author: author.into(),
        // `get_query_var( 'author' )` of an author query, which the authors
        // menu chooses.
        author_query: author_ids.map_or_else(String::new, |ids| {
            let ids: Vec<String> = ids.iter().map(i64::to_string).collect();
            ids.join(",")
        }),
        m: args.m.clone(),
        post_format: args.post_format.clone(),
        cat: args.cat.clone(),
        search: args.search.clone(),
        mode: view_mode(site, args.mode.as_deref(), args.settings.as_deref()),
        notice,
    };
    let hidden = hidden_columns(site, &format!("edit-{}", kind.name));
    let hidden_keys: Vec<&str> = hidden.iter().map(String::as_str).collect();
    let content = list_content(
        site,
        &listing,
        &hidden_keys,
        (canonical, origin),
        request_uri,
    );
    let columns = columns(kind, filter, args.orderby.is_some());
    let screen_options = screen_options(site, kind, &columns, &hidden, per_page, &listing.mode);
    let (help, sidebar) = help(kind);
    let file = if kind == POST {
        "edit.php".to_string()
    } else {
        format!("edit.php?post_type={}", kind.name)
    };
    let id = format!("edit-{}", kind.name);
    let page = Page {
        screen: Screen {
            pagenow: "edit.php",
            typenow: kind.name,
            taxonomy: "",
            parent_file: Some(&file),
            submenu_file: Some(&file),
        },
        id: &id,
        title: kind.plural,
        help: &help,
        sidebar,
        screen_options: &screen_options,
        styles: &[],
        scripts: &["inline-edit-post", "heartbeat"],
        late_scripts: &[],
        script_data: &[],
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical,
        origin,
        view: None,
    };
    admin_page(site, &page, browser).map(Answer::Page)
}

#[cfg(test)]
pub(crate) mod content;
#[cfg(test)]
pub(crate) mod tests;
