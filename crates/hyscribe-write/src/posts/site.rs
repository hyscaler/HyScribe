// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the post functions call of the option, meta and term functions:
//! `update_option()`, `add_post_meta()`, `delete_post_meta()`, the post term
//! counts and `wp_delete_object_term_relationships()`, as far as posts use
//! them.

use hyscribe_db::{Meta, Write};
use hyscribe_phpser::Value;

use super::{Kind, PostError, Result};
use crate::terms::{Taxonomy, TermRef, wp_remove_object_terms};

/// `(int) get_option( $name )`.
pub(super) async fn option_int(write: &mut Write<'_>, name: &str) -> Result<u64> {
    let value = write.option_value(name).await?.unwrap_or_default();
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
    match write.option_value(name).await? {
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
            let value = write.option_value(option).await?.unwrap_or_default();
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

/// `wp_delete_object_term_relationships( $object, $taxonomies )`. The
/// terms port has no post formats, so a post with one is refused.
pub(super) async fn delete_object_terms(
    write: &mut Write<'_>,
    object: u64,
    taxonomies: &[&str],
) -> Result<()> {
    for &name in taxonomies {
        let terms = write.object_terms(object, name).await?;
        let terms: Vec<TermRef<'_>> = terms.iter().map(|&(id, _)| TermRef::Id(id)).collect();
        let taxonomy = match name {
            "category" => Taxonomy::Category,
            "post_tag" => Taxonomy::PostTag,
            _ if terms.is_empty() => continue,
            _ => return Err(PostError::Unported("a post with a post format")),
        };
        wp_remove_object_terms(write, object, &terms, taxonomy).await?;
    }
    Ok(())
}
