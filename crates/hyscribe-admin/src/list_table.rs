// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_List_Table`: the parts every admin list shares.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_phpser::{add_query_arg, is_empty_string, remove_query_arg};
use hyscribe_render::date::MONTHS;
use hyscribe_render::number_format;

use crate::menu::esc_link;

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// A view link: its class, URL, label and whether it is the current one.
pub struct View {
    /// The `<li>`'s class.
    pub class: String,
    /// Where it links, a relative `wp-admin` URL in its PHP form.
    pub url: String,
    /// Its label, as markup.
    pub label: String,
    /// The view shown.
    pub current: bool,
}

/// `views()` with its screen reader `heading`.
pub fn views(heading: &str, views: &[View]) -> String {
    let links: Vec<String> = views
        .iter()
        .map(|view| {
            let current = if view.current {
                " class=\"current\" aria-current=\"page\""
            } else {
                ""
            };
            format!(
                "\t<li class='{}'><a href=\"{}\"{current}>{}</a>",
                view.class,
                esc_link(&view.url),
                view.label
            )
        })
        .collect();
    format!(
        "<h2 class='screen-reader-text'>{heading}</h2><ul class='subsubsub'>\n{}</li>\n</ul>",
        links.join(" |</li>\n")
    )
}

/// `search_box( $text, $input_id )` of a list not searched, the requested
/// `orderby` and `order` kept in hidden fields.
pub fn search_box(label: &str, input_id: &str, order: &[(&str, &str)]) -> String {
    let mut out = String::new();
    for (name, value) in order
        .iter()
        .filter(|(_, value)| !is_empty_string(value.as_bytes()))
    {
        out.push_str(&format!(
            "<input type=\"hidden\" name=\"{name}\" value=\"{}\" />",
            esc_attr(value.as_bytes())
        ));
    }
    let id = esc_attr(format!("{input_id}-search-input").as_bytes());
    out.push_str(&format!(
        "<p class=\"search-box\">\n\t<label class=\"screen-reader-text\" for=\"{id}\">{label}:</label>\n\t<input type=\"search\" id=\"{id}\" name=\"s\" value=\"\" />\n\t\t<input type=\"submit\" id=\"search-submit\" class=\"button button-compact\" value=\"{}\"  /></p>\n\t\t",
        esc_attr(label.as_bytes())
    ));
    out
}

/// `bulk_actions( $which )`: `actions` are values and labels; the bottom
/// copy names its fields with a `2`.
pub fn bulk_actions(which: &str, actions: &[(&str, &str)]) -> String {
    let two = if which == "bottom" { "2" } else { "" };
    let mut out = format!(
        "<label for=\"bulk-action-selector-{which}\" class=\"screen-reader-text\">Select bulk action</label><select name=\"action{two}\" id=\"bulk-action-selector-{which}\">\n<option value=\"-1\">Bulk actions</option>\n"
    );
    for (value, label) in actions {
        let class = if *value == "edit" {
            " class=\"hide-if-no-js\""
        } else {
            ""
        };
        out.push_str(&format!(
            "\t<option value=\"{}\"{class}>{label}</option>\n",
            esc_attr(value.as_bytes())
        ));
    }
    out.push_str(&format!(
        "</select>\n<input type=\"submit\" name=\"bulk_action\" id=\"doaction{two}\" class=\"button action button-compact\" value=\"Apply\"  />\n"
    ));
    out
}

/// `row_actions( $actions )`: the links under a row's primary column.
pub fn row_actions(actions: &[(&str, String)]) -> String {
    if actions.is_empty() {
        return String::new();
    }
    let count = actions.len();
    let spans: String = actions
        .iter()
        .enumerate()
        .map(|(i, (action, link))| {
            let separator = if i + 1 < count { " | " } else { "" };
            format!("<span class='{action}'>{link}{separator}</span>")
        })
        .collect();
    format!(
        "<div class=\"row-actions\">{spans}</div><button type=\"button\" class=\"toggle-row\"><span class=\"screen-reader-text\">Show more details</span></button>"
    )
}

/// `months_dropdown()` of `months`, years and months newest first, all
/// dates chosen.
pub fn months_dropdown(months: &[(u32, u32)], filter_label: &str) -> String {
    let months: Vec<&(u32, u32)> = months.iter().filter(|(year, _)| *year != 0).collect();
    if months.is_empty() {
        return String::new();
    }
    let mut out = format!(
        "\t\t<label for=\"filter-by-date\" class=\"screen-reader-text\">{filter_label}</label>\n\t\t<select name=\"m\" id=\"filter-by-date\">\n\t\t\t<option selected='selected' value=\"0\">All dates</option>\n\t\t"
    );
    for (year, month) in months {
        let value = format!("{year}{month:02}");
        let name = MONTHS
            .get(usize::try_from(*month).unwrap_or_default().wrapping_sub(1))
            .copied()
            .unwrap_or_default();
        out.push_str(&format!(
            "<option  value='{value}'>{}</option>\n",
            esc_html(format!("{name} {year}").as_bytes())
        ));
    }
    out.push_str("\t\t</select>\n\t\t");
    out
}

/// `pagination( $which )`: `total` items over `pages` pages, page `current`
/// of `url`, the page's without its removable arguments.
pub fn pagination(which: &str, total: u64, pages: u64, current: u64, url: &str) -> String {
    if total == 0 {
        return "<div class=\"tablenav-pages no-pages\"><span class=\"displaying-num\">0 items</span></div>".into();
    }
    let more = pages > 1;
    let mut out = String::new();
    if which == "top" && more {
        out.push_str("<h2 class='screen-reader-text'>Posts list navigation</h2>");
    }
    let items = if total == 1 { "item" } else { "items" };
    let page_url = |page: Option<u64>| {
        let url = match page {
            None => remove_query_arg(&[b"paged"], url.as_bytes()),
            Some(page) => add_query_arg(
                &[(b"paged", Some(page.to_string().as_bytes()))],
                url.as_bytes(),
            ),
        };
        text(&esc_url(&url))
    };
    let link = |class: &str, label: &str, glyph: &str, page| {
        format!(
            "<a class='{class} button' href='{}'><span class='screen-reader-text'>{label}</span><span aria-hidden='true'>{glyph}</span></a>",
            page_url(page)
        )
    };
    let disabled = |glyph: &str| {
        format!(
            "<span class=\"tablenav-pages-navspan button disabled\" aria-hidden=\"true\">{glyph}</span>"
        )
    };
    let first = current == 1;
    let last = current == pages;
    let here = if which == "bottom" {
        format!(
            "<span class=\"screen-reader-text\">Current Page</span><span id=\"table-paging\" class=\"paging-input\"><span class=\"tablenav-paging-text\">{current}"
        )
    } else {
        format!(
            "<span class=\"paging-input\"><label for=\"current-page-selector\" class=\"screen-reader-text\">Current Page</label><input class='current-page' id='current-page-selector' type='text'\n\t\t\t\t\tname='paged' value='{current}' size='{}' aria-describedby='table-paging' /><span class='tablenav-paging-text'>",
            pages.to_string().len()
        )
    };
    let links = [
        if first {
            disabled("&laquo;")
        } else {
            link("first-page", "First page", "&laquo;", None)
        },
        if first {
            disabled("&lsaquo;")
        } else {
            link("prev-page", "Previous page", "&lsaquo;", Some(current - 1))
        },
        format!(
            "{here} of <span class='total-pages'>{}</span></span></span>",
            number_format(pages)
        ),
        if last {
            disabled("&rsaquo;")
        } else {
            link("next-page", "Next page", "&rsaquo;", Some(current + 1))
        },
        if last {
            disabled("&raquo;")
        } else {
            link("last-page", "Last page", "&raquo;", Some(pages))
        },
    ];
    let class = if more { "" } else { " one-page" };
    out.push_str(&format!(
        "<div class='tablenav-pages{class}'><span class=\"displaying-num\">{} {items}</span>\n<span class='pagination-links'>{}</span></div>",
        number_format(total),
        links.join("\n")
    ));
    out
}

/// A column: its key, its label as markup, and how it sorts.
pub struct Column {
    /// The column's key, its class and ID.
    pub key: &'static str,
    /// Its heading, as markup.
    pub label: String,
    /// `get_sortable_columns()`'s entry, if it sorts.
    pub sort: Option<Sort>,
}

/// A sortable column: `orderby`, whether it sorts descending first, its
/// `abbr` and caption, and its order in the initial view if sorted by it.
pub struct Sort {
    /// The `orderby` value.
    pub orderby: &'static str,
    /// Sorted descending first.
    pub desc_first: bool,
    /// The heading's `abbr`.
    pub abbr: &'static str,
    /// The table's caption when sorted by it.
    pub caption: &'static str,
    /// `asc` or `desc` when the initial view sorts by it.
    pub initial: Option<&'static str>,
}

/// The requested order: `$_GET['orderby']` and whether `$_GET['order']` is
/// `desc`.
#[derive(Default)]
pub struct Order<'a> {
    /// `$_GET['orderby']`, if given.
    pub orderby: Option<&'a str>,
    /// `$_GET['order']` is `desc`.
    pub desc: bool,
}

/// The sorted column's `orderby` and direction: the requested one, or the
/// initial view's.
fn current<'a>(columns: &'a [Column], order: &Order<'a>) -> (&'a str, &'a str) {
    let direction = if order.desc { "desc" } else { "asc" };
    match order.orderby.filter(|orderby| !orderby.is_empty()) {
        Some(orderby) => (orderby, direction),
        None => columns
            .iter()
            .filter_map(|column| column.sort.as_ref())
            .find_map(|sort| sort.initial.map(|initial| (sort.orderby, initial)))
            .unwrap_or(("", direction)),
    }
}

/// `print_column_headers( $with_id )` sorted by `order` on `url`, the page's
/// without `paged`; `counter` numbers the select all box.
pub fn column_headers(
    columns: &[Column],
    hidden: &[&str],
    order: &Order,
    url: &str,
    counter: u32,
    with_id: bool,
) -> String {
    let (current, direction) = current(columns, order);
    let primary = columns
        .iter()
        .find(|column| column.key != "cb")
        .map_or("", |column| column.key);
    let mut out = String::new();
    for column in columns {
        let key = column.key;
        let mut class = vec!["manage-column".to_string(), format!("column-{key}")];
        if hidden.contains(&key) {
            class.push("hidden".into());
        }
        if key == "cb" {
            class.push("check-column".into());
        } else if matches!(key, "posts" | "comments" | "links") {
            class.push("num".into());
        }
        if key == primary {
            class.push("column-primary".into());
        }
        let mut label = column.label.clone();
        let (mut aria, mut abbr) = (String::new(), String::new());
        if key == "cb" {
            label = format!(
                "<input id=\"cb-select-all-{counter}\" type=\"checkbox\" />\n\t\t\t<label for=\"cb-select-all-{counter}\"><span class=\"screen-reader-text\">Select All</span></label>"
            );
        }
        if let Some(sort) = &column.sort {
            let mut order_text = "";
            let next = if current == sort.orderby {
                let (next, sorted) = if direction == "asc" {
                    ("desc", "ascending")
                } else {
                    ("asc", "descending")
                };
                aria = format!(" aria-sort=\"{sorted}\"");
                class.push("sorted".into());
                class.push(direction.into());
                next
            } else {
                let (next, other) = if sort.desc_first {
                    ("desc", "asc")
                } else {
                    ("asc", "desc")
                };
                class.push("sortable".into());
                class.push(other.into());
                order_text = if sort.desc_first {
                    " <span class=\"screen-reader-text\">Sort descending.</span>"
                } else {
                    " <span class=\"screen-reader-text\">Sort ascending.</span>"
                };
                next
            };
            abbr = format!(" abbr=\"{}\"", esc_attr(sort.abbr.as_bytes()));
            let href = add_query_arg(
                &[
                    (b"orderby", Some(sort.orderby.as_bytes())),
                    (b"order", Some(next.as_bytes())),
                ],
                url.as_bytes(),
            );
            label = format!(
                "<a href=\"{}\"><span>{label}</span><span class=\"sorting-indicators\"><span class=\"sorting-indicator asc\" aria-hidden=\"true\"></span><span class=\"sorting-indicator desc\" aria-hidden=\"true\"></span></span>{order_text}</a>",
                text(&esc_url(&href))
            );
        }
        let (tag, scope) = if key == "cb" {
            ("td", "")
        } else {
            ("th", "scope=\"col\"")
        };
        let id = if with_id {
            format!("id='{key}'")
        } else {
            String::new()
        };
        out.push_str(&format!(
            "<{tag} {scope} {id} class='{}' {aria} {abbr}>{label}</{tag}>",
            class.join(" ")
        ));
    }
    out
}

/// `print_table_description()`: the caption naming the sorted column.
pub fn table_description(columns: &[Column], order: &Order) -> String {
    let (current, direction) = current(columns, order);
    columns
        .iter()
        .filter_map(|column| column.sort.as_ref())
        .find(|sort| sort.orderby == current)
        .map_or_else(String::new, |sort| {
            let direction = if direction == "asc" {
                "Ascending."
            } else {
                "Descending."
            };
            format!(
                "<caption class=\"screen-reader-text\">{} {direction}</caption>",
                sort.caption
            )
        })
}
