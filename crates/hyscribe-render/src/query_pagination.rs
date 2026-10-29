// SPDX-License-Identifier: AGPL-3.0-or-later

//! The query pagination blocks of a query that inherits the main one, with
//! plain permalinks.

use hyscribe_kses::{esc_html, esc_url};
use hyscribe_phpser::{
    Key, Value, add_query_arg, add_query_values, parse_str, remove_query_arg, urlencode_deep,
};

use crate::blocks::Block;
use crate::core_blocks::{prepared, render};
use crate::formatting::{encode_ampersands, html_entity_decode};
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
        // get_previous_posts_link()
        if !self.inherits() || is_single(request) || request.paged <= 1 {
            return Vec::new();
        }
        let name = "core/query-pagination-previous";
        let (wrapper, label) = self.link_parts(name, block, b"Previous Page");
        let label = [self.query_arrow("previous"), label].concat();
        let href = esc_url(&pagenum_link(request, &self.home(), request.paged - 1));
        link(&href, &wrapper, &label)
    }

    /// `render_block_core_query_pagination_next()`.
    pub(crate) fn query_pagination_next(&self, block: &Block) -> Vec<u8> {
        let request = self.context.request;
        // get_next_posts_link(), up to the query's `pages` if fewer
        let next = request.paged.max(1) + 1;
        if !self.inherits() || is_single(request) || next > self.total() {
            return Vec::new();
        }
        let name = "core/query-pagination-next";
        let (wrapper, label) = self.link_parts(name, block, b"Next Page");
        let label = [label, self.query_arrow("next")].concat();
        let href = esc_url(&pagenum_link(request, &self.home(), next));
        link(&href, &wrapper, &label)
    }

    /// `render_block_core_query_pagination_numbers()`.
    pub(crate) fn query_pagination_numbers(&self, block: &Block) -> Vec<u8> {
        if !self.inherits() {
            return Vec::new();
        }
        let name = "core/query-pagination-numbers";
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let mid_size = prepared(name, parsed)
            .get_str("midSize")
            .map_or(2, Value::to_int);
        let numbers = paginate_links(self.context.request, &self.home(), self.total(), mid_size);
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

    /// The `query.inherit` context.
    fn inherits(&self) -> bool {
        self.context
            .query
            .as_ref()
            .and_then(|q| q.query.get_str("inherit"))
            .is_some_and(|v| !v.is_empty())
    }

    /// The pages there are: `max_num_pages`, or the query's `pages` when
    /// that is set and fewer.
    fn total(&self) -> u64 {
        let pages = self
            .context
            .query
            .as_ref()
            .and_then(|q| q.query.get_str("pages"))
            .map_or(0, Value::to_int);
        let max = self.context.request.max_pages;
        // A negative count is no fallback, and leaves no pages.
        match u64::try_from(pages) {
            Ok(0) => max,
            Ok(pages) => pages.min(max),
            Err(_) => 0,
        }
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
        &encode_ampersands(label, false),
        b"</a>",
    ]
    .concat()
}

/// `get_pagenum_link( $page )` with plain permalinks: the request without
/// `paged`, on the home URL, with `paged` set past the first page.
fn pagenum_link(request: &Request, home: &[u8], page: u64) -> Vec<u8> {
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

/// `paginate_links()` without previous and next links: the page numbers,
/// current and ends shown, `mid_size` either side of the current page.
fn paginate_links(request: &Request, home: &[u8], total: u64, mid_size: i64) -> Vec<u8> {
    if total < 2 {
        return Vec::new();
    }
    let first = html_entity_decode(&pagenum_link(request, home, 1));
    let mut parts = first.split(|&b| b == b'?');
    let mut base = parts.next().unwrap_or_default().to_vec();
    if base.last() != Some(&b'/') {
        base.push(b'/');
    }
    // The current query, without `paged`, goes on every link.
    let query = urlencode_deep(parse_str(parts.next().unwrap_or_default()));
    let args: Vec<(Key, Value)> = query
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter(|(key, _)| *key != Key::String(b"paged".to_vec()))
        .cloned()
        .collect();
    let current = request.paged.max(1);
    let mid = u64::try_from(mid_size).unwrap_or(2);
    let mut links: Vec<Vec<u8>> = Vec::new();
    let mut dots = false;
    for n in 1..=total {
        if n == current {
            links.push(
                format!(
                    "<span aria-current=\"page\" class=\"page-numbers current\">{}</span>",
                    thousands(n)
                )
                .into_bytes(),
            );
            dots = true;
        } else if n <= 1 || (n + mid >= current && n <= current + mid) || n > total - 1 {
            let mut link = base.clone();
            if n > 1 {
                link.extend_from_slice(format!("?paged={n}").as_bytes());
            }
            let link = if args.is_empty() {
                link
            } else {
                add_query_values(&args, &link)
            };
            links.push(
                [
                    b"<a class=\"page-numbers\" href=\"".as_slice(),
                    &esc_url(&link),
                    b"\">",
                    thousands(n).as_bytes(),
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

/// `number_format_i18n( $n )` for en_US: thousands separated by commas.
fn thousands(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests;
