// SPDX-License-Identifier: AGPL-3.0-or-later

//! The legacy comments block, the old `core/post-comments`:
//! `comments_template()` over WordPress's `theme-compat/comments.php`, which
//! `wp_list_comments()` fills with `Walker_Comment`'s HTML5 items, and the
//! classic comment form. A visitor has no edit links.

use std::collections::{HashMap, HashSet};

use hyscribe_kses::esc_url;
use hyscribe_model::{Comment, Post};
use hyscribe_phpser::{Value, is_empty_string, isset};

use super::avatar::{Subject, get_avatar};
use super::comment_form::legacy_comment_form;
use super::{
    Block, Context, align_class, comment_author, comment_author_link, comment_link_at,
    comment_text, comments_open, number_format, reply_link,
};
use crate::comment_pages::{comments_pagenum_link, cpage, listed, paging};
use crate::comments::{get_comment_class, post_comments};
use crate::date;
use crate::kses::wp_kses_no_tags;
use crate::post::the_title;

/// `render_block_core_comments()` of the legacy block, for a `post` that
/// takes comments or has some: `comments_template()`'s output, which a
/// request that is not a single post or page leaves empty, in the block's
/// wrapper.
pub(super) fn legacy_comments(context: &Context<'_>, block: &Block<'_>, post: &Post) -> Vec<u8> {
    // The global post is this one while the template runs.
    let request = crate::site::Request {
        post: Some(post.id),
        ..context.request.clone()
    };
    let context = Context {
        request: &request,
        ..context.clone()
    };
    let output = if context.request.singular {
        comments_template(&context, post)
    } else {
        Vec::new()
    };
    let mut classes = Vec::new();
    if isset(block.attribute("legacy")) {
        classes.push(b"wp-block-post-comments".to_vec());
    }
    classes.extend(align_class(block));
    let wrapper = block.wrapper(&context, &classes.join(&b' '));
    [format!("<div {wrapper}>").as_bytes(), &output, b"</div>"].concat()
}

/// `comments_template()`: the comments of the post as the compat template
/// lays them out, then the form.
fn comments_template(context: &Context<'_>, post: &Post) -> Vec<u8> {
    // The compat template stops at a password protected post.
    if !post.password.is_empty() {
        return b"\t\t<p class=\"nocomments\">This post is password protected. Enter the password to view comments.</p>\n\t".to_vec();
    }
    let site = context.site;
    let (all, held) = post_comments(site, context.request, post.id);
    let option = |name| site.option(name).unwrap_or_default();
    let mut top = listed(site, &all);
    let paging = paging(site, context.request, top.len());
    if let Some(paging) = &paging {
        // Pages below the first list the first.
        let offset = paging.paged.map_or(0, |p| (p.max(1) - 1) * paging.number);
        top = top
            .into_iter()
            .skip(usize::try_from(offset).unwrap_or(usize::MAX))
            .take(usize::try_from(paging.number).unwrap_or(usize::MAX))
            .collect();
    }
    let max_pages = paging.as_ref().map_or(0, |paging| paging.max_pages);
    let page = cpage_var(context, max_pages);
    let mut out = String::from("\n<!-- You can start editing here. -->\n\n");
    if top.is_empty() {
        out.push_str(if comments_open(context, post) {
            "\n\t\t\t<!-- If comments are open, but there are no comments. -->\n\n\t\n"
        } else {
            "\n\t\t\t<!-- If comments are closed. -->\n\t\t<p class=\"nocomments\">Comments are closed.</p>\n\n\t\n"
        });
    } else {
        if option("comment_order") == b"desc" {
            top.reverse();
        }
        let nav = navigation(context, &page, max_pages);
        out.push_str(&format!(
            "\t<h3 id=\"comments\">\n\t\t{}\t</h3>\n\n{nav}\n\n\t<ol class=\"commentlist\">\n\t",
            title(post)
        ));
        // get_comment_link() names the page shown, the first of the oldest
        // comments having none.
        let oldest = option("default_comments_page") == b"oldest";
        let link_page = if paging.is_none() || oldest && page.page == 1 {
            0
        } else {
            page.page
        };
        let list = List {
            context,
            all: &all,
            held: &held,
            link_page,
        };
        out.push_str(&list.walk(&top));
        out.push_str(&format!("\t</ol>\n\n{nav}\n\n"));
    }
    let mut out = out.into_bytes();
    out.extend(legacy_comment_form(context, post));
    out
}

/// The heading: how many responses the post has.
fn title(post: &Post) -> String {
    let title = String::from_utf8_lossy(&the_title(post)).into_owned();
    if post.comment_count == 1 {
        format!("One response to &#8220;{title}&#8221;")
    } else {
        format!(
            "{} responses to &#8220;{title}&#8221;",
            number_format(post.comment_count)
        )
    }
}

/// The `cpage` query var after `comments_template()`: what the request
/// names or else, with several pages, the page it sets.
struct Cpage {
    page: i64,
    /// Whether PHP takes it for true, as `abc` is.
    truthy: bool,
    /// Set by `comments_template()`, as the request had none.
    set: bool,
}

fn cpage_var(context: &Context<'_>, max_pages: i64) -> Cpage {
    let given = match cpage(context.request) {
        Value::Null => None,
        Value::String(page) if page.is_empty() => None,
        page => Some(page),
    };
    if let Some(page) = given {
        return Cpage {
            page: page.to_int(),
            truthy: !page.is_empty(),
            set: false,
        };
    }
    let newest = context
        .site
        .option("default_comments_page")
        .unwrap_or_default()
        == b"newest";
    match (max_pages > 1, newest) {
        (false, _) => Cpage {
            page: 0,
            truthy: false,
            set: false,
        },
        (true, newest) => Cpage {
            page: if newest { max_pages } else { 1 },
            truthy: true,
            set: true,
        },
    }
}

/// The `cpage` query var `comments_template()` sets for a request with
/// several comment pages and none named, which `wp_head()` then reads.
pub(super) fn legacy_cpage(context: &Context<'_>, post: &Post) -> Option<i64> {
    let site = context.site;
    let (all, _) = post_comments(site, context.request, post.id);
    let pages = paging(site, context.request, listed(site, &all).len());
    let page = cpage_var(context, pages.map_or(0, |paging| paging.max_pages));
    page.set.then_some(page.page)
}

/// The navigation `<div>` of the older and newer comments links.
fn navigation(context: &Context<'_>, page: &Cpage, max_pages: i64) -> String {
    let link = |page, max_page, label: &str| {
        format!(
            "<a href=\"{}\" >{label}</a>",
            String::from_utf8_lossy(&esc_url(&comments_pagenum_link(context, page, max_page)))
        )
    };
    let singular = context.request.singular;
    let previous = if singular && page.page > 1 {
        link(page.page - 1, 0, "&laquo; Older Comments")
    } else {
        String::new()
    };
    // A page count the comment query did not make is 1, of a post with
    // comments.
    let next_page = if page.truthy { page.page } else { 1 } + 1;
    let max_page = max_pages.max(1);
    let next = if singular && next_page <= max_page {
        link(next_page, max_page, "Newer Comments &raquo;")
    } else {
        String::new()
    };
    format!(
        "\t<div class=\"navigation\">\n\t\t<div class=\"alignleft\">{previous}</div>\n\t\t<div class=\"alignright\">{next}</div>\n\t</div>"
    )
}

/// What `wp_list_comments()` lists.
struct List<'a, 'c> {
    context: &'a Context<'c>,
    /// Every comment of the post, which the replies come from.
    all: &'a [Comment],
    /// The ids of those held for the commenter.
    held: &'a HashSet<u64>,
    /// The page `get_comment_link()` names, 0 for none.
    link_page: i64,
}

/// A step of the walk.
enum Step<'a> {
    Show(&'a Comment, usize),
    CloseLevel,
    CloseItem,
}

const CLOSE_ITEM: &str = "</li><!-- #comment-## -->\n";

impl List<'_, '_> {
    /// `Walker_Comment::paged_walk()` over `top` and their replies, without
    /// recursing: a level is open while its replies are on the stack.
    fn walk(&self, top: &[&Comment]) -> String {
        let site = self.context.site;
        let option = |name| site.option(name).unwrap_or_default();
        let threaded = !is_empty_string(&option("thread_comments"));
        // The depth replies nest to; a flat list has none.
        let max_depth = Value::String(option("thread_comments_depth")).to_int();
        let mut children: HashMap<u64, Vec<&Comment>> = HashMap::new();
        if threaded {
            for comment in self.all {
                children.entry(comment.parent).or_default().push(comment);
            }
        }
        let (mut alt, mut thread_alt) = (0, 0);
        let mut out = String::new();
        let mut steps: Vec<Step> = top.iter().rev().map(|c| Step::Show(c, 0)).collect();
        while let Some(step) = steps.pop() {
            match step {
                Step::Show(comment, depth) => {
                    let replies = children.get(&comment.id);
                    let class = get_comment_class(
                        site,
                        comment,
                        (&mut alt, &mut thread_alt),
                        depth as u64 + 1,
                        if replies.is_some() { "parent" } else { "" },
                    );
                    out.push_str(&self.item(comment, depth + 1, &class));
                    let depth_i = i64::try_from(depth).unwrap_or(i64::MAX);
                    let nested = max_depth == 0 || max_depth > depth_i + 1;
                    match replies {
                        Some(replies) if nested => {
                            out.push_str("<ul class=\"children\">\n");
                            steps.push(Step::CloseItem);
                            steps.push(Step::CloseLevel);
                            steps.extend(replies.iter().rev().map(|c| Step::Show(c, depth + 1)));
                        }
                        // At the deepest level the replies follow in it.
                        Some(replies) => {
                            out.push_str(CLOSE_ITEM);
                            steps.extend(replies.iter().rev().map(|c| Step::Show(c, depth)));
                        }
                        None => out.push_str(CLOSE_ITEM),
                    }
                }
                Step::CloseLevel => out.push_str("</ul><!-- .children -->\n"),
                Step::CloseItem => out.push_str(CLOSE_ITEM),
            }
        }
        out
    }

    /// `Walker_Comment::html5_comment()`, an `<li>` left open, for a comment
    /// at `depth`.
    fn item(&self, comment: &Comment, depth: usize, class: &str) -> String {
        let context = self.context;
        let site = context.site;
        let commenter = &context.request.commenter;
        let show_pending = !is_empty_string(&commenter.author);
        let held = self.held.contains(&comment.id);
        let note = if is_empty_string(&commenter.email) {
            "Your comment is awaiting moderation. This is a preview; your comment will be visible after it has been approved."
        } else {
            "Your comment is awaiting moderation."
        };
        let avatar = get_avatar(context, &Subject::Comment(comment), 32, b"", "", "");
        let author = if held && !show_pending {
            comment_author(context, comment)
        } else {
            comment_author_link(context, comment)
        };
        let option = |name| site.option(name).unwrap_or_default();
        let zone = context.zone();
        let when = |format: &str| date::format(format, &comment.date, zone);
        let when_in = |name| when(&String::from_utf8_lossy(&option(name)));
        let link = comment_link_at(context, comment, self.link_page);
        let mut text = comment_text(site, &comment.content);
        if held && !show_pending && comment.comment_type == b"comment" {
            text = wp_kses_no_tags(&text);
        }
        let reply = if held && !show_pending {
            None
        } else {
            let url = || comment_link_at(context, comment, self.link_page);
            reply_link(context, comment, ("div-comment", depth), &url)
        };
        let moderation = if held {
            format!("\t\t\t\t\t<em class=\"comment-awaiting-moderation\">{note}</em>\n\t\t\t\t\t")
        } else {
            String::new()
        };
        let reply = reply.map_or_else(String::new, |link| {
            format!(
                "<div class=\"reply\">{}</div>",
                String::from_utf8_lossy(&link)
            )
        });
        format!(
            "\t\t<li id=\"comment-{id}\" class=\"{class}\">\n\t\t\t<article id=\"div-comment-{id}\" class=\"comment-body\">\n\t\t\t\t<footer class=\"comment-meta\">\n\t\t\t\t\t<div class=\"comment-author vcard\">\n\t\t\t\t\t\t{avatar}\t\t\t\t\t\t<b class=\"fn\">{author}</b> <span class=\"says\">says:</span>\t\t\t\t\t</div><!-- .comment-author -->\n\n\t\t\t\t\t<div class=\"comment-metadata\">\n\t\t\t\t\t\t<a href=\"{href}\"><time datetime=\"{c}\">{date} at {time}</time></a>\t\t\t\t\t</div><!-- .comment-metadata -->\n\n\t\t\t\t\t{moderation}\t\t\t\t</footer><!-- .comment-meta -->\n\n\t\t\t\t<div class=\"comment-content\">\n\t\t\t\t\t{text}\t\t\t\t</div><!-- .comment-content -->\n\n\t\t\t\t{reply}\t\t\t</article><!-- .comment-body -->\n\t\t",
            id = comment.id,
            avatar = String::from_utf8_lossy(&avatar),
            author = String::from_utf8_lossy(&author),
            href = String::from_utf8_lossy(&esc_url(&link)),
            c = when("c"),
            date = when_in("date_format"),
            time = when_in("time_format"),
            text = String::from_utf8_lossy(&text),
        )
    }
}
