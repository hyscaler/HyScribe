// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_list_pages()` over `get_pages()` and `Walker_Page`, as the pages
//! widget calls it: no title item, every level, no dates.

use std::collections::HashSet;

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::{Post, PostQuery};
use hyscribe_phpser::{Key, Value, php_trim};

use crate::blocks::is_pcre_space;
use crate::core_blocks::Context;
use crate::navigation::post_ancestors;
use crate::post::title_filters;
use crate::query_loop::order_by;

/// `wp_list_pages( array( 'title_li' => '', 'sort_column' => $sort_column,
/// 'exclude' => $exclude ) )`: the published pages as nested list items,
/// sorted by the columns of `sort_column`; nothing without pages.
pub(crate) fn wp_list_pages(context: &Context<'_>, sort_column: &[u8], exclude: &[u8]) -> Vec<u8> {
    let pages = get_pages(context, sort_column, exclude);
    if pages.is_empty() {
        return Vec::new();
    }
    let request = context.request;
    let queried = request.queried_object_id;
    // is_page() or is_attachment(), or the page for posts.
    let current = if (request.singular
        && context
            .site
            .post(queried)
            .is_some_and(|post| post.post_type == "page" || post.post_type == "attachment"))
        || request.home
    {
        queried
    } else {
        0
    };
    let current_post = context.site.post(current);
    let walker = Walker {
        context,
        current,
        ancestors: current_post
            .as_ref()
            .map(|post| post_ancestors(context.site, post))
            .unwrap_or_default(),
        current_parent: current_post.map(|post| post.parent),
        page_for_posts: context
            .site
            .option("page_for_posts")
            .map_or(0, |id| Value::String(id).to_int()),
        with_children: pages.iter().map(|page| page.parent).collect(),
    };
    walker.walk(&pages)
}

/// `get_pages()` with the published pages, no tree, and `sort_column` and
/// `exclude` as given.
fn get_pages(context: &Context<'_>, sort_column: &[u8], exclude: &[u8]) -> Vec<Post> {
    // wp_list_pages() keeps digits and commas; wp_parse_id_list() then
    // takes each number through absint().
    let exclude: Vec<u8> = exclude
        .iter()
        .copied()
        .filter(|b| b.is_ascii_digit() || *b == b',')
        .collect();
    let exclude = exclude
        .split(|&b| b == b',')
        .filter(|id| !id.is_empty())
        .map(|id| Value::String(id.to_vec()).to_int().unsigned_abs())
        .collect();
    // wp_parse_list(), all ascending (a repeated key orders nothing
    // more); none leaves the query's own `post_title`.
    let keys: Vec<(Key, Value)> = sort_column
        .split(|&b| b == b',' || is_pcre_space(b))
        .filter(|key| !key.is_empty())
        .map(|key| {
            // trim() also takes a NUL off the ends.
            let key: &[u8] = match php_trim(key) {
                b"post_modified_gmt" => b"post_modified",
                b"modified_gmt" => b"modified",
                key => key,
            };
            (Key::String(key.to_vec()), Value::String(b"ASC".to_vec()))
        })
        .collect();
    let by = if keys.is_empty() {
        Value::String(b"post_title".to_vec())
    } else {
        Value::Array(keys)
    };
    let mut query = PostQuery {
        post_types: vec!["page".into()],
        exclude,
        limit: u32::MAX,
        ..PostQuery::default()
    };
    query.order = order_by(&by, true, &query);
    context.site.query(&query)
}

/// `Walker_Page` with `item_spacing` kept.
struct Walker<'a> {
    context: &'a Context<'a>,
    /// The page the request shows, or none.
    current: u64,
    /// The pages above it.
    ancestors: Vec<u64>,
    /// Its parent, when it exists.
    current_parent: Option<u64>,
    /// `page_for_posts`, marked as the parent when no page is current.
    page_for_posts: i64,
    /// The pages some listed page has as its parent.
    with_children: HashSet<u64>,
}

impl Walker<'_> {
    /// `Walker::walk()` to any depth: the top level, each page followed by
    /// its children, then the pages whose parent was not reached, flat.
    fn walk(&self, pages: &[Post]) -> Vec<u8> {
        // Without a top level page, those sharing the first's parent are
        // the top level.
        let top_parent = if pages.iter().any(|page| page.parent == 0) {
            0
        } else {
            pages.first().map_or(0, |page| page.parent)
        };
        let mut children: Vec<(u64, Vec<&Post>)> = Vec::new();
        let mut top = Vec::new();
        for page in pages {
            if page.parent == top_parent {
                top.push(page);
            } else {
                add_child(&mut children, page.parent, page);
            }
        }
        let mut output = Vec::new();
        for page in top {
            self.display(page, &mut children, 0, &mut output);
        }
        for (_, orphans) in std::mem::take(&mut children) {
            for page in orphans {
                self.display(page, &mut Vec::new(), 0, &mut output);
            }
        }
        output
    }

    /// `display_element()`: `page` and, below it, its children.
    fn display(
        &self,
        page: &Post,
        children: &mut Vec<(u64, Vec<&Post>)>,
        depth: usize,
        output: &mut Vec<u8>,
    ) {
        self.start_el(page, depth, output);
        if let Some(at) = children.iter().position(|(parent, _)| *parent == page.id) {
            let (_, below) = children.remove(at);
            let indent = "\t".repeat(depth);
            output.extend(format!("\n{indent}<ul class='children'>\n").into_bytes());
            for child in below {
                self.display(child, children, depth + 1, output);
            }
            output.extend(format!("{indent}</ul>\n").into_bytes());
        }
        output.extend_from_slice(b"</li>\n");
    }

    /// `Walker_Page::start_el()`: the item and its link.
    fn start_el(&self, page: &Post, depth: usize, output: &mut Vec<u8>) {
        let mut classes = format!("page_item page-item-{}", page.id);
        if self.with_children.contains(&page.id) {
            classes.push_str(" page_item_has_children");
        }
        if self.current != 0 {
            if self.ancestors.contains(&page.id) {
                classes.push_str(" current_page_ancestor");
            }
            if page.id == self.current {
                classes.push_str(" current_page_item");
            } else if self.current_parent == Some(page.id) {
                classes.push_str(" current_page_parent");
            }
        } else if i64::try_from(page.id) == Ok(self.page_for_posts) {
            classes.push_str(" current_page_parent");
        }
        let title = if page.title.is_empty() {
            format!("#{} (no title)", page.id).into_bytes()
        } else {
            page.title.clone()
        };
        let current = if page.id == self.current {
            &b" aria-current=\"page\""[..]
        } else {
            b""
        };
        output.extend("\t".repeat(depth).into_bytes());
        output.extend_from_slice(b"<li class=\"");
        output.extend(esc_attr(classes.as_bytes()).into_bytes());
        output.extend_from_slice(b"\"><a href=\"");
        output.extend(esc_url(&self.context.permalink(page)));
        output.extend_from_slice(b"\"");
        output.extend_from_slice(current);
        output.extend_from_slice(b">");
        output.extend(title_filters(&title));
        output.extend_from_slice(b"</a>");
    }
}

/// Adds `item` to the children of `parent`, a group made when it is the
/// first, so the groups keep the order their first child came in.
pub(crate) fn add_child<T>(children: &mut Vec<(u64, Vec<T>)>, parent: u64, item: T) {
    match children.iter_mut().find(|(id, _)| *id == parent) {
        Some((_, siblings)) => siblings.push(item),
        None => children.push((parent, vec![item])),
    }
}
