// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the post functions call of the option, meta and term functions:
//! `update_option()`, `add_post_meta()`, `delete_post_meta()`,
//! `wp_set_object_terms()`, `wp_remove_object_terms()` and the post term
//! counts, as far as posts use them.

use hyscribe_db::{Meta, Write};
use hyscribe_phpser::Value;

use super::{Kind, Result};

/// `(int) get_option( $name )`.
pub(super) async fn option_int(write: &mut Write<'_>, name: &str) -> Result<u64> {
    let value = write.option(name).await?.unwrap_or_default();
    Ok(u64::try_from(Value::String(value).to_int()).unwrap_or(0))
}

/// `update_option( $name, $value, $autoload )` of a value already
/// serialized as `maybe_serialize()` would.
pub(super) async fn update_option(
    write: &mut Write<'_>,
    name: &str,
    value: &[u8],
    autoload: Option<&str>,
) -> Result<()> {
    // wp_filter_default_autoload_value_via_option_size().
    let default = if value.len() > 150_000 {
        "auto-off"
    } else {
        "auto"
    };
    match write.option(name).await? {
        None => {
            write
                .add_option(name, value, autoload.unwrap_or(default))
                .await?;
        }
        Some(old) if old == value => {}
        Some(_) => match autoload {
            Some(autoload) => {
                write.update_option_autoload(name, value, autoload).await?;
            }
            None => {
                let raw = write.option_autoload(name).await?.unwrap_or_default();
                if ["auto-on", "auto-off", "auto"].contains(&raw.as_str()) && raw != default {
                    write.update_option_autoload(name, value, default).await?;
                } else {
                    write.update_option(name, value).await?;
                }
            }
        },
    }
    Ok(())
}

/// `get_default_comment_status( $type, ... )` from the option `option`.
pub(super) async fn default_status(
    write: &mut Write<'_>,
    kind: Kind,
    option: &str,
) -> Result<String> {
    Ok(match kind {
        Kind::Post => {
            let value = write.option(option).await?.unwrap_or_default();
            String::from_utf8_lossy(&value).into_owned()
        }
        Kind::Page | Kind::Revision => "closed".into(),
    })
}

/// `add_post_meta( $post, $key, $value, true )`.
pub(super) async fn add_unique_post_meta(
    write: &mut Write<'_>,
    post: u64,
    key: &str,
    value: &[u8],
) -> Result<()> {
    if write.meta_ids(Meta::Post, post, key).await?.is_empty() {
        write.add_meta(Meta::Post, post, key, value).await?;
    }
    Ok(())
}

/// `delete_post_meta( $post, $key )`.
pub(super) async fn delete_post_meta(write: &mut Write<'_>, post: u64, key: &str) -> Result<()> {
    let ids = write.meta_ids(Meta::Post, post, key).await?;
    write.delete_meta(Meta::Post, &ids).await?;
    Ok(())
}

/// `delete_post_meta( $post, $key, $value )`.
pub(super) async fn delete_post_meta_value(
    write: &mut Write<'_>,
    post: u64,
    key: &str,
    value: &[u8],
) -> Result<()> {
    let ids = write
        .meta_ids_valued(Meta::Post, (post, key), value)
        .await?;
    write.delete_meta(Meta::Post, &ids).await?;
    Ok(())
}

/// `wp_update_term_count( $tt_ids )` for the post taxonomies: published
/// posts only.
pub(super) async fn update_term_counts(write: &mut Write<'_>, tt_ids: &[u64]) -> Result<()> {
    for &tt_id in tt_ids {
        let count = write
            .term_post_count(tt_id, &["publish"], &["post"])
            .await?;
        write.set_term_count(tt_id, count).await?;
    }
    Ok(())
}

/// `wp_set_object_terms( $object, $terms, $taxonomy )` with term IDs.
pub(super) async fn set_object_terms(
    write: &mut Write<'_>,
    object: u64,
    terms: &[u64],
    taxonomy: &str,
) -> Result<()> {
    let old: Vec<u64> = write
        .object_terms(object, taxonomy)
        .await?
        .into_iter()
        .map(|(_, tt_id)| tt_id)
        .collect();
    let (mut tt_ids, mut added) = (Vec::new(), Vec::new());
    for &term in terms {
        let Some(tt_id) = write.term_taxonomy_id(term, taxonomy).await? else {
            continue;
        };
        if tt_ids.contains(&tt_id) {
            continue;
        }
        tt_ids.push(tt_id);
        if !old.contains(&tt_id) {
            write.add_relationship(object, tt_id).await?;
            added.push(tt_id);
        }
    }
    update_term_counts(write, &added).await?;
    let removed: Vec<u64> = old.into_iter().filter(|tt| !tt_ids.contains(tt)).collect();
    remove_relationships(write, object, &removed).await
}

/// `wp_remove_object_terms()` of the term taxonomies `tt_ids`.
async fn remove_relationships(write: &mut Write<'_>, object: u64, tt_ids: &[u64]) -> Result<()> {
    write.remove_relationships(object, tt_ids).await?;
    update_term_counts(write, tt_ids).await
}

/// `wp_delete_object_term_relationships( $object, $taxonomy )`.
pub(super) async fn remove_object_terms(
    write: &mut Write<'_>,
    object: u64,
    taxonomy: &str,
) -> Result<()> {
    let terms = write.object_terms(object, taxonomy).await?;
    let tt_ids: Vec<u64> = terms.into_iter().map(|(_, tt_id)| tt_id).collect();
    remove_relationships(write, object, &tt_ids).await
}
