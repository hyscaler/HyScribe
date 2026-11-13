// SPDX-License-Identifier: AGPL-3.0-or-later

//! `edit-tags.php`: the categories and tags lists, `WP_Terms_List_Table`.

use std::collections::{HashMap, HashSet};

use hyscribe_kses::{esc_attr, esc_html, esc_textarea, esc_url};
use hyscribe_model::{Term, TermOrder, TermQuery};
use hyscribe_phpser::{
    Key, Maybe, Value, add_query_arg, addslashes, is_empty_string, maybe_unserialize, php_trim,
    remove_query_arg, urldecode, urlencode,
};
use hyscribe_render::autop::wpautop;
use hyscribe_render::formatting::{convert_chars, wptexturize};
use hyscribe_render::kses::wp_kses_data;
use hyscribe_render::number_format;
use hyscribe_render::term_template::parse_orderby;

use crate::admin_bar::BarSite;
use crate::admin_bar::Browser;
use crate::list_table::{
    Column, Order, Sort, bulk_actions, column_headers, fill, pagination, row_actions,
    screen_options, search_box, table_description,
};
use crate::menu::{Screen, esc_link, link};
use crate::page::{HelpTab, Page, admin_page};
use crate::posts::{Answer, PostsSite, hidden_columns, per_page, term_name, term_options};

/// What the terms lists ask of the site beyond the posts list's.
pub trait TermsSite: PostsSite {
    /// `current_user_can( $cap, $term->term_id )`.
    fn can_term(&self, cap: &str, term: &Term) -> bool;
    /// `get_term_link( $term, $taxonomy )`.
    fn term_link(&self, term: &Term, taxonomy: &str) -> String;
    /// The terms `query` selects, in its order.
    fn list_terms(&self, query: &TermQuery) -> Vec<Term>;
    /// `wp_count_terms()` of `taxonomy`, those `search` finds when given.
    fn count_terms(&self, taxonomy: &str, search: Option<&str>) -> u64;
}

/// A taxonomy the lists show, as `register_taxonomy()` labels it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Taxonomy {
    /// Its name.
    name: &'static str,
    /// `labels->name` and `labels->singular_name`.
    plural: &'static str,
    singular: &'static str,
    /// Its query var, which the count links filter the posts list by.
    query_var: &'static str,
}

/// Categories, which nest.
pub(crate) const CATEGORY: Taxonomy = Taxonomy {
    name: "category",
    plural: "Categories",
    singular: "Category",
    query_var: "category_name",
};

/// Tags.
pub(crate) const POST_TAG: Taxonomy = Taxonomy {
    name: "post_tag",
    plural: "Tags",
    singular: "Tag",
    query_var: "tag",
};

impl Taxonomy {
    /// The taxonomy the lists show by `name`, if any.
    pub fn named(name: &[u8]) -> Option<Self> {
        [CATEGORY, POST_TAG]
            .into_iter()
            .find(|taxonomy| taxonomy.name.as_bytes() == name)
    }

    fn hierarchical(self) -> bool {
        self == CATEGORY
    }

    /// Its `manage_terms` capability.
    fn manage_cap(self) -> &'static str {
        if self == CATEGORY {
            "manage_categories"
        } else {
            "manage_post_tags"
        }
    }
}

/// The terms list's query arguments this port reads.
pub struct TermArgs {
    /// `$_REQUEST['taxonomy']`.
    pub taxonomy: Taxonomy,
    /// `$_REQUEST['paged']`, if given.
    pub paged: Option<String>,
    /// `$_REQUEST['s']`, unslashed, if given.
    pub search: Option<String>,
    /// `$_REQUEST['orderby']`, unslashed, if given.
    pub orderby: Option<String>,
    /// `$_REQUEST['order']`, unslashed, if given.
    pub order: Option<String>,
}

/// `get_ancestors( $term_id, $taxonomy, 'taxonomy' )` over the terms by
/// ID: its parent's ID, then theirs, until one is missing or repeats.
fn ancestors(by_id: &HashMap<u64, &Term>, term: &Term) -> Vec<u64> {
    let mut ids = Vec::new();
    let mut seen = HashSet::new();
    let mut parent = term.parent;
    while parent != 0 && seen.insert(parent) {
        ids.push(parent);
        match by_id.get(&parent) {
            Some(above) => parent = above.parent,
            None => break,
        }
    }
    ids
}

/// `column_description()`: the description through `term_description`'s
/// filters, the admin's `wp_kses_data()` first; shortcodes are not ported.
fn column_description(text: &[u8]) -> String {
    let text = convert_chars(&wptexturize(&wp_kses_data(text), &[]));
    let text = wpautop(&text, true);
    if is_empty_string(&text) {
        return "<span aria-hidden=\"true\">&#8212;</span><span class=\"screen-reader-text\">No description</span>".into();
    }
    String::from_utf8_lossy(&text).into_owned()
}

/// The list being printed: its taxonomy, every term and the request.
struct Listing<'a> {
    site: &'a dyn TermsSite,
    taxonomy: Taxonomy,
    /// The terms listed, and the taxonomy's by ID where their parents count.
    terms: &'a [Term],
    by_id: HashMap<u64, &'a Term>,
    /// The parents `{$taxonomy}_children` caches, which alone are opened.
    cached: Option<HashSet<u64>>,
    columns: &'a [Column],
    hidden: &'a [&'a str],
    /// The request's path and query, which edit links come back to.
    request_uri: &'a str,
}

impl Listing<'_> {
    /// `single_row( $tag, $level )`.
    fn row(&self, term: &Term, level: usize) -> String {
        let name = term_name(&term.name);
        let mut out = format!(
            "<tr id=\"tag-{}\" class=\"level-{}\">",
            term.id,
            ancestors(&self.by_id, term).len()
        );
        for column in self.columns {
            let key = column.key;
            if key == "cb" {
                out.push_str(&format!(
                    "<td class=\"check-column\">{}</td>",
                    self.column_cb(term, &name)
                ));
                continue;
            }
            let mut classes = format!("{key} column-{key}");
            let primary = key == "name";
            if primary {
                classes.push_str(" has-row-actions column-primary");
            }
            if self.hidden.contains(&key) {
                classes.push_str(" hidden");
            }
            let label = hyscribe_kses::wp_strip_all_tags(column.label.as_bytes(), false);
            let attributes = format!("class='{classes}' data-colname=\"{}\"", esc_attr(&label));
            if primary {
                let label = if name.is_empty() {
                    String::new()
                } else {
                    format!(" aria-label=\"{}\"", esc_attr(name.as_bytes()))
                };
                out.push_str(&format!(
                    "<th {attributes} scope=\"row\"{label}>{}{}</th>",
                    self.column_name(term, level),
                    self.actions(term, &name)
                ));
                continue;
            }
            let cell = match key {
                "description" => column_description(&term.description),
                "slug" => editable_slug(&term.slug),
                _ => self.column_posts(term),
            };
            out.push_str(&format!("<td {attributes}>{cell}</td>"));
        }
        out.push_str("</tr>");
        out
    }

    /// `column_cb()`: a box for a term the user may delete.
    fn column_cb(&self, term: &Term, name: &str) -> String {
        if !self.site.can_term("delete_term", term) {
            return "&nbsp;".into();
        }
        let id = term.id;
        format!(
            "<input type=\"checkbox\" name=\"delete_tags[]\" value=\"{id}\" id=\"cb-select-{id}\" /><label for=\"cb-select-{id}\"><span class=\"screen-reader-text\">Select {name}</span></label>"
        )
    }

    /// `get_edit_term_link()` with the request to come back to, for a term
    /// the user may edit.
    fn edit_link(&self, term: &Term) -> Option<String> {
        if !self.site.can_term("edit_term", term) {
            return None;
        }
        let id = term.id.to_string();
        let link = add_query_arg(
            &[
                (b"taxonomy", Some(self.taxonomy.name.as_bytes())),
                (b"tag_ID", Some(id.as_bytes())),
                (b"post_type", Some(b"post")),
            ],
            BarSite::admin_url(self.site, "term.php").as_bytes(),
        );
        let referer = urlencode(self.request_uri.as_bytes());
        let link = add_query_arg(&[(b"wp_http_referer", Some(&referer))], &link);
        Some(String::from_utf8_lossy(&esc_url(&link)).into_owned())
    }

    /// `column_name()`: the name, padded to `level`, and the Quick Edit data.
    fn column_name(&self, term: &Term, level: usize) -> String {
        let pad = "&#8212; ".repeat(level);
        let mut name = term_name(format!("{pad} {}", term_name(&term.name)).as_bytes());
        if let Some(link) = self.edit_link(term) {
            name = format!("<a class=\"row-title\" href=\"{link}\">{name}</a>");
        }
        format!(
            "<strong>{name}</strong><br /><div class=\"hidden\" id=\"inline_{}\"><div class=\"name\">{}</div><div class=\"slug\">{}</div><div class=\"parent\">{}</div></div>",
            term.id,
            esc_attr(&term.name),
            editable_slug(esc_attr(&term.slug).as_bytes()),
            term.parent
        )
    }

    /// `handle_row_actions()` for the name column.
    fn actions(&self, term: &Term, name: &str) -> String {
        let mut actions: Vec<(&str, String)> = Vec::new();
        let quoted = |text: &str| esc_attr(format!("{text} &#8220;{name}&#8221;").as_bytes());
        if let Some(edit) = self.edit_link(term) {
            actions.push((
                "edit",
                format!(
                    "<a href=\"{}\" aria-label=\"{}\">Edit</a>",
                    edit,
                    quoted("Edit")
                ),
            ));
            actions.push((
                "inline hide-if-no-js",
                format!(
                    "<button type=\"button\" class=\"button-link editinline\" aria-label=\"{}\" aria-expanded=\"false\">Quick&nbsp;Edit</button>",
                    esc_attr(format!("Quick edit &#8220;{name}&#8221; inline").as_bytes())
                ),
            ));
        }
        if self.site.can_term("delete_term", term) {
            let nonce = self.site.nonce(&format!("delete-tag_{}", term.id));
            actions.push((
                "delete",
                format!(
                    "<a href=\"{}\" class=\"delete-tag aria-button-if-js\" aria-label=\"{}\">Delete</a>",
                    // `wp_nonce_url()` escapes for HTML only.
                    link(&format!(
                        "edit-tags.php?action=delete&amp;taxonomy={}&amp;tag_ID={}&amp;_wpnonce={nonce}",
                        self.taxonomy.name, term.id
                    )),
                    quoted("Delete")
                ),
            ));
        }
        actions.push((
            "view",
            format!(
                "<a href=\"{}\" aria-label=\"{}\">View</a>",
                self.site.term_link(term, self.taxonomy.name),
                esc_attr(format!("View &#8220;{name}&#8221; archive").as_bytes())
            ),
        ));
        row_actions(&actions, false)
    }

    /// `column_posts()`: the count, linked to the posts list of the term.
    fn column_posts(&self, term: &Term) -> String {
        let link = add_query_arg(
            &[(self.taxonomy.query_var.as_bytes(), Some(&term.slug))],
            b"edit.php",
        );
        format!(
            "<a href='{}'>{}</a>",
            esc_link(&String::from_utf8_lossy(&link)),
            number_format(term.count)
        )
    }
    /// `_rows()` from `start` to `end`: a tree by name, a subtree after its
    /// parents and terms under unlisted parents left out; flat for a search.
    fn tree(&self, start: u64, end: u64, searching: bool) -> String {
        if searching {
            // A search lists what it finds flat.
            let skip = usize::try_from(start).unwrap_or(usize::MAX);
            let take = usize::try_from(end - start).unwrap_or(usize::MAX);
            let rows = self.terms.iter().skip(skip).take(take);
            return rows
                .map(|term| format!("\t{}", self.row(term, 0)))
                .collect();
        }
        let mut children: HashMap<u64, Vec<&Term>> = HashMap::new();
        for term in self.terms {
            children.entry(term.parent).or_default().push(term);
        }
        let mut below = |parent: u64| children.remove(&parent).unwrap_or_default().into_iter();
        let mut out = String::new();
        let mut count = 0;
        let mut stack = vec![(below(0), 0usize)];
        while let Some((siblings, level)) = stack.last_mut() {
            let level = *level;
            let Some(term) = siblings.next().filter(|_| count < end) else {
                stack.pop();
                continue;
            };
            if count == start {
                let above = ancestors(&self.by_id, term);
                // A term reached from the top has every ancestor listed.
                let parents = above.iter().rev().filter_map(|id| self.by_id.get(id));
                for (i, parent) in parents.enumerate() {
                    out.push('\t');
                    out.push_str(&self.row(parent, (level + i).saturating_sub(above.len())));
                }
            }
            if count >= start {
                out.push('\t');
                out.push_str(&self.row(term, level));
            }
            count += 1;
            // PHP opens only the parents its cached hierarchy knows.
            if self
                .cached
                .as_ref()
                .is_none_or(|parents| parents.contains(&term.id))
            {
                stack.push((below(term.id), level + 1));
            }
        }
        out
    }
}

/// `editable_slug`'s filters: the slug decoded, for a textarea.
fn editable_slug(slug: &[u8]) -> String {
    String::from_utf8_lossy(&esc_textarea(&urldecode(slug))).into_owned()
}

/// The list's columns, sorted as `get_sortable_columns()` sorts them; the
/// name's caption says whether a tree is listed.
fn columns(tree: bool) -> Vec<Column> {
    let sort = |orderby, abbr, caption, initial| {
        Some(Sort {
            orderby,
            desc_first: false,
            abbr,
            caption,
            initial,
        })
    };
    let column = |key, label: &str, sort| Column {
        key,
        label: label.into(),
        sort,
    };
    let caption = if tree {
        "Table ordered hierarchically."
    } else {
        "Table ordered by Name."
    };
    vec![
        column("cb", "<input type=\"checkbox\" />", None),
        column("name", "Name", sort("name", "Name", caption, Some("asc"))),
        column(
            "description",
            "Description",
            sort(
                "description",
                "Description",
                "Table ordered by Description.",
                None,
            ),
        ),
        column(
            "slug",
            "Slug",
            sort("slug", "Slug", "Table ordered by Slug.", None),
        ),
        column(
            "posts",
            "Count",
            sort("count", "Count", "Table ordered by Posts Count.", None),
        ),
    ]
}

/// The parents a stored `{$taxonomy}_children` caches, as
/// `_get_term_hierarchy()` reads it; none when it is not an array.
fn cached_parents(stored: &[u8]) -> Option<HashSet<u64>> {
    match maybe_unserialize(stored) {
        Maybe::Value(Value::Array(parents)) => Some(
            parents
                .iter()
                .filter_map(|(key, _)| match key {
                    Key::Int(id) => u64::try_from(*id).ok(),
                    Key::String(_) => None,
                })
                .collect(),
        ),
        _ => None,
    }
}

const PAGE: &str = include_str!("terms/page.html");

const HELP_CATEGORIES: &str = "<p>You can use categories to define sections of your site and group related posts. The default category is &#8220;Uncategorized&#8221; until you change it in your <a href=\"options-writing\">writing settings</a>.</p>";

const HELP_TAGS: &str = "<p>You can assign keywords to your posts using <strong>tags</strong>. Unlike categories, tags have no hierarchy, meaning there is no relationship from one tag to another.</p>";

const HELP_DIFFERENCE: &str = "<p>What&#8217;s the difference between categories and tags? Normally, tags are ad-hoc keywords that identify important information in your post (names, subjects, etc) that may or may not recur in other posts, while categories are pre-determined sections. If you think of your site like a book, the categories are like the Table of Contents and the tags are like the terms in the index.</p>";

const HELP_FIELDS: &str = "<li><strong>Name</strong> &mdash; The name is how it appears on your site.</li><li><strong>Slug</strong> &mdash; The &#8220;slug&#8221; is the URL-friendly version of the name. It is usually all lowercase and contains only letters, numbers, and hyphens.</li>";

const HELP_PARENT: &str = "<li><strong>Parent</strong> &mdash; Categories, unlike tags, can have a hierarchy. You might have a Jazz category, and under that have child categories for Bebop and Big Band. Totally optional. To create a subcategory, just choose another category from the Parent dropdown.</li>";

const HELP_DESCRIPTION: &str = "<li><strong>Description</strong> &mdash; The description is not prominent by default; however, some themes may display it.</li></ul><p>You can change the display of this screen using the Screen Options tab to set how many items are displayed per screen and to display/hide columns in the table.</p>";

/// `edit-tags.php`'s help tabs for `taxonomy` and its sidebar.
fn help(taxonomy: Taxonomy) -> ([String; 2], String) {
    let categories = taxonomy == CATEGORY;
    let (overview, lower, parent, article) = if categories {
        (
            HELP_CATEGORIES,
            "category",
            HELP_PARENT,
            "posts-categories-screen/\">Documentation on Categories",
        )
    } else {
        (
            HELP_TAGS,
            "tag",
            "",
            "posts-tags-screen/\">Documentation on Tags",
        )
    };
    (
        [
            format!("{overview}{HELP_DIFFERENCE}"),
            format!(
                "<p>When adding a new {lower} on this screen, you&#8217;ll fill in the following fields:</p><ul>{HELP_FIELDS}{parent}{HELP_DESCRIPTION}"
            ),
        ],
        format!(
            "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/{article}</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>"
        ),
    )
}

/// The notes under the list: the default category's, and the converter
/// for those who may import.
fn notes(site: &dyn TermsSite, taxonomy: Taxonomy, terms: &[Term]) -> String {
    let import = site.can("import");
    let converter = BarSite::admin_url(site, "import.php");
    if taxonomy != CATEGORY {
        if !import {
            return String::new();
        }
        return format!(
            "<div class=\"form-wrap edit-term-notes\">\n<p>\n\tTags can be selectively converted to categories using the <a href=\"{converter}\">tag to category converter</a>.\t</p>\n</div>\n\t"
        );
    }
    let default = Value::String(site.option("default_category").unwrap_or_default()).to_int();
    let name = terms
        .iter()
        .find(|term| term.id as i64 == default)
        .map(|term| String::from_utf8_lossy(&term.name).into_owned())
        .unwrap_or_default();
    let mut out = format!(
        "<div class=\"form-wrap edit-term-notes\">\n<p>\n\tDeleting a category does not delete the posts in that category. Instead, posts that were only assigned to the deleted category are set to the default category <strong>{name}</strong>. The default category cannot be deleted.</p>\n"
    );
    if import {
        out.push_str(&format!(
            "\t\t<p>\n\t\tCategories can be selectively converted to tags using the <a href=\"{converter}\">category to tag converter</a>.\t</p>\n"
        ));
    }
    out.push_str("\t</div>\n");
    out
}

/// `edit-tags.php` for `args`: the terms of a taxonomy and the form to add
/// one, refused to a user who may not manage them.
pub fn edit_tags(
    site: &dyn TermsSite,
    args: &TermArgs,
    canonical: &str,
    origin: &str,
    request_uri: &str,
    browser: &Browser,
) -> Answer {
    let taxonomy = args.taxonomy;
    if !site.can(taxonomy.manage_cap()) {
        return Answer::Denied(
            "<h1>You need a higher level of permission.</h1><p>Sorry, you are not allowed to manage terms in this taxonomy.</p>",
        );
    }
    let option = format!("edit_{}_per_page", taxonomy.name);
    let per_page = per_page(site, &option);
    let hierarchical = taxonomy.hierarchical();
    let given = |value: &Option<String>| {
        value
            .as_deref()
            .filter(|value| !is_empty_string(value.as_bytes()))
            .map(|value| String::from_utf8_lossy(php_trim(value.as_bytes())).into_owned())
    };
    let searching = given(&args.search);
    // `WP_Term_Query` searches only for what is left once trimmed.
    let search = searching.clone().filter(|s| !is_empty_string(s.as_bytes()));
    let orderby = given(&args.orderby);
    // `prepare_items()` lists a whole tree unless another order is asked.
    let tree = hierarchical && orderby.is_none();
    let columns = columns(hierarchical && args.orderby.is_none());
    let screen_id = format!("edit-{}", taxonomy.name);
    let hidden = hidden_columns(site, &screen_id);
    let hidden_keys: Vec<&str> = hidden.iter().map(String::as_str).collect();
    // `get_column_count()`: the columns not hidden.
    let visible = columns
        .iter()
        .filter(|column| !hidden_keys.contains(&column.key))
        .count();
    let total = site.count_terms(taxonomy.name, search.as_deref());
    let pages = total.div_ceil(per_page);
    // `get_pagenum()`, and the last page for one past it.
    let page = Value::String(args.paged.clone().unwrap_or_default().into_bytes())
        .to_int()
        .unsigned_abs()
        .max(1);
    if pages > 0 && page > pages {
        let last = pages.to_string();
        let to = add_query_arg(&[(b"paged", Some(last.as_bytes()))], request_uri.as_bytes());
        return Answer::Redirect(String::from_utf8_lossy(&to).into_owned());
    }
    let start = (page - 1).saturating_mul(per_page);
    let end = start.saturating_add(per_page);
    // `parse_order()`: what reads ASC ascends, anything else descends.
    let descending = given(&args.order).is_some_and(|order| !order.eq_ignore_ascii_case("asc"));
    let base = TermQuery {
        taxonomy: taxonomy.name.into(),
        search: search.clone(),
        ..TermQuery::default()
    };
    // A hierarchical taxonomy is sorted whole and sliced, as PHP does.
    let page_of = |terms: Vec<Term>| {
        let skip = usize::try_from(start).unwrap_or(usize::MAX);
        let take = usize::try_from(per_page).unwrap_or(usize::MAX);
        terms.into_iter().skip(skip).take(take).collect()
    };
    let items = if tree {
        site.list_terms(&TermQuery {
            order: Some((TermOrder::Name, descending)),
            ..base
        })
    } else {
        match parse_orderby(orderby.as_deref().unwrap_or("name").as_bytes(), false) {
            Some(order) if hierarchical => page_of(site.list_terms(&TermQuery {
                order: order.map(|order| (order, descending)),
                ..base
            })),
            Some(order) => site.list_terms(&TermQuery {
                order: order.map(|order| (order, descending)),
                limit: Some(per_page),
                offset: start,
                ..base
            }),
            // The query fails, as PHP's does.
            None => Vec::new(),
        }
    };
    // Every term by name, for ancestors and the add form, as a whole tree
    // lists them; tags are read only when one listed has a parent.
    let terms = if tree && searching.is_none() && !descending {
        items.clone()
    } else if hierarchical || items.iter().any(|term| term.parent > 0) {
        site.terms(taxonomy.name)
    } else {
        Vec::new()
    };
    let listing = Listing {
        site,
        taxonomy,
        terms: &items,
        by_id: terms.iter().map(|term| (term.id, term)).collect(),
        cached: if tree {
            site.option(&format!("{}_children", taxonomy.name))
                .and_then(|stored| cached_parents(&stored))
        } else {
            None
        },
        columns: &columns,
        hidden: &hidden_keys,
        request_uri,
    };
    let empty = items.is_empty();
    let rows = if empty {
        format!(
            "<tr class=\"no-items\"><td class=\"colspanchange\" colspan=\"{visible}\">No {} found.</td></tr>",
            taxonomy.plural.to_ascii_lowercase()
        )
    } else if tree {
        listing.tree(start, end, searching.is_some())
    } else {
        items.iter().map(|term| listing.row(term, 0)).collect()
    };
    let referer = String::from_utf8_lossy(&esc_url(&remove_query_arg(
        &[b"_wp_http_referer"],
        request_uri.as_bytes(),
    )))
    .into_owned();
    let order = Order {
        orderby: args.orderby.as_deref(),
        desc: args.order.as_deref() == Some("desc"),
    };
    // Deleting these terms takes the capability that managing them does.
    let actions = [("delete", "Delete")];
    let heading = format!("{} list navigation", taxonomy.plural);
    let tablenav = |which| {
        format!(
            "\t<div class=\"tablenav {which}\">\n\n\t\t\t\t<div class=\"alignleft actions bulkactions{}\">\n\t\t\t{}\t\t</div>\n\t\t{}\n\t\t<br class=\"clear\" />\n\t</div>\n\t\t",
            if empty { " hidden" } else { "" },
            bulk_actions(which, &actions),
            pagination(which, &heading, total, pages, page, canonical)
        )
    };
    let unpaged =
        String::from_utf8_lossy(&remove_query_arg(&[b"paged"], canonical.as_bytes())).into_owned();
    let mut list = format!(
        "<input type=\"hidden\" id=\"_wpnonce\" name=\"_wpnonce\" value=\"{}\" /><input type=\"hidden\" name=\"_wp_http_referer\" value=\"{referer}\" />{}<h2 class='screen-reader-text'>{} list</h2><table class=\"wp-list-table widefat fixed striped table-view-list tags\">\n\t\t{}\t<thead>\n\t<tr>\n\t\t{}\t</tr>\n\t</thead>\n\n\t<tbody id=\"the-list\"\n\t\t data-wp-lists='list:tag'\t\t>\n\t\t{rows}\t</tbody>\n\n\t<tfoot>\n\t<tr>\n\t\t{}\t</tr>\n\t</tfoot>\n\n</table>\n\t\t",
        site.nonce("bulk-tags"),
        tablenav("top"),
        taxonomy.plural,
        table_description(&columns, &order),
        column_headers(&columns, &hidden_keys, &order, &unpaged, 1, true),
        column_headers(&columns, &hidden_keys, &order, &unpaged, 2, false)
    );
    if !empty {
        list.push_str(&tablenav("bottom"));
    }
    let parent = if hierarchical {
        format!(
            "\t<div class=\"form-field term-parent-wrap\">\n\t<label for=\"parent\">Parent {}</label>\n\t\t<select  name='parent' id='parent' class='postform' aria-describedby=\"parent-description\">\n\t<option value='-1'>None</option>\n{}</select>\n\t\t\t\t<p id=\"parent-description\">Categories, unlike tags, can have a hierarchy. You might have a Jazz category, and under that have children categories for Bebop and Big Band. Totally optional.</p>\n\t</div>\n",
            taxonomy.singular,
            term_options(&terms, "")
        )
    } else {
        String::new()
    };
    let requested = args.search.as_deref().unwrap_or_default();
    let search_field = if empty && is_empty_string(requested.as_bytes()) {
        String::new()
    } else {
        // The order fields are printed as requested, slashed.
        let slashed = |value: &Option<String>| {
            String::from_utf8_lossy(&addslashes(value.as_deref().unwrap_or_default().as_bytes()))
                .into_owned()
        };
        let (orderby, order) = (slashed(&args.orderby), slashed(&args.order));
        let fields = [("orderby", orderby.as_str()), ("order", order.as_str())];
        search_box(
            &format!("Search {}", taxonomy.plural),
            "tag",
            requested,
            &fields,
        )
    };
    let subtitle = if requested.is_empty() {
        String::new()
    } else {
        format!(
            "<span class=\"subtitle\">Search results for: <strong>{}</strong></span>",
            esc_html(requested.as_bytes())
        )
    };
    let content = fill(
        PAGE,
        &[
            ("title", taxonomy.plural.into()),
            ("subtitle", subtitle),
            ("taxonomy", taxonomy.name.into()),
            ("screen", screen_id.clone()),
            ("search", search_field),
            ("add", format!("Add {}", taxonomy.singular)),
            ("referer", referer.clone()),
            ("nonce:add-tag", site.nonce("add-tag")),
            ("parent", parent),
            ("list", list),
            ("notes", notes(site, taxonomy, &terms)),
            (
                "focus",
                if browser.is_mobile() {
                    String::new()
                } else {
                    "<script>\ntry{document.forms.addtag['tag-name'].focus();}catch(e){}\n</script>\n\t".into()
                },
            ),
            ("columns", visible.to_string()),
            ("update", format!("Update {}", taxonomy.singular)),
            ("nonce:taxinlineeditnonce", site.nonce("taxinlineeditnonce")),
        ],
    );
    let screen_options = screen_options(
        &columns,
        &hidden,
        &option,
        per_page,
        "",
        site.nonce("screen-options-nonce"),
    );
    let ([overview, adding], sidebar) = help(taxonomy);
    let help = [
        HelpTab {
            id: "overview",
            title: "Overview",
            content: &overview,
        },
        HelpTab {
            id: "adding-terms",
            title: &format!("Adding {}", taxonomy.plural),
            content: &adding,
        },
    ];
    let submenu = format!("edit-tags.php?taxonomy={}", taxonomy.name);
    let page = Page {
        screen: Screen {
            pagenow: "edit-tags.php",
            typenow: "post",
            taxonomy: taxonomy.name,
            parent_file: Some("edit.php"),
            submenu_file: Some(&submenu),
        },
        id: &screen_id,
        title: taxonomy.plural,
        help: &help,
        sidebar: &sidebar,
        screen_options: &screen_options,
        styles: &[],
        scripts: &["admin-tags", "inline-edit-tax"],
        late_scripts: &[],
        script_data: &[],
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical,
        origin,
    };
    // The menu lets in whoever may manage terms.
    admin_page(site, &page, browser).map_or(
        Answer::Denied("Sorry, you are not allowed to access this page."),
        Answer::Page,
    )
}

#[cfg(test)]
pub(crate) mod content;
#[cfg(test)]
mod tests;
