// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_unique_post_slug()`, `_truncate_post_slug()` and the `__trashed`
//! suffix trashed posts give up their slug with.

use hyscribe_db::Write;
use hyscribe_phpser::Value;
use hyscribe_render::formatting::sanitize_title_for_query;

use crate::options::{Meta, add_metadata};

use super::{Error, Kind, Result, UNSLUGGED};

/// `$wp_rewrite->feeds`.
const FEEDS: [&[u8]; 5] = [b"feed", b"rdf", b"rss", b"rss2", b"atom"];

/// `_truncate_post_slug( $slug, $length )`.
fn truncate(slug: &[u8], length: usize) -> Result<Vec<u8>> {
    let mut slug = slug.to_vec();
    if slug.len() > length {
        if hyscribe_phpser::urldecode(&slug) != slug {
            return Err(Error::Unported("a long percent-encoded slug"));
        }
        slug.truncate(length);
    }
    while slug.last() == Some(&b'-') {
        slug.pop();
    }
    Ok(slug)
}

/// `/^(page)?\d+$/`, which would read as a page number.
fn paged(slug: &[u8]) -> bool {
    let digits = slug.strip_prefix(b"page").unwrap_or(slug);
    !digits.is_empty() && digits.iter().all(u8::is_ascii_digit)
}

/// Whether the numeric slug `number` would be read as a year, a month or a
/// day under the permalink `structure`.
fn reads_as_date(structure: &[u8], number: u64) -> bool {
    // array_filter() drops empty parts and "0".
    let parts: Vec<&[u8]> = structure
        .split(|&b| b == b'/')
        .filter(|part| !part.is_empty() && *part != b"0")
        .collect();
    match parts.iter().position(|part| *part == b"%postname%") {
        Some(0) => true,
        Some(at) => match parts.get(at - 1).copied() {
            Some(b"%year%") => number < 13,
            Some(b"%monthnum%") => number < 32,
            _ => false,
        },
        None => false,
    }
}

/// Whether a new numeric post slug would be read as a date archive.
async fn date_archive(write: &mut Write<'_>, slug: &[u8], id: u64) -> Result<bool> {
    let numeric = !slug.is_empty() && slug.iter().all(u8::is_ascii_digit);
    if !numeric || slug.iter().all(|&b| b == b'0') {
        return Ok(false);
    }
    if write.post(id).await?.is_some_and(|post| post.name == slug) {
        return Ok(false);
    }
    let structure = write
        .option_value("permalink_structure")
        .await?
        .unwrap_or_default();
    // (int) of a number past PHP_INT_MAX is PHP_INT_MAX.
    let number = std::str::from_utf8(slug)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(u64::MAX);
    Ok(reads_as_date(&structure, number))
}

/// `wp_unique_post_slug( $slug, $id, $status, $type, $parent )`.
pub(super) async fn unique(
    write: &mut Write<'_>,
    slug: Vec<u8>,
    id: u64,
    (status, kind, post_type): (&str, Kind, &str),
    parent: u64,
) -> Result<Vec<u8>> {
    let revision = status == "inherit" && kind == Kind::Revision;
    if UNSLUGGED.contains(&status) || revision || kind == Kind::UserRequest {
        return Ok(slug);
    }
    if matches!(kind, Kind::Template | Kind::TemplatePart) {
        return template_unique(write, slug, id, post_type).await;
    }
    let reserved = FEEDS.contains(&&slug[..]) || slug == b"embed";
    // Attachment slugs are unique across all types.
    let attachment = kind == Kind::Attachment;
    // PHP takes the name it finds as its check, which an empty one fails.
    let found = super::truthy(&slug);
    let taken = if attachment {
        found && write.any_post_name_used(&slug, id).await? || reserved
    } else if kind.hierarchical() {
        let used = write
            .sibling_name_used(&slug, (post_type, parent), id)
            .await?;
        used || reserved || paged(&slug)
    } else {
        let used = write.post_name_used(&slug, post_type, id).await?;
        used || reserved || (kind == Kind::Post && date_archive(write, &slug, id).await?)
    };
    if !taken {
        return Ok(slug);
    }
    let mut number = 2_u64;
    loop {
        let suffix = number.to_string();
        let mut alt = truncate(&slug, 200 - (suffix.len() + 1))?;
        alt.push(b'-');
        alt.extend_from_slice(suffix.as_bytes());
        let used = if attachment {
            write.any_post_name_used(&alt, id).await?
        } else if kind.hierarchical() {
            write
                .sibling_name_used(&alt, (post_type, parent), id)
                .await?
        } else {
            write.post_name_used(&alt, post_type, id).await?
        };
        if !used {
            return Ok(alt);
        }
        number += 1;
    }
}

/// `wp_filter_wp_template_unique_post_slug()`, the `pre_wp_unique_post_slug`
/// filter of templates and parts: unique within the post's theme, else the
/// active one's.
async fn template_unique(
    write: &mut Write<'_>,
    slug: Vec<u8>,
    id: u64,
    post_type: &str,
) -> Result<Vec<u8>> {
    let mut theme = write.option_value("stylesheet").await?.unwrap_or_default();
    // `get_the_terms()` orders them by name.
    let mut names = Vec::new();
    for (term, _) in write.object_terms(id, "wp_theme").await? {
        names.extend(write.term_in("wp_theme", term).await?.map(|term| term.name));
    }
    names.sort();
    if let Some(name) = names.into_iter().next() {
        theme = name;
    }
    let mut alt = slug.clone();
    let mut number = 2_u64;
    while write
        .template_name_used(&alt, (post_type, &theme), id)
        .await?
    {
        let suffix = number.to_string();
        alt = truncate(&slug, 200 - (suffix.len() + 1))?;
        alt.push(b'-');
        alt.extend_from_slice(suffix.as_bytes());
        number += 1;
    }
    Ok(alt)
}

/// `wp_add_trashed_suffix_to_post_name_for_post( $id )`: the post's new
/// name.
pub(super) async fn suffix_trashed(write: &mut Write<'_>, id: u64) -> Result<Vec<u8>> {
    let name = write
        .post(id)
        .await?
        .map(|post| post.name)
        .unwrap_or_default();
    if name.ends_with(b"__trashed") {
        return Ok(name);
    }
    let desired = Value::String(name.clone());
    add_metadata(
        write,
        Meta::Post,
        id,
        "_wp_desired_post_slug",
        &desired,
        false,
    )
    .await?;
    let mut trashed = truncate(&name, 191)?;
    trashed.extend_from_slice(b"__trashed");
    write.set_post_name(id, &trashed).await?;
    Ok(trashed)
}

/// `wp_add_trashed_suffix_to_post_name_for_trashed_posts( $name, $id )`.
pub(super) async fn suffix_trashed_namesakes(
    write: &mut Write<'_>,
    name: &[u8],
    id: u64,
) -> Result<()> {
    let name = sanitize_title_for_query(name);
    for trashed in write.trashed_post_ids_named(&name, id).await? {
        suffix_trashed(write, trashed).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
