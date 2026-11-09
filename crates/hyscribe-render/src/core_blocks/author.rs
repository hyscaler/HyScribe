// SPDX-License-Identifier: AGPL-3.0-or-later

//! The post author blocks: `render_block_core_post_author()`, its name and
//! its biography.

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::Post;
use hyscribe_phpser::{Value, is_empty_string};

use super::avatar::{Subject, get_avatar};
use super::{Block, Context, align_link_classes};
use crate::kses::wp_kses_post;

/// The post in context with an author, which a template without one (an
/// author archive, not served) would take from the `author` query var.
fn authored(context: &Context<'_>) -> Option<Post> {
    context.post().filter(|post| post.author != 0)
}

/// The authored post in context, if its type has authors:
/// `post_type_supports( $post_type, 'author' )`.
fn supported(context: &Context<'_>) -> Option<Post> {
    authored(context).filter(|post| {
        matches!(
            post.post_type.as_str(),
            "post" | "page" | "attachment" | "revision"
        )
    })
}

/// `name` linked to the author's posts in `target`, with `class` when
/// given.
fn author_link(
    context: &Context<'_>,
    post: &Post,
    target: &[u8],
    class: &str,
    name: &[u8],
) -> Vec<u8> {
    let author = i64::try_from(post.author).unwrap_or_default();
    let url = esc_url(&crate::permalinks::author_posts_url(context.site, author));
    let class = if class.is_empty() {
        String::new()
    } else {
        format!(" class=\"{class}\"")
    };
    [
        &b"<a href=\""[..],
        &url,
        b"\" target=\"",
        esc_attr(target).as_bytes(),
        b"\"",
        class.as_bytes(),
        b">",
        name,
        b"</a>",
    ]
    .concat()
}

/// `render_block_core_post_author_name()`: the author's display name, as
/// stored, linked to their posts when `isLink` is set.
pub(super) fn post_author_name(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(post) = supported(context) else {
        return Vec::new();
    };
    let mut name = context
        .site
        .user(post.author)
        .map(|user| user.display_name)
        .unwrap_or_default();
    if block.attribute("isLink").is_some_and(|v| !v.is_empty()) {
        let target = block
            .attribute("linkTarget")
            .map(Value::to_php_string)
            .unwrap_or_default();
        name = author_link(
            context,
            &post,
            &target,
            "wp-block-post-author-name__link",
            &name,
        );
    }
    let wrapper = block.wrapper(context, &align_link_classes(block, Vec::new()));
    [format!("<div {wrapper}>").as_bytes(), &name, b"</div>"].concat()
}

/// `render_block_core_post_author()`.
pub(super) fn post_author(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(post) = supported(context) else {
        return Vec::new();
    };
    let on = |name| block.attribute(name).is_some_and(|v| !v.is_empty());
    let text = |name| block.attribute(name).map(Value::to_php_string);
    let user = context.site.user(post.author);
    let mut name = user
        .as_ref()
        .map(|user| user.display_name.clone())
        .unwrap_or_default();
    if on("isLink") && on("linkTarget") {
        let target = text("linkTarget").unwrap_or_default();
        name = author_link(context, &post, &target, "", &name);
    }
    let justify = text("itemsJustification").map(|j| [&b"items-justified-"[..], &j].concat());
    let classes = align_link_classes(block, justify.into_iter().collect());
    let mut out = format!("<div {}>", block.wrapper(context, &classes)).into_bytes();
    if on("showAvatar") {
        out.extend_from_slice(b"<div class=\"wp-block-post-author__avatar\">");
        if on("avatarSize") {
            let size = block.attribute("avatarSize").map_or(0, Value::to_int);
            let subject = Subject::User(user.clone());
            out.extend(get_avatar(context, &subject, size, b"", "", ""));
        }
        out.extend_from_slice(b"</div>");
    }
    out.extend_from_slice(b"<div class=\"wp-block-post-author__content\">");
    if on("byline") {
        let byline = wp_kses_post(&text("byline").unwrap_or_default(), &context.upload_host());
        out.extend_from_slice(b"<p class=\"wp-block-post-author__byline\">");
        out.extend(byline);
        out.extend_from_slice(b"</p>");
    }
    out.extend_from_slice(b"<p class=\"wp-block-post-author__name\">");
    out.extend(name);
    out.extend_from_slice(b"</p>");
    if on("showBio") {
        out.extend_from_slice(b"<p class=\"wp-block-post-author__bio\">");
        out.extend(user.map(|user| user.description).unwrap_or_default());
        out.extend_from_slice(b"</p>");
    }
    out.extend_from_slice(b"</div></div>");
    out
}

/// `render_block_core_post_author_biography()`: the author's
/// `description`, unless it is empty.
pub(super) fn post_author_biography(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(post) = authored(context) else {
        return Vec::new();
    };
    let biography = context
        .site
        .user(post.author)
        .map(|user| user.description)
        .unwrap_or_default();
    if is_empty_string(&biography) {
        return Vec::new();
    }
    let wrapper = block.wrapper(context, &block.text_align_class());
    [format!("<div {wrapper}>").as_bytes(), &biography, b"</div>"].concat()
}

#[cfg(test)]
mod tests;
