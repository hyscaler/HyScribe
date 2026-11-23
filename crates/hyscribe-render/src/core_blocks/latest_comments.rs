// SPDX-License-Identifier: AGPL-3.0-or-later

//! The latest comments block, `render_block_core_latest_comments()`.

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::Comment;
use hyscribe_phpser::{Value, absint, is_empty_string};

use super::avatar::{Subject, get_avatar};
use super::{Block, Context, comment_author, comment_author_url, comment_link};
use crate::autop::wpautop;
use crate::date;
use crate::excerpt::wp_trim_words;
use crate::post::the_title;

/// `render_block_core_latest_comments()`.
pub(super) fn latest_comments(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let on = |name| block.attribute(name).is_some_and(|v| !v.is_empty());
    // The old `displayExcerpt` wins over `displayContent`.
    let content = match block.attribute("displayExcerpt") {
        Some(excerpt) if !excerpt.is_empty() => b"excerpt".to_vec(),
        Some(_) => b"none".to_vec(),
        None => block
            .attribute("displayContent")
            .map_or(b"excerpt".to_vec(), Value::to_php_string),
    };
    // The attribute's schema keeps it at 1 or more.
    let limit = block.attribute("commentsToShow").map_or(5, absint);
    let comments = context.site.recent_comments(limit);
    let mut items = Vec::new();
    for comment in &comments {
        items.extend_from_slice(b"<li class=\"wp-block-latest-comments__comment\">");
        if on("displayAvatar") {
            let class = "wp-block-latest-comments__comment-avatar";
            items.extend(get_avatar(
                context,
                &Subject::Comment(comment),
                48,
                b"",
                class,
                "",
            ));
        }
        items.extend_from_slice(
            b"<article><footer class=\"wp-block-latest-comments__comment-meta\">",
        );
        items.extend(author_markup(context, comment));
        items.extend_from_slice(b" on <a class=\"wp-block-latest-comments__comment-link\" href=\"");
        items.extend(esc_url(&comment_link(context, comment)));
        items.extend_from_slice(b"\">");
        let title = context
            .site
            .post(comment.post_id)
            .map(|post| the_title(&post));
        match title.filter(|title| !is_empty_string(title)) {
            Some(title) => items.extend(title),
            None => items.extend_from_slice(b"(no title)"),
        }
        items.extend_from_slice(b"</a>");
        if on("displayDate") {
            let zone = context.zone();
            let format = context.site.option("date_format").unwrap_or_default();
            items.extend(
                format!(
                    "<time datetime=\"{}\" class=\"wp-block-latest-comments__comment-date\">{}</time>",
                    esc_attr(date::format("c", &comment.date, zone).as_bytes()),
                    date::format(&String::from_utf8_lossy(&format), &comment.date, zone)
                )
                .into_bytes(),
            );
        }
        items.extend_from_slice(b"</footer>");
        let text = match content.as_slice() {
            b"full" if protected(context, comment) => Some(b"Password protected".to_vec()),
            b"full" => Some(comment.content.clone()),
            b"excerpt" => Some(comment_excerpt(context, comment)),
            _ => None,
        };
        if let Some(text) = text {
            items.extend_from_slice(b"<div class=\"wp-block-latest-comments__comment-excerpt\">");
            items.extend(wpautop(&text, true));
            items.extend_from_slice(b"</div>");
        }
        items.extend_from_slice(b"</article></li>");
    }
    let mut classes = Vec::new();
    if on("displayAvatar") {
        classes.push(&b"has-avatars"[..]);
    }
    if on("displayDate") {
        classes.push(b"has-dates");
    }
    if content != b"none" {
        classes.push(b"has-excerpts");
    }
    if comments.is_empty() {
        classes.push(b"no-comments");
        let wrapper = block.wrapper(context, &classes.join(&b' '));
        return format!("<div {wrapper}>No comments to show.</div>").into_bytes();
    }
    let wrapper = block.wrapper(context, &classes.join(&b' '));
    [format!("<ol {wrapper}>").as_bytes(), &items, b"</ol>"].concat()
}

/// The comment's author, linked to their site, or else to their posts
/// when they are a user.
fn author_markup(context: &Context<'_>, comment: &Comment) -> Vec<u8> {
    let mut url = comment_author_url(comment);
    if is_empty_string(&url) && comment.user_id != 0 {
        let user = i64::try_from(comment.user_id).unwrap_or_default();
        url = crate::permalinks::author_posts_url(context.site, user);
    }
    let author = comment_author(context, comment);
    if is_empty_string(&url) {
        return [
            &b"<span class=\"wp-block-latest-comments__comment-author\">"[..],
            &author,
            b"</span>",
        ]
        .concat();
    }
    [
        &b"<a class=\"wp-block-latest-comments__comment-author\" href=\""[..],
        &esc_url(&url),
        b"\">",
        &author,
        b"</a>",
    ]
    .concat()
}

/// `post_password_required()` for the comment's post, with no cookie.
fn protected(context: &Context<'_>, comment: &Comment) -> bool {
    context
        .site
        .post(comment.post_id)
        .is_some_and(|post| !post.password.is_empty())
}

/// `get_comment_excerpt()`: the first 20 words of the comment's text
/// without tags. Its line breaks to spaces change nothing, as
/// `wp_trim_words()` joins the words with spaces.
fn comment_excerpt(context: &Context<'_>, comment: &Comment) -> Vec<u8> {
    if protected(context, comment) {
        return b"Password protected".to_vec();
    }
    let text = hyscribe_kses::strip_tags(&comment.content);
    wp_trim_words(&text, 20, b"&hellip;")
}

#[cfg(test)]
mod tests;
