// SPDX-License-Identifier: AGPL-3.0-or-later

//! Terms: `wp_insert_term()`, `wp_update_term()`, `wp_delete_term()`, relationships and counts.
//!
//! Each function writes inside the caller's [`Write`], so an action is one
//! transaction (ADR 0012); an action whose function fails must not be
//! committed. Texts are taken slashed, as the PHP functions take them. Only
//! the core taxonomies of [`Taxonomy`] are ported.

use hyscribe_db::{DbError, TermRow, Write};
use hyscribe_kses::wp_specialchars;
use hyscribe_phpser::{Key, Value, encode, php_trim, stripslashes};
use hyscribe_render::formatting::{sanitize_text_field, sanitize_title};
use hyscribe_render::kses::wp_kses_data;

mod edit;
mod objects;

pub use edit::{Deletion, wp_delete_term, wp_insert_term, wp_update_term};
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
}

impl Taxonomy {
    /// The taxonomy's name.
    pub fn name(self) -> &'static str {
        match self {
            Taxonomy::Category => "category",
            Taxonomy::PostTag => "post_tag",
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

/// An action's writes, keeping its first database error or unported step:
/// the action goes on with empty reads and fails at the end, so what it
/// wrote is never committed, and no early return is left untested.
struct Act<'a, 'w> {
    write: &'a mut Write<'w>,
    refusal: Option<TermError>,
}

impl<'a, 'w> Act<'a, 'w> {
    fn new(write: &'a mut Write<'w>) -> Act<'a, 'w> {
        Act {
            write,
            refusal: None,
        }
    }

    fn keep<T: Default>(&mut self, result: Result<T, DbError>) -> T {
        result.unwrap_or_else(|err| {
            self.refusal.get_or_insert(TermError::Db(err));
            T::default()
        })
    }

    fn unported(&mut self, what: &'static str) {
        self.refusal.get_or_insert(TermError::Unported(what));
    }

    fn finish<T>(self, result: Result<T, TermError>) -> Result<T, TermError> {
        self.refusal.map_or(result, Err)
    }
}

/// `act.write.$method(...)` kept by the act: its value, or the default
/// when it failed.
macro_rules! db {
    ($act:ident . $method:ident ( $($arg:expr),* $(,)? )) => {{
        let result = $act.write.$method($($arg),*).await;
        $act.keep(result)
    }};
}
use db;

/// The `pre_term_name` filters of a slashed name, unslashed:
/// `sanitize_text_field()`, `wp_filter_kses()` and `_wp_specialchars()`.
fn sanitize_name(name: &[u8]) -> Vec<u8> {
    let kept = wp_kses_data(&stripslashes(&sanitize_text_field(name)));
    wp_specialchars(&kept)
}

/// The `pre_term_description` filter of a slashed description, unslashed.
fn sanitize_description(description: &[u8]) -> Vec<u8> {
    wp_kses_data(&stripslashes(description))
}

/// PHP's `empty()` of a string.
fn is_empty(text: &[u8]) -> bool {
    text.is_empty() || text == b"0"
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
async fn existing(act: &mut Act<'_, '_>, taxonomy: Taxonomy, term: TermRef<'_>) -> Option<TermRow> {
    let text = match term {
        TermRef::Id(0) => return None,
        TermRef::Id(id) => return db!(act.term_in(taxonomy.name(), id)),
        TermRef::Text(text) => stripslashes(text),
    };
    let text = php_trim(&text);
    if text.is_empty() {
        return None;
    }
    let slug = exists_slug(text);
    if let Some(term) = db!(act.term_by_slug(taxonomy.name(), slug.as_deref())) {
        return Some(term);
    }
    let name = lossy(&sanitize_name(text));
    db!(act.term_by_name(taxonomy.name(), &name))
}

/// `get_term_by( 'slug', $slug, $taxonomy )`.
async fn term_by_slug(act: &mut Act<'_, '_>, taxonomy: Taxonomy, slug: &[u8]) -> Option<TermRow> {
    if slug.is_empty() {
        return None;
    }
    let slug = lossy(&sanitize_title(slug));
    db!(act.term_by_slug(taxonomy.name(), Some(&slug)))
}

/// `_get_term_hierarchy()` computed: each parent with its children, both
/// in term ID order.
async fn hierarchy(act: &mut Act<'_, '_>, taxonomy: Taxonomy) -> Vec<(u64, Vec<u64>)> {
    let mut children: Vec<(u64, Vec<u64>)> = Vec::new();
    for (term, parent) in db!(act.term_parents(taxonomy.name())) {
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
async fn in_hierarchy(act: &mut Act<'_, '_>, taxonomy: Taxonomy, parent: u64) -> bool {
    parent == 0
        || taxonomy.hierarchical()
            && hierarchy(act, taxonomy)
                .await
                .iter()
                .any(|(p, _)| *p == parent)
}

/// `clean_taxonomy_cache()`'s writes: a hierarchical taxonomy's
/// `{$taxonomy}_children` option deleted and added again.
async fn clean_taxonomy(act: &mut Act<'_, '_>, taxonomy: Taxonomy) {
    if !taxonomy.hierarchical() {
        return;
    }
    let option = format!("{}_children", taxonomy.name());
    db!(act.delete_option(&option));
    let id = |n: u64| Value::Int(n as i64);
    let children = hierarchy(act, taxonomy)
        .await
        .into_iter()
        .map(|(parent, terms)| {
            let terms = terms.into_iter().enumerate();
            let terms = terms.map(|(i, term)| (Key::Int(i as i64), id(term)));
            (Key::Int(parent as i64), Value::Array(terms.collect()))
        });
    let value = encode(&Value::Array(children.collect()));
    db!(act.add_option(&option, &value, default_autoload(&value)));
}

/// The autoload `update_option()` gives a new option without one:
/// `wp_filter_default_autoload_value_via_option_size()`.
fn default_autoload(value: &[u8]) -> &'static str {
    if value.len() > 150_000 {
        "auto-off"
    } else {
        "auto"
    }
}

#[cfg(test)]
mod tests;
