// SPDX-License-Identifier: AGPL-3.0-or-later

//! Terms: `wp_insert_term()`, `wp_update_term()`, `wp_delete_term()`, relationships and counts.
//!
//! Each function writes inside the caller's [`Write`], so an action is one
//! transaction (ADR 0012); an action whose function fails must not be
//! committed. Texts are taken slashed, as the PHP functions take them. Only
//! the core taxonomies of [`Taxonomy`] are ported.

use hyscribe_db::{DbError, TermRow};
use hyscribe_phpser::{Key, Value, encode, php_trim, stripslashes};
use hyscribe_render::formatting::sanitize_title;
use hyscribe_render::kses::wp_kses_data;

use crate::name_filters;
use crate::options::autoload_value;
use crate::steps::{Steps, step};

mod edit;
mod objects;

pub use edit::{Deletion, wp_delete_term, wp_insert_term, wp_update_term};
pub(crate) use objects::update_term_count;
pub use objects::{
    TermList, TermRef, wp_remove_object_terms, wp_set_object_terms, wp_set_post_categories,
    wp_set_post_terms,
};

/// A core taxonomy whose terms these functions write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taxonomy {
    /// `category`, hierarchical.
    Category,
    /// `post_tag`.
    PostTag,
    /// `wp_theme`, the theme a template or global styles post belongs to.
    WpTheme,
}

impl Taxonomy {
    /// The taxonomy's name.
    pub fn name(self) -> &'static str {
        match self {
            Taxonomy::Category => "category",
            Taxonomy::PostTag => "post_tag",
            Taxonomy::WpTheme => "wp_theme",
        }
    }

    /// The post types `_update_post_term_count()` counts of the taxonomy
    /// named `name`.
    pub(crate) fn counted(name: &str) -> &'static [&'static str] {
        match name {
            "wp_theme" => &["wp_template", "wp_template_part", "wp_global_styles"],
            _ => &["post"],
        }
    }

    fn hierarchical(self) -> bool {
        self == Taxonomy::Category
    }
}

/// Why a term function failed: PHP's `WP_Error` codes, then the refusals
/// PHP does not have.
#[derive(Debug)]
pub enum TermError {
    /// `empty_term_name`.
    EmptyName,
    /// `missing_parent`.
    MissingParent,
    /// `invalid_term_name`: the name sanitizes to nothing.
    InvalidName,
    /// `term_exists`, with the ID of the term that has the name.
    Exists(u64),
    /// `duplicate_term_slug`, with the slug.
    DuplicateSlug(Vec<u8>),
    /// `invalid_term`: no such term in the taxonomy.
    InvalidTerm,
    /// PHP would do what is not ported yet.
    Unported(&'static str),
    /// The database failed.
    Db(DbError),
}

/// The arguments of `wp_insert_term()` and `wp_update_term()`, slashed;
/// `None` is an argument not given.
#[derive(Clone, Copy, Debug, Default)]
pub struct TermArgs<'a> {
    /// `name`, which only `wp_update_term()` takes from here.
    pub name: Option<&'a [u8]>,
    /// `slug`.
    pub slug: Option<&'a [u8]>,
    /// `description`.
    pub description: Option<&'a [u8]>,
    /// `parent`.
    pub parent: Option<i64>,
    /// `alias_of`: the slug of the term whose group the term joins.
    pub alias_of: Option<&'a [u8]>,
    /// `term_group`, which only `wp_update_term()` writes.
    pub term_group: Option<i64>,
}

/// A term's IDs, as the PHP functions return them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TermIds {
    /// `term_id`.
    pub term_id: u64,
    /// `term_taxonomy_id`.
    pub term_taxonomy_id: u64,
}

impl From<&TermRow> for TermIds {
    fn from(term: &TermRow) -> TermIds {
        TermIds {
            term_id: term.term_id,
            term_taxonomy_id: term.term_taxonomy_id,
        }
    }
}

impl From<DbError> for TermError {
    fn from(err: DbError) -> TermError {
        TermError::Db(err)
    }
}

/// The `pre_term_name` filters of a slashed name, unslashed.
fn sanitize_name(name: &[u8]) -> Vec<u8> {
    stripslashes(&name_filters(name))
}

/// The `pre_term_description` filter of a slashed description, unslashed.
fn sanitize_description(description: &[u8]) -> Vec<u8> {
    wp_kses_data(&stripslashes(description))
}

fn lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// The slug filter `term_exists()` gives `get_terms()` for an unslashed,
/// trimmed text: none when the slug is empty.
fn exists_slug(text: &[u8]) -> Option<String> {
    let slug = sanitize_title(text);
    (!slug.is_empty()).then(|| lossy(&sanitize_title(&slug)))
}

/// `term_exists( $term, $taxonomy )`: the term `term` names in `taxonomy`.
async fn existing(
    steps: &mut Steps<'_, '_, TermError>,
    taxonomy: Taxonomy,
    term: TermRef<'_>,
) -> Option<TermRow> {
    let text = match term {
        TermRef::Id(0) => return None,
        TermRef::Id(id) => return step!(steps, term_in(taxonomy.name(), id)),
        TermRef::Text(text) => stripslashes(text),
    };
    let text = php_trim(&text);
    if text.is_empty() {
        return None;
    }
    let slug = exists_slug(text);
    if let Some(term) = step!(steps, term_by_slug(taxonomy.name(), slug.as_deref())) {
        return Some(term);
    }
    let name = lossy(&sanitize_name(text));
    step!(steps, term_by_name(taxonomy.name(), &name))
}

/// `get_term_by( 'slug', $slug, $taxonomy )`.
async fn term_by_slug(
    steps: &mut Steps<'_, '_, TermError>,
    taxonomy: Taxonomy,
    slug: &[u8],
) -> Option<TermRow> {
    if slug.is_empty() {
        return None;
    }
    let slug = lossy(&sanitize_title(slug));
    step!(steps, term_by_slug(taxonomy.name(), Some(&slug)))
}

/// `_get_term_hierarchy()` computed: each parent with its children, both
/// in term ID order.
async fn hierarchy(
    steps: &mut Steps<'_, '_, TermError>,
    taxonomy: Taxonomy,
) -> Vec<(u64, Vec<u64>)> {
    let mut children: Vec<(u64, Vec<u64>)> = Vec::new();
    for (term, parent) in step!(steps, term_parents(taxonomy.name())) {
        match children.iter_mut().find(|(p, _)| *p == parent) {
            Some((_, terms)) => terms.push(term),
            None if parent > 0 => children.push((parent, vec![term])),
            None => {}
        }
    }
    children
}

/// Whether `get_terms()` looks under `parent`: a hierarchical taxonomy's
/// parent with children (the `{$taxonomy}_children` option it keeps).
async fn in_hierarchy(
    steps: &mut Steps<'_, '_, TermError>,
    taxonomy: Taxonomy,
    parent: u64,
) -> bool {
    parent == 0
        || taxonomy.hierarchical()
            && hierarchy(steps, taxonomy)
                .await
                .iter()
                .any(|(p, _)| *p == parent)
}

/// `clean_taxonomy_cache()`'s writes: a hierarchical taxonomy's
/// `{$taxonomy}_children` option deleted and added again.
async fn clean_taxonomy(steps: &mut Steps<'_, '_, TermError>, taxonomy: Taxonomy) {
    if !taxonomy.hierarchical() {
        return;
    }
    let option = format!("{}_children", taxonomy.name());
    step!(steps, delete_option(&option));
    let id = |n: u64| Value::Int(n as i64);
    let children = hierarchy(steps, taxonomy)
        .await
        .into_iter()
        .map(|(parent, terms)| {
            let terms = terms.into_iter().enumerate();
            let terms = terms.map(|(i, term)| (Key::Int(i as i64), id(term)));
            (Key::Int(parent as i64), Value::Array(terms.collect()))
        });
    let value = encode(&Value::Array(children.collect()));
    let autoload = autoload_value(&value, None);
    step!(steps, add_option(&option, &value, autoload));
}

#[cfg(test)]
mod tests;
