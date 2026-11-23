// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_insert_term()`, `wp_update_term()` and `wp_delete_term()`, with
//! `wp_unique_term_slug()` and the hierarchy loop check.

use hyscribe_db::{Meta, Write};
use hyscribe_phpser::{Value, addslashes, is_empty_string, php_trim, stripslashes};
use hyscribe_render::formatting::sanitize_title;

use super::objects::{TermRef, remove_object_terms, set_object_terms};
use super::{
    Taxonomy, TermArgs, TermError, TermIds, clean_taxonomy, existing, exists_slug, in_hierarchy,
    lossy, sanitize_description, sanitize_name, term_by_slug,
};
use crate::options::delete_object_meta;
use crate::steps::{Steps, step};

/// `wp_insert_term( $term, $taxonomy, $args )`: the new term's IDs, or an
/// older term's with the same slug under the same parent.
pub async fn wp_insert_term(
    write: &mut Write<'_>,
    taxonomy: Taxonomy,
    term: &[u8],
    args: &TermArgs<'_>,
) -> Result<TermIds, TermError> {
    let mut steps = Steps::new(write);
    let inserted = insert(&mut steps, taxonomy, term, args).await;
    steps.finish(inserted)
}

/// `wp_update_term( $term_id, $taxonomy, $args )`: the arguments not given
/// keep the term's values.
pub async fn wp_update_term(
    write: &mut Write<'_>,
    taxonomy: Taxonomy,
    term_id: u64,
    args: &TermArgs<'_>,
) -> Result<TermIds, TermError> {
    let mut steps = Steps::new(write);
    let updated = update(&mut steps, taxonomy, term_id, args).await;
    steps.finish(updated)
}

/// The `term_group` an alias joins: the group of the term `alias_of`
/// names, or a new one given to it with `wp_update_term()`.
async fn alias_group(
    steps: &mut Steps<'_, '_, TermError>,
    taxonomy: Taxonomy,
    alias_of: Option<&[u8]>,
    group: i64,
) -> i64 {
    let Some(alias_of) = alias_of.filter(|alias| !is_empty_string(alias)) else {
        return group;
    };
    match term_by_slug(steps, taxonomy, alias_of).await {
        Some(alias) if alias.term_group != 0 => alias.term_group,
        Some(alias) => {
            let group = step!(steps, max_term_group()) + 1;
            let args = TermArgs {
                term_group: Some(group),
                ..TermArgs::default()
            };
            // PHP ignores how the update went.
            let _ = Box::pin(update(steps, taxonomy, alias.term_id, &args)).await;
            group
        }
        None => group,
    }
}

pub(super) async fn insert(
    steps: &mut Steps<'_, '_, TermError>,
    taxonomy: Taxonomy,
    term: &[u8],
    args: &TermArgs<'_>,
) -> Result<TermIds, TermError> {
    if php_trim(term).is_empty() {
        return Err(TermError::EmptyName);
    }
    let parent = args.parent.unwrap_or_default();
    if parent > 0 && !step!(steps, term_exists(parent.unsigned_abs())) {
        return Err(TermError::MissingParent);
    }
    let parent = parent.max(0).unsigned_abs();
    let name = sanitize_name(term);
    let description = sanitize_description(args.description.unwrap_or_default());
    if name.is_empty() {
        return Err(TermError::InvalidName);
    }
    let given = args
        .slug
        .map(sanitize_title)
        .filter(|slug| !is_empty_string(slug));
    let slug_provided = given.is_some();
    let slug = given.unwrap_or_else(|| sanitize_title(&name));
    let term_group = alias_group(steps, taxonomy, args.alias_of, 0).await;

    // A term of the same name, but for the case, under the parent.
    let matches = if in_hierarchy(steps, taxonomy, parent).await {
        let query = lossy(&sanitize_name(&name));
        step!(steps, terms_under(taxonomy.name(), parent, Some(&query)))
    } else {
        Vec::new()
    };
    let lower = name.to_ascii_lowercase();
    let name_match = matches
        .into_iter()
        .find(|m| m.name.to_ascii_lowercase() == lower);
    if let Some(name_match) = name_match {
        let slug_match = term_by_slug(steps, taxonomy, &slug).await;
        let same_slug = !slug_provided || name_match.slug == slug;
        if same_slug || slug_match.is_some() {
            if !taxonomy.hierarchical() {
                return Err(TermError::Exists(name_match.term_id));
            }
            let siblings = step!(steps, terms_under(taxonomy.name(), parent, None));
            if same_slug && siblings.iter().any(|s| s.name == name) {
                return Err(TermError::Exists(name_match.term_id));
            }
            if let Some(slug_match) = slug_match
                && siblings.iter().any(|s| s.slug == slug)
            {
                return Err(TermError::Exists(slug_match.term_id));
            }
        }
    }

    let slug = unique_slug(steps, taxonomy, slug, (parent, 0)).await;
    let term_id = step!(steps, insert_term(&name, &slug, term_group));
    // A name of punctuation only has an empty slug: its ID stands in.
    let slug = if slug.is_empty() {
        let slug = term_id.to_string().into_bytes();
        step!(steps, set_term_slug(term_id, &slug));
        slug
    } else {
        slug
    };
    let row = (term_id, taxonomy.name());
    let tt_id = step!(steps, insert_term_taxonomy(row, &description, parent));
    let ids = (term_id, tt_id);
    let older = step!(
        steps,
        older_duplicate((taxonomy.name(), parent), &lossy(&slug), ids)
    );
    let ids = match older {
        Some(older) => {
            step!(steps, delete_term(term_id));
            step!(steps, delete_term_taxonomy(tt_id));
            TermIds::from(&older)
        }
        None => TermIds {
            term_id,
            term_taxonomy_id: tt_id,
        },
    };
    clean_taxonomy(steps, taxonomy).await;
    Ok(ids)
}

/// `wp_unique_term_slug( $slug, $term )` for a term of `taxonomy` under
/// `parent`, other than term `id` (0 for a new term).
async fn unique_slug(
    steps: &mut Steps<'_, '_, TermError>,
    taxonomy: Taxonomy,
    mut slug: Vec<u8>,
    (parent, id): (u64, u64),
) -> Vec<u8> {
    // `term_exists( $slug )` holds whenever `get_term_by()` finds the slug.
    if term_by_slug(steps, taxonomy, &slug).await.is_none() {
        return slug;
    }
    if taxonomy.hierarchical() {
        let mut ancestor = parent;
        while ancestor != 0 {
            let Some(term) = step!(steps, term_in(taxonomy.name(), ancestor)) else {
                break;
            };
            slug.push(b'-');
            slug.extend(&term.slug);
            if !text_exists(steps, &slug).await {
                break;
            }
            ancestor = term.parent;
        }
    }
    // `$wpdb->get_var()` gives the slug back, and `0` reads as false.
    if slug != b"0" && step!(steps, slug_taken(&lossy(&slug), id)) {
        let mut num = 2;
        loop {
            let alternative = [&slug[..], format!("-{num}").as_bytes()].concat();
            num += 1;
            // A failed read ends the loop, as a free slug would.
            if !step!(steps, slug_taken(&lossy(&alternative), 0)) {
                return alternative;
            }
        }
    }
    slug
}

/// `term_exists( $text )` in any taxonomy, for a text that is not empty.
async fn text_exists(steps: &mut Steps<'_, '_, TermError>, text: &[u8]) -> bool {
    let text = stripslashes(text);
    let text = php_trim(&text);
    let name = lossy(&sanitize_name(text));
    step!(steps, term_matches(exists_slug(text).as_deref(), &name))
}

async fn update(
    steps: &mut Steps<'_, '_, TermError>,
    taxonomy: Taxonomy,
    term_id: u64,
    args: &TermArgs<'_>,
) -> Result<TermIds, TermError> {
    let Some(term) = step!(steps, term_in(taxonomy.name(), term_id)) else {
        return Err(TermError::InvalidTerm);
    };
    // The term's values are slashed, as the arguments are.
    let merged = |given: Option<&[u8]>, value: &[u8]| {
        given.map_or_else(|| addslashes(value), <[u8]>::to_vec)
    };
    let name = sanitize_name(&merged(args.name, &term.name));
    let description = sanitize_description(&merged(args.description, &term.description));
    let slug = sanitize_title(&merged(args.slug, &term.slug));
    let parent = args
        .parent
        .unwrap_or(term.parent as i64)
        .max(0)
        .unsigned_abs();
    let term_group = args.term_group.unwrap_or(term.term_group).max(0);
    if php_trim(&name).is_empty() {
        return Err(TermError::EmptyName);
    }
    if parent > 0 && !step!(steps, term_exists(parent)) {
        return Err(TermError::MissingParent);
    }
    let empty_slug = is_empty_string(&slug);
    let mut slug = if empty_slug {
        sanitize_title(&name)
    } else {
        slug
    };
    let term_group = alias_group(steps, taxonomy, args.alias_of, term_group).await;
    let new_parent = wp_check_term_hierarchy_for_loops(steps, taxonomy, parent, term_id).await;

    let duplicate = term_by_slug(steps, taxonomy, &slug).await;
    if duplicate.is_some_and(|d| d.term_id != term_id) {
        // A new slug only when none was given or the term moves.
        if !empty_slug && new_parent == term.parent {
            return Err(TermError::DuplicateSlug(slug));
        }
        slug = unique_slug(steps, taxonomy, slug, (parent, term_id)).await;
    }
    if step!(steps, term_taxonomy_count(term_id)) > 1 {
        steps.keep::<()>(Err(TermError::Unported(
            "splitting a term shared by taxonomies",
        )));
    }
    let tt_id = term.term_taxonomy_id;
    step!(steps, update_term(term_id, &name, &slug, term_group));
    if slug.is_empty() {
        step!(
            steps,
            set_term_slug(term_id, term_id.to_string().as_bytes())
        );
    }
    let row = (term_id, taxonomy.name());
    step!(
        steps,
        update_term_taxonomy(tt_id, row, &description, new_parent)
    );
    clean_taxonomy(steps, taxonomy).await;
    Ok(TermIds {
        term_id,
        term_taxonomy_id: tt_id,
    })
}

/// `wp_check_term_hierarchy_for_loops()`, the `wp_update_term_parent`
/// filter: no parent when `parent` would close a loop through `term`; a
/// loop above it is broken by moving its terms to the top.
async fn wp_check_term_hierarchy_for_loops(
    steps: &mut Steps<'_, '_, TermError>,
    taxonomy: Taxonomy,
    parent: u64,
    term: u64,
) -> u64 {
    // No parent, or the term itself, needs no check of PHP's: the walk finds
    // no loop, or one through the term.
    let start = (term, parent);
    let Some(member) = tortoise_hare(steps, taxonomy, start, term, false).await.0 else {
        return parent;
    };
    let members = tortoise_hare(steps, taxonomy, start, member, true).await.1;
    if members.contains(&term) {
        return 0;
    }
    let top = TermArgs {
        parent: Some(0),
        ..TermArgs::default()
    };
    for member in members {
        // PHP ignores how each update went.
        let _ = Box::pin(update(steps, taxonomy, member, &top)).await;
    }
    parent
}

/// `wp_find_hierarchy_loop_tortoise_hare()` over the parents of terms,
/// with `start`'s parent overridden: a term of a loop above `from`, and
/// with `collect` the loop's terms, in the order PHP finds them.
async fn tortoise_hare(
    steps: &mut Steps<'_, '_, TermError>,
    taxonomy: Taxonomy,
    start: (u64, u64),
    from: u64,
    collect: bool,
) -> (Option<u64>, Vec<u64>) {
    let mut members = Vec::new();
    let (mut tortoise, mut hare) = (from, from);
    while tortoise != 0 {
        let evanescent = parent_of(steps, taxonomy, start, hare).await;
        if evanescent == 0 {
            break;
        }
        hare = parent_of(steps, taxonomy, start, evanescent).await;
        if hare == 0 {
            break;
        }
        if collect {
            for id in [tortoise, evanescent, hare] {
                if !members.contains(&id) {
                    members.push(id);
                }
            }
        }
        if tortoise == evanescent || tortoise == hare {
            return (Some(tortoise), members);
        }
        tortoise = parent_of(steps, taxonomy, start, tortoise).await;
    }
    (None, members)
}

/// `wp_get_term_taxonomy_parent_id()`, `start`'s parent overridden; 0 for
/// no parent or no term.
async fn parent_of(
    steps: &mut Steps<'_, '_, TermError>,
    taxonomy: Taxonomy,
    (term, parent): (u64, u64),
    id: u64,
) -> u64 {
    if id == term {
        return parent;
    }
    let term = step!(steps, term_in(taxonomy.name(), id));
    term.map_or(0, |term| term.parent)
}

/// What `wp_delete_term()` did, but for an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deletion {
    /// `true`: the term is deleted.
    Deleted,
    /// `false`: no such term in the taxonomy.
    Missing,
    /// `0`: the default category is never deleted.
    Default,
}

/// `wp_delete_term( $term, $taxonomy )`: its objects lose it, a category's
/// objects left without one get the default category, and its children
/// move to its parent.
pub async fn wp_delete_term(
    write: &mut Write<'_>,
    taxonomy: Taxonomy,
    term: u64,
) -> Result<Deletion, TermError> {
    let mut steps = Steps::new(write);
    let deletion = delete(&mut steps, taxonomy, term).await;
    steps.finish(Ok(deletion))
}

async fn delete(steps: &mut Steps<'_, '_, TermError>, taxonomy: Taxonomy, term: u64) -> Deletion {
    let Some(row) = existing(steps, taxonomy, TermRef::Id(term)).await else {
        return Deletion::Missing;
    };
    let mut default = None;
    if taxonomy == Taxonomy::Category {
        let option = step!(steps, option_value("default_category"));
        let id = Value::String(option.unwrap_or_default()).to_int();
        if id == term as i64 {
            return Deletion::Default;
        }
        let id = id.unsigned_abs();
        if existing(steps, taxonomy, TermRef::Id(id)).await.is_some() {
            default = Some(id);
        }
    }
    if step!(steps, term_menu_items(term, taxonomy.name())) > 0 {
        steps.keep::<()>(Err(TermError::Unported(
            "deleting the menu items of a term",
        )));
    }
    if taxonomy.hierarchical() {
        step!(steps, reparent_terms(taxonomy.name(), term, row.parent));
        clean_taxonomy(steps, taxonomy).await;
    }
    for object in step!(steps, term_object_ids(row.term_taxonomy_id)) {
        let Some(default) = default else {
            remove_object_terms(steps, object, &[TermRef::Id(term)], taxonomy).await;
            continue;
        };
        let terms = step!(steps, object_terms(object, taxonomy.name()));
        let terms: Vec<TermRef<'_>> = if terms.len() == 1 {
            vec![TermRef::Id(default)]
        } else {
            let others = terms.into_iter().filter(|&(id, _)| id != term);
            others.map(|(id, _)| TermRef::Id(id)).collect()
        };
        // Only IDs: no term is inserted, so nothing fails.
        let _ = set_object_terms(steps, object, &terms, taxonomy, false).await;
    }
    steps
        .run(async |w| delete_object_meta(w, Meta::Term, term).await)
        .await;
    step!(steps, delete_term_taxonomy(row.term_taxonomy_id));
    if step!(steps, term_taxonomy_count(term)) == 0 {
        step!(steps, delete_term(term));
    }
    clean_taxonomy(steps, taxonomy).await;
    Deletion::Deleted
}
