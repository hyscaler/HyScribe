// SPDX-License-Identifier: AGPL-3.0-or-later

//! The latest posts block, `render_block_core_latest_posts()`: a list of
//! posts from a query of its own, with their authors, dates, excerpts or
//! content.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_model::{Post, PostQuery, TermFilter};
use hyscribe_phpser::{Key, Value, absint, is_empty_string, isset};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::date;
use crate::excerpt::EXCERPT_MORE;
use crate::formatting::{CORE_SHORTCODE_TAGS, sanitize_title, wptexturize};
use crate::permalinks::permalink;
use crate::post::listed_title;
use crate::query_loop::order_by;
use crate::supports::get_block_wrapper_attributes;
use crate::template::{Renderer, replace, site_smilies};

const PROTECTED: &[u8] = b"This content is password protected.";

impl Renderer<'_> {
    /// `render_block_core_latest_posts()`.
    pub(crate) fn latest_posts(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/latest-posts", &migrate_categories(parsed));
        let get = |name| attribute(&attributes, name);
        let on = |name| on(&attributes, name);
        let site = self.context.site;
        let posts = site.query(&self.latest_posts_query(&attributes));
        let content = get("displayPostContentRadio").and_then(Value::as_bytes);
        let excerpt = on("displayPostContent") && content == Some(b"excerpt");
        let full = on("displayPostContent") && content == Some(b"full_post");
        let length = get("excerptLength").map_or(0, Value::to_int);
        let zone = self.context.zone();
        let date_format =
            String::from_utf8_lossy(&site.option("date_format").unwrap_or_default()).into_owned();
        let mut items = Vec::new();
        for post in posts {
            let link = esc_url(&permalink(site, &post));
            let title = listed_title(&post);
            items.extend_from_slice(b"<li><a class=\"wp-block-latest-posts__post-title\" href=\"");
            items.extend_from_slice(&esc_url(&link));
            items.extend_from_slice(b"\">");
            items.extend_from_slice(&title);
            items.extend_from_slice(b"</a>");
            if on("displayAuthor") {
                let name = site
                    .user(post.author)
                    .map(|u| u.display_name)
                    .unwrap_or_default();
                if !is_empty_string(&name) {
                    items.extend_from_slice(
                        b"<div class=\"wp-block-latest-posts__post-author\">by ",
                    );
                    items.extend_from_slice(&name);
                    items.extend_from_slice(b"</div>");
                }
            }
            if on("displayPostDate") {
                let iso = date::format("c", &post.date, zone);
                let shown = date::format(&date_format, &post.date, zone);
                items.extend_from_slice(
                    format!(
                        "<time datetime=\"{}\" class=\"wp-block-latest-posts__post-date\">{shown}</time>",
                        esc_attr(iso.as_bytes())
                    )
                    .as_bytes(),
                );
            }
            if excerpt {
                let mut text = self.excerpt(&post, length, EXCERPT_MORE);
                if let Some(kept) = text.strip_suffix(EXCERPT_MORE) {
                    text = [
                        kept,
                        "… <a class=\"wp-block-latest-posts__read-more\" href=\"".as_bytes(),
                        &esc_url(&link),
                        b"\" rel=\"noopener\">Read more<span class=\"screen-reader-text\">: ",
                        esc_html(&title).as_bytes(),
                        b"</span></a>",
                    ]
                    .concat();
                }
                if !post.password.is_empty() {
                    text = PROTECTED.to_vec();
                }
                items.extend_from_slice(b"<div class=\"wp-block-latest-posts__post-excerpt\">");
                items.extend_from_slice(&text);
                items.extend_from_slice(b"</div>");
            }
            if full {
                let text = self.full_content(&post);
                items
                    .extend_from_slice(b"<div class=\"wp-block-latest-posts__post-full-content\">");
                items.extend_from_slice(&replace(&text, b"]]>", b"]]&gt;"));
                items.extend_from_slice(b"</div>");
            }
            items.extend_from_slice(b"</li>\n");
        }
        let wrapper = get_block_wrapper_attributes(
            "core/latest-posts",
            parsed,
            &[("class", Value::String(list_classes(&attributes)))],
            &self.context.theme.global,
        );
        [format!("<ul {wrapper}>").as_bytes(), &items, b"</ul>"].concat()
    }

    /// The block's `WP_Query`: published posts, as many as `postsToShow`
    /// in its order, of its categories and author, no sticky ones first.
    fn latest_posts_query(&self, attributes: &Value) -> PostQuery {
        let get = |name| attributes.get_str(name).cloned().unwrap_or_default();
        let per_page = get("postsToShow");
        let per_page = if per_page.is_empty() {
            Value::String(
                self.context
                    .site
                    .option("posts_per_page")
                    .unwrap_or_default(),
            )
            .to_int()
        } else {
            per_page.to_int()
        };
        let limit = crate::query_loop::query_limit(per_page);
        let ascending = matches!(get("order"), Value::String(o) if o.eq_ignore_ascii_case(b"asc"));
        let mut categories: Vec<u64> = get("categories")
            .as_array()
            .unwrap_or_default()
            .iter()
            .filter_map(|(_, c)| c.get_str("id").map(absint))
            .collect();
        categories.sort_unstable();
        categories.dedup();
        // WP_Query keeps only digits and `-` of `author`, so 1.5 is user 15;
        // a negative ID excludes that user.
        let author: Vec<u8> = get("selectedAuthor")
            .to_php_string()
            .into_iter()
            .filter(|b| b.is_ascii_digit() || *b == b'-')
            .collect();
        let author = Value::String(author).to_int();
        PostQuery {
            post_types: vec!["post".into()],
            order: order_by(&get("orderBy"), ascending, &PostQuery::default()),
            terms: (!categories.is_empty())
                .then(|| TermFilter {
                    taxonomy: "category".into(),
                    terms: categories,
                    exclude: false,
                })
                .into_iter()
                .collect(),
            authors: (author > 0)
                .then_some(author.unsigned_abs())
                .into_iter()
                .collect(),
            excluded_authors: (author < 0)
                .then_some(author.unsigned_abs())
                .into_iter()
                .collect(),
            limit,
            ..PostQuery::default()
        }
    }

    /// `_wp_apply_block_content_filters()` on a post's raw content, empty
    /// for a post this block is already rendering.
    fn full_content(&mut self, post: &Post) -> Vec<u8> {
        if !post.password.is_empty() {
            return PROTECTED.to_vec();
        }
        if self.latest_posts_open.contains(&post.id) {
            return Vec::new();
        }
        self.latest_posts_open.push(post.id);
        let outer = self.context.post_id.replace(post.id);
        let rendered = self.render(&post.content);
        self.context.post_id = outer;
        self.latest_posts_open.pop();
        let site = self.context.site;
        site_smilies(site, &wptexturize(&rendered, &CORE_SHORTCODE_TAGS))
    }
}

/// The attribute `name`, unless it is null.
fn attribute<'a>(attributes: &'a Value, name: &str) -> Option<&'a Value> {
    attributes.get_str(name).filter(|v| **v != Value::Null)
}

/// Whether the attribute `name` is set and not PHP `empty()`.
fn on(attributes: &Value, name: &str) -> bool {
    attribute(attributes, name).is_some_and(|v| !v.is_empty())
}

/// `block_core_latest_posts_migrate_categories()`: a category ID given as
/// a string becomes the one category in the list.
fn migrate_categories(parsed: &[(Key, Value)]) -> Vec<(Key, Value)> {
    parsed
        .iter()
        .map(|(key, value)| match value {
            Value::String(id) if key.to_php_string() == b"categories" && !is_empty_string(id) => {
                let entry = vec![(
                    Key::String(b"id".to_vec()),
                    Value::Int(i64::try_from(absint(value)).unwrap_or(i64::MAX)),
                )];
                (
                    key.clone(),
                    Value::Array(vec![(Key::Int(0), Value::Array(entry))]),
                )
            }
            _ => (key.clone(), value.clone()),
        })
        .collect()
}

/// The list's classes: a grid's, with its columns, and those for dates,
/// authors and a link color.
fn list_classes(attributes: &Value) -> Vec<u8> {
    let get = |name| attribute(attributes, name);
    let on = |name| on(attributes, name);
    let layout = get("layout");
    let legacy = matches!(get("postLayout"), Some(Value::String(l)) if l == b"grid");
    let grid = match layout.and_then(|l| l.get_str("type")) {
        Some(kind) => *kind == Value::String(b"grid".to_vec()),
        None => legacy,
    };
    let columns = layout
        .and_then(|l| l.get_str("columnCount"))
        .filter(|v| **v != Value::Null)
        .or_else(|| get("columns"))
        .filter(|v| !v.is_empty());
    let mut classes = vec![b"wp-block-latest-posts__list".to_vec()];
    if grid {
        classes.push(b"is-grid".to_vec());
        if let Some(count) = columns {
            classes.push(sanitize_title(
                &[b"columns-", &count.to_php_string()[..]].concat(),
            ));
            let minimum = layout.and_then(|l| l.get_str("minimumColumnWidth"));
            if minimum.is_some_and(|m| !m.is_empty()) {
                classes.push(b"has-native-responsive-grid".to_vec());
            }
        }
    }
    if on("displayPostDate") {
        classes.push(b"has-dates".to_vec());
    }
    if on("displayAuthor") {
        classes.push(b"has-author".to_vec());
    }
    let link_color = isset(attributes.get_path(&["style", "elements", "link", "color", "text"]));
    if link_color {
        classes.push(b"has-link-color".to_vec());
    }
    classes.join(&b' ')
}

#[cfg(test)]
mod tests;
