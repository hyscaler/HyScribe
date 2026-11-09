// SPDX-License-Identifier: AGPL-3.0-or-later

//! The post author blocks: `render_block_core_post_author()` and
//! `render_block_core_post_author_biography()`.

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::Post;
use hyscribe_phpser::Value;

use super::avatar::{Subject, get_avatar};
use super::{Block, Context};
use crate::kses::wp_kses_post;

/// The post in context with an author, which a template without one (an
/// author archive, not served) would take from the `author` query var.
fn authored(context: &Context<'_>) -> Option<Post> {
    context.post().filter(|post| post.author != 0)
}

/// `render_block_core_post_author()`.
pub(super) fn post_author(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(post) = authored(context) else {
        return Vec::new();
    };
    // post_type_supports( $post_type, 'author' )
    if !matches!(
        post.post_type.as_str(),
        "post" | "page" | "attachment" | "revision"
    ) {
        return Vec::new();
    }
    let on = |name| block.attribute(name).is_some_and(|v| !v.is_empty());
    let text = |name| block.attribute(name).map(Value::to_php_string);
    let user = context.site.user(post.author);
    let mut name = user
        .as_ref()
        .map(|user| user.display_name.clone())
        .unwrap_or_default();
    if on("isLink") && on("linkTarget") {
        let author = i64::try_from(post.author).unwrap_or_default();
        let url = esc_url(&crate::permalinks::author_posts_url(context.site, author));
        name = [
            &b"<a href=\""[..],
            &url,
            b"\" target=\"",
            esc_attr(&text("linkTarget").unwrap_or_default()).as_bytes(),
            b"\">",
            &name,
            b"</a>",
        ]
        .concat();
    }
    let mut classes = Vec::new();
    if let Some(justify) = text("itemsJustification") {
        classes.push([&b"items-justified-"[..], &justify].concat());
    }
    if let Some(align) = text("textAlign") {
        classes.push([&b"has-text-align-"[..], &align].concat());
    }
    if block.has_link_color() {
        classes.push(b"has-link-color".to_vec());
    }
    let mut out = format!("<div {}>", block.wrapper(context, &classes.join(&b' '))).into_bytes();
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
    if Value::String(biography.clone()).is_empty() {
        return Vec::new();
    }
    let align = block
        .attribute("textAlign")
        .filter(|align| !align.is_empty())
        .map(|align| [&b"has-text-align-"[..], &align.to_php_string()].concat())
        .unwrap_or_default();
    [
        format!("<div {}>", block.wrapper(context, &align)).as_bytes(),
        &biography,
        b"</div>",
    ]
    .concat()
}

#[cfg(test)]
mod tests;
