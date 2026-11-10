// SPDX-License-Identifier: AGPL-3.0-or-later

//! The query loop: `core/query`, which gives its descendants the query as
//! block context, and `core/post-template`, which renders its inner blocks
//! once for each post the query finds.
//!
//! `build_query_vars_from_query_block()` is ported; of the `WP_Query` it
//! builds, the unsorted order of an `orderBy` array without an allowed key
//! is not yet.

use hyscribe_html::TagProcessor;
use hyscribe_kses::esc_attr;
use hyscribe_model::{FormatFilter, OrderBy, Post, PostQuery, TermFilter};
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
    // `found_posts` is set before the sticky posts are added.
    let unstuck = PostQuery {
        sticky: vec![],
        ..vars.clone()
    };
    let found = if context.site.query(&unstuck).is_empty() {
        0
    } else {
        context.site.count_posts(&unstuck)
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

/// `build_query_vars_from_query_block( $block, $page )`.
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
    let sticky = || sticky_posts(&site.option("sticky_posts").unwrap_or_default());
    let ignore_sticky = match field("sticky")
        .filter(|v| !v.is_empty())
        .map(Value::to_php_string)
        .as_deref()
    {
        Some(b"only") => {
            vars.include = Some(sticky());
            true
        }
        Some(b"exclude") => {
            vars.exclude.extend(sticky());
            false
        }
        Some(b"ignore") => true,
        _ => false,
    };
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
    vars.formats = field("format").and_then(format_filter);
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
    let ascending = field("order").is_some_and(|v| v.to_php_string().eq_ignore_ascii_case(b"asc"));
    vars.order = field("orderBy").map_or_else(
        || vec![(OrderBy::Date, ascending)],
        |by| order_by(by, ascending, &vars),
    );
    vars.authors = field("author").map(authors).unwrap_or_default();
    if let Some(search) = field("search").filter(|v| !v.is_empty()) {
        let text = stripslashes(&search.to_php_string());
        vars.search = search_terms(&String::from_utf8_lossy(&text));
        // Relevance first for `relevance`, or for an empty `orderBy`.
        if field("orderBy")
            .is_some_and(|by| by.is_empty() || urldecode(&by.to_php_string()) == b"relevance")
        {
            vars.order.insert(0, (OrderBy::Relevance, false));
        }
    }
    // A home list, which WP_Query puts sticky posts first in: no search, no
    // single author, and no top level taxonomy clause but `NOT IN`; formats
    // and other clauses together nest, so neither is top level.
    let home = field("search").is_none_or(Value::is_empty)
        && !matches!(field("author"), Some(&Value::Int(id)) if id > 0)
        && vars.formats.as_ref().map_or_else(
            || vars.terms.iter().all(|filter| filter.exclude),
            |_| !vars.terms.is_empty(),
        );
    if home && !ignore_sticky {
        vars.sticky = sticky();
    }
    vars
}

/// The post format filter of `format`: `standard`, then the formats of
/// `get_post_format_slugs()`, others left out.
fn format_filter(format: &Value) -> Option<FormatFilter> {
    const FORMATS: [&[u8]; 10] = [
        b"standard",
        b"aside",
        b"chat",
        b"gallery",
        b"link",
        b"image",
        b"quote",
        b"status",
        b"video",
        b"audio",
    ];
    let mut formats: Vec<Vec<u8>> = format
        .as_array()?
        .iter()
        .map(|(_, format)| format.to_php_string())
        .filter(|format| FORMATS.contains(&&format[..]))
        .collect();
    // Only the first `standard` is taken out of the list.
    let standard = formats
        .iter()
        .position(|format| format == b"standard")
        .map(|i| formats.remove(i))
        .is_some();
    let slugs: Vec<String> = formats
        .iter()
        .map(|format| format!("post-format-{}", String::from_utf8_lossy(format)))
        .collect();
    (standard || !slugs.is_empty()).then_some(FormatFilter { standard, slugs })
}

/// The post IDs of a stored `sticky_posts` option.
pub fn sticky_posts(stored: &[u8]) -> Vec<u64> {
    ids(&decode(stored).unwrap_or(Value::Null))
}

/// `WP_Query::parse_search()`'s terms: quoted phrases and words split on
/// spaces, tabs, commas and plus signs, less single letters, dashes and the
/// English stopwords; the whole search when that leaves none or more than
/// nine.
pub fn search_terms(search: &str) -> Vec<String> {
    const STOPWORDS: [&str; 29] = [
        "about", "an", "are", "as", "at", "be", "by", "com", "for", "from", "how", "in", "is",
        "it", "of", "on", "or", "that", "the", "this", "to", "was", "what", "when", "where", "who",
        "will", "with", "www",
    ];
    let asked = !search.is_empty();
    // There are no line breaks in a search field.
    let search: String = search.chars().filter(|&c| c != '\r' && c != '\n').collect();
    let terms: Vec<String> = search_matches(&search)
        .into_iter()
        .map(|term| {
            // A closed quoted phrase keeps its spaces; only one ends in a quote.
            let quoted = term.ends_with('"');
            let strip: &[char] = if quoted {
                &['"', '\'']
            } else {
                &['"', '\'', ' ']
            };
            term.trim_matches(strip).to_string()
        })
        .filter(|term| {
            let single =
                term.len() == 1 && term.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-');
            !term.is_empty()
                && term != "0"
                && !single
                && !STOPWORDS.contains(&term.to_lowercase().as_str())
        })
        .collect();
    if (terms.is_empty() && asked) || terms.len() > 9 {
        return vec![search];
    }
    terms
}

/// The matches of `/".*?("|$)|((?<=[\t ",+])|^)[^\t ",+]+/` in `search`.
fn search_matches(search: &str) -> Vec<&str> {
    let delimiter = |c: char| matches!(c, '\t' | ' ' | '"' | ',' | '+');
    let mut matches = Vec::new();
    let mut rest = search;
    // A word always follows a delimiter or the start, as the lookbehind wants.
    while let Some(c) = rest.chars().next() {
        let len = if c == '"' {
            let close = rest.char_indices().skip(1).find(|&(_, c)| c == '"');
            close.map_or(rest.len(), |(i, _)| i + 1)
        } else {
            rest.find(delimiter).unwrap_or(rest.len())
        };
        let (taken, left) = rest
            .split_at_checked(len.max(c.len_utf8()))
            .unwrap_or((rest, ""));
        if len > 0 {
            matches.push(taken);
        }
        rest = left;
    }
    matches
}

/// The `orderby` of `WP_Query` for `orderBy`: each allowed key of a
/// space separated list in `ascending` order, or of an array in its own.
/// `post__in` and `post_parent__in` count only when `query` has the list.
pub(crate) fn order_by(by: &Value, ascending: bool, query: &PostQuery) -> Vec<(OrderBy, bool)> {
    let column = |key: &[u8]| {
        order_column(&urldecode(key)).filter(|c| match c {
            OrderBy::Included => query.include.is_some(),
            OrderBy::Parents => !query.parents.is_empty(),
            _ => true,
        })
    };
    // `rand` and the `__in` keys alone take no order, which sorts ascending.
    let ascending = ascending
        || matches!(by, Value::String(key) if [&b"rand"[..], b"post__in", b"post_parent__in", b"post_name__in"].contains(&key.as_slice()));
    let order = if let Some(list) = by.as_array() {
        // parse_order(): only the string `ASC`, in any case, is ascending.
        list.iter()
            .filter_map(|(key, order)| {
                let asc = matches!(order, Value::String(o) if o.eq_ignore_ascii_case(b"asc"));
                column(&key.to_php_string()).map(|c| (c, asc))
            })
            .collect()
    } else {
        urldecode(&by.to_php_string())
            .split(|&b| b == b' ')
            .filter_map(&column)
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
pub(crate) fn stripslashes(text: &[u8]) -> Vec<u8> {
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

/// `WP_Query::parse_orderby()`'s key: a post column, a random order, or
/// the order of a list. `RAND(n)` anywhere in the key seeds the order.
fn order_column(key: &[u8]) -> Option<OrderBy> {
    if let Some(seed) = rand_seed(key) {
        return Some(OrderBy::Seeded(seed));
    }
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
        b"rand" => OrderBy::Random,
        b"post__in" => OrderBy::Included,
        b"post_parent__in" => OrderBy::Parents,
        _ => return None,
    })
}

/// The seed of the first `RAND(digits)` in `key`, in any case, as PHP's
/// `(int)` reads the digits: at most `PHP_INT_MAX`.
fn rand_seed(key: &[u8]) -> Option<u64> {
    let lower = key.to_ascii_lowercase();
    lower.windows(5).enumerate().find_map(|(i, w)| {
        let digits: Vec<u8> = (w == b"rand(")
            .then(|| lower.get(i + 5..))
            .flatten()?
            .iter()
            .copied()
            .take_while(u8::is_ascii_digit)
            .collect();
        let close = lower.get(i + 5 + digits.len()) == Some(&b')');
        (close && !digits.is_empty()).then(|| {
            digits
                .iter()
                .fold(0_u64, |n, d| {
                    n.saturating_mul(10).saturating_add(u64::from(d - b'0'))
                })
                .min(i64::MAX as u64)
        })
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
