// SPDX-License-Identifier: AGPL-3.0-or-later

//! The terms query and term template blocks, and the `WP_Term_Query` the
//! template runs: its SQL from the site, the rest here.

use hyscribe_kses::esc_attr;
use hyscribe_model::{Term, TermOrder, TermQuery};
use hyscribe_phpser::{Value, absint, isset};

use crate::blocks::Block;
use crate::categories::keep_used;
use crate::core_blocks::prepared;
use crate::elements::elements_class;
use crate::site::{Archive, Site};
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

/// The arguments the term template gives `WP_Term_Query`.
#[derive(Clone, Debug, Default)]
pub struct TermArgs {
    /// `taxonomy`.
    pub taxonomy: String,
    /// `number`, 0 for all.
    pub number: u64,
    /// `order` as given.
    pub order: Value,
    /// `orderby` as given.
    pub orderby: Vec<u8>,
    /// `hide_empty`.
    pub hide_empty: bool,
    /// `parent`, unset by default.
    pub parent: Option<i64>,
    /// `child_of`.
    pub child_of: i64,
    /// `include`.
    pub include: Vec<u64>,
}

impl Renderer<'_> {
    /// The static terms query block, giving its descendants `termQuery`.
    pub(crate) fn terms_query(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let query = prepared("core/terms-query", parsed)
            .get_str("termQuery")
            .cloned();
        let outer = std::mem::replace(&mut self.context.terms.query, query);
        let content = self.saved_markup(block);
        self.context.terms.query = outer;
        content
    }

    /// `render_block_core_term_template()`: the inner blocks once for each
    /// term, with the term as their context.
    pub(crate) fn term_template(&mut self, block: &Block) -> Vec<u8> {
        let Some(query) = self.context.terms.query.clone().filter(|q| !q.is_empty()) else {
            return Vec::new();
        };
        let Some(args) = term_args(self.context.request.archive.as_ref(), &query) else {
            return Vec::new();
        };
        let terms = get_terms(self.context.site, &args);
        if terms.is_empty() {
            return Vec::new();
        }
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/term-template", parsed);
        let class = if isset(attributes.get_path(&["style", "elements", "link", "color", "text"])) {
            "has-link-color"
        } else {
            ""
        };
        let wrapper = get_block_wrapper_attributes(
            "core/term-template",
            parsed,
            &[("class", Value::String(class.into()))],
            &self.context.theme.global,
        );
        let outer = self.context.terms.term.take();
        let taxonomy = &args.taxonomy;
        let mut items = Vec::new();
        for term in &terms {
            self.context.terms.term = Some((taxonomy.clone(), term.id));
            let class = format!(
                "wp-block-term term-{} {taxonomy} taxonomy-{taxonomy}",
                term.id
            );
            items.extend(format!("<li class=\"{}\">", esc_attr(class.as_bytes())).into_bytes());
            // Each term renders as a `core/null` block with the template's
            // attributes, as the post template's posts do.
            let content = self.saved_markup(block);
            items.extend(elements_class(content, block));
            items.extend_from_slice(b"</li>");
        }
        self.context.terms.term = outer;
        [format!("<ul {wrapper}>").as_bytes(), &items, b"</ul>"].concat()
    }
}

/// The term template's arguments from `termQuery`: inheriting the queried
/// term's taxonomy and children on a term archive, or the block's own.
/// None on a term archive without its term, where PHP would read a null
/// queried object.
fn term_args(archive: Option<&Archive>, query: &Value) -> Option<TermArgs> {
    let get = |key| query.get_str(key).cloned().unwrap_or(Value::Null);
    let mut args = TermArgs {
        number: absint(&get("perPage")),
        order: get("order"),
        orderby: get("orderBy").to_php_string(),
        hide_empty: !get("hideEmpty").is_empty(),
        ..TermArgs::default()
    };
    let nested = !get("showNested").is_empty();
    match archive.filter(|_| !get("inherit").is_empty()) {
        Some(Archive::Term(taxonomy, term)) => {
            let term = term.as_ref()?;
            args.taxonomy = taxonomy.name().into();
            if hierarchical(&args.taxonomy) {
                let id = i64::try_from(term.id).unwrap_or(i64::MAX);
                if nested {
                    args.child_of = id;
                } else {
                    args.parent = Some(id);
                }
            }
        }
        _ => {
            args.taxonomy = String::from_utf8_lossy(&get("taxonomy").to_php_string()).into_owned();
            let include = get("include");
            if !include.is_empty() {
                for (_, id) in include.as_array().unwrap_or_default() {
                    let id = absint(id);
                    if !args.include.contains(&id) {
                        args.include.push(id);
                    }
                }
                args.orderby = b"include".to_vec();
                args.order = Value::String(b"asc".to_vec());
            } else if hierarchical(&args.taxonomy) && !nested {
                args.parent = Some(0);
            }
        }
    }
    Some(args)
}

/// `is_taxonomy_hierarchical()` for the taxonomies core registers.
fn hierarchical(taxonomy: &str) -> bool {
    taxonomy == "category"
}

/// `WP_Term_Query::get_terms()` for the term template's arguments.
pub fn get_terms(site: &dyn Site, args: &TermArgs) -> Vec<Term> {
    let hierarchical = hierarchical(&args.taxonomy);
    // A negative parent or `child_of` is in no `_get_term_hierarchy()`, or
    // no term's parent; one without children finds none below.
    // 'parent' overrides 'child_of'.
    let child_of = if args.parent.is_some_and(|p| p > 0) {
        0
    } else {
        args.child_of
    };
    if child_of < 0 || args.parent.is_some_and(|p| p < 0) {
        return Vec::new();
    }
    let child_of = child_of.unsigned_abs();
    let orderby = &args.orderby[..];
    let lower = orderby.to_ascii_lowercase();
    let order = match &lower[..] {
        // Without object IDs, `term_order` falls back to the ID; spelled
        // otherwise it names a missing table, and the query fails.
        b"term_order" if orderby == b"term_order" => Some(TermOrder::TermId),
        b"term_order" => return Vec::new(),
        b"" | b"id" | b"term_id" => Some(TermOrder::TermId),
        b"slug" => Some(TermOrder::Slug),
        b"term_group" => Some(TermOrder::TermGroup),
        b"count" => Some(TermOrder::Count),
        b"parent" => Some(TermOrder::Parent),
        b"taxonomy" => Some(TermOrder::Taxonomy),
        b"term_taxonomy_id" => Some(TermOrder::TermTaxonomyId),
        b"description" => Some(TermOrder::Description),
        b"include" if !args.include.is_empty() => Some(TermOrder::Include),
        b"none" => None,
        _ => Some(TermOrder::Name),
    };
    // `parse_order()`: only a string reading ASC ascends.
    let ascending =
        matches!(&args.order, Value::String(order) if order.eq_ignore_ascii_case(b"asc"));
    let query = TermQuery {
        taxonomy: args.taxonomy.clone(),
        include: args.include.clone(),
        parent: args.parent.map(|p| p.unsigned_abs()),
        nonempty: args.hide_empty && !hierarchical,
        order: order.map(|order| (order, !ascending)),
        limit: (args.number > 0 && !hierarchical && child_of == 0 && args.parent.is_none())
            .then_some(args.number),
    };
    let mut terms = site.term_query(&query);
    if child_of != 0 {
        terms = term_children(child_of, &terms, &mut vec![child_of]);
    }
    if hierarchical && args.hide_empty {
        keep_used(&site.taxonomy_terms(&args.taxonomy), &mut terms);
    }
    // Only a hierarchical query is cut to `number` here: others went
    // without a limit only with a parent or `child_of`, and keep every term.
    if hierarchical && args.number > 0 {
        terms.truncate(usize::try_from(args.number).unwrap_or(usize::MAX));
    }
    terms
}

/// `_get_term_children()`: the terms of `terms` below `id`, each followed
/// by its own, skipping any seen in a loop of parents.
fn term_children(id: u64, terms: &[Term], ancestors: &mut Vec<u64>) -> Vec<Term> {
    let mut list = Vec::new();
    for term in terms {
        if ancestors.contains(&term.id) || term.parent != id {
            continue;
        }
        list.push(term.clone());
        ancestors.push(term.id);
        list.extend(term_children(term.id, terms, ancestors));
    }
    list
}

#[cfg(test)]
mod tests;
