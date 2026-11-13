// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_List_Table`: the parts every admin list shares.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_phpser::add_query_arg;
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

/// `search_box( $text, $input_id )` of a list not searched or sorted.
pub fn search_box(label: &str, input_id: &str) -> String {
    let id = esc_attr(format!("{input_id}-search-input").as_bytes());
    format!(
        "<p class=\"search-box\">\n\t<label class=\"screen-reader-text\" for=\"{id}\">{label}:</label>\n\t<input type=\"search\" id=\"{id}\" name=\"s\" value=\"\" />\n\t\t<input type=\"submit\" id=\"search-submit\" class=\"button button-compact\" value=\"{}\"  /></p>\n\t\t",
        esc_attr(label.as_bytes())
    )
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

/// `pagination( $which )` of the first page: `total` items over `pages`
/// pages of `url`.
pub fn pagination(which: &str, total: u64, pages: u64, url: &str) -> String {
    if total == 0 {
        return "<div class=\"tablenav-pages no-pages\"><span class=\"displaying-num\">0 items</span></div>".into();
    }
    let more = pages > 1;
    let mut out = String::new();
    if which == "top" && more {
        out.push_str("<h2 class='screen-reader-text'>Posts list navigation</h2>");
    }
    let items = if total == 1 { "item" } else { "items" };
    let link = |class: &str, label: &str, glyph: &str, page: u64| {
        let url = add_query_arg(
            &[(b"paged", Some(page.to_string().as_bytes()))],
            url.as_bytes(),
        );
        format!(
            "<a class='{class} button' href='{}'><span class='screen-reader-text'>{label}</span><span aria-hidden='true'>{glyph}</span></a>",
            text(&esc_url(&url))
        )
    };
    let disabled = |glyph: &str| {
        format!(
            "<span class=\"tablenav-pages-navspan button disabled\" aria-hidden=\"true\">{glyph}</span>"
        )
    };
    let current = if which == "bottom" {
        "<span class=\"screen-reader-text\">Current Page</span><span id=\"table-paging\" class=\"paging-input\"><span class=\"tablenav-paging-text\">1".to_string()
    } else {
        format!(
            "<span class=\"paging-input\"><label for=\"current-page-selector\" class=\"screen-reader-text\">Current Page</label><input class='current-page' id='current-page-selector' type='text'\n\t\t\t\t\tname='paged' value='1' size='{}' aria-describedby='table-paging' /><span class='tablenav-paging-text'>",
            pages.to_string().len()
        )
    };
    let mut links = vec![
        disabled("&laquo;"),
        disabled("&lsaquo;"),
        format!(
            "{current} of <span class='total-pages'>{}</span></span></span>",
            number_format(pages)
        ),
    ];
    if more {
        links.push(link("next-page", "Next page", "&rsaquo;", 2));
        links.push(link("last-page", "Last page", "&raquo;", pages));
    } else {
        links.push(disabled("&rsaquo;"));
        links.push(disabled("&raquo;"));
    }
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

/// A sortable column: `orderby`, its `abbr` and caption, and whether the
/// initial view sorts by it, descending; others sort ascending first.
pub struct Sort {
    /// The `orderby` value.
    pub orderby: &'static str,
    /// The heading's `abbr`.
    pub abbr: &'static str,
    /// The table's caption when sorted by it.
    pub caption: &'static str,
    /// The initial view sorts by it, newest first.
    pub initial: bool,
}

/// `print_column_headers( $with_id )` of the initial view, the first column
/// after the checkbox the primary: `counter` numbers the select all box.
pub fn column_headers(
    columns: &[Column],
    hidden: &[&str],
    url: &str,
    counter: u32,
    with_id: bool,
) -> String {
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
            let next = if sort.initial {
                aria = " aria-sort=\"descending\"".into();
                class.push("sorted".into());
                class.push("desc".into());
                "asc"
            } else {
                class.push("sortable".into());
                class.push("desc".into());
                order_text = " <span class=\"screen-reader-text\">Sort ascending.</span>";
                "asc"
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

/// `print_table_description()` of the initial view.
pub fn table_description(columns: &[Column]) -> String {
    columns
        .iter()
        .filter_map(|column| column.sort.as_ref())
        .find(|sort| sort.initial)
        .map_or_else(String::new, |sort| {
            format!(
                "<caption class=\"screen-reader-text\">{} Descending.</caption>",
                sort.caption
            )
        })
}
