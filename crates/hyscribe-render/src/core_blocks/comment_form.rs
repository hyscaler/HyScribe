// SPDX-License-Identifier: AGPL-3.0-or-later

//! The post comments form block with `comment_form()`, for a visitor who
//! is not logged in. Comment cookies are not read yet, so the fields start
//! empty.

use hyscribe_kses::esc_url;
use hyscribe_model::{Comment, Post};
use hyscribe_phpser::{is_empty_string, remove_query_arg};

use super::{Block, Context, align_link_classes, comment_author, comments_open, wp_login_url};
use crate::template::replace;

const REQUIRED: &str = " <span class=\"required\">*</span>";

/// `render_block_core_post_comments_form()`.
pub(super) fn post_comments_form(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(post_id) = context.post_id else {
        return Vec::new();
    };
    let post = context.site.post(post_id);
    // post_password_required(): the password cookie is not read.
    if post.as_ref().is_some_and(|post| !post.password.is_empty()) {
        return Vec::new();
    }
    let classes = align_link_classes(block, vec![b"comment-respond".to_vec()]);
    let wrapper = block.wrapper(context, &classes);
    let Some(post) = post.filter(|post| comments_open(context, post)) else {
        return Vec::new();
    };
    replace(
        &comment_form(context, &post),
        b"class=\"comment-respond\"",
        wrapper.as_bytes(),
    )
}

/// `comment_form( array(), $post )` with the block's submit button, in
/// HTML5 as block themes have it.
fn comment_form(context: &Context<'_>, post: &Post) -> Vec<u8> {
    let truthy = |name| {
        context
            .site
            .option(name)
            .is_some_and(|v| !is_empty_string(&v))
    };
    let reply = reply_to(context, post);
    let mut form = String::from("\t<div id=\"respond\" class=\"comment-respond\">\n\t\t");
    form.push_str("<h3 id=\"reply-title\" class=\"comment-reply-title\">");
    // comment_form_title()
    match &reply {
        Some(comment) => form.push_str(&format!(
            "Leave a Reply to <a href=\"#comment-{}\">{}</a>",
            comment.id,
            String::from_utf8_lossy(&comment_author(context, comment))
        )),
        None => form.push_str("Leave a Reply"),
    }
    if truthy("thread_comments") {
        // get_cancel_comment_reply_link()
        let link = remove_query_arg(
            &[b"replytocom", b"unapproved", b"moderation-hash"],
            &context.request.uri,
        );
        // It reads the global post, not the form's.
        let global = context.request.post.and_then(|id| context.site.post(id));
        let style = if global.is_some_and(|post| reply_to(context, &post).is_some()) {
            ""
        } else {
            " style=\"display:none;\""
        };
        form.push_str(&format!(
            " <small><a rel=\"nofollow\" id=\"cancel-comment-reply-link\" href=\"{}#respond\"{style}>Cancel reply</a></small>",
            String::from_utf8_lossy(&esc_url(&link))
        ));
    }
    form.push_str("</h3>");
    if truthy("comment_registration") {
        let login = wp_login_url(context, &context.permalink(post));
        form.push_str(&format!(
            "<p class=\"must-log-in\">You must be <a href=\"{}\">logged in</a> to post a comment.</p>",
            String::from_utf8_lossy(&login)
        ));
    } else {
        form.push_str(&fields(
            context,
            post,
            reply.as_ref(),
            truthy("require_name_email"),
            truthy("show_comments_cookies_opt_in"),
        ));
    }
    form.push_str("\t</div><!-- #respond -->\n\t");
    form.into_bytes()
}

/// The `<form>`, with the notes, the fields and the submit field.
fn fields(
    context: &Context<'_>,
    post: &Post,
    reply: Option<&Comment>,
    required: bool,
    cookies: bool,
) -> String {
    let (indicator, attribute) = if required {
        (REQUIRED, " required")
    } else {
        ("", "")
    };
    let action = esc_url(&context.site_url(b"/wp-comments-post.php"));
    let mut form = format!(
        "<form action=\"{}\" method=\"post\" id=\"commentform\" class=\"comment-form\">",
        String::from_utf8_lossy(&action)
    );
    form.push_str("<p class=\"comment-notes\"><span id=\"email-notes\">Your email address will not be published.</span> <span class=\"required-field-message\">Required fields are marked <span class=\"required\">*</span></span></p>");
    form.push_str(&format!(
        "<p class=\"comment-form-comment\"><label for=\"comment\">Comment{REQUIRED}</label> <textarea id=\"comment\" name=\"comment\" cols=\"45\" rows=\"8\" maxlength=\"65525\" required></textarea></p>"
    ));
    form.push_str(&format!(
        "<p class=\"comment-form-author\"><label for=\"author\">Name{indicator}</label> <input id=\"author\" name=\"author\" type=\"text\" value=\"\" size=\"30\" maxlength=\"245\" autocomplete=\"name\"{attribute} /></p>\n"
    ));
    form.push_str(&format!(
        "<p class=\"comment-form-email\"><label for=\"email\">Email{indicator}</label> <input id=\"email\" name=\"email\" type=\"email\" value=\"\" size=\"30\" maxlength=\"100\" aria-describedby=\"email-notes\" autocomplete=\"email\"{attribute} /></p>\n"
    ));
    form.push_str("<p class=\"comment-form-url\"><label for=\"url\">Website</label> <input id=\"url\" name=\"url\" type=\"url\" value=\"\" size=\"30\" maxlength=\"200\" autocomplete=\"url\" /></p>\n");
    if cookies {
        form.push_str("<p class=\"comment-form-cookies-consent\"><input id=\"wp-comment-cookies-consent\" name=\"wp-comment-cookies-consent\" type=\"checkbox\" value=\"yes\" /> <label for=\"wp-comment-cookies-consent\">Save my name, email, and website in this browser for the next time I comment.</label></p>\n");
    }
    // The submit field, with get_comment_id_fields().
    form.push_str(&format!(
        "<p class=\"form-submit wp-block-button\"><input name=\"submit\" type=\"submit\" id=\"submit\" class=\"wp-block-button__link wp-element-button\" value=\"Post Comment\" /> <input type='hidden' name='comment_post_ID' value='{}' id='comment_post_ID' />\n<input type='hidden' name='comment_parent' id='comment_parent' value='{}' />\n</p></form>",
        post.id,
        reply.map_or(0, |comment| comment.id)
    ));
    form
}

/// `_get_comment_reply_id()`: the approved comment on `post`, notes
/// included, that `$_GET['replytocom']` names.
fn reply_to(context: &Context<'_>, post: &Post) -> Option<Comment> {
    let id = context
        .request
        .get
        .get_str("replytocom")
        .filter(|id| id.is_numeric())?
        .to_int();
    context
        .site
        .comment(u64::try_from(id).ok()?)
        .filter(|comment| comment.post_id == post.id)
}
