// SPDX-License-Identifier: AGPL-3.0-or-later

//! The query loop: `core/query`, which gives its descendants the query as
//! block context, and `core/post-template`, which renders its inner blocks
//! once for each post the query finds.
//!
//! Of `build_query_vars_from_query_block()`, everything but the post format
//! filter is ported; of the `WP_Query` it builds, sticky posts at the top of
//! the list, random order, the `post__in` orders and the unsorted order of an
//! `orderBy` array without an allowed key are not yet.

use hyscribe_html::TagProcessor;
use hyscribe_kses::esc_attr;
use hyscribe_model::{OrderBy, Post, PostQuery, TermFilter};
use hyscribe_phpser::{Key, Value, absint, decode, isset, php_trim, urldecode};

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
    vars.terms = term_filters(query);
    let ascending = field("order").is_some_and(|v| v.to_php_string().eq_ignore_ascii_case(b"asc"));
    vars.order = field("orderBy").map_or_else(
        || vec![(OrderBy::Date, ascending)],
        |by| order_by(by, ascending),
    );
    vars.authors = field("author").map(authors).unwrap_or_default();
    if let Some(search) = field("search").filter(|v| !v.is_empty()) {
        // parse_search(): stripslashes(), then line breaks removed.
        let mut text = stripslashes(&search.to_php_string());
        text.retain(|&b| b != b'\r' && b != b'\n');
        vars.search = search_terms(&String::from_utf8_lossy(&text));
        // Relevance first for `relevance`, or for an empty `orderBy`.
        if field("orderBy")
            .is_some_and(|by| by.is_empty() || urldecode(&by.to_php_string()) == b"relevance")
        {
            vars.order.insert(0, (OrderBy::Relevance, false));
        }
    }
    if vars.post_types == ["page"] {
        // `array_unique( array_map( 'intval', $parents ) )`, then absint().
        vars.parents = field("parents")
            .filter(|v| !v.is_empty())
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .map(|(_, id)| id.to_int().unsigned_abs())
                    .collect()
            })
            .unwrap_or_default();
    }
    vars
}

/// Search terms as WordPress splits them: on spaces, tabs, commas and plus
/// signs. Stopword removal comes with i18n (M9.4).
pub fn search_terms(search: &str) -> Vec<String> {
    search
        .split([' ', '\t', ',', '+'])
        .filter(|term| !term.is_empty())
        .map(String::from)
        .collect()
}

/// The `orderby` of `WP_Query` for `orderBy`: each allowed key of a
/// space separated list in `ascending` order, or of an array in its own.
fn order_by(by: &Value, ascending: bool) -> Vec<(OrderBy, bool)> {
    let order = if let Some(list) = by.as_array() {
        // parse_order(): only the string `ASC`, in any case, is ascending.
        list.iter()
            .filter_map(|(key, order)| {
                let asc = matches!(order, Value::String(o) if o.eq_ignore_ascii_case(b"asc"));
                order_column(&urldecode(&key.to_php_string())).map(|c| (c, asc))
            })
            .collect()
    } else {
        urldecode(&by.to_php_string())
            .split(|&b| b == b' ')
            .filter_map(order_column)
            .map(|column| (column, ascending))
            .collect::<Vec<_>>()
    };
    // With no allowed key, a string sorts by date; an array leaves the
    // database's order, which this takes as the date order too.
    if order.is_empty() {
        return vec![(OrderBy::Date, ascending)];
    }
    order
}

/// PHP's `stripslashes()`: a backslash escapes the next byte, `\0` is NUL.
fn stripslashes(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut bytes = text.iter();
    while let Some(&b) = bytes.next() {
        match (b == b'\\').then(|| bytes.next()) {
            None => out.push(b),
            Some(Some(b'0')) => out.push(0),
            Some(Some(&next)) => out.push(next),
            Some(None) => {}
        }
    }
    out
}

/// `WP_Query::parse_orderby()` for the post columns.
fn order_column(key: &[u8]) -> Option<OrderBy> {
    Some(match key {
        b"post_date" | b"date" => OrderBy::Date,
        b"post_title" | b"title" => OrderBy::Title,
        b"post_modified" | b"modified" => OrderBy::Modified,
        b"post_name" | b"name" => OrderBy::Name,
        b"post_author" | b"author" => OrderBy::Author,
        b"post_parent" | b"parent" => OrderBy::Parent,
        b"post_type" | b"type" => OrderBy::Type,
        b"ID" => OrderBy::Id,
        b"menu_order" => OrderBy::MenuOrder,
        b"comment_count" => OrderBy::CommentCount,
        _ => return None,
    })
}

/// `author__in` from the `author` context: a list, a comma separated
/// string, or one positive integer.
fn authors(author: &Value) -> Vec<u64> {
    match author {
        Value::Array(_) => ids(author),
        Value::String(text) => text
            .split(|&b| b == b',')
            .map(|id| Value::String(id.to_vec()).to_int().unsigned_abs())
            .filter(|&id| id != 0)
            .collect(),
        &Value::Int(id) if id > 0 => vec![id.unsigned_abs()],
        _ => vec![],
    }
}

/// The taxonomies a default install registers as viewable.
const VIEWABLE_TAXONOMIES: [&[u8]; 3] = [b"category", b"post_tag", b"post_format"];

/// The `tax_query` of `categoryIds`, `tagIds` and `taxQuery`, in either
/// format of `taxQuery`: `{"category":[4]}`, or `{"include":{...},
/// "exclude":{...}}`.
fn term_filters(query: &Value) -> Vec<TermFilter> {
    let filled = |name| query.get_str(name).filter(|v| !v.is_empty());
    let clause = |taxonomy: &[u8], terms: &Value, exclude| TermFilter {
        taxonomy: String::from_utf8_lossy(taxonomy).into_owned(),
        terms: ids(terms),
        exclude,
    };
    let mut filters: Vec<TermFilter> = [
        ("categoryIds", b"category".as_slice()),
        ("tagIds", b"post_tag"),
    ]
    .into_iter()
    .filter_map(|(name, taxonomy)| filled(name).map(|terms| clause(taxonomy, terms, false)))
    .collect();
    let Some(tax_query) = filled("taxQuery").and_then(Value::as_array) else {
        return filters;
    };
    let conditions = |terms: &[(Key, Value)], exclude| {
        terms
            .iter()
            .filter(|(taxonomy, terms)| {
                !terms.is_empty() && VIEWABLE_TAXONOMIES.contains(&&taxonomy.to_php_string()[..])
            })
            .map(|(taxonomy, terms)| clause(&taxonomy.to_php_string(), terms, exclude))
            .collect::<Vec<_>>()
    };
    let part = |name: &[u8]| {
        tax_query
            .iter()
            .find(|(key, _)| *key == Key::String(name.to_vec()))
            .and_then(|(_, terms)| terms.as_array())
            .unwrap_or_default()
    };
    let old_format = tax_query
        .iter()
        .any(|(key, _)| !matches!(key, Key::String(k) if k == b"include" || k == b"exclude"));
    if old_format {
        filters.extend(conditions(tax_query, false));
    } else {
        filters.extend(conditions(part(b"include"), false));
        filters.extend(conditions(part(b"exclude"), true));
    }
    filters
}

/// `array_filter( array_map( 'intval', $ids ) )`, as `WP_Query` then reads
/// them with absint().
fn ids(list: &Value) -> Vec<u64> {
    list.as_array()
        .unwrap_or_default()
        .iter()
        .map(|(_, id)| id.to_int().unsigned_abs())
        .filter(|&id| id != 0)
        .collect()
}

#[cfg(test)]
mod tests;
