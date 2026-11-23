// SPDX-License-Identifier: AGPL-3.0-or-later

//! The breadcrumbs block, `render_block_core_breadcrumbs()`: the trail to
//! the request's page, through the page for posts, a page's ancestors, a
//! post's first term and its ancestors, or a date archive's year and month.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_model::{Post, Term};
use hyscribe_phpser::Value;

use super::{Block, Context, number_format};
use crate::date::MONTHS;
use crate::excerpt::wp_trim_words;
use crate::kses::wp_kses_post;
use crate::navigation::post_ancestors;
use crate::permalinks::{date_archive_link, term_link};
use crate::post::the_title;
use crate::query_pagination::pagenum_link;
use crate::site::{Archive, Site};
use crate::supports::get_block_wrapper_attributes;

/// One crumb: its label, its link, and whether the label may hold markup.
struct Item {
    label: Vec<u8>,
    url: Option<Vec<u8>>,
    html: bool,
}

impl Item {
    fn text(label: impl Into<Vec<u8>>) -> Item {
        Item {
            label: label.into(),
            url: None,
            html: false,
        }
    }

    fn link(label: impl Into<Vec<u8>>, url: Vec<u8>) -> Item {
        Item {
            label: label.into(),
            url: Some(url),
            html: false,
        }
    }
}

/// `render_block_core_breadcrumbs()`.
pub(super) fn breadcrumbs(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let on = |name| block.attribute(name).is_some_and(|v| !v.is_empty());
    let request = context.request;
    let site = context.site;
    if !on("showOnHomePage") && request.front_page {
        return Vec::new();
    }
    let var = |name: &str| request.vars.get_str(name).map_or(0, Value::to_int);
    let page_for_posts = option_id(site, "page_for_posts");
    let paged = request.is_paged();
    let page_on_front = site.option("show_on_front").as_deref() == Some(b"page");
    // `block_core_breadcrumbs_create_item()`: a link to the first page when paged.
    let first_page = |label: Vec<u8>| Item {
        url: paged.then(|| pagenum_link(request, &site.home_url(), 1)),
        ..Item::text(label)
    };
    // `block_core_breadcrumbs_create_page_number_item()` for a paged list.
    let paged_item = || paged.then(|| page_number("Page", request.paged));
    let mut items = Vec::new();
    if on("showHomeItem") {
        if !request.front_page || page_on_front && var("page") > 1 {
            items.push(Item::link("Home", [&site.home_url()[..], b"/"].concat()));
        } else {
            items.push(first_page(b"Home".to_vec()));
        }
    }
    if request.home {
        if let Some(page) = page_for_posts.and_then(|id| site.post(id)) {
            items.push(first_page(title(&page)));
        }
        items.extend(paged_item());
    } else if request.front_page {
        if var("page") > 1 {
            items.push(page_number("Page", var("page").unsigned_abs()));
        }
    } else if let Some(search) = &request.search {
        let query = wp_trim_words(esc_attr(search).as_bytes(), 10, b"&hellip;");
        let label = [b"Search results for: \"".as_slice(), &query, b"\""].concat();
        items.push(first_page(label));
        items.extend(paged_item());
    } else if request.not_found {
        items.push(Item::text("Page not found"));
    } else if request.date {
        let (year, month, day) = match var("year") {
            0 => {
                let m = request
                    .vars
                    .get_str("m")
                    .map(Value::to_php_string)
                    .unwrap_or_default();
                let part = |range: std::ops::Range<usize>| {
                    Value::String(m.get(range).unwrap_or_default().to_vec()).to_int()
                };
                (part(0..4), part(4..6), part(6..8))
            }
            year => (year, var("monthnum"), var("day")),
        };
        let year = u64::try_from(year).unwrap_or(0);
        if year != 0 {
            let label = year.to_string().into_bytes();
            match u64::try_from(month).unwrap_or(0) {
                0 => items.push(first_page(label)),
                month => {
                    items.push(Item::link(label, date_archive_link(site, year, None)));
                    // mktime() carries a month past December into the next year.
                    let name = MONTHS
                        .get(((month - 1) % 12) as usize)
                        .copied()
                        .unwrap_or_default();
                    if day == 0 {
                        items.push(first_page(name.into()));
                    } else {
                        items.push(Item::link(
                            name,
                            date_archive_link(site, year, Some((month, None))),
                        ));
                        items.push(first_page(day.to_string().into_bytes()));
                    }
                }
            }
        }
        items.extend(paged_item());
    } else if let Some(Archive::Term(taxonomy, Some(term))) = &request.archive {
        // Other archives are dates, and a term archive without its term a 404.
        items.extend(term_ancestors(site, taxonomy.name(), term));
        items.push(first_page(term.name.clone()));
        items.extend(paged_item());
    } else {
        let Some(post) = context.post_id.and_then(|id| site.post(id)) else {
            return Vec::new();
        };
        if post.post_type == "post" {
            // get_post_type_archive_link( 'post' ): the page for posts, when
            // the front page is a page, else the home URL, which is left out.
            if let Some(page) = page_for_posts
                .filter(|_| page_on_front)
                .and_then(|id| site.post(id))
            {
                items.push(Item::link(title(&page), context.permalink(&page)));
            }
            items.extend(post_terms(context, &post));
        } else {
            for id in post_ancestors(site, &post).into_iter().rev() {
                // A missing parent shows `(no title)`, unlinked.
                let page = site.post(id);
                items.push(Item {
                    label: page.as_ref().map_or_else(|| b"(no title)".to_vec(), title),
                    url: page.map(|page| context.permalink(&page)),
                    html: true,
                });
            }
        }
        let cpage = var("cpage") > 1;
        let label = title(&post);
        if var("page") > 1 || cpage {
            items.push(Item {
                html: true,
                ..Item::link(label, context.permalink(&post))
            });
            items.push(if cpage {
                page_number("Comments Page", var("cpage").unsigned_abs())
            } else {
                page_number("Page", var("page").unsigned_abs())
            });
        } else {
            items.push(Item {
                html: true,
                ..Item::text(label)
            });
        }
    }
    if !on("showCurrentItem") {
        items.pop();
    }
    if items.is_empty() {
        return Vec::new();
    }
    let separator = block
        .attribute("separator")
        .map(Value::to_php_string)
        .unwrap_or_default();
    let mut escaped = Vec::new();
    for b in separator {
        if b == b'\\' || b == b'"' {
            escaped.push(b'\\');
        }
        escaped.push(b);
    }
    let style = [b"--separator: \"".as_slice(), &escaped, b"\";"].concat();
    let extra = [
        ("style", Value::String(style)),
        ("aria-label", Value::String(b"Breadcrumbs".to_vec())),
    ];
    let wrapper =
        get_block_wrapper_attributes(block.name, block.parsed, &extra, &context.theme.global);
    let mut html = format!("<nav {wrapper}><ol>").into_bytes();
    for item in items {
        let label = if item.html {
            wp_kses_post(&item.label, &context.upload_host())
        } else {
            esc_html(&item.label).into_bytes()
        };
        match item.url.filter(|url| !url.is_empty()) {
            Some(url) => html.extend_from_slice(
                &[
                    b"<li><a href=\"".as_slice(),
                    &esc_url(&url),
                    b"\">",
                    &label,
                    b"</a></li>",
                ]
                .concat(),
            ),
            None => html.extend_from_slice(
                &[
                    b"<li><span aria-current=\"page\">".as_slice(),
                    &label,
                    b"</span></li>",
                ]
                .concat(),
            ),
        }
    }
    html.extend_from_slice(b"</ol></nav>");
    html
}

/// A page number's crumb, such as `Page 2`.
fn page_number(prefix: &str, page: u64) -> Item {
    Item::text(format!("{prefix} {}", number_format(page)))
}

/// `block_core_breadcrumbs_get_post_title()`.
fn title(post: &Post) -> Vec<u8> {
    let title = the_title(post);
    if title.is_empty() {
        b"(no title)".to_vec()
    } else {
        title
    }
}

/// An option holding a post ID, `None` when 0.
fn option_id(site: &dyn Site, name: &str) -> Option<u64> {
    let id = Value::String(site.option(name).unwrap_or_default()).to_int();
    u64::try_from(id).ok().filter(|&id| id != 0)
}

/// `block_core_breadcrumbs_get_term_ancestors_items()`: links to the
/// ancestors of a category, the top first.
fn term_ancestors(site: &dyn Site, taxonomy: &str, term: &Term) -> Vec<Item> {
    if taxonomy != "category" {
        return Vec::new();
    }
    let mut chain: Vec<Term> = Vec::new();
    let mut parent = term.parent;
    // get_ancestors() stops at a parent it has listed, so a loop lists the
    // term itself.
    while parent != 0 && !chain.iter().any(|t| t.id == parent) {
        let Some(next) = site.term(parent, taxonomy) else {
            break;
        };
        parent = next.parent;
        chain.push(next);
    }
    chain
        .iter()
        .rev()
        .map(|t| Item::link(t.name.clone(), term_link(site, taxonomy, t)))
        .collect()
}

/// `block_core_breadcrumbs_get_terms_breadcrumbs()`: the first term of the
/// post's first taxonomy with any, after its ancestors.
fn post_terms(context: &Context<'_>, post: &Post) -> Vec<Item> {
    let site = context.site;
    for taxonomy in ["category", "post_tag", "post_format"] {
        if let Some(term) = site.post_terms(post.id, taxonomy).into_iter().next() {
            let mut items = term_ancestors(site, taxonomy, &term);
            let url = term_link(site, taxonomy, &term);
            items.push(Item::link(term.name, url));
            return items;
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests;
