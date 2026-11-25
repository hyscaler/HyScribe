// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_ajax_query_attachments()`'s `WP_Query`: the media grid's and the
//! media modal's query of the attachments, from the request's `query`.

use hyscribe_model::{OrderBy, PostQuery, StatusFilter};
use hyscribe_phpser::{Value, absint};
use hyscribe_render::{order_by, search_count, search_terms};

use super::library::mime_patterns;
use crate::posts::{PostsSite, author_ids, author_lists};

/// The attachments `query` (`$_REQUEST['query']`) asks for, with
/// `MEDIA_TRASH` as `media_trash`: the query and its posts per page; `None`
/// for a value PHP fails on or reads in a way not ported.
pub fn attachments_query(
    site: &dyn PostsSite,
    query: &Value,
    media_trash: bool,
) -> Option<(PostQuery, i64)> {
    let get = |key: &str| query.get_str(key);
    let trash = media_trash && get("post_status").and_then(Value::as_bytes) == Some(b"trash");
    let mut statuses = vec![if trash { "trash" } else { "inherit" }];
    if site.can("read_private_posts") {
        statuses.push("private");
    }
    let mut found = PostQuery {
        post_types: vec!["attachment".into()],
        statuses: statuses
            .into_iter()
            .map(|status| StatusFilter {
                status: status.into(),
                author: None,
                post_type: None,
            })
            .collect(),
        ..PostQuery::default()
    };
    let per_page = per_page(site, get("posts_per_page"))?;
    found.limit = match per_page {
        -1 => u32::MAX,
        n => u32::try_from(n).ok()?,
    };
    let paged = get("paged").filter(|paged| !matches!(paged, Value::Array(_)));
    let page = paged.map_or(0, absint).max(1);
    if per_page != -1 {
        let offset = (page - 1).checked_mul(per_page.unsigned_abs())?;
        found.offset = u32::try_from(offset).ok()?;
    }
    if let Some(mime) = get("post_mime_type").filter(|mime| mime.as_bytes() != Some(b"")) {
        found.mime_types = mime_patterns(&mime_list(mime)?);
    }
    if let Some(parent) = get("post_parent").filter(|parent| parent.is_numeric()) {
        found.parents = vec![u64::try_from(parent.to_int()).ok()?];
    }
    match get("author") {
        Some(Value::Array(_)) => return None,
        Some(author) => {
            let ids = author_ids(&String::from_utf8_lossy(&author.to_php_string()));
            (found.authors, found.excluded_authors) = author_lists(&ids.unwrap_or_default());
        }
        None => {}
    }
    found.include = ids(get("post__in"))?;
    if found.include.is_none() {
        found.exclude = ids(get("post__not_in"))?.unwrap_or_default();
    }
    let scalar = |key| get(key).filter(|value| !matches!(value, Value::Array(_)));
    let year = scalar("year").map_or(0, absint);
    let month = scalar("monthnum").map_or(0, absint);
    if month > 12 {
        return None;
    }
    found.year = (year != 0).then_some(year);
    found.month = (month != 0).then_some(month);
    // `parse_query()` drops a search that is not a string or is too long.
    let search = get("s").and_then(Value::as_bytes).unwrap_or_default();
    let search = String::from_utf8_lossy(search).replace(['\r', '\n'], "");
    if !search.is_empty() && search.len() <= 1600 {
        found.search = search_terms(&search);
        found.sentence.clone_from(&search);
        found.search_count = search_count(&search);
        found.filenames = true;
        found.logged_in = true;
    }
    let orderby = get("orderby").cloned().unwrap_or_default();
    let ascending = get("order")
        .and_then(Value::as_bytes)
        .is_some_and(|order| order.eq_ignore_ascii_case(b"asc"));
    found.order = order_by(&orderby, ascending, &found);
    // A search is ordered by relevance first unless another order is asked.
    let titled = found.search.iter().any(|term| !term.starts_with('-'));
    let relevance = orderby.is_empty() || orderby.as_bytes() == Some(b"relevance");
    if titled && relevance {
        found.order.insert(0, (OrderBy::Relevance, false));
    }
    Some((found, per_page))
}

/// `WP_Query`'s `posts_per_page`: the site's when empty, -1 for all, a
/// lower number as its absolute value; `None` for a value that is not a
/// whole number.
fn per_page(site: &dyn PostsSite, given: Option<&Value>) -> Option<i64> {
    let given = match given.filter(|given| !given.is_empty()) {
        Some(Value::String(text)) => text.clone(),
        Some(_) => return None,
        None => site.option("posts_per_page").unwrap_or_default(),
    };
    let text = String::from_utf8_lossy(&given);
    let number = text.parse::<i64>().ok()?;
    Some(match number {
        -1 => -1,
        n => n.saturating_abs().max(1),
    })
}

/// `wp_post_mime_type_where()`'s types of `mime`, a list or a comma
/// separated string, as one comma separated string.
fn mime_list(mime: &Value) -> Option<String> {
    let types = match mime {
        Value::Array(types) => types
            .iter()
            .map(|(_, mime)| match mime {
                Value::Array(_) => None,
                mime => Some(mime.to_php_string()),
            })
            .collect::<Option<Vec<_>>>()?,
        mime => vec![mime.to_php_string()],
    };
    // A type holding a comma would split where PHP's list does not.
    if mime.as_bytes().is_none() && types.iter().any(|mime| mime.contains(&b',')) {
        return None;
    }
    Some(String::from_utf8_lossy(&types.join(&b","[..])).into_owned())
}

/// `post__in` or `post__not_in`: the `absint()` of each ID of a list, none
/// when empty; `None` for a string, which PHP fails on.
fn ids(given: Option<&Value>) -> Option<Option<Vec<u64>>> {
    match given.filter(|given| !given.is_empty()) {
        None => Some(None),
        Some(Value::Array(ids)) => Some(Some(ids.iter().map(|(_, id)| absint(id)).collect())),
        Some(_) => None,
    }
}

#[cfg(test)]
mod tests;
