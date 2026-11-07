// SPDX-License-Identifier: AGPL-3.0-or-later

//! Comment pages: the paging of `build_comment_query_vars_from_block()`
//! and the comments pagination blocks.
//!
//! Not ported: the page numbers block (`paginate_comments_links()`), and
//! the previous link outside a post, which counts every post's comments.

use hyscribe_kses::esc_url;
use hyscribe_model::Comment;
use hyscribe_phpser::{Value, add_query_arg, is_empty_string};

use crate::blocks::Block;
use crate::core_blocks::{Context, prepared, render};
use crate::formatting::encode_ampersands;
use crate::query_pagination::arrow_span;
use crate::site::{Request, Site};
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

/// The comments a comment query counts and pages: the top level of a
/// threaded list, every comment of a flat one.
pub(crate) fn listed<'c>(site: &dyn Site, all: &'c [Comment]) -> Vec<&'c Comment> {
    let threaded = !is_empty_string(&site.option("thread_comments").unwrap_or_default());
    all.iter().filter(|c| !threaded || c.parent == 0).collect()
}

/// A paged comment query: comments per page, the page asked for, and
/// `max_num_pages`.
pub(crate) struct Paging {
    pub(crate) number: i64,
    pub(crate) paged: Option<i64>,
    pub(crate) max_pages: i64,
}

/// The paging of `build_comment_query_vars_from_block()` over `found`
/// listed comments; `None` when comments are not paged.
pub(crate) fn paging(site: &dyn Site, request: &Request, found: usize) -> Option<Paging> {
    let option = |name| site.option(name).unwrap_or_default();
    if option("page_comments") != b"1" {
        return None;
    }
    let number = Value::String(option("comments_per_page")).to_int();
    if number <= 0 {
        return None;
    }
    let found = i64::try_from(found).unwrap_or(i64::MAX);
    let max_pages = (found + number - 1) / number;
    let page = cpage(request).to_int();
    let paged = match option("default_comments_page").as_slice() {
        _ if page != 0 => Some(page),
        b"oldest" => Some(1),
        // PHP skips a last page of 0, which lists and links as page 0 does.
        b"newest" => Some(max_pages),
        _ => None,
    };
    Some(Paging {
        number,
        paged,
        max_pages,
    })
}

impl Renderer<'_> {
    /// The comments pagination block; its inner blocks get
    /// `paginationArrow` as `comments/paginationArrow`.
    pub(crate) fn comments_pagination(&mut self, block: &Block) -> Vec<u8> {
        let name = "core/comments-pagination";
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let arrow = prepared(name, parsed).get_str("paginationArrow").cloned();
        let outer = std::mem::replace(&mut self.pagination_arrow, arrow);
        let content = self.saved_markup(block);
        self.pagination_arrow = outer;
        render(&self.context, name, parsed, &content).unwrap_or_default()
    }

    /// `render_block_core_comments_pagination_previous()`.
    pub(crate) fn comments_pagination_previous(&self, block: &Block) -> Vec<u8> {
        let name = "core/comments-pagination-previous";
        let paging = self.comment_paging();
        // get_previous_comments_link()
        let page = match paging.and_then(|p| p.paged) {
            Some(paged) => paged,
            None => cpage(self.context.request).to_int(),
        };
        if !self.context.request.singular || page <= 1 {
            return Vec::new();
        }
        let label = [
            self.arrow("previous"),
            self.label(name, block, b"Older Comments"),
        ]
        .concat();
        self.comments_link(name, block, &label, page - 1, 0)
    }

    /// `render_block_core_comments_pagination_next()`.
    pub(crate) fn comments_pagination_next(&self, block: &Block) -> Vec<u8> {
        let name = "core/comments-pagination-next";
        if self.context.post_id.is_none_or(|id| id == 0) {
            return Vec::new();
        }
        let paging = self.comment_paging();
        let max_page = paging.as_ref().map_or(0, |p| p.max_pages);
        // get_next_comments_link(); without a page count of its own, the
        // main query's is 0 in a block theme.
        let page = match paging.and_then(|p| p.paged) {
            Some(paged) => paged,
            None => {
                let raw = cpage(self.context.request);
                if raw.is_empty() { 1 } else { raw.to_int() }
            }
        };
        let next = page + 1;
        if !self.context.request.singular || next > max_page {
            return Vec::new();
        }
        let label = [
            self.label(name, block, b"Newer Comments"),
            self.arrow("next"),
        ]
        .concat();
        self.comments_link(name, block, &label, next, max_page)
    }

    /// The paging of the comments of the `postId` context.
    fn comment_paging(&self) -> Option<Paging> {
        let site = self.context.site;
        let all = self
            .context
            .post_id
            .map(|id| site.comments(id))
            .unwrap_or_default();
        paging(site, self.context.request, listed(site, &all).len())
    }

    /// The `label` attribute of `name`, or `default` when it is empty.
    fn label(&self, name: &str, block: &Block, default: &[u8]) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        prepared(name, parsed)
            .get_str("label")
            .filter(|label| !label.is_empty())
            .map_or_else(|| default.to_vec(), Value::to_php_string)
    }

    /// `get_comments_pagination_arrow()`.
    fn arrow(&self, direction: &str) -> Vec<u8> {
        arrow_span("comments", self.pagination_arrow.as_ref(), direction)
    }

    /// The link of `get_previous_comments_link()` and
    /// `get_next_comments_link()`, with the block's wrapper attributes.
    fn comments_link(
        &self,
        name: &str,
        block: &Block,
        label: &[u8],
        page: i64,
        max_page: i64,
    ) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes =
            get_block_wrapper_attributes(name, parsed, &[], &self.context.theme.global);
        [
            b"<a href=\"".as_slice(),
            &esc_url(&comments_pagenum_link(&self.context, page, max_page)),
            b"\" ",
            attributes.as_bytes(),
            b">",
            &encode_ampersands(label, false),
            b"</a>",
        ]
        .concat()
    }
}

/// `get_comments_pagenum_link()` for the global post.
pub(crate) fn comments_pagenum_link(context: &Context<'_>, page: i64, max_page: i64) -> Vec<u8> {
    let permalink = context
        .global_post()
        .map(|post| context.permalink(&post))
        .unwrap_or_default();
    let newest = context
        .site
        .option("default_comments_page")
        .unwrap_or_default()
        == b"newest";
    let paged = if newest { page != max_page } else { page > 1 };
    let links = crate::permalinks::Permalinks::of(context.site);
    let part = format!("comment-page-{page}");
    let mut url = if !paged {
        permalink
    } else if links.pretty() {
        links
            .under(&String::from_utf8_lossy(&permalink), &part)
            .into_bytes()
    } else {
        add_query_arg(&[(b"cpage", Some(page.to_string().as_bytes()))], &permalink)
    };
    url.extend_from_slice(b"#comments");
    url
}

/// `get_query_var( 'cpage' )`.
pub(crate) fn cpage(request: &Request) -> Value {
    request.vars.get_str("cpage").cloned().unwrap_or_default()
}
