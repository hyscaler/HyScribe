// SPDX-License-Identifier: AGPL-3.0-or-later

//! The comment template block: the post's approved comments, threaded, each
//! rendering the template's inner blocks with the comment as context.
//!
//! Comment paging (`page_comments`) and the unapproved comments a visitor's
//! cookie would show are not ported yet.

use hyscribe_model::Comment;
use hyscribe_phpser::{Value, is_empty_string};

use crate::blocks::Block;
use crate::post::sanitize_html_class;
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

impl Renderer<'_> {
    /// `render_block_core_comment_template()`.
    pub(crate) fn comment_template(&mut self, block: &Block) -> Vec<u8> {
        let site = self.context.site;
        let Some(post_id) = self.context.post_id.filter(|&id| id != 0) else {
            return Vec::new();
        };
        // post_password_required() for a visitor without the cookie
        if site
            .post(post_id)
            .is_some_and(|post| !post.password.is_empty())
        {
            return Vec::new();
        }
        let option = |name| site.option(name).unwrap_or_default();
        let threaded = !is_empty_string(&option("thread_comments"));
        let all = site.comments(post_id);
        // A threaded query lists the top level; replies come with their parents.
        let mut listed: Vec<&Comment> = all.iter().filter(|c| !threaded || c.parent == 0).collect();
        if listed.is_empty() {
            return Vec::new();
        }
        if option("comment_order") == b"desc" {
            listed.reverse();
        }
        let depth_limit = Value::String(option("thread_comments_depth")).to_int();
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let wrapper = get_block_wrapper_attributes(
            "core/comment-template",
            parsed,
            &[],
            &self.context.theme.global,
        );
        let threads = Threads {
            all: &all,
            threaded,
            depth_limit,
        };
        let items = self.render_comments(block, &listed, &threads);
        [format!("<ol {wrapper}>").as_bytes(), &items, b"</ol>"].concat()
    }

    /// `block_core_comment_template_render_comments()`.
    fn render_comments(
        &mut self,
        block: &Block,
        comments: &[&Comment],
        threads: &Threads,
    ) -> Vec<u8> {
        if self.comment_depth == 0 {
            self.comment_depth = 1;
        }
        let mut content = Vec::new();
        for comment in comments {
            let outer = self.context.comment.replace((*comment).clone());
            let mut html = self.saved_markup(block);
            self.context.comment = outer;
            let children: Vec<&Comment> = threads
                .all
                .iter()
                .filter(|c| c.parent == comment.id)
                .collect();
            let classes = self.comment_class(comment);
            if !children.is_empty() && threads.threaded {
                if i64::try_from(self.comment_depth).unwrap_or(i64::MAX) < threads.depth_limit {
                    self.comment_depth += 1;
                    let inner = self.render_comments(block, &children, threads);
                    html = [&html[..], b"<ol>", &inner, b"</ol>"].concat();
                    self.comment_depth -= 1;
                } else {
                    html.extend(self.render_comments(block, &children, threads));
                }
            }
            content.extend(
                format!("<li id=\"comment-{}\" class=\"{}\">", comment.id, classes).into_bytes(),
            );
            content.extend(html);
            content.extend_from_slice(b"</li>");
        }
        content
    }

    /// `get_comment_class()`, joined, with its alternation counters.
    fn comment_class(&mut self, comment: &Comment) -> String {
        let site = self.context.site;
        let mut classes: Vec<Vec<u8>> = vec![if comment.comment_type.is_empty() {
            b"comment".to_vec()
        } else {
            comment.comment_type.clone()
        }];
        if let Some(user) = (comment.user_id != 0)
            .then(|| site.user(comment.user_id))
            .flatten()
        {
            classes.push(b"byuser".to_vec());
            let fallback = comment.user_id.to_string();
            classes.push(
                [
                    &b"comment-author-"[..],
                    &sanitize_html_class(&user.nicename, fallback.as_bytes()),
                ]
                .concat(),
            );
            if site
                .post(comment.post_id)
                .is_some_and(|post| post.author == comment.user_id)
            {
                classes.push(b"bypostauthor".to_vec());
            }
        }
        if self.comment_alt % 2 == 1 {
            classes.push(b"odd".to_vec());
            classes.push(b"alt".to_vec());
        } else {
            classes.push(b"even".to_vec());
        }
        self.comment_alt += 1;
        if self.comment_depth == 1 {
            if self.comment_thread_alt % 2 == 1 {
                classes.push(b"thread-odd".to_vec());
                classes.push(b"thread-alt".to_vec());
            } else {
                classes.push(b"thread-even".to_vec());
            }
            self.comment_thread_alt += 1;
        }
        classes.push(format!("depth-{}", self.comment_depth).into_bytes());
        let classes: Vec<String> = classes
            .iter()
            .map(|class| hyscribe_kses::esc_attr(class))
            .collect();
        classes.join(" ")
    }
}

/// The comments a thread draws its replies from.
struct Threads<'a> {
    all: &'a [Comment],
    threaded: bool,
    depth_limit: i64,
}

#[cfg(test)]
mod tests;
