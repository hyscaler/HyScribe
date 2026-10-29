// SPDX-License-Identifier: AGPL-3.0-or-later

//! The query loop: `core/query`, which gives its descendants the query as
//! block context, and `core/post-template`, which renders its inner blocks
//! once for each post the query finds.
//!
//! Of `build_query_vars_from_query_block()`, the post type, paging, sticky
//! posts, exclusions and the date order are ported; taxonomy, author,
//! search, parent and format filters, other orders and sticky posts at the
//! top of the list are not yet.

use hyscribe_html::TagProcessor;
use hyscribe_kses::esc_attr;
use hyscribe_model::{Post, PostQuery};
use hyscribe_phpser::{Value, absint, decode, isset, php_trim};

use crate::blocks::Block;
use crate::core_blocks::{Context, QueryContext, prepared};
use crate::elements::elements_class;
use crate::formatting::sanitize_title;
use crate::post::post_class;
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

impl Renderer<'_> {
    /// `render_block_core_query()`, which renders the saved markup with the
    /// query as context.
    pub(crate) fn query(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/query", parsed);
        let id = attributes
            .get_str("queryId")
            .filter(|v| **v != Value::Null)
            .cloned();
        let enhanced_pagination =
            attributes.get_str("enhancedPagination") == Some(&Value::Bool(true));
        let context = QueryContext {
            id: id.clone(),
            query: attributes.get_str("query").cloned().unwrap_or(Value::Null),
            display_layout: attributes.get_str("displayLayout").cloned(),
            enhanced_pagination,
        };
        let outer = self.context.query.replace(context);
        let content = self.saved_markup(block);
        self.context.query = outer;
        match id.filter(|_| enhanced_pagination) {
            Some(id) => {
                let id = id.to_php_string();
                let mut tags = TagProcessor::new(&content);
                // Off a tag, as when there is none, set_attribute() does nothing.
                tags.next_tag(None, None);
                tags.set_attribute(b"data-wp-interactive", b"core/query");
                tags.set_attribute(b"data-wp-router-region", &[b"query-", &id[..]].concat());
                tags.set_attribute(b"data-wp-context", b"{}");
                tags.set_attribute(b"data-wp-key", &id);
                tags.get_updated_html().to_vec()
            }
            None => content,
        }
    }

    /// `render_block_core_post_template()` on the first page.
    pub(crate) fn post_template(&mut self, block: &Block) -> Vec<u8> {
        let query = self.context.query.clone().unwrap_or_default();
        let posts = query_posts(&self.context);
        if posts.is_empty() {
            return Vec::new();
        }
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/post-template", parsed);
        let mut classes = Vec::new();
        let flex = query.display_layout.as_ref().filter(|layout| {
            layout.get_str("type").map(Value::to_php_string).as_deref() == Some(b"flex")
        });
        if let Some(layout) = flex {
            let columns = layout
                .get_str("columns")
                .map(Value::to_php_string)
                .unwrap_or_default();
            classes.extend_from_slice(b"is-flex-container columns-");
            classes.extend_from_slice(&columns);
        }
        if isset(attributes.get_path(&["style", "elements", "link", "color", "text"])) {
            classes.extend_from_slice(b" has-link-color");
        }
        let grid = attributes
            .get_path(&["layout", "type"])
            .map(Value::to_php_string)
            .as_deref()
            == Some(b"grid");
        let columns = attributes
            .get_path(&["layout", "columnCount"])
            .filter(|v| !v.is_empty());
        if let (true, Some(columns)) = (grid, columns) {
            classes.push(b' ');
            classes.extend(sanitize_title(
                &[b"columns-", &columns.to_php_string()[..]].concat(),
            ));
            if attributes
                .get_path(&["layout", "minimumColumnWidth"])
                .is_some_and(|v| !v.is_empty())
            {
                classes.extend_from_slice(b" has-native-responsive-grid");
            }
        }
        let extra = [("class", Value::String(php_trim(&classes).to_vec()))];
        let wrapper = get_block_wrapper_attributes(
            "core/post-template",
            parsed,
            &extra,
            &self.context.theme.global,
        );
        let outer = self.context.post_id;
        let outer_loop = self.context.loading.in_the_loop;
        self.context.loading.in_the_loop |= query
            .query
            .get_str("inherit")
            .is_some_and(|v| !v.is_empty());
        let mut items = Vec::new();
        for post in &posts {
            self.context.post_id = Some(post.id);
            let class = post_class(
                self.context.site,
                self.context.request,
                "wp-block-post",
                post,
            );
            let key = if query.enhanced_pagination {
                format!(" data-wp-key=\"post-template-item-{}\"", post.id)
            } else {
                String::new()
            };
            items.extend_from_slice(format!("<li{key} class=\"{}\">", esc_attr(&class)).as_bytes());
            // Each post renders as a `core/null` block with the template's
            // attributes, whose `render_block` filters see its class.
            let content = self.saved_markup(block);
            items.extend(elements_class(content, block));
            items.extend_from_slice(b"</li>");
        }
        self.context.post_id = outer;
        self.context.loading.in_the_loop = outer_loop;
        [
            b"<ul ".as_slice(),
            wrapper.as_bytes(),
            b">",
            &items,
            b"</ul>",
        ]
        .concat()
    }
}

/// The posts a query block's descendants list: the main query's when the
/// query inherits it, else those its own arguments find.
pub(crate) fn query_posts(context: &Context<'_>) -> Vec<Post> {
    if inherits(context) {
        context.request.posts.clone()
    } else {
        let (_, page) = own_page(context);
        context
            .site
            .query(&query_vars(context, own_query(context), page))
    }
}

/// The page of a query of its own: the `query-<queryId>-page` request
/// argument (`query-page` without an ID), 1 when it is empty.
pub(crate) fn own_page(context: &Context<'_>) -> (Vec<u8>, i64) {
    let id = context.query.as_ref().and_then(|q| q.id.as_ref());
    let key = id.map_or_else(
        || b"query-page".to_vec(),
        |id| [b"query-", &id.to_php_string()[..], b"-page"].concat(),
    );
    let name = String::from_utf8_lossy(&key).into_owned();
    let page = context
        .request
        .get
        .get_str(&name)
        .filter(|v| !v.is_empty())
        .map_or(1, Value::to_int);
    (key, page)
}

/// A query of its own at `page`: `found_posts`, `posts_per_page` and
/// `max_num_pages`, with `found_posts` 0 when the page has no posts.
pub(crate) fn own_counts(context: &Context<'_>, page: i64) -> (u64, i64, i64) {
    let vars = query_vars(context, own_query(context), page);
    let found = if context.site.query(&vars).is_empty() {
        0
    } else {
        context.site.count_posts(&vars)
    };
    let max = found.div_ceil(u64::from(vars.limit).max(1));
    (
        found,
        i64::from(vars.limit),
        i64::try_from(max).unwrap_or(i64::MAX),
    )
}

/// The `query.inherit` context: the query block lists the main query's posts.
pub(crate) fn inherits(context: &Context<'_>) -> bool {
    own_query(context)
        .get_str("inherit")
        .is_some_and(|v| !v.is_empty())
}

/// The `query` context, the query block's arguments.
pub(crate) fn own_query<'a>(context: &'a Context<'_>) -> &'a Value {
    context.query.as_ref().map_or(&Value::Null, |q| &q.query)
}

/// `build_query_vars_from_query_block( $block, 1 )`, for the arguments
/// listed in the module documentation.
pub(crate) fn query_vars(context: &Context<'_>, query: &Value, page: i64) -> PostQuery {
    let site = context.site;
    let field = |name| query.get_str(name).filter(|v| **v != Value::Null);
    let post_type = field("postType")
        .filter(|v| !v.is_empty())
        .map(|v| String::from_utf8_lossy(&v.to_php_string()).into_owned())
        .filter(|t| matches!(t.as_str(), "post" | "page" | "attachment"));
    let mut vars = PostQuery {
        post_types: vec![post_type.unwrap_or_else(|| "post".into())],
        limit: absint(&Value::String(
            site.option("posts_per_page").unwrap_or_default(),
        )) as u32,
        ..PostQuery::default()
    };
    let sticky = || {
        let stored = site.option("sticky_posts").unwrap_or_default();
        ids(&decode(&stored).unwrap_or(Value::Null))
    };
    match field("sticky")
        .filter(|v| !v.is_empty())
        .map(Value::to_php_string)
        .as_deref()
    {
        Some(b"only") => vars.include = Some(sticky()),
        Some(b"exclude") => vars.exclude.extend(sticky()),
        _ => {}
    }
    if let Some(exclude) = field("exclude").filter(|v| !v.is_empty()) {
        vars.exclude.extend(ids(exclude));
    }
    let current = context
        .post_id
        .filter(|_| field("excludeCurrent").is_some_and(|v| !v.is_empty()));
    vars.exclude.extend(current);
    if let Some(per_page) = field("perPage").filter(|v| v.is_numeric()) {
        let count = absint(per_page);
        // A count of 0 is empty(), and WP_Query takes the option's.
        vars.limit = Some(count as u32).filter(|&n| n != 0).unwrap_or(vars.limit);
        let offset = field("offset").filter(|v| v.is_numeric()).map_or(0, absint);
        // The page's offset, of which WP_Query takes absint().
        let skipped = i64::try_from(count)
            .unwrap_or(i64::MAX)
            .saturating_mul(page - 1);
        let start = skipped.saturating_add(i64::try_from(offset).unwrap_or(i64::MAX));
        vars.offset = u32::try_from(start.unsigned_abs()).unwrap_or(u32::MAX);
    }
    vars.ascending = field("order").is_some_and(|v| v.to_php_string().eq_ignore_ascii_case(b"asc"));
    vars
}

/// `array_filter( array_map( 'intval', $ids ) )`.
fn ids(list: &Value) -> Vec<u64> {
    list.as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(|(_, id)| u64::try_from(id.to_int()).ok().filter(|&id| id != 0))
        .collect()
}

#[cfg(test)]
mod tests;
