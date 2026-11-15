// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a default install hooks to saving a post: `wp_transition_post_status()`
//! and its actions, `post_updated`'s old slugs and dates, the revision
//! `wp_save_post_revision()` makes, and the checks on the post's parent.

use hyscribe_db::{PostFields, Write};
use hyscribe_model::Post;
use hyscribe_phpser::{Value, unserialize};

use crate::options::{Autoload, Meta, add_metadata, delete_metadata, get_metadata, update_option};

use super::{Context, Error, Kind, PostArgs, Result, UNSLUGGED, cron, date, truthy};

/// `get_permalink( $id )` of a post saved as `post`: the GUID a new post
/// gets. Only plain links are ported, as pretty ones are not yet.
pub(super) async fn permalink(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    post: &PostFields,
) -> Result<Vec<u8>> {
    let structure = write
        .option_value("permalink_structure")
        .await?
        .unwrap_or_default();
    // Pages in the future take pretty links; posts do not.
    let plain_statuses: &[&str] = match post.post_type.as_str() {
        "page" => &UNSLUGGED,
        _ => &["draft", "pending", "auto-draft", "future"],
    };
    if !structure.is_empty() && !plain_statuses.contains(&post.status.as_str()) {
        return Err(Error::Unported("a GUID under pretty permalinks"));
    }
    let show_on_front = write
        .option_value("show_on_front")
        .await?
        .unwrap_or_default();
    let page_on_front = write
        .option_value("page_on_front")
        .await?
        .unwrap_or_default();
    let front_page = hyscribe_render::link::front_page(&show_on_front, &page_on_front);
    let post = Post {
        id,
        post_type: post.post_type.clone(),
        ..Post::default()
    };
    Ok(hyscribe_render::link::permalink(
        &ctx.home, front_page, &post,
    ))
}

/// The ID `wp_get_post_parent_id()` gives, 0 for none.
async fn parent_of(write: &mut Write<'_>, id: u64, link: (u64, u64)) -> Result<u64> {
    if id == link.0 {
        return Ok(link.1);
    }
    Ok(write.post(id).await?.map_or(0, |post| post.parent))
}

/// `wp_find_hierarchy_loop_tortoise_hare()` from `start`, with `link` as
/// the first's parent: a member of a loop, and the loop's members.
async fn tortoise_hare(
    write: &mut Write<'_>,
    start: u64,
    link: (u64, u64),
) -> Result<Option<(u64, Vec<u64>)>> {
    let (mut tortoise, mut hare) = (start, start);
    let mut members = Vec::new();
    // The tortoise trails the hare, so the hare reaches the top first.
    loop {
        let evanescent = parent_of(write, hare, link).await?;
        if evanescent == 0 {
            return Ok(None);
        }
        hare = parent_of(write, evanescent, link).await?;
        if hare == 0 {
            return Ok(None);
        }
        members.extend([tortoise, evanescent, hare]);
        if tortoise == evanescent || tortoise == hare {
            return Ok(Some((tortoise, members)));
        }
        tortoise = parent_of(write, tortoise, link).await?;
    }
}

/// The `wp_insert_post_parent` filter, `wp_check_post_hierarchy_for_loops()`:
/// no parent for a post that would be its own ancestor.
pub(super) async fn parent_without_loop(
    write: &mut Write<'_>,
    parent: u64,
    id: u64,
) -> Result<u64> {
    if parent == 0 || id == 0 {
        return Ok(parent);
    }
    if parent == id {
        return Ok(0);
    }
    let Some((member, _)) = tortoise_hare(write, id, (id, parent)).await? else {
        return Ok(parent);
    };
    let members = tortoise_hare(write, member, (id, parent)).await?;
    if members.is_some_and(|(_, members)| members.contains(&id)) {
        return Ok(0);
    }
    // PHP breaks a loop the post is not in by updating its members.
    Err(Error::Unported("a parent already in a loop"))
}

/// `block_core_calendar_update_has_published_posts()`.
pub(super) async fn update_has_published_posts(write: &mut Write<'_>) -> Result<()> {
    let has = Value::Bool(write.published_post_exists().await?);
    update_option(write, "wp_calendar_block_has_published_posts", &has, None).await?;
    Ok(())
}

/// `"1"`, the value of the flags `_publish_post_hook()` adds.
fn one() -> Value {
    Value::String(b"1".to_vec())
}

/// `_delete_option_fresh_site()`.
async fn fresh_site_is_used(write: &mut Write<'_>) -> Result<()> {
    let zero = Value::String(b"0".to_vec());
    update_option(write, "fresh_site", &zero, Some(Autoload::Off)).await?;
    Ok(())
}

/// `wp_slash( $value )`: `addslashes()` on every string, in arrays too.
fn slash(value: Value) -> Value {
    match value {
        Value::String(text) => Value::String(hyscribe_phpser::addslashes(&text)),
        Value::Array(entries) => Value::Array(
            entries
                .into_iter()
                .map(|(key, value)| (key, slash(value)))
                .collect(),
        ),
        other => other,
    }
}

/// `_publish_post_hook( $id )`.
async fn publish_post(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    post: &PostFields,
) -> Result<()> {
    if truthy(
        &write
            .option_value("default_pingback_flag")
            .await?
            .unwrap_or_default(),
    ) {
        add_metadata(write, Meta::Post, id, "_pingme", &one(), true).await?;
    }
    add_metadata(write, Meta::Post, id, "_encloseme", &one(), true).await?;
    if !super::sanitize_trackback_urls(&post.to_ping).is_empty() {
        add_metadata(write, Meta::Post, id, "_trackbackme", &one(), false).await?;
    }
    if !cron::scheduled(write, "do_pings").await? {
        cron::schedule(write, (ctx.now, ctx.now), "do_pings", None).await?;
    }
    Ok(())
}

/// Whether `nav_menu_options` adds new top level pages to a menu.
async fn auto_adds_pages(write: &mut Write<'_>) -> Result<bool> {
    let stored = write
        .option_value("nav_menu_options")
        .await?
        .unwrap_or_default();
    let menus = unserialize(&stored);
    let auto_add = menus
        .as_ref()
        .and_then(|options| options.get_str("auto_add"));
    Ok(auto_add
        .and_then(Value::as_array)
        .is_some_and(|menus| !menus.is_empty()))
}

/// `wp_transition_post_status( $new, $old, $post )` and what a default
/// install hooks to it.
pub(super) async fn transition(
    write: &mut Write<'_>,
    ctx: &Context,
    kind: Kind,
    (new, old): (&str, &str),
    id: u64,
) -> Result<()> {
    let post = super::existing(write, id).await?;
    // _transition_post_status().
    if old != "publish" && new == "publish" && post.guid.is_empty() {
        let guid = permalink(write, ctx, id, &post).await?;
        write.set_post_guid(id, &guid).await?;
    }
    cron::clear(write, "publish_future_post", id).await?;
    let crosses_publish = new != old && (new == "publish") != (old == "publish");
    // _update_term_count_on_transition_post_status().
    if crosses_publish {
        for taxonomy in kind.taxonomies() {
            let terms = write.object_terms(id, taxonomy).await?;
            let tt_ids: Vec<u64> = terms.into_iter().map(|(_, tt_id)| tt_id).collect();
            crate::terms::update_term_count(write, &tt_ids).await?;
        }
    }
    // _wp_auto_add_pages_to_menu().
    if kind == Kind::Page
        && new == "publish"
        && old != "publish"
        && post.parent == 0
        && auto_adds_pages(write).await?
    {
        return Err(Error::Unported("pages added to a classic menu"));
    }
    if crosses_publish && kind == Kind::Post {
        update_has_published_posts(write).await?;
    }
    match (new, kind) {
        ("publish", Kind::Post) => {
            fresh_site_is_used(write).await?;
            publish_post(write, ctx, id, &post).await
        }
        ("publish", Kind::Page) => fresh_site_is_used(write).await,
        // _future_post_hook().
        ("future", _) => {
            cron::clear(write, "publish_future_post", id).await?;
            let at = date::Dates::read(write, ctx.now).await?.unix(&post.date);
            cron::schedule(write, (ctx.now, at), "publish_future_post", Some(id)).await
        }
        _ => Ok(()),
    }
}

/// The day `gmdate( 'Y-m-d', strtotime( $date ) )` gives of a stored date.
fn day(date: &str) -> String {
    match date.get(..10) {
        Some("0000-00-00") => "-0001-11-30".into(),
        day => day.unwrap_or_default().into(),
    }
}

/// `wp_check_for_changed_slugs()` or `wp_check_for_changed_dates()`: keeps
/// `old` under `key` and forgets `new` there.
async fn remember_old(
    write: &mut Write<'_>,
    id: u64,
    key: &str,
    (old, new): (&[u8], &[u8]),
) -> Result<()> {
    let kept = get_metadata(write, Meta::Post, id, key).await?;
    let (old, new) = (Value::String(old.to_vec()), Value::String(new.to_vec()));
    if !old.is_empty() && !kept.contains(&old) {
        add_metadata(write, Meta::Post, id, key, &old, false).await?;
    }
    if kept.contains(&new) {
        delete_metadata(write, Meta::Post, id, key, &new, false).await?;
    }
    Ok(())
}

/// The `post_updated` action's old slugs and dates.
pub(super) async fn post_updated(
    write: &mut Write<'_>,
    kind: Kind,
    id: u64,
    before: &PostFields,
) -> Result<()> {
    let after = super::existing(write, id).await?;
    if after.status != "publish" || kind.hierarchical() {
        return Ok(());
    }
    if after.name != before.name {
        remember_old(write, id, "_wp_old_slug", (&before.name, &after.name)).await?;
    }
    let (old, new) = (day(&before.date), day(&after.date));
    if old != new {
        remember_old(write, id, "_wp_old_date", (old.as_bytes(), new.as_bytes())).await?;
    }
    Ok(())
}

/// `normalize_whitespace( $text )`.
fn normalize_whitespace(text: &[u8]) -> Vec<u8> {
    let text = hyscribe_phpser::php_trim(text);
    let mut out: Vec<u8> = Vec::with_capacity(text.len());
    for &b in text {
        let b = if b == b'\r' { b'\n' } else { b };
        let blank = b == b' ' || b == b'\t';
        match out.last() {
            Some(b'\n') if b == b'\n' => {}
            Some(b' ') if blank => {}
            _ => out.push(if blank { b' ' } else { b }),
        }
    }
    out
}

/// `wp_save_post_revision( $id )`: a revision of the post unless its last
/// one has the same title, content, excerpt and footnotes.
pub(super) async fn save_revision(
    write: &mut Write<'_>,
    ctx: &Context,
    kind: Kind,
    id: u64,
) -> Result<()> {
    let post = super::existing(write, id).await?;
    if kind == Kind::Revision || post.status == "auto-draft" {
        return Ok(());
    }
    let footnotes = get_metadata(write, Meta::Post, id, "footnotes").await?;
    let marker = format!("{id}-revision").into_bytes();
    for revision in write.revision_ids(id).await? {
        let latest = super::existing(write, revision).await?;
        if !latest.name.windows(marker.len()).any(|w| w == marker) {
            continue;
        }
        let fields = [
            (&post.title, &latest.title),
            (&post.content, &latest.content),
            (&post.excerpt, &latest.excerpt),
        ];
        let changed = fields
            .iter()
            .any(|(now, then)| normalize_whitespace(now) != normalize_whitespace(then));
        if !changed && get_metadata(write, Meta::Post, revision, "footnotes").await? == footnotes {
            return Ok(());
        }
        break;
    }
    let args = PostArgs {
        title: post.title,
        content: post.content,
        excerpt: post.excerpt,
        parent: id,
        status: "inherit".into(),
        post_type: "revision".into(),
        name: Some(format!("{id}-revision-v1").into_bytes()),
        date: post.modified,
        date_gmt: post.modified_gmt,
        ..PostArgs::default()
    };
    let (revision, ..) = super::insert(write, ctx, args).await?;
    // wp_save_revisioned_meta_fields(), through _wp_copy_post_meta()'s
    // wp_slash().
    for value in footnotes {
        let value = slash(value);
        add_metadata(write, Meta::Post, revision, "footnotes", &value, false).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
