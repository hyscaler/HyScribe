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
            items.extend(self.saved_markup(block));
            items.extend_from_slice(b"</li>");
        }
        self.context.post_id = outer;
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
    let query = context.query.as_ref().map_or(&Value::Null, |q| &q.query);
    if query.get_str("inherit").is_some_and(|v| !v.is_empty()) {
        context.request.posts.clone()
    } else {
        context.site.query(&query_vars(context, query))
    }
}

/// `build_query_vars_from_query_block( $block, 1 )`, for the arguments
/// listed in the module documentation.
fn query_vars(context: &Context<'_>, query: &Value) -> PostQuery {
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
        vars.limit = absint(per_page) as u32;
        vars.offset = field("offset").filter(|v| v.is_numeric()).map_or(0, absint) as u32;
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
