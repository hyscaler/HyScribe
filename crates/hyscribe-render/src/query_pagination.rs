// SPDX-License-Identifier: AGPL-3.0-or-later

//! The query pagination blocks of a query that inherits the main one, with
//! plain permalinks.

use hyscribe_kses::{esc_html, esc_url};
use hyscribe_phpser::{
    Key, Value, add_query_arg, add_query_values, parse_str, remove_query_arg, urlencode_deep,
};

use crate::blocks::Block;
use crate::core_blocks::{number_format, prepared, render};
use crate::formatting::{encode_ampersands, html_entity_decode};
use crate::query_loop::{inherits, own_counts, own_page, own_query};
use crate::site::Request;
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

impl Renderer<'_> {
    /// The query pagination block; its links get its `paginationArrow`
    /// and `showLabel` as context.
    pub(crate) fn query_pagination(&mut self, block: &Block) -> Vec<u8> {
        let name = "core/query-pagination";
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let outer = self.query_pagination.replace(prepared(name, parsed));
        let content = self.saved_markup(block);
        self.query_pagination = outer;
        render(&self.context, name, parsed, &content).unwrap_or_default()
    }

    /// `render_block_core_query_pagination_previous()`.
    pub(crate) fn query_pagination_previous(&self, block: &Block) -> Vec<u8> {
        let request = self.context.request;
        let name = "core/query-pagination-previous";
        let (wrapper, label) = self.link_parts(name, block, b"Previous Page");
        let label = [self.query_arrow("previous"), label].concat();
        if inherits(&self.context) {
            // get_previous_posts_link()
            if is_single(request) || request.paged <= 1 {
                return Vec::new();
            }
            let href = esc_url(&pagenum_link(request, &self.home(), request.paged - 1));
            return link(&href, &wrapper, &encode_ampersands(&label, false));
        }
        let (key, page) = own_page(&self.context);
        let total = self.total(own_counts(&self.context, page).2);
        if page <= 1 || page > total {
            return Vec::new();
        }
        link(&own_link(request, &key, page - 1), &wrapper, &label)
    }

    /// `render_block_core_query_pagination_next()`.
    pub(crate) fn query_pagination_next(&self, block: &Block) -> Vec<u8> {
        let request = self.context.request;
        let name = "core/query-pagination-next";
        let (wrapper, label) = self.link_parts(name, block, b"Next Page");
        let label = [label, self.query_arrow("next")].concat();
        if inherits(&self.context) {
            // get_next_posts_link(), up to the query's `pages` if fewer
            let next = request.paged.max(1) + 1;
            let max = i64::try_from(request.max_pages).unwrap_or(i64::MAX);
            if is_single(request) || i64::try_from(next).unwrap_or(i64::MAX) > self.total(max) {
                return Vec::new();
            }
            let href = esc_url(&pagenum_link(request, &self.home(), next));
            return link(&href, &wrapper, &encode_ampersands(&label, false));
        }
        // Past the query's `pages`, or on its last page, there is no next.
        let (key, page) = own_page(&self.context);
        let pages = self.query_pages();
        let max = own_counts(&self.context, page).2;
        if pages != 0 && pages <= page || max == 0 || max == page {
            return Vec::new();
        }
        link(&own_link(request, &key, page + 1), &wrapper, &label)
    }

    /// `render_block_core_query_pagination_numbers()`.
    pub(crate) fn query_pagination_numbers(&self, block: &Block) -> Vec<u8> {
        let name = "core/query-pagination-numbers";
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let mid_size = prepared(name, parsed)
            .get_str("midSize")
            .map_or(2, Value::to_int);
        let request = self.context.request;
        let home = self.home();
        let numbers = if inherits(&self.context) {
            let max = i64::try_from(request.max_pages).unwrap_or(i64::MAX);
            let numbers = Numbers {
                base: None,
                key: b"paged".to_vec(),
                args: Vec::new(),
                current: request.paged.max(1),
                total: self.total(max),
                mid_size,
            };
            numbers.links(request, &home)
        } else {
            let (key, page) = own_page(&self.context);
            let total = self.total(own_counts(&self.context, page).2);
            // `paged`, which a query that inherits reads, stays on the links.
            let paged = request
                .get
                .get_str("paged")
                .map(Value::to_int)
                .filter(|&n| n != 0);
            let args = match paged {
                Some(paged) => vec![(string_key(b"paged"), Value::Int(paged))],
                None if page != 1 => vec![(string_key(b"cst"), Value::String(Vec::new()))],
                None => Vec::new(),
            };
            let current = u64::try_from(page.max(1)).unwrap_or(1);
            let numbers = Numbers {
                base: Some(Vec::new()),
                key,
                args,
                current,
                total,
                mid_size,
            };
            numbers.links(request, &home)
        };
        if numbers.is_empty() {
            return Vec::new();
        }
        let wrapper = get_block_wrapper_attributes(name, parsed, &[], &self.context.theme.global);
        [
            b"<div ".as_slice(),
            wrapper.as_bytes(),
            b">",
            &numbers,
            b"</div>",
        ]
        .concat()
    }

    /// The query's `pages`, as `(int)` casts it.
    fn query_pages(&self) -> i64 {
        own_query(&self.context)
            .get_str("pages")
            .map_or(0, Value::to_int)
    }

    /// The pages there are: `max`, or the query's `pages` when that is set
    /// and fewer; a negative `pages` leaves none.
    fn total(&self, max: i64) -> i64 {
        let pages = self.query_pages();
        if pages == 0 { max } else { pages.min(max) }
    }

    fn home(&self) -> Vec<u8> {
        self.context.site.home_url()
    }

    /// A link's wrapper attributes and label: the `label` attribute, or
    /// `default`, which only names the link when the label is hidden.
    fn link_parts(&self, name: &str, block: &Block, default: &[u8]) -> (Vec<u8>, Vec<u8>) {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let mut wrapper =
            get_block_wrapper_attributes(name, parsed, &[], &self.context.theme.global)
                .into_bytes();
        let text = prepared(name, parsed)
            .get_str("label")
            .filter(|label| !label.is_empty())
            .map_or_else(
                || default.to_vec(),
                |l| esc_html(&l.to_php_string()).into_bytes(),
            );
        let shown = self
            .query_pagination
            .as_ref()
            .and_then(|p| p.get_str("showLabel"))
            .is_none_or(|v| !v.is_empty());
        if shown {
            (wrapper, text)
        } else {
            wrapper.extend_from_slice(&[b" aria-label=\"", &text[..], b"\""].concat());
            (wrapper, Vec::new())
        }
    }

    /// `get_query_pagination_arrow()`.
    fn query_arrow(&self, direction: &str) -> Vec<u8> {
        let arrow = self
            .query_pagination
            .as_ref()
            .and_then(|p| p.get_str("paginationArrow"));
        arrow_span("query", arrow, direction)
    }
}

/// A pagination link's arrow: `arrow` (`arrow` or `chevron`) for `block`
/// (`comments` or `query`), pointing `direction`; none for other arrows.
pub(crate) fn arrow_span(block: &str, arrow: Option<&Value>, direction: &str) -> Vec<u8> {
    let arrow = arrow.map(Value::to_php_string);
    let (symbol, kind) = match (arrow.as_deref(), direction) {
        (Some(b"arrow"), "next") => ("→", "arrow"),
        (Some(b"arrow"), _) => ("←", "arrow"),
        (Some(b"chevron"), "next") => ("»", "chevron"),
        (Some(b"chevron"), _) => ("«", "chevron"),
        _ => return Vec::new(),
    };
    format!(
        "<span class='wp-block-{block}-pagination-{direction}-arrow is-arrow-{kind}' aria-hidden='true'>{symbol}</span>"
    )
    .into_bytes()
}

/// `is_single()`: a post, not a page, is the main query's.
fn is_single(request: &Request) -> bool {
    request.singular && request.posts.first().is_some_and(|p| p.post_type != "page")
}

/// The previous or next posts link, `href` already escaped.
fn link(href: &[u8], wrapper: &[u8], label: &[u8]) -> Vec<u8> {
    [
        b"<a href=\"".as_slice(),
        href,
        b"\" ",
        wrapper,
        b">",
        label,
        b"</a>",
    ]
    .concat()
}

/// A page of a query of its own: `add_query_arg( $key, $page )` on the
/// request, escaped.
fn own_link(request: &Request, key: &[u8], page: i64) -> Vec<u8> {
    esc_url(&add_query_arg(
        &[(key, Some(page.to_string().as_bytes()))],
        &request.uri,
    ))
}

fn string_key(key: &[u8]) -> Key {
    Key::String(key.to_vec())
}

/// `get_pagenum_link( $page )` with plain permalinks: the request without
/// `paged`, on the home URL, with `paged` set past the first page.
pub(crate) fn pagenum_link(request: &Request, home: &[u8], page: u64) -> Vec<u8> {
    let uri = remove_query_arg(&[b"paged"], &request.uri);
    let path = home_path(home);
    let uri = uri
        .get(..path.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(&path))
        .and_then(|_| uri.get(path.len()..))
        .unwrap_or(&uri);
    let start = uri.iter().position(|&b| b != b'/').unwrap_or(uri.len());
    let mut base = home.strip_suffix(b"/").unwrap_or(home).to_vec();
    base.push(b'/');
    base.extend_from_slice(uri.get(start..).unwrap_or_default());
    let url = if page > 1 {
        add_query_arg(&[(b"paged", Some(page.to_string().as_bytes()))], &base)
    } else {
        base
    };
    esc_url(&url)
}

/// The path of the home URL, `parse_url( home_url() )['path'] ?? ''`.
fn home_path(home: &[u8]) -> Vec<u8> {
    // `scheme:`, the empty part before the host, the host, then the path.
    home.splitn(4, |&b| b == b'/')
        .nth(3)
        .map_or_else(Vec::new, |path| [b"/", path].concat())
}

/// `paginate_links()` without previous and next links: `base` (the request's
/// path when `None`), with `?<key>=<n>` past the first page, then `args` and
/// the request's query without `key`, on every link.
struct Numbers {
    base: Option<Vec<u8>>,
    key: Vec<u8>,
    args: Vec<(Key, Value)>,
    current: u64,
    total: i64,
    mid_size: i64,
}

impl Numbers {
    /// The page numbers, current and ends shown, `mid_size` either side of
    /// the current page.
    fn links(mut self, request: &Request, home: &[u8]) -> Vec<u8> {
        let total = u64::try_from(self.total).unwrap_or(0);
        if total < 2 {
            return Vec::new();
        }
        let (mut path, query) = current_url(request, home);
        if path.last() != Some(&b'/') {
            path.push(b'/');
        }
        let base = self.base.take().unwrap_or(path);
        // array_merge(): a later string key replaces an earlier one in
        // place; integer keys are appended, numbered from 0.
        let key = string_key(&self.key);
        let mut index = 0;
        for (name, value) in query
            .as_array()
            .unwrap_or_default()
            .iter()
            .filter(|(k, _)| *k != key)
        {
            match (name, self.args.iter_mut().find(|(k, _)| k == name)) {
                (Key::Int(_), _) => {
                    self.args.push((Key::Int(index), value.clone()));
                    index += 1;
                }
                (_, Some(arg)) => arg.1 = value.clone(),
                (_, None) => self.args.push((name.clone(), value.clone())),
            }
        }
        let mid = u64::try_from(self.mid_size).unwrap_or(2);
        page_numbers(total, self.current, mid, |n| {
            let mut link = base.clone();
            if n > 1 {
                link.extend_from_slice(
                    &[b"?", &self.key[..], b"=", n.to_string().as_bytes()].concat(),
                );
            }
            with_args(&self.args, link)
        })
    }
}

/// `paginate_links()`'s current URL, `get_pagenum_link()`: its path and
/// its query args, URL encoded.
pub(crate) fn current_url(request: &Request, home: &[u8]) -> (Vec<u8>, Value) {
    let first = html_entity_decode(&pagenum_link(request, home, 1));
    let mut parts = first.splitn(2, |&b| b == b'?');
    let path = parts.next().unwrap_or_default().to_vec();
    (
        path,
        urlencode_deep(parse_str(parts.next().unwrap_or_default())),
    )
}

/// `link` with `args` added, unchanged without any (where adding none would
/// still append a `?`).
pub(crate) fn with_args(args: &[(Key, Value)], link: Vec<u8>) -> Vec<u8> {
    if args.is_empty() {
        link
    } else {
        add_query_values(args, &link)
    }
}

/// `paginate_links()`'s page numbers, joined by new lines: page `current`
/// marked, the first and last pages and `mid` either side of `current`
/// linked to `href( n )`, dots for the gaps.
pub(crate) fn page_numbers(
    total: u64,
    current: u64,
    mid: u64,
    href: impl Fn(u64) -> Vec<u8>,
) -> Vec<u8> {
    let mut links: Vec<Vec<u8>> = Vec::new();
    let mut dots = false;
    for n in 1..=total {
        if n == current {
            let number = number_format(n);
            links.push(
                format!(
                    "<span aria-current=\"page\" class=\"page-numbers current\">{number}</span>"
                )
                .into_bytes(),
            );
            dots = true;
        } else if n <= 1
            || (current > 0 && n + mid >= current && n <= current + mid)
            || n > total - 1
        {
            links.push(
                [
                    b"<a class=\"page-numbers\" href=\"".as_slice(),
                    &esc_url(&href(n)),
                    b"\">",
                    number_format(n).as_bytes(),
                    b"</a>",
                ]
                .concat(),
            );
            dots = true;
        } else if dots {
            links.push(b"<span class=\"page-numbers dots\">&hellip;</span>".to_vec());
            dots = false;
        }
    }
    links.join(&b'\n')
}

#[cfg(test)]
mod tests;
