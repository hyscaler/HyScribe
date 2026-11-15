// SPDX-License-Identifier: AGPL-3.0-or-later

//! The categories and tags writes: `edit-tags.php`'s actions and the Ajax
//! actions of its list and form, `add-tag`, `inline-save-tax` and `delete-tag`.

use hyscribe_admin::actions::{Asked, items};
use hyscribe_admin::posts::Answer;
use hyscribe_admin::terms::{Added, Taxonomy as Listed, add_failed};
use hyscribe_auth::caps::taxonomy_cap;
use hyscribe_db::{TermRow, Write};
use hyscribe_kses::sanitize_key;
use hyscribe_model::Term;
use hyscribe_phpser::{Value, absint, add_query_arg, addslashes, lookup, remove_query_arg};
use hyscribe_write::Error;
use hyscribe_write::terms::{
    Deletion, Taxonomy, TermArgs, TermError, wp_delete_term, wp_insert_term, wp_update_term,
};

use super::{Ajax, Saving, Stop, referer};

/// The taxonomy `name`, if hyscribe-write writes its terms.
fn written(name: &[u8]) -> Result<(Taxonomy, Listed), Error> {
    let listed = Listed::named(name).ok_or(Error::Unported("the terms of this taxonomy"))?;
    let taxonomy = match name {
        b"category" => Taxonomy::Category,
        _ => Taxonomy::PostTag,
    };
    Ok((taxonomy, listed))
}

/// The term fields of `$_POST`, as `wp_insert_term()` and
/// `wp_update_term()` read their arguments.
struct Fields {
    /// `tag-name`, the name of a term to add.
    tag_name: Option<Vec<u8>>,
    name: Option<Vec<u8>>,
    slug: Option<Vec<u8>>,
    description: Option<Vec<u8>>,
    alias_of: Option<Vec<u8>>,
    parent: Option<i64>,
    term_group: Option<i64>,
}

impl Fields {
    /// The fields, slashed as the term functions take them; an array as
    /// a text is not ported.
    fn read(asked: &Asked) -> Result<Fields, Error> {
        let [tag_name, name, slug, description, alias_of] =
            ["tag-name", "name", "slug", "description", "alias_of"].map(|n| lookup(asked.post, n));
        let texts = [tag_name, name, slug, description, alias_of];
        if texts
            .iter()
            .flatten()
            .any(|text| matches!(text, Value::Array(_)))
        {
            return Err(Error::Unported("an array as a term's field"));
        }
        let slashed = |text: Option<&Value>| text.map(|text| addslashes(&text.to_php_string()));
        let int = |name| lookup(asked.post, name).map(Value::to_int);
        Ok(Fields {
            tag_name: slashed(tag_name),
            name: slashed(name),
            slug: slashed(slug),
            description: slashed(description),
            alias_of: slashed(alias_of),
            parent: int("parent"),
            term_group: int("term_group"),
        })
    }

    /// The name of a term to add: none is empty.
    fn new_name(&self) -> &[u8] {
        self.tag_name.as_deref().unwrap_or_default()
    }

    fn args(&self) -> TermArgs<'_> {
        TermArgs {
            name: self.name.as_deref(),
            slug: self.slug.as_deref(),
            description: self.description.as_deref(),
            parent: self.parent,
            alias_of: self.alias_of.as_deref(),
            term_group: self.term_group,
        }
    }
}

/// A `WP_Error`'s code and message.
type Failure = (&'static str, String);

/// A term function's result: its database and unported errors stop the
/// action, the others are PHP's `WP_Error`.
fn kept<T>(result: Result<T, TermError>, taxonomy: Taxonomy) -> Result<Result<T, Failure>, Error> {
    let (code, message) = match result {
        Ok(value) => return Ok(Ok(value)),
        Err(TermError::EmptyName) => ("empty_term_name", "A name is required for this term."),
        Err(TermError::MissingParent) => ("missing_parent", "Parent term does not exist."),
        Err(TermError::InvalidName) => ("invalid_term_name", "Invalid term name."),
        Err(TermError::InvalidTerm) => ("invalid_term", "Empty Term."),
        Err(TermError::Exists(_)) if taxonomy == Taxonomy::Category => (
            "term_exists",
            "A term with the name provided already exists with this parent.",
        ),
        Err(TermError::Exists(_)) => (
            "term_exists",
            "A term with the name provided already exists in this taxonomy.",
        ),
        Err(TermError::DuplicateSlug(slug)) => {
            let slug = String::from_utf8_lossy(&slug);
            let message =
                format!("The slug &#8220;{slug}&#8221; is already in use by another term.");
            return Ok(Err(("duplicate_term_slug", message)));
        }
        // The database's errors and what is not ported.
        Err(err) => return Err(err.into()),
    };
    Ok(Err((code, message.into())))
}

/// `edit-tags.php`'s `$referer`, which an action sends the list back to
/// with its notice.
fn noticed(asked: &Asked, notice: &[(&[u8], Option<&[u8]>)]) -> Vec<u8> {
    let referer = referer(asked).unwrap_or_else(|| asked.uri.to_vec());
    let keys: [&[u8]; 5] = [
        b"_wp_http_referer",
        b"_wpnonce",
        b"error",
        b"message",
        b"paged",
    ];
    add_query_arg(notice, &remove_query_arg(&keys, &referer))
}

/// `edit-tags.php`'s redirect to `location`, on the list's page.
fn sent(asked: &Asked, location: &[u8]) -> Answer {
    let page = asked.request("paged").map_or(0, absint).max(1);
    let location = if page > 1 {
        add_query_arg(&[(b"paged", Some(page.to_string().as_bytes()))], location)
    } else {
        remove_query_arg(&[b"paged"], location)
    };
    Answer::Redirect(String::from_utf8_lossy(&location).into_owned())
}

/// The terms of `edit-tags.php` are those of `$_REQUEST['taxonomy']`.
fn listed_taxonomy(asked: &Asked) -> Result<Taxonomy, Error> {
    let name = asked.request("taxonomy").and_then(Value::as_bytes);
    Ok(written(name.unwrap_or_default())?.0)
}

/// A term ID as `(int)` casts it, none below 1.
fn term_id(value: Option<&Value>) -> u64 {
    u64::try_from(value.map_or(0, Value::to_int)).unwrap_or_default()
}

/// `add-tag`: the term of the list's form added, then the list with
/// `message=1`, or `message=4` with `error`.
pub(super) fn add_tag<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let taxonomy = listed_taxonomy(asked)?;
        let fields = Fields::read(asked)?;
        let added = wp_insert_term(write, taxonomy, fields.new_name(), &fields.args()).await;
        let notice: &[(&[u8], Option<&[u8]>)] = match kept(added, taxonomy)? {
            Ok(_) => &[(b"message", Some(b"1"))],
            Err(_) => &[(b"error", Some(b"1")), (b"message", Some(b"4"))],
        };
        Ok(sent(asked, &noticed(asked, notice)))
    })
}

/// `delete`, a row's Delete: the term deleted, then the list with `message=2`.
pub(super) fn delete<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let taxonomy = listed_taxonomy(asked)?;
        // Without a term the list is shown, which is not ported here.
        let id = asked
            .request("tag_ID")
            .ok_or(Error::Unported("the list after a delete of no term"))?;
        // What it returns is not looked at.
        wp_delete_term(write, taxonomy, term_id(Some(id)))
            .await
            .map_err(Error::from)?;
        let location = noticed(asked, &[(b"message", Some(b"2"))]);
        Ok(sent(
            asked,
            &remove_query_arg(&[b"tag_ID", b"action"], &location),
        ))
    })
}

/// `bulk-delete`: each term of `delete_tags` deleted, then the list with
/// `message=6`.
pub(super) fn bulk_delete<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let taxonomy = listed_taxonomy(asked)?;
        let tags = asked.request("delete_tags").map(items).unwrap_or_default();
        for tag in tags {
            wp_delete_term(write, taxonomy, term_id(Some(&tag)))
                .await
                .map_err(Error::from)?;
        }
        Ok(sent(asked, &noticed(asked, &[(b"message", Some(b"6"))])))
    })
}

/// `editedtag`, the edit screen's Update: the term saved, then back with
/// `message=3`, or `message=5` with `error`.
pub(super) fn edited_tag<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a> {
    Box::pin(async move {
        let taxonomy = listed_taxonomy(asked)?;
        let id = term_id(lookup(asked.post, "tag_ID"));
        if write.term_in(taxonomy.name(), id).await?.is_none() {
            let message =
                "You attempted to edit an item that does not exist. Perhaps it was deleted?";
            return Err(Stop::Died(Answer::Died(message)));
        }
        let fields = Fields::read(asked)?;
        let updated = wp_update_term(write, taxonomy, id, &fields.args()).await;
        let notice: &[(&[u8], Option<&[u8]>)] = match kept(updated, taxonomy)? {
            Ok(_) => &[(b"message", Some(b"3"))],
            Err(_) => &[(b"error", Some(b"1")), (b"message", Some(b"5"))],
        };
        Ok(sent(asked, &noticed(asked, notice)))
    })
}

/// A term as the lists show it.
fn shown(row: TermRow) -> Term {
    Term {
        id: row.term_id,
        name: row.name,
        slug: row.slug,
        description: row.description,
        parent: row.parent,
        count: u64::try_from(row.count).unwrap_or_default(),
    }
}

/// `$_POST['taxonomy']` if not empty, else `post_tag`; an array is not ported.
fn posted_taxonomy(asked: &Asked) -> Result<Vec<u8>, Error> {
    match lookup(asked.post, "taxonomy").filter(|name| !name.is_empty()) {
        Some(Value::Array(_)) => Err(Error::Unported("an array as the taxonomy")),
        name => Ok(name.map_or(b"post_tag".to_vec(), Value::to_php_string)),
    }
}

/// `wp_ajax_add_tag()`: the term of the list's form added, answered with
/// its rows, or the error.
pub(super) fn ajax_add_tag<'a>(write: &'a mut Write<'_>, asked: &'a Asked<'a>) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let (taxonomy, listed) = written(&posted_taxonomy(asked)?)?;
        // The rows are the list's of the screen posted, its own here.
        let screen = format!("edit-{}", taxonomy.name());
        if lookup(asked.post, "screen").and_then(Value::as_bytes) != Some(screen.as_bytes()) {
            return Err(Error::Unported("the rows of another screen").into());
        }
        let fields = Fields::read(asked)?;
        let added = wp_insert_term(write, taxonomy, fields.new_name(), &fields.args()).await;
        let ids = match kept(added, taxonomy)? {
            Ok(ids) => ids,
            Err((code, message)) => return Ok(Ajax::Xml(add_failed(code, &message))),
        };
        // The action's own term, which it reads back.
        let row = write.term_in(taxonomy.name(), ids.term_id).await?;
        let row = row.unwrap_or_default();
        let added = Added {
            term_group: row.term_group,
            term_taxonomy_id: row.term_taxonomy_id,
            term: shown(row),
        };
        Ok(Ajax::TermAdded(listed, added))
    })
}

/// `wp_ajax_inline_save_tax()`, Quick Edit's save: the term with its own
/// description, answered with its row, or the error.
pub(super) fn inline_save_tax<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let name = lookup(asked.post, "taxonomy").and_then(Value::as_bytes);
        let (taxonomy, listed) = written(sanitize_key(name.unwrap_or_default()).as_bytes())?;
        let id = term_id(lookup(asked.post, "tax_ID"));
        let mut fields = Fields::read(asked)?;
        // `$_POST['description'] = $tag->description`, unslashed.
        fields.description = write
            .term_in(taxonomy.name(), id)
            .await?
            .map(|term| term.description);
        let updated = wp_update_term(write, taxonomy, id, &fields.args()).await;
        if let Err((_, message)) = kept(updated, taxonomy)? {
            return Ok(Ajax::Text(message));
        }
        let row = write.term_in(taxonomy.name(), id).await?;
        Ok(Ajax::TermRow(listed, shown(row.unwrap_or_default())))
    })
}

/// `wp_ajax_delete_tag()`: `1` once the term is deleted or for a term not
/// in the taxonomy, else `0`.
pub(super) fn ajax_delete_tag<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let id = term_id(lookup(asked.post, "tag_ID"));
        let name = posted_taxonomy(asked)?;
        let name = String::from_utf8_lossy(&name);
        // `get_term()` fails for a taxonomy not registered, or a term not in it.
        let registered = taxonomy_cap(&name, "delete_terms").is_some();
        let taxonomy = match registered {
            true => Some(written(name.as_bytes())?.0),
            false => None,
        };
        let term = match taxonomy {
            Some(taxonomy) => write.term_in(taxonomy.name(), id).await?,
            None => None,
        };
        let (Some(taxonomy), Some(_)) = (taxonomy, term) else {
            return Err(Stop::Died(Ajax::Text("1".into())));
        };
        let deleted = wp_delete_term(write, taxonomy, id)
            .await
            .map_err(Error::from)?;
        // `wp_die( 1 )` once deleted, else `wp_die( 0 )`.
        Ok(Ajax::Text(
            u8::from(deleted == Deletion::Deleted).to_string(),
        ))
    })
}

#[cfg(test)]
mod tests;
