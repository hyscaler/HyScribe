// SPDX-License-Identifier: AGPL-3.0-or-later

//! The posts and pages lists' writes: `post.php`'s trash, restore and
//! delete, `edit.php`'s bulk actions with Empty Trash and Bulk Edit, and
//! Quick Edit, `admin-ajax.php`'s `inline-save`.

use hyscribe_admin::actions::{Asked, action};
use hyscribe_admin::posts::Answer;
use hyscribe_admin::revisions::RevisionArgs;
use hyscribe_config::Constants;
use hyscribe_db::{DbError, Meta, PostFields, UserFields, Write};
use hyscribe_phpser::{Key, Value, absint, add_query_arg, addslashes, lookup, remove_query_arg};
use hyscribe_render::date::{Zone, format_unix, stored_unix};
use hyscribe_write::caps::Caps;
use hyscribe_write::options::update_metadata;
use hyscribe_write::posts::{
    Context, PostArgs, stick_post, unstick_post, wp_checkdate, wp_delete_post,
    wp_restore_post_revision, wp_trash_post, wp_unique_post_slug, wp_untrash_post, wp_update_post,
};
use hyscribe_write::terms::{Taxonomy, TermList, TermRef, wp_set_post_terms};
use hyscribe_write::{Error, wp_timezone};

use super::{Acting, Ajax, Saving, Stop, id_of, referer, text};

/// What the lists' forms never send and `edit_post()` or `wp_insert_post()`
/// act on: custom fields, post format fields, the edit screen's buttons.
const UNSENT: [&str; 25] = [
    "tags_input",
    "_thumbnail_id",
    "file",
    "context",
    "trackback_url",
    "post_author_override",
    "saveasdraft",
    "saveasprivate",
    "publish",
    "advanced",
    "pending",
    "visibility",
    "meta",
    "deletemeta",
    "metakeyselect",
    "metakeyinput",
    "_format_url",
    "_format_link_url",
    "_format_quote_source_url",
    "_format_quote",
    "_format_quote_source_name",
    "_format_image",
    "_format_gallery",
    "_format_audio_embed",
    "_format_video_embed",
];

/// The fields read as text, where PHP fails on an array.
const TEXTS: [&str; 21] = [
    "post_title",
    "post_name",
    "post_date",
    "post_date_gmt",
    "post_content",
    "post_content_filtered",
    "post_excerpt",
    "post_status",
    "_status",
    "comment_status",
    "ping_status",
    "post_password",
    "to_ping",
    "pinged",
    "page_template",
    "aa",
    "mm",
    "jj",
    "hh",
    "mn",
    "ss",
];

/// Refuses as not ported a request with a field of [`UNSENT`], or an array
/// for a field of [`TEXTS`].
fn sent_by_the_lists(fields: &Data) -> Result<(), Error> {
    let unsent = UNSENT.iter().any(|name| fields.get(name).is_some());
    let array = |name: &&str| matches!(fields.get(name), Some(Value::Array(_)));
    match unsent || TEXTS.iter().any(array) {
        true => Err(Error::Unported("a field the lists do not send")),
        false => Ok(()),
    }
}

/// The statuses `get_post_status_object()` finds.
const STATUSES: [&str; 12] = [
    "publish",
    "future",
    "draft",
    "pending",
    "private",
    "trash",
    "auto-draft",
    "inherit",
    "request-pending",
    "request-confirmed",
    "request-failed",
    "request-completed",
];

/// `get_post_format_slugs()` but `standard`.
const FORMATS: [&str; 9] = [
    "aside", "chat", "gallery", "link", "image", "quote", "status", "video", "audio",
];

/// `add_query_arg()`'s arguments.
type QueryArgs<'a> = [(&'a [u8], Option<&'a [u8]>)];

/// `str_contains( $url, $file )`, for hs-admin's path of `file` too.
fn mentions(url: &[u8], file: &str) -> bool {
    let has = |part: &[u8]| url.windows(part.len()).any(|window| window == part);
    let path = format!("/hs-admin/{}", file.trim_end_matches(".php"));
    let at = url
        .windows(path.len())
        .position(|window| window == path.as_bytes());
    let ends = at.map(|at| url.get(at + path.len()));
    has(file.as_bytes()) || matches!(ends, Some(None | Some(b'?' | b'#')))
}

/// `wp_check_post_lock( $id )` for `user`: the other user whose lock is
/// fresh, with their fields.
pub(super) async fn locked(
    write: &mut Write<'_>,
    id: u64,
    user: u64,
    now: i64,
) -> Result<Option<UserFields>, DbError> {
    let lock = write.meta_value(Meta::Post, id, "_edit_lock").await?;
    let lock = lock.unwrap_or_default();
    let int = |text: &[u8]| Value::String(text.to_vec()).to_int();
    let mut parts = lock.split(|&b| b == b':');
    let time = int(parts.next().unwrap_or_default());
    let holder = match parts.next() {
        Some(holder) => int(holder),
        None => {
            let last = write.meta_value(Meta::Post, id, "_edit_last").await?;
            int(&last.unwrap_or_default())
        }
    };
    let holder = u64::try_from(holder).unwrap_or_default();
    if time == 0 || time <= now - 150 || holder == user {
        return Ok(None);
    }
    Ok(write.user(holder).await?.map(|(_, fields)| fields))
}

/// `current_user_can( 'unfiltered_html' )` of `user` in the action, which
/// `kses_init()` asks.
pub(super) async fn unfiltered_html(
    write: &mut Write<'_>,
    user: u64,
    constants: Constants,
) -> Result<bool, DbError> {
    let caps = Caps::load(write, user, None, constants).await?;
    Ok(caps.can("unfiltered_html", &[]))
}

/// The action's context for posts of `post_type`.
fn context(asked: &Asked, site: &dyn Acting, (caps, post_type): (&Caps, &str)) -> Context {
    Context {
        can_publish: caps.can_type("publish_posts", post_type),
        unfiltered_html: caps.can("unfiltered_html", &[]),
        ..super::dashboard::context(
            asked.user,
            asked.home,
            &site.admin_url(""),
            site.constants(),
        )
    }
}

/// The post the request names, which its checks found.
async fn existing(write: &mut Write<'_>, id: u64) -> Result<PostFields, Error> {
    let post = write.post(id).await?;
    post.ok_or(Error::Unported("a post gone since its checks"))
}

/// `post.php`'s action on the post the request names.
async fn post_php(
    write: &mut Write<'_>,
    (asked, site): (&Asked<'_>, &dyn Acting),
    action: &str,
) -> Result<Answer, Stop> {
    let id = lookup(asked.get, "post").or_else(|| lookup(asked.post, "post_ID"));
    let id = id_of(id);
    let post = existing(write, id).await?;
    if post.post_type == "attachment" {
        return Err(Error::Unported("an attachment, which the media library handles").into());
    }
    let sendback = match referer(asked) {
        Some(referer) if !mentions(&referer, "post.php") && !mentions(&referer, "post-new.php") => {
            remove_query_arg(&[b"trashed", b"untrashed", b"deleted", b"ids"], &referer)
        }
        _ => add_query_arg(
            &[(b"post_type", Some(post.post_type.as_bytes()))],
            site.admin_url("edit.php").as_bytes(),
        ),
    };
    let caps = Caps::load(write, asked.user, None, site.constants()).await?;
    let ctx = context(asked, site, (&caps, &post.post_type));
    let id_text = id.to_string();
    let ids = (b"ids".as_slice(), Some(id_text.as_bytes()));
    let (done, failed, args): (bool, _, &QueryArgs) = match action {
        "trash" => (
            wp_trash_post(write, &ctx, id).await?,
            "Error in moving the item to Trash.",
            &[(b"trashed", Some(b"1")), ids],
        ),
        "untrash" => (
            wp_untrash_post(write, &ctx, id, false).await?,
            "Error in restoring the item from Trash.",
            &[(b"untrashed", Some(b"1")), ids],
        ),
        _ => (
            wp_delete_post(write, &ctx, id, true).await?,
            "Error in deleting the item.",
            &[(b"deleted", Some(b"1"))],
        ),
    };
    if !done {
        return Err(Stop::Died(Answer::Died(failed)));
    }
    Ok(Answer::Redirect(text(&add_query_arg(args, &sendback))))
}

/// `revision.php?action=restore`: `wp_restore_post_revision()`, then the
/// post's edit screen with its notice.
pub(super) fn restore<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        let args = RevisionArgs::read(&|name| asked.request(name).cloned());
        let id = args.map_or(0, |args| args.revision);
        let revision = existing(write, id).await?;
        let post = existing(write, revision.parent).await?;
        if !["post", "page"].contains(&post.post_type.as_str()) {
            return Err(Error::Unported("revisions of a type that may turn them off").into());
        }
        let caps = Caps::load(write, asked.user, None, site.constants()).await?;
        let ctx = context(asked, site, (&caps, &post.post_type));
        wp_restore_post_revision(write, &ctx, id).await?;
        let edit = site.admin_url(&format!("post.php?post={}&action=edit", revision.parent));
        let id = id.to_string();
        let args: &QueryArgs = &[(b"message", Some(b"5")), (b"revision", Some(id.as_bytes()))];
        Ok(Answer::Redirect(text(&add_query_arg(
            args,
            edit.as_bytes(),
        ))))
    })
}

/// `post.php?action=trash`.
pub(super) fn trash<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(post_php(write, (asked, site), "trash"))
}

/// `post.php?action=untrash`.
pub(super) fn untrash<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(post_php(write, (asked, site), "untrash"))
}

/// `post.php?action=delete`, and its Delete button.
pub(super) fn delete<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(post_php(write, (asked, site), "delete"))
}

/// `edit.php`'s IDs of the request: Empty Trash's, `ids` or `post`.
async fn bulk_ids(
    write: &mut Write<'_>,
    asked: &Asked<'_>,
    (doaction, post_type): (&[u8], &str),
) -> Result<Vec<Value>, Stop> {
    if doaction == b"delete_all" {
        let status = asked.request("post_status").map(Value::to_php_string);
        let mut status = text(&status.unwrap_or_default());
        status.retain(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
        if !STATUSES.contains(&status.as_str()) {
            return Ok(Vec::new());
        }
        let ids = write.status_post_ids(post_type, &status).await?;
        return Ok(ids
            .into_iter()
            .map(|id| Value::String(id.to_string().into_bytes()))
            .collect());
    }
    // The media list's IDs, and PHP's fatal errors: `explode()` and
    // `array_map()` of what is not a string, or an array.
    let ids = match (
        asked.request("media"),
        asked.request("ids"),
        asked.request("post"),
    ) {
        (None, Some(Value::String(ids)), _) => ids
            .split(|&b| b == b',')
            .map(|id| Value::String(id.to_vec()))
            .collect(),
        (None, None, Some(Value::Array(posts))) => posts
            .iter()
            .map(|(_, id)| Value::Int(id.to_int()))
            .collect(),
        (None, None, None) => Vec::new(),
        (None, None, Some(post)) if post.is_empty() => Vec::new(),
        _ => return Err(Error::Unported("IDs PHP cannot read").into()),
    };
    Ok(ids)
}

/// `add_query_arg()` of each of `names` with its count.
fn with_counts(names: &[&[u8]], counts: impl IntoIterator<Item = usize>, url: &[u8]) -> Vec<u8> {
    let counts: Vec<String> = counts.into_iter().map(|count| count.to_string()).collect();
    let args: Vec<(&[u8], Option<&[u8]>)> = names
        .iter()
        .zip(&counts)
        .map(|(name, count)| (*name, Some(count.as_bytes())))
        .collect();
    add_query_arg(&args, url)
}

/// `edit.php`'s trash, restore or delete of `ids` as `doaction`, each post
/// in turn: `sendback` with what was done.
async fn bulk_write(
    write: &mut Write<'_>,
    asked: &Asked<'_>,
    (ctx, doaction): (&Context, &[u8]),
    ids: &[Value],
    sendback: &[u8],
) -> Result<Vec<u8>, Stop> {
    let (denied, failed, done): (&str, &str, &[u8]) = match doaction {
        b"trash" => (
            "Sorry, you are not allowed to move this item to the Trash.",
            "Error in moving the item to Trash.",
            b"trashed",
        ),
        b"untrash" => (
            "Sorry, you are not allowed to restore this item from the Trash.",
            "Error in restoring the item from Trash.",
            b"untrashed",
        ),
        _ => (
            "Sorry, you are not allowed to delete this item.",
            "Error in deleting the item.",
            b"deleted",
        ),
    };
    let undo = lookup(asked.get, "doaction").and_then(Value::as_bytes) == Some(b"undo");
    let (mut count, mut skipped) = (0, 0);
    for id in ids {
        let post = id.is_numeric().then(|| u64::try_from(id.to_int()).ok());
        let caps = Caps::load(write, asked.user, post.flatten(), ctx.constants).await?;
        if !caps.can("delete_post", std::slice::from_ref(id)) {
            return Err(Stop::Died(Answer::Died(denied)));
        }
        // Past its capability, the post is there.
        let id = u64::try_from(id.to_int()).unwrap_or_default();
        let written = match doaction {
            b"trash" if locked(write, id, asked.user, ctx.now).await?.is_some() => {
                skipped += 1;
                continue;
            }
            b"trash" => wp_trash_post(write, ctx, id).await?,
            b"untrash" => wp_untrash_post(write, ctx, id, undo).await?,
            _ => wp_delete_post(write, ctx, id, false).await?,
        };
        if !written {
            return Err(Stop::Died(Answer::Died(failed)));
        }
        count += 1;
    }
    if doaction != b"trash" {
        return Ok(with_counts(&[done], [count], sendback));
    }
    let listed: Vec<Vec<u8>> = ids.iter().map(Value::to_php_string).collect();
    let (trashed, listed, locked) = (count.to_string(), listed.join(&b','), skipped.to_string());
    let args: [(&[u8], Option<&[u8]>); 3] = [
        (b"trashed", Some(trashed.as_bytes())),
        (b"ids", Some(&listed)),
        (b"locked", Some(locked.as_bytes())),
    ];
    Ok(add_query_arg(&args, sendback))
}

/// `edit.php`'s bulk actions, Empty Trash and Bulk Edit.
pub(super) fn bulk<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a> {
    Box::pin(async move {
        let post_type = match asked.request("post_type").map(Value::to_php_string) {
            None => "post",
            Some(name) if name == b"post" => "post",
            Some(name) if name == b"page" => "page",
            Some(_) => return Err(Error::Unported("a list of another post type").into()),
        };
        // Back-compat for viewing comments of an entry.
        for name in ["p", "attachment_id", "page_id"] {
            if let Some(id) = asked.request(name).filter(|id| !id.is_empty()) {
                let path = format!("edit-comments.php?p={}", absint(id));
                return Ok(Answer::Redirect(site.admin_url(&path)));
            }
        }
        // `remove_query_arg()` of no referer acts on the request's URI.
        let base = referer(asked).unwrap_or_else(|| addslashes(asked.uri));
        let notices: [&[u8]; 5] = [b"trashed", b"untrashed", b"deleted", b"locked", b"ids"];
        let paged = asked.request("paged").map_or(0, absint).max(1).to_string();
        let mut sendback = remove_query_arg(&notices, &base);
        sendback = add_query_arg(&[(b"paged", Some(paged.as_bytes()))], &sendback);
        if mentions(&sendback, "post.php") {
            let new = match post_type {
                "post" => "post-new.php".to_string(),
                _ => format!("post-new.php?post_type={post_type}"),
            };
            sendback = site.admin_url(&new).into_bytes();
        }
        let doaction = action("edit", asked);
        let ids = bulk_ids(write, asked, (&doaction, post_type)).await?;
        if ids.is_empty() {
            return Ok(Answer::Redirect(text(&sendback)));
        }
        let caps = Caps::load(write, asked.user, None, site.constants()).await?;
        match &doaction[..] {
            b"edit" if asked.request("bulk_edit").is_some() => {
                let done = bulk_edit_posts(write, (asked, site), (&caps, post_type)).await?;
                let names: [&[u8]; 3] = [b"updated", b"skipped", b"locked"];
                sendback = with_counts(&names, done, &sendback);
            }
            b"trash" | b"untrash" | b"delete" | b"delete_all" => {
                let ctx = context(asked, site, (&caps, post_type));
                let acting = (&ctx, &doaction[..]);
                sendback = bulk_write(write, asked, acting, &ids, &sendback).await?;
            }
            _ => {}
        }
        let kept: [&[u8]; 10] = [
            b"action",
            b"action2",
            b"tags_input",
            b"post_author",
            b"comment_status",
            b"ping_status",
            b"_status",
            b"post",
            b"bulk_edit",
            b"post_view",
        ];
        Ok(Answer::Redirect(text(&remove_query_arg(&kept, &sendback))))
    })
}

/// A request's fields as `edit_post()` and `bulk_edit_posts()` change them;
/// a field set to null is not set.
#[derive(Clone)]
struct Data(Vec<(Key, Value)>);

impl Data {
    /// `$data[ $name ]` when `isset()`.
    fn get(&self, name: &str) -> Option<&Value> {
        lookup(&self.0, name).filter(|value| **value != Value::Null)
    }

    /// `! empty( $data[ $name ] )`.
    fn filled(&self, name: &str) -> bool {
        self.get(name).is_some_and(|value| !value.is_empty())
    }

    /// `$data[ $name ]` as a string, if set: one of [`TEXTS`], which
    /// [`sent_by_the_lists`] keeps from being an array.
    fn text(&self, name: &str) -> Option<Vec<u8>> {
        self.get(name).map(Value::to_php_string)
    }

    fn set(&mut self, name: &str, value: Value) {
        let key = Key::from_php_string(name.as_bytes().to_vec());
        match self.0.iter_mut().find(|(found, _)| *found == key) {
            Some((_, old)) => *old = value,
            None => self.0.push((key, value)),
        }
    }

    fn unset(&mut self, name: &str) {
        let key = Key::from_php_string(name.as_bytes().to_vec());
        self.0.retain(|(found, _)| *found != key);
    }
}

/// The given status as a key, but `inherit`, which is left out.
fn inherited(data: &mut Data) {
    if data.filled("post_status") {
        let status = hyscribe_kses::sanitize_key(&data.text("post_status").unwrap_or_default());
        match status.as_str() {
            "inherit" => data.unset("post_status"),
            _ => data.set("post_status", Value::String(status.into_bytes())),
        }
    }
}

/// What `_wp_translate_postdata()` and `edit_post()` read beside the data.
struct Saved<'a> {
    caps: &'a Caps,
    post: &'a PostFields,
    id: u64,
    zone: Zone,
    now: i64,
}

/// The date's parts, `aa` to `ss`: integers as PHP reads them, or none;
/// a part that is not a whole number is not ported.
fn units(data: &Data) -> Result<[Option<i64>; 6], Error> {
    let parts = ["aa", "mm", "jj", "hh", "mn", "ss"].map(|name| data.text(name));
    let whole = |part: &Vec<u8>| {
        let trimmed = String::from_utf8_lossy(part).trim().to_string();
        let digits = trimmed.strip_prefix(['-', '+']).unwrap_or(&trimmed);
        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
    };
    if !parts.iter().flatten().all(whole) {
        return Err(Error::Unported("a date part that is not a whole number"));
    }
    Ok(parts.map(|part| part.map(|part| Value::String(part).to_int())))
}

/// `_wp_translate_postdata( true, $data )`: the data, or its error message.
fn translate(
    saved: &Saved,
    mut data: Data,
    user: u64,
) -> Result<Result<Data, &'static str>, Error> {
    let (caps, post) = (saved.caps, saved.post);
    let page = post.post_type == "page";
    data.set(
        "ID",
        Value::Int(i64::try_from(saved.id).unwrap_or_default()),
    );
    for (from, to) in [("content", "post_content"), ("excerpt", "post_excerpt")] {
        if let Some(value) = data.get(from).cloned() {
            data.set(to, value);
        }
    }
    if let Some(parent) = data.get("parent_id").map(Value::to_int) {
        data.set("post_parent", Value::Int(parent));
    }
    let me = i64::try_from(user).unwrap_or_default();
    data.set("user_ID", Value::Int(me));
    let author = match data.get("post_author").filter(|author| !author.is_empty()) {
        Some(author) => author.to_int(),
        None => me,
    };
    data.set("post_author", Value::Int(author));
    if author != me && !caps.can_type("edit_others_posts", &post.post_type) {
        return Ok(Err(if page {
            "Sorry, you are not allowed to edit pages as this user."
        } else {
            "Sorry, you are not allowed to edit posts as this user."
        }));
    }
    let mut status = data.text("post_status");
    if data.filled("post_status") {
        let key = hyscribe_kses::sanitize_key(&status.take().unwrap_or_default());
        let key = if key == "auto-draft" {
            "draft".into()
        } else {
            key
        };
        status = STATUSES.contains(&key.as_str()).then(|| key.into_bytes());
    }
    let previous = post.status.as_bytes();
    let publishes = caps.can_type("publish_posts", &post.post_type);
    if status.as_deref() == Some(b"private") && !publishes {
        status = Some(previous.to_vec());
    }
    // The callers checked `edit_post` already.
    let published = |status: &[u8]| status == b"publish" || status == b"future";
    if status.as_deref().is_some_and(published) && !publishes && !published(previous) {
        status = Some(b"pending".to_vec());
    }
    let status = status.unwrap_or_else(|| match previous {
        b"auto-draft" => b"draft".to_vec(),
        previous => previous.to_vec(),
    });
    data.set("post_status", Value::String(status));
    if data.get("post_password").is_some() && !publishes {
        data.unset("post_password");
    }
    // The callers set `comment_status` and `ping_status` already.
    for name in ["aa", "mm", "jj", "hh", "mn"] {
        let hidden = data
            .get(&format!("hidden_{name}"))
            .filter(|value| !value.is_empty());
        if hidden.is_some_and(|hidden| Some(hidden) != data.get(name)) {
            data.set("edit_date", Value::String(b"1".to_vec()));
            break;
        }
    }
    if data.filled("edit_date") {
        let gm = |format: &str| {
            Value::String(format_unix(format, saved.now, Zone::Utc).into_bytes()).to_int()
        };
        let positive = |value: Option<i64>, default: &str| {
            value.filter(|&v| v > 0).unwrap_or_else(|| gm(default))
        };
        let wrapped = |value: Option<i64>, limit: i64| {
            value.map_or(0, |v| if v > limit { v - limit - 1 } else { v })
        };
        let [year, month, day, hour, minute, second] = units(&data)?;
        let year = positive(year, "Y");
        let month = positive(month, "n");
        let day = positive(day.map(|day| day.min(31)), "j");
        let (hour, minute, second) = (wrapped(hour, 23), wrapped(minute, 59), wrapped(second, 59));
        let date = format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}");
        let calendar = |value: i64| u32::try_from(value).unwrap_or_default();
        if !wp_checkdate(calendar(month), calendar(day), calendar(year)) {
            return Ok(Err("Invalid date."));
        }
        if post.date == date {
            data.set("edit_date", Value::Bool(false));
            data.unset("post_date");
            data.unset("post_date_gmt");
        } else {
            let gmt = format_unix("Y-m-d H:i:s", stored_unix(&date, saved.zone), Zone::Utc);
            data.set("edit_date", Value::Bool(true));
            data.set("post_date", Value::String(date.into_bytes()));
            data.set("post_date_gmt", Value::String(gmt.into_bytes()));
        }
    }
    if data.get("post_category").is_some() && !caps.can("edit_posts", &[]) {
        data.unset("post_category");
    }
    Ok(Ok(data))
}

/// `wp_update_post( $data )` of the translated data, with its terms of
/// `tax_input` (category IDs and tag IDs or names, slashed).
async fn update(
    write: &mut Write<'_>,
    (ctx, id): (&Context, u64),
    data: &Data,
    terms: &[(Taxonomy, Vec<Value>)],
) -> Result<(), Error> {
    let field = |name| data.text(name);
    let int = |name: &str| data.get(name).map(Value::to_int);
    let (title, name, date, date_gmt) = (
        field("post_title"),
        field("post_name"),
        field("post_date"),
        field("post_date_gmt"),
    );
    let (content, filtered, excerpt, status) = (
        field("post_content"),
        field("post_content_filtered"),
        field("post_excerpt"),
        field("post_status"),
    );
    let (comments, pings, password, to_ping, pinged, template) = (
        field("comment_status"),
        field("ping_status"),
        field("post_password"),
        field("to_ping"),
        field("pinged"),
        field("page_template"),
    );
    let category = match data.get("post_category") {
        Some(Value::Array(ids)) => Some(
            ids.iter()
                .filter_map(|(_, id)| u64::try_from(id.to_int()).ok())
                .collect::<Vec<u64>>(),
        ),
        _ => None,
    };
    // `_wp_translate_postdata()` set the author.
    let author = data.get("post_author").map_or(0, Value::to_int);
    let (parent, order) = (int("post_parent"), int("menu_order"));
    let edit_date = data.filled("edit_date");
    let lossy = |value: &Option<Vec<u8>>| value.as_deref().map(text);
    let (date, date_gmt, status, comments, pings) = (
        lossy(&date),
        lossy(&date_gmt),
        lossy(&status),
        lossy(&comments),
        lossy(&pings),
    );
    let change = |args: &mut PostArgs| {
        let keep = |to: &mut Vec<u8>, value: &Option<Vec<u8>>| {
            if let Some(value) = value {
                to.clone_from(value);
            }
        };
        keep(&mut args.title, &title);
        keep(&mut args.content, &content);
        keep(&mut args.content_filtered, &filtered);
        keep(&mut args.excerpt, &excerpt);
        keep(&mut args.password, &password);
        keep(&mut args.to_ping, &to_ping);
        keep(&mut args.pinged, &pinged);
        keep(&mut args.page_template, &template);
        let keep_text = |to: &mut String, value: &Option<String>| {
            if let Some(value) = value {
                to.clone_from(value);
            }
        };
        keep_text(&mut args.date, &date);
        keep_text(&mut args.date_gmt, &date_gmt);
        keep_text(&mut args.status, &status);
        keep_text(&mut args.comment_status, &comments);
        keep_text(&mut args.ping_status, &pings);
        if let Some(name) = &name {
            args.name = Some(name.clone());
        }
        args.author = Some(u64::try_from(author).unwrap_or_default());
        if let Some(parent) = parent {
            args.parent = u64::try_from(parent).unwrap_or_default();
        }
        if let Some(order) = order {
            args.menu_order = order;
        }
        if category.is_some() {
            args.category.clone_from(&category);
        }
        args.edit_date = edit_date;
    };
    wp_update_post(write, ctx, id, &change).await?;
    for (taxonomy, values) in terms {
        // A term ID as text is read as the ID, as `term_exists()` reads it.
        let texts: Vec<Vec<u8>> = values
            .iter()
            .filter(|value| !value.is_empty())
            .map(Value::to_php_string)
            .collect();
        let refs: Vec<TermRef> = texts.iter().map(|text| TermRef::Text(text)).collect();
        let set = wp_set_post_terms(write, id, TermList::List(&refs), *taxonomy, false).await;
        set.map_err(Error::from)?;
    }
    Ok(())
}

/// `set_post_format( $id, $format )`: only a post left without one is ported.
async fn set_post_format(write: &mut Write<'_>, id: u64, format: &Value) -> Result<(), Error> {
    let slug = hyscribe_kses::sanitize_key(&format.to_php_string());
    let chosen = !format.is_empty() && FORMATS.contains(&slug.as_str());
    if chosen || !write.object_terms(id, "post_format").await?.is_empty() {
        return Err(Error::Unported("a post format"));
    }
    Ok(())
}

/// `update_post_meta( $id, $key, $value )` of a value with no slashes.
pub(super) async fn update_post_meta(
    write: &mut Write<'_>,
    id: u64,
    key: &str,
    value: Value,
) -> Result<(), Error> {
    let any = Value::String(Vec::new());
    update_metadata(write, Meta::Post, id, key, &value, &any).await?;
    Ok(())
}

/// `edit_post( $data )` of Quick Edit: its message when it dies.
async fn edit_post(
    write: &mut Write<'_>,
    (asked, site): (&Asked<'_>, &dyn Acting),
    (post, id): (&PostFields, u64),
    mut data: Data,
) -> Result<Option<&'static str>, Error> {
    let caps = Caps::load(write, asked.user, Some(id), site.constants()).await?;
    data.unset("filter");
    data.set(
        "post_type",
        Value::String(post.post_type.clone().into_bytes()),
    );
    inherited(&mut data);
    // `_wp_upgrade_revisions_of_post()` for revisions older than version 1.
    if let Some(&oldest) = write.revision_ids(id).await?.last() {
        let name = existing(write, oldest).await?.name;
        let name = text(&name);
        let versioned = name
            .split_once("-revision-v")
            .or_else(|| name.split_once("-autosave-v"))
            .is_some_and(|(_, version)| version.parse::<u64>().is_ok_and(|v| v > 0));
        if !versioned {
            return Err(Error::Unported("revisions from before WordPress 3.6"));
        }
    }
    // Quick Edit's only visibility, `private`, which it sets itself.
    if data.get("visibility").is_some() {
        data.set("post_status", Value::String(b"private".to_vec()));
        data.set("post_password", Value::String(Vec::new()));
        data.unset("sticky");
    }
    let saved = Saved {
        caps: &caps,
        post,
        id,
        zone: wp_timezone(write).await?,
        now: crate::now(),
    };
    let data = match translate(&saved, data, asked.user)? {
        Ok(data) => data,
        Err(message) => return Ok(Some(message)),
    };
    if let Some(format) = data.get("post_format") {
        set_post_format(write, id, format).await?;
    }
    let mut terms = Vec::new();
    if let Some(Value::Array(taxonomies)) = data.get("tax_input") {
        for (taxonomy, given) in taxonomies {
            let taxonomy = match &taxonomy.to_php_string()[..] {
                b"category" => Taxonomy::Category,
                b"post_tag" => Taxonomy::PostTag,
                _ => return Err(Error::Unported("a taxonomy but categories and tags")),
            };
            let values = match (taxonomy, given) {
                (Taxonomy::Category, Value::Array(ids)) => {
                    ids.iter().map(|(_, id)| Value::Int(id.to_int())).collect()
                }
                (Taxonomy::PostTag, Value::String(names)) => tag_names(names),
                _ => return Err(Error::Unported("terms PHP cannot read")),
            };
            terms.push((taxonomy, values));
        }
    }
    if !caps.can("edit_posts", &[]) {
        terms.clear();
    }
    // `_fix_attachment_links()` acts under pretty permalinks only, where
    // `wp_update_post()` is not ported yet.
    let me = i64::try_from(asked.user).unwrap_or_default();
    update_post_meta(write, id, "_edit_last", Value::Int(me)).await?;
    let ctx = context(asked, site, (&caps, &post.post_type));
    match update(write, (&ctx, id), &data, &terms).await {
        // What `wp_update_post()` refuses is left undone, and the rest goes on.
        Err(Error::Refused(_)) => {}
        updated => updated?,
    }
    let lock = format!("{}:{}", ctx.now, asked.user);
    update_post_meta(write, id, "_edit_lock", Value::String(lock.into_bytes())).await?;
    let stickies = caps.can_type("edit_others_posts", &post.post_type)
        && caps.can_type("publish_posts", &post.post_type);
    if stickies {
        if data.filled("sticky") {
            stick_post(write, id).await?;
        } else {
            unstick_post(write, id).await?;
        }
    }
    Ok(None)
}

/// `wp_ajax_inline_save()`: the row of the saved post is the answer, which
/// hs-admin reads once the action is committed.
pub(super) fn inline_save<'a>(
    write: &'a mut Write<'_>,
    asked: &'a Asked<'a>,
    site: &'a dyn Acting,
) -> Saving<'a, Ajax> {
    Box::pin(async move {
        let mut data = Data(asked.post.to_vec());
        sent_by_the_lists(&data)?;
        let id = id_of(data.get("post_ID"));
        let screen = data.get("screen").and_then(Value::as_bytes);
        if !matches!(screen, Some(b"edit-post" | b"edit-page")) {
            return Err(Error::Unported("a list of another post type").into());
        }
        let post = existing(write, id).await?;
        if !["post", "page"].contains(&post.post_type.as_str()) {
            return Err(Error::Unported("a post type other than post and page").into());
        }
        data.set("content", Value::String(post.content.clone()));
        data.set("excerpt", Value::String(post.excerpt.clone()));
        if let Some(parent) = data.get("post_parent").cloned() {
            data.set("parent_id", parent);
        }
        if data.get("keep_private").and_then(Value::as_bytes) == Some(b"private") {
            data.set("visibility", Value::String(b"private".to_vec()));
            data.set("post_status", Value::String(b"private".to_vec()));
        } else if let Some(status) = data.get("_status").cloned() {
            data.set("post_status", status);
        }
        for name in ["comment_status", "ping_status"] {
            if !data.filled(name) {
                data.set(name, Value::String(b"closed".to_vec()));
            }
        }
        // Taxonomies Quick Edit does not show are left out.
        if let Some(Value::Array(mut taxonomies)) = data.get("tax_input").cloned() {
            let shown = [
                &b"category"[..],
                b"post_tag",
                b"link_category",
                b"wp_pattern_category",
            ];
            taxonomies.retain(|(name, _)| shown.contains(&&name.to_php_string()[..]));
            data.set("tax_input", Value::Array(taxonomies));
        }
        // wp_unique_post_slug() of a draft's slug, as if it were published.
        if data.filled("post_name") && ["draft", "pending"].contains(&post.status.as_str()) {
            let name = data.text("post_name").unwrap_or_default();
            let unique = (id, "publish");
            let name =
                wp_unique_post_slug(write, name, unique, &post.post_type, post.parent).await?;
            data.set("post_name", Value::String(name));
        }
        match edit_post(write, (asked, site), (&post, id), data).await? {
            Some(message) => Err(Stop::Died(Ajax::Text(message.into()))),
            None => Ok(Ajax::PostRow(id)),
        }
    })
}

/// `$_REQUEST`: `$_GET`'s fields, `$_POST`'s over them.
fn request(asked: &Asked) -> Data {
    let mut data = Data(asked.get.to_vec());
    for (key, value) in asked.post {
        data.set(
            &String::from_utf8_lossy(&key.to_php_string()),
            value.clone(),
        );
    }
    data
}

/// `explode( ',', trim( $names, " \n\t\r\0\x0B," ) )`, slashed.
fn tag_names(names: &[u8]) -> Vec<Value> {
    let trimmed = |b: &u8| b" \n\t\r\0\x0B,".contains(b);
    let start = names
        .iter()
        .position(|b| !trimmed(b))
        .unwrap_or(names.len());
    let end = names
        .iter()
        .rposition(|b| !trimmed(b))
        .map_or(start, |end| end + 1);
    let names = names.get(start..end).unwrap_or_default();
    names
        .split(|&b| b == b',')
        .map(|name| Value::String(addslashes(name)))
        .collect()
}

/// `bulk_edit_posts( $_REQUEST )`: how many posts were updated, skipped and
/// locked.
async fn bulk_edit_posts(
    write: &mut Write<'_>,
    (asked, site): (&Asked<'_>, &dyn Acting),
    (list_caps, list_type): (&Caps, &str),
) -> Result<[usize; 3], Error> {
    let mut shared = request(asked);
    sent_by_the_lists(&shared)?;
    // A `-1`, No Change, is unset with the other fields below.
    let status = shared.get("_status").cloned();
    shared.set("post_status", status.unwrap_or(Value::Null));
    shared.unset("_status");
    inherited(&mut shared);
    // `edit.php` read the IDs as an array already.
    let ids = shared
        .get("post")
        .and_then(Value::as_array)
        .unwrap_or_default();
    let ids: Vec<i64> = ids.iter().map(|(_, id)| id.to_int()).collect();
    for name in [
        "post_author",
        "post_status",
        "post_password",
        "post_parent",
        "page_template",
        "comment_status",
        "ping_status",
        "keep_private",
        "tax_input",
        "post_category",
        "sticky",
        "post_format",
    ] {
        if shared
            .get(name)
            .and_then(Value::as_bytes)
            .is_some_and(|value| value == b"" || value == b"-1")
        {
            shared.unset(name);
        }
    }
    let new_cats: Option<Vec<i64>> = match shared.get("post_category") {
        // A request's array is never empty.
        Some(Value::Array(ids)) => Some(ids.iter().map(|(_, id)| id.to_int().abs()).collect()),
        Some(_) => {
            shared.unset("post_category");
            None
        }
        None => None,
    };
    let (mut new_tags, mut new_categories) = (Vec::new(), Vec::new());
    if let Some(Value::Array(taxonomies)) = shared.get("tax_input") {
        for (taxonomy, terms) in taxonomies.iter().filter(|(_, terms)| !terms.is_empty()) {
            match (&taxonomy.to_php_string()[..], terms) {
                (b"category", Value::Array(ids)) => {
                    new_categories = ids
                        .iter()
                        .map(|(_, id)| Value::Int(id.to_int().abs()))
                        .collect();
                }
                (b"post_tag", Value::String(names)) => new_tags = tag_names(names),
                (b"post_format", Value::String(_)) => {}
                _ => return Err(Error::Unported("terms PHP cannot read")),
            }
        }
    }
    // The page chosen as the parent and its ancestors are not moved under it.
    let mut children = Vec::new();
    let mut parent = shared.get("post_parent").map_or(0, Value::to_int);
    // No post has ID 0, so no parent ends the walk at once.
    while !children.contains(&parent) {
        children.push(parent);
        let page = write
            .post(u64::try_from(parent).unwrap_or_default())
            .await?;
        match page.filter(|page| page.post_type == "page") {
            Some(page) => parent = i64::try_from(page.parent).unwrap_or_default(),
            None => break,
        }
    }
    let (mut updated, mut skipped, mut locked_count) = (0, 0, 0);
    let (now, zone) = (crate::now(), wp_timezone(write).await?);
    for &signed in &ids {
        let id = u64::try_from(signed).unwrap_or_default();
        let post = write.post(id).await?;
        if post
            .as_ref()
            .is_some_and(|post| !["post", "page"].contains(&post.post_type.as_str()))
        {
            return Err(Error::Unported("a post type other than post and page"));
        }
        let caps = Caps::load(write, asked.user, Some(id), site.constants()).await?;
        let editable = caps.can("edit_post", &[Value::Int(signed)]);
        let Some(post) = post.filter(|_| editable && !children.contains(&signed)) else {
            skipped += 1;
            continue;
        };
        if locked(write, id, asked.user, now).await?.is_some() {
            locked_count += 1;
            continue;
        }
        let mut data = shared.clone();
        let mut terms = Vec::new();
        if post.post_type == "post" {
            let assign = caps.can("edit_posts", &[]);
            let ids = |terms: Vec<(u64, u64)>| -> Vec<Value> {
                let ids = terms.into_iter().map(|(term, _)| term);
                ids.map(|term| Value::Int(i64::try_from(term).unwrap_or_default()))
                    .collect()
            };
            let cats = ids(write.object_terms(id, "category").await?);
            let mut tags = ids(write.object_terms(id, "post_tag").await?);
            tags.extend(new_tags.iter().filter(|_| assign).cloned());
            match &new_cats {
                Some(new_cats) => {
                    let indeterminate: Vec<i64> = match data.get("indeterminate_post_category") {
                        Some(Value::Array(ids)) => ids.iter().map(|(_, id)| id.to_int()).collect(),
                        _ => Vec::new(),
                    };
                    let kept = cats
                        .iter()
                        .map(Value::to_int)
                        .filter(|cat| indeterminate.contains(cat));
                    let chosen = new_cats
                        .iter()
                        .copied()
                        .filter(|cat| !indeterminate.contains(cat));
                    let mut unique: Vec<i64> = Vec::new();
                    for cat in kept.chain(chosen) {
                        if !unique.contains(&cat) {
                            unique.push(cat);
                        }
                    }
                    let unique = (0..)
                        .zip(unique)
                        .map(|(n, cat)| (Key::Int(n), Value::Int(cat)));
                    data.set("post_category", Value::Array(unique.collect()));
                }
                None => {
                    let added = new_categories.iter().filter(|_| assign).cloned();
                    terms.push((Taxonomy::Category, cats.into_iter().chain(added).collect()));
                }
            }
            terms.push((Taxonomy::PostTag, tags));
        }
        data.set("post_ID", Value::Int(signed));
        for (name, kept) in [
            ("comment_status", &post.comment_status),
            ("ping_status", &post.ping_status),
        ] {
            if data.get(name).is_none() {
                data.set(name, Value::String(kept.clone().into_bytes()));
            }
        }
        if data.get("post_author").is_none() {
            let author = i64::try_from(post.author).unwrap_or_default();
            data.set("post_author", Value::Int(author));
        }
        let saved = Saved {
            caps: &caps,
            post: &post,
            id,
            zone,
            now,
        };
        let Ok(mut data) = translate(&saved, data, asked.user)? else {
            skipped += 1;
            continue;
        };
        if let Some(format) = shared.get("post_format") {
            set_post_format(write, id, format).await?;
        }
        let publishing = data.get("post_status").and_then(Value::as_bytes) == Some(b"publish");
        if ["future", "draft"].contains(&post.status.as_str()) && publishing {
            let local = format_unix("Y-m-d H:i:s", now, zone);
            data.set("post_date", Value::String(local.into_bytes()));
            data.set("post_date_gmt", Value::String(Vec::new()));
        }
        let ctx = context(asked, site, (&caps, &post.post_type));
        update(write, (&ctx, id), &data, &terms)
            .await
            .map_err(|err| match err {
                Error::Refused(_) => Error::Unported("a save PHP leaves undone"),
                err => err,
            })?;
        let me = i64::try_from(asked.user).unwrap_or_default();
        update_post_meta(write, id, "_edit_last", Value::Int(me)).await?;
        updated += 1;
        let sticky = data.get("sticky");
        if sticky.is_some() && list_caps.can_type("edit_others_posts", list_type) {
            if sticky.and_then(Value::as_bytes) == Some(b"sticky") {
                stick_post(write, id).await?;
            } else {
                unstick_post(write, id).await?;
            }
        }
    }
    Ok([updated, skipped, locked_count])
}

#[cfg(test)]
mod tests;
