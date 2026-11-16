// SPDX-License-Identifier: AGPL-3.0-or-later

//! The terms of objects: `wp_set_object_terms()`, `wp_remove_object_terms()`,
//! `wp_set_post_terms()`, `wp_set_post_categories()`, and the counts they
//! recompute (`wp_update_term_count_now()`).

use hyscribe_db::{DbError, Write};
use hyscribe_phpser::{Value, is_empty_string, php_trim};

use super::edit::insert;
use super::{Taxonomy, TermArgs, TermError, TermIds, existing};
use crate::steps::{Steps, step};

/// A term as the PHP functions take one: an ID, or a text matched as a
/// slug, then as a name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TermRef<'a> {
    /// An integer: a term ID.
    Id(u64),
    /// A string, slashed.
    Text(&'a [u8]),
}

impl TermRef<'_> {
    /// PHP's `'' === trim( $term )`.
    fn is_blank(self) -> bool {
        matches!(self, TermRef::Text(text) if php_trim(text).is_empty())
    }
}

/// The terms `wp_set_post_terms()` takes: a comma separated text, or a
/// list.
#[derive(Clone, Copy, Debug)]
pub enum TermList<'a> {
    /// Names or slugs separated by commas, slashed.
    Text(&'a [u8]),
    /// A list.
    List(&'a [TermRef<'a>]),
}

/// `wp_update_term_count_now()` for the core taxonomies, which count
/// published posts of the taxonomy's `types`: `_update_post_term_count()`.
pub(crate) async fn update_term_count(
    write: &mut Write<'_>,
    tt_ids: &[u64],
    types: &[&str],
) -> Result<(), DbError> {
    for &tt_id in tt_ids {
        let count = write.term_post_count(tt_id, &["publish"], types).await?;
        write.set_term_count(tt_id, count).await?;
    }
    Ok(())
}

/// `wp_set_object_terms( $object_id, $terms, $taxonomy, $append )`: the
/// term taxonomy IDs of `terms`. A text naming no term becomes a new term;
/// without `append`, the object's other terms of `taxonomy` are removed.
pub async fn wp_set_object_terms(
    write: &mut Write<'_>,
    object: u64,
    terms: &[TermRef<'_>],
    taxonomy: Taxonomy,
    append: bool,
) -> Result<Vec<u64>, TermError> {
    let mut steps = Steps::new(write);
    let set = set_object_terms(&mut steps, object, terms, taxonomy, append).await;
    steps.finish(set)
}

pub(super) async fn set_object_terms(
    steps: &mut Steps<'_, '_, TermError>,
    object: u64,
    terms: &[TermRef<'_>],
    taxonomy: Taxonomy,
    append: bool,
) -> Result<Vec<u64>, TermError> {
    let old = if append {
        Vec::new()
    } else {
        step!(steps, object_terms(object, taxonomy.name()))
    };
    let (mut tt_ids, mut added) = (Vec::new(), Vec::new());
    for &term in terms.iter().filter(|term| !term.is_blank()) {
        let ids = match (existing(steps, taxonomy, term).await, term) {
            (Some(row), _) => TermIds::from(&row),
            (None, TermRef::Id(_)) => continue,
            (None, TermRef::Text(text)) => {
                insert(steps, taxonomy, text, &TermArgs::default()).await?
            }
        };
        let tt_id = ids.term_taxonomy_id;
        tt_ids.push(tt_id);
        if !step!(steps, has_relationship(object, tt_id)) {
            step!(steps, add_relationship(object, tt_id));
            added.push(tt_id);
        }
    }
    steps
        .run(async |w| update_term_count(w, &added, taxonomy.object_types()).await)
        .await;
    let gone = old.into_iter().filter(|(_, tt_id)| !tt_ids.contains(tt_id));
    let gone: Vec<TermRef<'_>> = gone.map(|(id, _)| TermRef::Id(id)).collect();
    remove_object_terms(steps, object, &gone, taxonomy).await;
    Ok(tt_ids)
}

/// `wp_remove_object_terms( $object_id, $terms, $taxonomy )`: whether a
/// relationship was deleted.
pub async fn wp_remove_object_terms(
    write: &mut Write<'_>,
    object: u64,
    terms: &[TermRef<'_>],
    taxonomy: Taxonomy,
) -> Result<bool, TermError> {
    let mut steps = Steps::new(write);
    let removed = remove_object_terms(&mut steps, object, terms, taxonomy).await;
    steps.finish(Ok(removed))
}

pub(super) async fn remove_object_terms(
    steps: &mut Steps<'_, '_, TermError>,
    object: u64,
    terms: &[TermRef<'_>],
    taxonomy: Taxonomy,
) -> bool {
    let mut tt_ids = Vec::new();
    // A text naming no term gives PHP a null ID, which matches nothing.
    for &term in terms.iter().filter(|term| !term.is_blank()) {
        if let Some(row) = existing(steps, taxonomy, term).await {
            tt_ids.push(row.term_taxonomy_id);
        }
    }
    if tt_ids.is_empty() {
        return false;
    }
    let deleted = step!(steps, remove_relationships(object, &tt_ids));
    steps
        .run(async |w| update_term_count(w, &tt_ids, taxonomy.object_types()).await)
        .await;
    deleted > 0
}

/// `wp_set_post_terms( $post_id, $terms, $taxonomy, $append )`: `None`
/// for post 0. A hierarchical taxonomy takes IDs only, as integers.
pub async fn wp_set_post_terms(
    write: &mut Write<'_>,
    post: u64,
    terms: TermList<'_>,
    taxonomy: Taxonomy,
    append: bool,
) -> Result<Option<Vec<u64>>, TermError> {
    let mut steps = Steps::new(write);
    let set = set_post_terms(&mut steps, post, terms, taxonomy, append).await;
    steps.finish(set)
}

async fn set_post_terms(
    steps: &mut Steps<'_, '_, TermError>,
    post: u64,
    terms: TermList<'_>,
    taxonomy: Taxonomy,
    append: bool,
) -> Result<Option<Vec<u64>>, TermError> {
    if post == 0 {
        return Ok(None);
    }
    let mut list: Vec<TermRef<'_>> = match terms {
        TermList::Text(text) if is_empty_string(text) => Vec::new(),
        TermList::Text(text) => trim_commas(text)
            .split(|&b| b == b',')
            .map(TermRef::Text)
            .collect(),
        TermList::List(list) => list.to_vec(),
    };
    if taxonomy.hierarchical() {
        let mut ids = Vec::new();
        for term in list {
            // `intval()`; `term_exists()` takes the ID's absolute value.
            let id = match term {
                TermRef::Id(id) => id,
                TermRef::Text(text) => Value::String(text.to_vec()).to_int().unsigned_abs(),
            };
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        list = ids.into_iter().map(TermRef::Id).collect();
    }
    let set = set_object_terms(steps, post, &list, taxonomy, append).await;
    set.map(Some)
}

/// `trim( $terms, " \n\t\r\0\x0B," )`.
fn trim_commas(text: &[u8]) -> &[u8] {
    let strip = |b: &u8| b" \n\t\r\0\x0B,".contains(b);
    let start = text.iter().position(|b| !strip(b)).unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|b| !strip(b))
        .map_or(start, |i| i + 1);
    text.get(start..end).unwrap_or_default()
}

/// `wp_set_post_categories( $post_id, $post_categories, $append )`: no
/// categories give a post the default category. A single empty text
/// changes nothing (PHP's `true`, here `Some` of nothing).
pub async fn wp_set_post_categories(
    write: &mut Write<'_>,
    post: u64,
    categories: &[TermRef<'_>],
    append: bool,
) -> Result<Option<Vec<u64>>, TermError> {
    let mut steps = Steps::new(write);
    let default;
    let (list, append) = match categories {
        [] => match step!(steps, post(post)) {
            Some(row) if row.post_type == "post" && row.status != "auto-draft" => {
                default = step!(steps, option_value("default_category"));
                let id = default.as_deref().unwrap_or_default();
                (vec![TermRef::Text(id)], false)
            }
            _ => (Vec::new(), append),
        },
        [TermRef::Text([])] => return Ok(Some(Vec::new())),
        _ => (categories.to_vec(), append),
    };
    let list = TermList::List(&list);
    let set = set_post_terms(&mut steps, post, list, Taxonomy::Category, append).await;
    steps.finish(set)
}
