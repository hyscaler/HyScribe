// SPDX-License-Identifier: AGPL-3.0-or-later

//! The categories block, `render_block_core_categories()`, over
//! `wp_list_categories()` and `wp_dropdown_categories()` with their walkers.

use std::collections::{HashMap, HashSet};

use hyscribe_html::TagProcessor;
use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::Term;
use hyscribe_phpser::Value;

use crate::archives::dropdown_script;
use crate::blocks::Block;
use crate::core_blocks::{number_format, prepared};
use crate::kses::wp_kses_post;
use crate::permalinks::term_link;
use crate::site::{Archive, Site, Taxonomy};
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

/// The dropdown's script up to its arguments, as the block's PHP template
/// prints it.
const SCRIPT: &str = "( ( [ dropdownId, homeUrl ] ) => {
\t\tconst dropdown = document.getElementById( dropdownId );
\t\tfunction onSelectChange() {
\t\t\tsetTimeout( () => {
\t\t\t\tif ( 'escape' === dropdown.dataset.lastkey ) {
\t\t\t\t\treturn;
\t\t\t\t}
\t\t\t\t// Only navigate if a valid term is selected (not the default \"Select [taxonomy]\" option)
\t\t\t\tif ( dropdown.value && dropdown.value !== '-1' && dropdown instanceof HTMLSelectElement ) {
\t\t\t\t\tconst url = new URL( homeUrl );
\t\t\t\t\turl.searchParams.set( dropdown.name, dropdown.value );
\t\t\t\t\tlocation.href = url.href;
\t\t\t\t}
\t\t\t}, 250 );
\t\t}
\t\tfunction onKeyUp( event ) {
\t\t\tif ( 'Escape' === event.key ) {
\t\t\t\tdropdown.dataset.lastkey = 'escape';
\t\t\t} else {
\t\t\t\tdelete dropdown.dataset.lastkey;
\t\t\t}
\t\t}
\t\tfunction onClick() {
\t\t\tdelete dropdown.dataset.lastkey;
\t\t}
\t\tdropdown.addEventListener( 'keyup', onKeyUp );
\t\tdropdown.addEventListener( 'click', onClick );
\t\tdropdown.addEventListener( 'change', onSelectChange );
\t} )( ";

/// What a taxonomy the block can list is called, and the query var its
/// dropdown sets.
struct Labels {
    query_var: &'static str,
    name: &'static str,
    singular: &'static str,
    no_terms: &'static str,
}

fn labels(taxonomy: &str) -> Option<Labels> {
    Some(match taxonomy {
        "category" => Labels {
            query_var: "category_name",
            name: "Categories",
            singular: "Category",
            no_terms: "No categories",
        },
        "post_tag" => Labels {
            query_var: "tag",
            name: "Tags",
            singular: "Tag",
            no_terms: "No tags",
        },
        "post_format" => Labels {
            query_var: "post_format",
            name: "Formats",
            singular: "Format",
            no_terms: "No tags",
        },
        _ => return None,
    })
}

/// The terms `get_terms()` finds for the block, by name.
struct Listing {
    terms: Vec<Term>,
    /// `walk_category_tree()`'s depth: the full tree, or flat.
    hierarchical: bool,
    /// Every term's parent in the taxonomy.
    parents: HashMap<u64, u64>,
}

impl Renderer<'_> {
    /// `render_block_core_categories()`.
    pub(crate) fn categories(&mut self, block: &Block) -> Vec<u8> {
        self.categories_blocks += 1;
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/categories", parsed);
        let on = |name| attributes.get_str(name).is_some_and(|v| !v.is_empty());
        let taxonomy = attributes
            .get_str("taxonomy")
            .map(Value::to_php_string)
            .unwrap_or_default();
        let taxonomy = String::from_utf8_lossy(&taxonomy).into_owned();
        let site = self.context.site;
        let show_count = on("showPostCounts");
        let listed = labels(&taxonomy).map(|labels| {
            let hierarchical = on("showHierarchy");
            let (hide_empty, top_only) = (!on("showEmpty"), on("showOnlyTopLevel"));
            let listing = terms(
                site,
                &taxonomy,
                hierarchical,
                hide_empty,
                top_only,
                show_count,
            );
            (labels, listing)
        });
        let dropdown = on("displayAsDropdown");
        let class = format!(
            "wp-block-categories-{} wp-block-categories-taxonomy-{taxonomy}",
            if dropdown { "dropdown" } else { "list" }
        );
        let extra = [("class", Value::String(class.into_bytes()))];
        let theme = &self.context.theme.global;
        let wrapper = get_block_wrapper_attributes("core/categories", parsed, &extra, theme);
        if dropdown {
            let id = format!("wp-block-categories-{}", self.categories_blocks);
            let hidden = if on("showLabel") {
                ""
            } else {
                " screen-reader-text"
            };
            let label = match attributes.get_str("label").filter(|v| !v.is_empty()) {
                Some(label) => wp_kses_post(&label.to_php_string(), &self.context.upload_host()),
                None => listed
                    .as_ref()
                    .map_or("", |(l, _)| l.name)
                    .as_bytes()
                    .to_vec(),
            };
            let select = listed.as_ref().map_or_else(Vec::new, |(labels, listing)| {
                let selected = self.selected_term(&taxonomy, labels.query_var);
                let source = "build_dropdown_script_block_core_categories";
                [
                    dropdown_select(&id, labels, listing, show_count, &selected),
                    dropdown_script(site, SCRIPT, source, &id).into_bytes(),
                    b"\n".to_vec(),
                ]
                .concat()
            });
            return [
                format!("<div {wrapper}><label class=\"wp-block-categories__label{hidden}\" for=\"{}\">", esc_attr(id.as_bytes())).as_bytes(),
                &label,
                b"</label>",
                &select,
                b"</div>",
            ]
            .concat();
        }
        let items = listed.as_ref().map_or_else(Vec::new, |(labels, listing)| {
            self.category_list(&taxonomy, labels, listing, show_count)
        });
        let enhanced = self
            .context
            .query
            .as_ref()
            .is_some_and(|q| q.enhanced_pagination);
        let items = if enhanced {
            let mut tags = TagProcessor::new(&items);
            while tags.next_tag(Some(b"a"), None) {
                tags.set_attribute(b"data-wp-on--click", b"core/query::actions.navigate");
            }
            tags.get_updated_html().to_vec()
        } else {
            items
        };
        [format!("<ul {wrapper}>").as_bytes(), &items, b"</ul>"].concat()
    }

    /// `wp_list_categories()`'s items: each term linked, with its count,
    /// the current one and its ancestors marked, nested for a tree.
    fn category_list(
        &self,
        taxonomy: &str,
        labels: &Labels,
        listing: &Listing,
        show_count: bool,
    ) -> Vec<u8> {
        if listing.terms.is_empty() {
            return format!("<li class=\"cat-item-none\">{}</li>", labels.no_terms).into_bytes();
        }
        let current = match &self.context.request.archive {
            Some(Archive::Term(kind, Some(term))) if kind.name() == taxonomy => Some(term.id),
            _ => None,
        };
        let parents = &listing.parents;
        let mut out = Vec::new();
        walk(
            &listing.terms,
            listing.hierarchical,
            &mut |event| match event {
                Walk::Start(term) => {
                    let name = esc_attr(&term.name);
                    let mut classes = format!("cat-item cat-item-{}", term.id);
                    let aria = if current == Some(term.id) {
                        " aria-current=\"page\""
                    } else {
                        ""
                    };
                    let mut link = format!(
                        "<a{aria} href=\"{}\">{name}</a>",
                        String::from_utf8_lossy(&esc_url(&term_link(
                            self.context.site,
                            taxonomy,
                            term
                        )))
                    );
                    if let Some(current) = current {
                        if term.id == current {
                            classes.push_str(" current-cat");
                        } else if parents.get(&current) == Some(&term.id) {
                            classes.push_str(" current-cat-parent");
                        }
                        let mut ancestor = parents.get(&current).copied().unwrap_or_default();
                        while ancestor != 0 {
                            if ancestor == term.id {
                                classes.push_str(" current-cat-ancestor");
                                break;
                            }
                            ancestor = parents.get(&ancestor).copied().unwrap_or_default();
                        }
                    }
                    if show_count {
                        link.push_str(&format!(" ({})", number_format(term.count)));
                    }
                    if !name.is_empty() {
                        out.extend(
                            format!("\t<li class=\"{}\">{link}\n", esc_attr(classes.as_bytes()))
                                .into_bytes(),
                        );
                    }
                }
                Walk::Level(depth, open) => {
                    let indent = "\t".repeat(depth);
                    let tag = if open {
                        "<ul class='children'>"
                    } else {
                        "</ul>"
                    };
                    out.extend(format!("{indent}{tag}\n").into_bytes());
                }
                Walk::End => out.extend_from_slice(b"</li>\n"),
            },
        );
        out
    }

    /// The value of the dropdown's query var: on a category archive the
    /// slug of its category, as `WP_Query` sets `category_name`.
    fn selected_term(&self, taxonomy: &str, query_var: &str) -> Vec<u8> {
        let request = self.context.request;
        match &request.archive {
            Some(Archive::Term(Taxonomy::Category, Some(term))) if taxonomy == "category" => {
                term.slug.clone()
            }
            _ => request
                .vars
                .get_str(query_var)
                .map(Value::to_php_string)
                .unwrap_or_default(),
        }
    }
}

/// `get_terms()` for the block: by name, the top level only if asked; for
/// a tree with counts, each count padded with the posts below it
/// (`_pad_term_counts()`); without empty terms, though a tree keeps those
/// with a used term below them.
fn terms(
    site: &dyn Site,
    taxonomy: &str,
    hierarchical: bool,
    hide_empty: bool,
    top_only: bool,
    show_count: bool,
) -> Listing {
    let all = site.taxonomy_terms(taxonomy);
    let mut terms: Vec<Term> = all
        .iter()
        .filter(|term| {
            (!top_only || term.parent == 0) && (hierarchical || !hide_empty || term.count > 0)
        })
        .cloned()
        .collect();
    if hierarchical && show_count {
        pad_counts(&mut terms, &site.term_posts(taxonomy));
    }
    if hierarchical && hide_empty {
        keep_used(&all, &mut terms);
    }
    let parents = all.iter().map(|t| (t.id, t.parent)).collect();
    Listing {
        terms,
        hierarchical,
        parents,
    }
}

/// `get_terms()`'s `hide_empty` on a tree: `terms` keeps those used, or
/// with a used term of `all` below them.
pub(crate) fn keep_used(all: &[Term], terms: &mut Vec<Term>) {
    let counts: HashMap<u64, u64> = all.iter().map(|t| (t.id, t.count)).collect();
    let used_below = |id: u64| {
        descendants(all, id)
            .iter()
            .any(|d| counts.get(d).is_some_and(|c| *c > 0))
    };
    terms.retain(|term| term.count > 0 || used_below(term.id));
}

/// The IDs of every term below `id`, as `get_term_children()` gives them;
/// in a loop of parents, `id` itself too, which its caller has checked.
fn descendants(all: &[Term], id: u64) -> Vec<u64> {
    let mut found = Vec::new();
    let mut queue = vec![id];
    while let Some(parent) = queue.pop() {
        for term in all.iter().filter(|term| term.parent == parent) {
            if !found.contains(&term.id) {
                found.push(term.id);
                queue.push(term.id);
            }
        }
    }
    found
}

/// `_pad_term_counts()`: a listed term's count becomes the distinct posts
/// in it and in the listed terms below it; terms with no posts keep theirs.
fn pad_counts(terms: &mut [Term], posts: &[(u64, u64)]) {
    let parents: HashMap<u64, u64> = terms.iter().map(|t| (t.id, t.parent)).collect();
    let mut items: HashMap<u64, HashSet<u64>> = HashMap::new();
    for &(term, post) in posts.iter().filter(|(term, _)| parents.contains_key(term)) {
        items.entry(term).or_default().insert(post);
        let mut seen = Vec::new();
        let mut child = term;
        while let Some(&parent) = parents.get(&child).filter(|p| **p != 0) {
            if seen.contains(&parent) {
                break;
            }
            seen.push(parent);
            items.entry(parent).or_default().insert(post);
            child = parent;
        }
    }
    for term in terms.iter_mut() {
        if let Some(posts) = items.get(&term.id) {
            term.count = posts.len() as u64;
        }
    }
}

/// What `Walker::walk()` does at each step.
enum Walk<'a> {
    Start(&'a Term),
    /// A level of children opens or closes at a depth.
    Level(usize, bool),
    End,
}

/// `Walker::walk()` over `terms`: the full tree from the top level (or,
/// with none, from the first term's parent), then orphans flat; or all flat.
fn walk<'a>(terms: &'a [Term], tree: bool, visit: &mut dyn FnMut(Walk<'a>)) {
    if !tree {
        for term in terms {
            visit(Walk::Start(term));
            visit(Walk::End);
        }
        return;
    }
    let root = terms.first().map_or(0, |t| t.parent);
    let top_parent = if terms.iter().any(|t| t.parent == 0) {
        0
    } else {
        root
    };
    let mut children: Vec<(u64, Vec<&'a Term>)> = Vec::new();
    for entry in terms.iter().filter(|t| t.parent != top_parent) {
        match children
            .iter_mut()
            .find(|(parent, _)| *parent == entry.parent)
        {
            Some((_, list)) => list.push(entry),
            None => children.push((entry.parent, vec![entry])),
        }
    }
    for entry in terms.iter().filter(|t| t.parent == top_parent) {
        display(entry, &mut children, 0, visit);
    }
    for (_, orphans) in std::mem::take(&mut children) {
        for term in orphans {
            visit(Walk::Start(term));
            visit(Walk::End);
        }
    }
}

/// `Walker::display_element()`: an element, then its children a level down.
fn display<'a>(
    term: &'a Term,
    children: &mut Vec<(u64, Vec<&'a Term>)>,
    depth: usize,
    visit: &mut dyn FnMut(Walk<'a>),
) {
    visit(Walk::Start(term));
    if let Some(at) = children.iter().position(|(parent, _)| *parent == term.id) {
        let (_, list) = children.remove(at);
        visit(Walk::Level(depth, true));
        for child in list {
            display(child, children, depth + 1, visit);
        }
        visit(Walk::Level(depth, false));
    }
    visit(Walk::End);
}

/// `wp_dropdown_categories()` up to `</select>`, where the block puts its
/// script: the select, with the "none" option when
/// there are terms, each term's slug as its value, padded by depth.
fn dropdown_select(
    id: &str,
    labels: &Labels,
    listing: &Listing,
    show_count: bool,
    selected: &[u8],
) -> Vec<u8> {
    let name = labels.query_var;
    let mut out = format!(
        "<select  name='{name}' id='{}' class='postform'>\n",
        esc_attr(id.as_bytes())
    )
    .into_bytes();
    let none = format!("Select {}", labels.singular);
    if listing.terms.is_empty() {
        out.extend(
            format!("\t<option value='-1' selected='selected'>{none}</option>\n").into_bytes(),
        );
    } else {
        let chosen = if selected == b"-1" {
            " selected='selected'"
        } else {
            ""
        };
        out.extend(format!("\t<option value='-1'{chosen}>{none}</option>\n").into_bytes());
        let mut depth = 0;
        walk(
            &listing.terms,
            listing.hierarchical,
            &mut |event| match event {
                Walk::Start(term) => {
                    let chosen = if term.slug == selected {
                        " selected=\"selected\""
                    } else {
                        ""
                    };
                    out.extend(
                        format!(
                            "\t<option class=\"level-{depth}\" value=\"{}\"{chosen}>{}",
                            esc_attr(&term.slug),
                            "&nbsp;".repeat(depth * 3)
                        )
                        .into_bytes(),
                    );
                    out.extend_from_slice(&term.name);
                    if show_count {
                        out.extend(
                            format!("&nbsp;&nbsp;({})", number_format(term.count)).into_bytes(),
                        );
                    }
                    out.extend_from_slice(b"</option>\n");
                }
                Walk::Level(_, open) => {
                    if open {
                        depth += 1;
                    } else {
                        depth -= 1;
                    }
                }
                Walk::End => {}
            },
        );
    }
    out.extend_from_slice(b"</select>");
    out
}

#[cfg(test)]
mod tests;
