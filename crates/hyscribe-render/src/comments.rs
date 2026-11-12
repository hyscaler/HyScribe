// SPDX-License-Identifier: AGPL-3.0-or-later

//! The comment template block: the post's approved comments, threaded, each
//! rendering the template's inner blocks with the comment as context.
//!
//! The unapproved comments a visitor's cookie would show are not ported
//! yet.

use std::collections::HashMap;

use hyscribe_model::Comment;
use hyscribe_phpser::{Value, is_empty_string};

use crate::blocks::Block;
use crate::comment_pages::{listed, paging};
use crate::elements::elements_class;
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
        let mut listed = listed(site, &all);
        if let Some(paging) = paging(site, self.context.request, listed.len()) {
            // Pages below the first list the first.
            let offset = paging.paged.map_or(0, |p| (p.max(1) - 1) * paging.number);
            listed = listed
                .into_iter()
                .skip(usize::try_from(offset).unwrap_or(usize::MAX))
                .take(usize::try_from(paging.number).unwrap_or(usize::MAX))
                .collect();
        }
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
        let mut children: HashMap<u64, Vec<&Comment>> = HashMap::new();
        for comment in &all {
            children.entry(comment.parent).or_default().push(comment);
        }
        let threads = Threads {
            children,
            threaded,
            depth_limit,
        };
        let items = self.render_comments(block, &listed, &threads);
        [format!("<ol {wrapper}>").as_bytes(), &items, b"</ol>"].concat()
    }

    /// `block_core_comment_template_render_comments()`, which recurses
    /// once per reply; here a stack of the levels open, so a thread of any
    /// length takes no stack frames.
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
        // The comments left at each level, and whether the level is a
        // nested list, which closes with `</ol>` and one depth up.
        let mut levels = vec![(comments.iter(), false)];
        while let Some((rest, _)) = levels.last_mut() {
            let Some(comment) = rest.next() else {
                if let Some((_, nested)) = levels.pop().filter(|_| !levels.is_empty()) {
                    if nested {
                        content.extend_from_slice(b"</ol>");
                        self.comment_depth -= 1;
                    }
                    content.extend_from_slice(b"</li>");
                }
                continue;
            };
            let outer = self.context.comment.replace((*comment).clone());
            let outer_depth = std::mem::replace(&mut self.context.comment_depth, levels.len());
            // Each comment renders as the template block again, whose
            // `render_block` filters see its class.
            let html = elements_class(self.saved_markup(block), block);
            self.context.comment = outer;
            self.context.comment_depth = outer_depth;
            let classes = self.comment_class(comment);
            content.extend(
                format!("<li id=\"comment-{}\" class=\"{}\">", comment.id, classes).into_bytes(),
            );
            content.extend(html);
            match threads.children.get(&comment.id) {
                Some(children) if threads.threaded => {
                    let nested =
                        i64::try_from(self.comment_depth).unwrap_or(i64::MAX) < threads.depth_limit;
                    if nested {
                        self.comment_depth += 1;
                        content.extend_from_slice(b"<ol>");
                    }
                    levels.push((children.iter(), nested));
                }
                _ => content.extend_from_slice(b"</li>"),
            }
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

/// The replies to each comment, in the order of all the post's comments.
struct Threads<'a> {
    children: HashMap<u64, Vec<&'a Comment>>,
    threaded: bool,
    depth_limit: i64,
}

#[cfg(test)]
mod tests;
