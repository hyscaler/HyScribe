// SPDX-License-Identifier: AGPL-3.0-or-later

//! `comment.php?action=editcomment`: `wp-admin/edit-form-comment.php`.

use hyscribe_kses::{esc_attr, esc_html, esc_textarea, esc_url};
use hyscribe_phpser::{add_query_arg, urlencode};
use hyscribe_render::comment_author_of;
use hyscribe_render::date::{self, Zone};
use hyscribe_render::post::admin_title;

use super::{CommentQuery, CommentsSite, comment_order, comment_types, editor_css};
use crate::admin_bar::{BarSite, html_excerpt};
use crate::dashboard::RecentComment;
use crate::list_table::{fill, wp_nonce_field};
use crate::menu::{Screen, link};
use crate::page::{HelpTab, Page, Request, admin_page};
use crate::posts::{Answer, touch_time};
use crate::text;

const PAGE: &str = include_str!("edit.html");

const HELP: [HelpTab; 1] = [HelpTab {
    id: "overview",
    title: "Overview",
    content: "<p>You can edit the information left in a comment if needed. This is often useful when you notice that a commenter has made a typographical error.</p><p>You can also moderate the comment from this screen using the Status box, where you can also change the timestamp of the comment.</p>",
}];

const SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/comments-screen/\">Documentation on Comments</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// What the edit screen reads besides the comments list's.
pub trait CommentEditSite: CommentsSite {
    /// The comment `id` with what its row shows, `None` without it.
    fn comment_to_edit(&self, id: u64) -> Option<RecentComment>;
    /// `wp_get_referer()`.
    fn referer(&self) -> Option<Vec<u8>>;
}

/// `get_comment_excerpt()` cut by `wp_html_excerpt()` for the parent
/// dropdown.
fn option_label(row: &RecentComment) -> String {
    let content: Vec<u8> = (row.comment.content.iter())
        .map(|&b| if b == b'\n' || b == b'\r' { b' ' } else { b })
        .collect();
    let words =
        hyscribe_render::wp_trim_words(&hyscribe_kses::strip_tags(&content), 20, b"&hellip;");
    let author = text(&comment_author_of(&row.comment, row.user.as_ref()));
    format!("{author}: {}", html_excerpt(&text(&words), 50, "\u{2026}"))
}

/// `edit-form-comment.php`'s parent dropdown: the post's comments the
/// comment may move under, as threading and its depth allow.
fn parent_edit(site: &dyn CommentEditSite, row: &RecentComment) -> String {
    let comment = &row.comment;
    // Approved comments need publicly visible parents.
    let statuses: &[&str] = if row.status == "approved" {
        &["1"]
    } else {
        &["1", "0"]
    };
    let order = comment_order("comment_date_gmt", "DESC").unwrap_or_default();
    let (mut listed, _) = site.list_comments(&CommentQuery {
        approved: statuses,
        user: None,
        types: &comment_types("comment"),
        search: None,
        order: &order,
        post: comment.post_id,
        offset: 0,
        number: 100,
    });
    listed.reverse();
    let parent_of = |id: u64| {
        listed
            .iter()
            .find(|c| c.comment.id == id)
            .map(|c| c.comment.parent)
    };
    // The comment's descendants among them, and the height of its subtree.
    let (mut below, mut height, mut level) = (Vec::new(), 1, vec![comment.id]);
    loop {
        let next: Vec<u64> = listed
            .iter()
            .filter(|c| level.contains(&c.comment.parent))
            .map(|c| c.comment.id)
            .collect();
        if next.is_empty() {
            break;
        }
        height += 1;
        below.extend(&next);
        level = next;
    }
    let most = super::option_int(site, "thread_comments_depth");
    let mut options = String::new();
    let mut current_listed = false;
    for candidate in &listed {
        let id = candidate.comment.id;
        if id == comment.id || below.contains(&id) {
            continue;
        }
        let mut depth = 1;
        let mut ancestor = candidate.comment.parent;
        while let Some(up) = (ancestor != 0).then(|| parent_of(ancestor)).flatten() {
            depth += 1;
            ancestor = up;
        }
        if most != 0 && depth + height > most {
            continue;
        }
        current_listed |= id == comment.parent;
        let author = comment_author_of(&candidate.comment, candidate.user.as_ref());
        let selected = if id == comment.parent {
            " selected='selected'"
        } else {
            ""
        };
        options.push_str(&format!(
            "\t<option value='{id}' data-author='{}'{selected}>{}</option>\n",
            esc_attr(&author),
            esc_html(option_label(candidate).as_bytes())
        ));
    }
    let parent = (comment.parent != 0)
        .then(|| site.comment_to_edit(comment.parent))
        .flatten();
    if let Some(parent) = parent.filter(|_| !current_listed) {
        let author = comment_author_of(&parent.comment, parent.user.as_ref());
        options.push_str(&format!(
            "\t<option value='{}' data-author='{}' selected>{}</option>\n",
            comment.parent,
            esc_attr(&author),
            esc_html(option_label(&parent).as_bytes())
        ));
    }
    let none = if comment.parent == 0 {
        " selected='selected'"
    } else {
        ""
    };
    format!(
        "<a href=\"#edit_comment_parent\" class=\"edit-comment-parent hide-if-no-js\"><span aria-hidden=\"true\">Edit</span> <span class=\"screen-reader-text\">\n\tEdit parent comment</span></a>\n<fieldset id=\"comment-parent-div\" class=\"hide-if-js\">\n<legend class=\"screen-reader-text\">\n\tParent comment</legend>\n<label for=\"comment_parent\">Parent comment</label>\n<select name=\"comment_parent\" id=\"comment_parent\">\n\t<option value=\"0\"{none}>\n\t\tNone (top-level comment)\t</option>\n\t{options}</select>\n<input type=\"hidden\" id=\"hidden_comment_parent\" value=\"{}\" />\n<p>\n<a href=\"#edit_comment_parent\" class=\"save-comment-parent hide-if-no-js button\">OK</a>\n<a href=\"#edit_comment_parent\" class=\"cancel-comment-parent hide-if-no-js button-cancel\">Cancel</a>\n</p>\n</fieldset>\n",
        comment.parent
    )
}

/// `comment.php?action=editcomment&c=` `id`: the edit form, `None` for a
/// comment missing, in the Trash or the user may not edit, whose
/// `comment_footer_die()` is not ported.
pub fn comment_edit(site: &dyn CommentEditSite, id: u64, request: &Request) -> Option<Answer> {
    let row = site.comment_to_edit(id).filter(|row| row.can_edit)?;
    // `comment_approved` as `wp_get_comment_status()` names it.
    let approved = match row.status {
        "trash" => return None,
        "approved" => "1",
        "unapproved" => "0",
        status => status,
    };
    let comment = &row.comment;
    let (zone, now) = site.clock();
    let permalink = match row.status == "approved" && comment.post_id > 0 {
        true => format!(
            "<div class=\"inside\">\n\t<div id=\"comment-link-box\">\n\t\t<strong>Permalink:</strong>\n\t\t<span id=\"sample-permalink\">\n\t\t\t<a href=\"{}\">\n\t\t\t\t{}\t\t\t</a>\n\t\t</span>\n\t</div>\n</div>\n",
            text(&esc_url(row.link.as_bytes())),
            esc_html(row.link.as_bytes())
        ),
        false => String::new(),
    };
    let content = text(&esc_textarea(&comment.content));
    let editor = format!(
        "<div id=\"wp-content-wrap\" class=\"wp-core-ui wp-editor-wrap html-active\">{}<div id=\"wp-content-editor-container\" class=\"wp-editor-container\"><div id=\"qt_content_toolbar\" class=\"quicktags-toolbar hide-if-no-js\"></div><textarea class=\"wp-editor-area\" rows=\"20\" cols=\"40\" name=\"content\" id=\"content\">{content}</textarea></div>\n</div>\n",
        editor_css(site)
    );
    let checked = |value: &str| {
        if approved == value {
            " checked='checked'"
        } else {
            ""
        }
        .to_string()
    };
    let shown = [("1", "Approved"), ("0", "Pending"), ("spam", "Spam")];
    let status_display = shown
        .iter()
        .find(|(raw, _)| *raw == approved)
        .map_or("", |s| s.1);
    let submitted = format!(
        "{} at {}",
        date::format("M j, Y", &comment.date, Zone::Utc),
        date::format("H:i", &comment.date, Zone::Utc)
    );
    // The post's title linked to its editor: a user who may edit the
    // comment may edit its post.
    let post_link = row.post.as_ref().map_or_else(String::new, |post| {
        let edit = BarSite::admin_url(site, &format!("post.php?post={}&action=edit", post.id));
        let title = esc_html(&admin_title(post));
        format!("<a href='{}'>{title}</a>", text(&esc_url(edit.as_bytes())))
    });
    let parent_display = match (comment.parent != 0)
        .then(|| site.parent_comment(comment.parent))
        .flatten()
    {
        Some((link, author)) => format!(
            "<a href=\"{}\">{}</a>",
            text(&esc_url(link.as_bytes())),
            esc_html(&author)
        ),
        None => "None".to_string(),
    };
    let threaded = super::option_int(site, "thread_comments") != 0;
    let parent_edit = if threaded {
        parent_edit(site, &row)
    } else {
        String::new()
    };
    let referer = site.referer();
    let delete_url = format!(
        "comment.php?action=trashcomment&c={id}&_wp_original_http_referer={}",
        text(&urlencode(referer.as_deref().unwrap_or_default()))
    );
    let nonce = site.nonce(&format!("delete-comment_{id}"));
    let delete_url = add_query_arg(
        &[(b"_wpnonce", Some(nonce.as_bytes()))],
        delete_url.as_bytes(),
    );
    let delete = format!(
        "<a class='submitdelete deletion' href='{}'>Move to Trash</a>\n",
        link(&esc_html(&delete_url))
    );
    let referred = referer
        .as_deref()
        .map_or_else(String::new, |r| text(&esc_url(r)));
    let focus = "<script>\ntry{document.post.name.focus();}catch(e){}\n</script>\n";
    let focus = if request.browser.is_mobile() {
        ""
    } else {
        focus
    };
    let content = fill(
        PAGE,
        &[
            (
                "nonce_field",
                wp_nonce_field(
                    site,
                    &format!("update-comment_{id}"),
                    "_wpnonce",
                    request.uri,
                ),
            ),
            ("id", id.to_string()),
            ("post", comment.post_id.to_string()),
            ("permalink", permalink),
            ("author", esc_attr(&esc_textarea(&comment.author))),
            ("email", esc_attr(&esc_textarea(&comment.author_email))),
            (
                "url",
                text(&esc_url(&esc_url(&esc_textarea(&comment.author_url)))),
            ),
            ("editor", editor),
            ("closed_nonce", site.nonce("closedpostboxes")),
            ("status_display", status_display.to_string()),
            ("checked_1", checked("1")),
            ("checked_0", checked("0")),
            ("checked_spam", checked("spam")),
            ("submitted", submitted),
            (
                "touch_time",
                touch_time(Some(&comment.date), zone, now, false),
            ),
            ("post_link", post_link),
            ("parent_display", parent_display),
            ("parent_edit", parent_edit),
            ("delete", delete),
            ("referredby", referred.clone()),
            ("original", esc_attr(referer.as_deref().unwrap_or_default())),
            ("focus", focus.to_string()),
        ],
    );
    // The editor is the form's `content`, not the reply form's.
    let editor = super::editor_footer(site).replace(
        "'replycontent':{id:\"replycontent\"",
        "'content':{id:\"content\"",
    );
    let page = Page {
        screen: Screen {
            pagenow: "comment.php",
            typenow: "",
            taxonomy: "",
            parent_file: Some("edit-comments.php"),
            submenu_file: Some("edit-comments.php"),
        },
        id: "comment",
        title: "Edit Comment",
        help: &HELP,
        sidebar: SIDEBAR,
        screen_options: "",
        styles: &[],
        scripts: &["comment"],
        late_scripts: &["quicktags", "wplink", "jquery-ui-autocomplete"],
        script_data: &[],
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: &editor,
        content: &content,
        canonical: request.canonical,
        origin: request.origin,
        view: None,
    };
    Some(Answer::from_page(admin_page(site, &page, request.browser)))
}
