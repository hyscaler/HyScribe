// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the post functions read of the options and
//! `wp_delete_object_term_relationships()`, as far as posts use them.

use hyscribe_db::Write;
use hyscribe_phpser::Value;

use super::{Error, Kind, Result};
use crate::terms::{Taxonomy, TermRef, wp_remove_object_terms};

/// `(int) get_option( $name )`.
pub(super) async fn option_int(write: &mut Write<'_>, name: &str) -> Result<u64> {
    let value = write.option_value(name).await?.unwrap_or_default();
    Ok(u64::try_from(Value::String(value).to_int()).unwrap_or(0))
}

/// `get_default_comment_status( $type, ... )` from the option `option`.
pub(super) async fn default_status(
    write: &mut Write<'_>,
    kind: Kind,
    option: &str,
) -> Result<String> {
    // Attachments support comments but not trackbacks.
    let supported = match kind {
        Kind::Post => true,
        Kind::Attachment => option == "default_comment_status",
        Kind::Page
        | Kind::Revision
        | Kind::GlobalStyles
        | Kind::UserRequest
        | Kind::Navigation
        | Kind::Block => false,
    };
    if !supported {
        return Ok("closed".into());
    }
    let value = write.option_value(option).await?.unwrap_or_default();
    Ok(String::from_utf8_lossy(&value).into_owned())
}

/// `wp_delete_object_term_relationships( $object, $taxonomies )`. The
/// terms port has no post formats or pattern categories, so a post with one
/// is refused.
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
            _ => {
                return Err(Error::Unported(
                    "a post with terms of a taxonomy not ported",
                ));
            }
        };
        wp_remove_object_terms(write, object, &terms, taxonomy).await?;
    }
    Ok(())
}
