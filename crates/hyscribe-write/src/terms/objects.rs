// SPDX-License-Identifier: AGPL-3.0-or-later

//! The terms of objects: `wp_set_object_terms()`, `wp_remove_object_terms()`,
//! `wp_set_post_terms()`, `wp_set_post_categories()`, and the counts they
//! recompute (`wp_update_term_count_now()`).

use hyscribe_db::Write;
use hyscribe_phpser::{Value, php_trim};

use super::edit::insert;
use super::{Act, Taxonomy, TermArgs, TermError, TermIds, db, existing, is_empty};

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
/// published posts: `_update_post_term_count()`.
async fn update_term_count(act: &mut Act<'_, '_>, tt_ids: &[u64]) {
    for &tt_id in tt_ids {
        let count = db!(act.term_post_count(tt_id, &["publish"], &["post"]));
        db!(act.set_term_count(tt_id, count));
    }
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
    let mut act = Act::new(write);
    let set = set_object_terms(&mut act, object, terms, taxonomy, append).await;
    act.finish(set)
}

pub(super) async fn set_object_terms(
    act: &mut Act<'_, '_>,
    object: u64,
    terms: &[TermRef<'_>],
    taxonomy: Taxonomy,
    append: bool,
) -> Result<Vec<u64>, TermError> {
    let old = if append {
        Vec::new()
    } else {
        db!(act.object_terms(object, taxonomy.name()))
    };
    let (mut tt_ids, mut added) = (Vec::new(), Vec::new());
    for &term in terms.iter().filter(|term| !term.is_blank()) {
        let ids = match (existing(act, taxonomy, term).await, term) {
            (Some(row), _) => TermIds::from(&row),
            (None, TermRef::Id(_)) => continue,
            (None, TermRef::Text(text)) => {
                insert(act, taxonomy, text, &TermArgs::default()).await?
            }
        };
        let tt_id = ids.term_taxonomy_id;
        tt_ids.push(tt_id);
        if !db!(act.has_relationship(object, tt_id)) {
            db!(act.add_relationship(object, tt_id));
            added.push(tt_id);
        }
    }
    update_term_count(act, &added).await;
    let gone = old.into_iter().filter(|(_, tt_id)| !tt_ids.contains(tt_id));
    let gone: Vec<TermRef<'_>> = gone.map(|(id, _)| TermRef::Id(id)).collect();
    remove_object_terms(act, object, &gone, taxonomy).await;
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
    let mut act = Act::new(write);
    let removed = remove_object_terms(&mut act, object, terms, taxonomy).await;
    act.finish(Ok(removed))
}

pub(super) async fn remove_object_terms(
    act: &mut Act<'_, '_>,
    object: u64,
    terms: &[TermRef<'_>],
    taxonomy: Taxonomy,
) -> bool {
    let mut tt_ids = Vec::new();
    // A text naming no term gives PHP a null ID, which matches nothing.
    for &term in terms.iter().filter(|term| !term.is_blank()) {
        if let Some(row) = existing(act, taxonomy, term).await {
            tt_ids.push(row.term_taxonomy_id);
        }
    }
    if tt_ids.is_empty() {
        return false;
    }
    let deleted = db!(act.remove_relationships(object, &tt_ids));
    update_term_count(act, &tt_ids).await;
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
    let mut act = Act::new(write);
    let set = set_post_terms(&mut act, post, terms, taxonomy, append).await;
    act.finish(set)
}

async fn set_post_terms(
    act: &mut Act<'_, '_>,
    post: u64,
    terms: TermList<'_>,
    taxonomy: Taxonomy,
    append: bool,
) -> Result<Option<Vec<u64>>, TermError> {
    if post == 0 {
        return Ok(None);
    }
    let mut list: Vec<TermRef<'_>> = match terms {
        TermList::Text(text) if is_empty(text) => Vec::new(),
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
    let set = set_object_terms(act, post, &list, taxonomy, append).await;
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
    let mut act = Act::new(write);
    let default;
    let (list, append) = match categories {
        [] => match db!(act.post_type_status(post)) {
            Some((post_type, status)) if post_type == "post" && status != "auto-draft" => {
                default = db!(act.option_value("default_category"));
                let id = default.as_deref().unwrap_or_default();
                (vec![TermRef::Text(id)], false)
            }
            _ => (Vec::new(), append),
        },
        [TermRef::Text([])] => return Ok(Some(Vec::new())),
        _ => (categories.to_vec(), append),
    };
    let list = TermList::List(&list);
    let set = set_post_terms(&mut act, post, list, Taxonomy::Category, append).await;
    act.finish(set)
}
