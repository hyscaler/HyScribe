// SPDX-License-Identifier: AGPL-3.0-or-later

//! Posts: `wp_insert_post()`, `wp_update_post()`, `wp_trash_post()`,
//! `wp_untrash_post()` and `wp_delete_post()`, with the hooks a default
//! install runs on them. Each runs inside the caller's [`Write`], which the
//! caller commits. Posts and pages are ported; other types, and what a
//! default install never reaches, answer [`Error::Unported`], and the
//! caller's action rolls back.

use hyscribe_config::Constants;
use hyscribe_db::{PostFields, Write};
use hyscribe_phpser::Value;
use hyscribe_render::formatting::sanitize_title;

use crate::options::{Meta, add_metadata, delete_metadata, get_metadata, update_metadata};
use crate::terms::{
    Taxonomy, TermError, TermList, TermRef, wp_set_post_categories, wp_set_post_terms,
};
use crate::{Error, cron};

mod date;
mod hooks;
mod link;
mod site;
mod slug;
mod trash;

pub use date::checkdate as wp_checkdate;
pub use trash::{stick_post, unstick_post, wp_delete_post, wp_trash_post, wp_untrash_post};

/// Who writes and where: what the PHP functions read from the request and
/// the configuration rather than the database.
#[derive(Clone, Debug)]
pub struct Context {
    /// `get_current_user_id()`.
    pub user: u64,
    /// Whether the user can publish the post: `publish_posts` of its type,
    /// or `publish_post` on an update.
    pub can_publish: bool,
    /// `home_url()`, without a trailing slash.
    pub home: Vec<u8>,
    /// The host of `wp_upload_dir()`'s URL, with `:port` where it has one,
    /// as `wp_kses_post()` takes it.
    pub upload_host: Vec<u8>,
    /// `current_user_can( 'unfiltered_html' )`: `kses_init()` leaves the
    /// post's fields unfiltered for such a user.
    pub unfiltered_html: bool,
    /// The configuration's constants capability checks read.
    pub constants: Constants,
    /// `time()`.
    pub now: i64,
}

/// `wp_insert_post()`'s `$postarr`, unslashed.
#[derive(Clone, Debug, Default)]
pub struct PostArgs {
    /// `ID`: the post to update, or 0 for a new one.
    pub id: u64,
    /// `post_author`; the current user when `None`.
    pub author: Option<u64>,
    /// `post_date`, `YYYY-MM-DD hh:mm:ss` in the site's time zone, or empty.
    pub date: String,
    /// `post_date_gmt`, or empty.
    pub date_gmt: String,
    /// `post_content`.
    pub content: Vec<u8>,
    /// `post_content_filtered`.
    pub content_filtered: Vec<u8>,
    /// `post_title`.
    pub title: Vec<u8>,
    /// `post_excerpt`.
    pub excerpt: Vec<u8>,
    /// `post_status`; `draft` when empty.
    pub status: String,
    /// `post_type`; `post` when empty.
    pub post_type: String,
    /// `comment_status`; the type's default when empty.
    pub comment_status: String,
    /// `ping_status`; the type's default when empty.
    pub ping_status: String,
    /// `post_password`.
    pub password: Vec<u8>,
    /// `post_name`; on an update, `None` keeps the post's.
    pub name: Option<Vec<u8>>,
    /// `to_ping`.
    pub to_ping: Vec<u8>,
    /// `pinged`.
    pub pinged: Vec<u8>,
    /// `post_parent`.
    pub parent: u64,
    /// `menu_order`.
    pub menu_order: i64,
    /// `post_mime_type`.
    pub mime_type: String,
    /// `post_category`: term IDs; on an update, `None` keeps the post's.
    pub category: Option<Vec<u64>>,
    /// `import_id`: the ID to give a new post if no post has it.
    pub import_id: u64,
    /// `edit_date`: `wp_update_post()` keeps a draft's date only with it.
    pub edit_date: bool,
    /// `page_template`: `default`, or empty for none given; the theme's own
    /// templates are not ported.
    pub page_template: Vec<u8>,
}

impl From<TermError> for Error {
    fn from(err: TermError) -> Error {
        match err {
            TermError::Db(err) => Error::Db(err),
            TermError::Unported(what) => Error::Unported(what),
            // Terms given by ID that do not exist are skipped instead.
            _ => Error::Refused("invalid_term"),
        }
    }
}

type Result<T> = std::result::Result<T, Error>;

/// The statuses `wp_insert_post()` and `wp_unique_post_slug()` give no slug.
const UNSLUGGED: [&str; 3] = ["draft", "pending", "auto-draft"];

/// `'0000-00-00 00:00:00'`.
const ZERO_DATE: &str = "0000-00-00 00:00:00";

/// The post types ported: what WordPress registers of them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Post,
    Page,
    Revision,
    /// A privacy request, which `_wp_personal_data_cleanup_requests()` updates.
    UserRequest,
    /// `wp_navigation`, the navigation block's menus.
    Navigation,
    /// `wp_block`, a synced pattern.
    Block,
}

impl Kind {
    fn of(post_type: &str) -> Result<Kind> {
        match post_type {
            "post" => Ok(Kind::Post),
            "page" => Ok(Kind::Page),
            "revision" => Ok(Kind::Revision),
            "user_request" => Ok(Kind::UserRequest),
            "wp_navigation" => Ok(Kind::Navigation),
            "wp_block" => Ok(Kind::Block),
            _ => Err(Error::Unported("a post type other than post and page")),
        }
    }

    fn hierarchical(self) -> bool {
        self == Kind::Page
    }

    /// `get_object_taxonomies()`.
    fn taxonomies(self) -> &'static [&'static str] {
        match self {
            Kind::Post => &["category", "post_tag", "post_format"],
            Kind::Block => &["wp_pattern_category"],
            Kind::Page | Kind::Revision | Kind::UserRequest | Kind::Navigation => &[],
        }
    }
}

/// PHP's truthiness of a string: neither empty nor `"0"`.
fn truthy(value: &[u8]) -> bool {
    !value.is_empty() && value != b"0"
}

/// `get_post( $id )` of a post the caller needs.
async fn existing(write: &mut Write<'_>, id: u64) -> Result<PostFields> {
    let post = write.post(id).await?;
    post.ok_or(Error::Refused("invalid_post"))
}

/// `wp_insert_post( $postarr )`: the post's ID.
pub async fn wp_insert_post(write: &mut Write<'_>, ctx: &Context, args: PostArgs) -> Result<u64> {
    let (id, kind, update) = insert(write, ctx, args).await?;
    // wp_after_insert_post: wp_save_post_revision_on_insert().
    if update {
        hooks::save_revision(write, ctx, kind, id).await?;
    }
    Ok(id)
}

/// `wp_update_post( $postarr )`: `change` sets the fields to change on the
/// post's own, as the keys of `$postarr` would.
pub async fn wp_update_post(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    change: &dyn Fn(&mut PostArgs),
) -> Result<u64> {
    let post = existing(write, id).await?;
    let category = if Kind::of(&post.post_type)? == Kind::Post {
        let terms = write.object_terms(id, "category").await?;
        terms.into_iter().map(|(term, _)| term).collect()
    } else {
        Vec::new()
    };
    let draft = UNSLUGGED.contains(&post.status.as_str()) && post.date_gmt == ZERO_DATE;
    let mut args = PostArgs {
        id,
        author: Some(post.author),
        date: post.date,
        date_gmt: post.date_gmt,
        content: post.content,
        content_filtered: post.content_filtered,
        title: post.title,
        excerpt: post.excerpt,
        status: post.status,
        post_type: post.post_type,
        comment_status: post.comment_status,
        ping_status: post.ping_status,
        password: post.password,
        name: Some(post.name),
        to_ping: post.to_ping,
        pinged: post.pinged,
        parent: post.parent,
        menu_order: post.menu_order,
        mime_type: post.mime_type,
        category: Some(category.clone()),
        import_id: 0,
        edit_date: false,
        page_template: Vec::new(),
    };
    change(&mut args);
    // Passed categories replace the post's only when there are some.
    if args.category.as_ref().is_none_or(Vec::is_empty) {
        args.category = Some(category);
    }
    // Drafts are not given a date unless the user asked for one.
    if draft && !args.edit_date {
        args.date = date::Dates::read(write, ctx.now).await?.local_now();
        args.date_gmt = String::new();
    }
    wp_insert_post(write, ctx, args).await
}

/// `wp_unique_post_slug( $slug, $id, $status, $post_type, $parent )`.
pub async fn wp_unique_post_slug(
    write: &mut Write<'_>,
    slug: Vec<u8>,
    (id, status): (u64, &str),
    post_type: &str,
    parent: u64,
) -> Result<Vec<u8>> {
    let kind = Kind::of(post_type)?;
    slug::unique(write, slug, id, (status, kind, post_type), parent).await
}

/// `get_default_post_to_edit( 'post', true )`: a new `auto-draft`, with
/// the daily deletion of old ones scheduled. A default post format is not ported.
pub async fn get_default_post_to_edit(write: &mut Write<'_>, ctx: &Context) -> Result<u64> {
    let args = PostArgs {
        title: b"Auto Draft".to_vec(),
        status: "auto-draft".into(),
        ..PostArgs::default()
    };
    let id = wp_insert_post(write, ctx, args).await?;
    let format = write.option_value("default_post_format").await?;
    if format.is_some_and(|format| truthy(&format)) {
        return Err(Error::Unported("a default post format"));
    }
    const HOOK: &str = "wp_scheduled_auto_draft_delete";
    let none = Value::Array(Vec::new());
    if cron::wp_next_scheduled(write, HOOK, &none).await?.is_none() {
        cron::wp_schedule_event(write, ctx.now, "daily", HOOK, &none).await?;
    }
    Ok(id)
}

/// `wp_restore_post_revision( $revision )` of a revision: its post given
/// the revision's title, content, excerpt and footnotes.
pub async fn wp_restore_post_revision(
    write: &mut Write<'_>,
    ctx: &Context,
    revision_id: u64,
) -> Result<u64> {
    let revision = existing(write, revision_id).await?;
    let (title, content, excerpt) = (revision.title, revision.content, revision.excerpt);
    let id = wp_update_post(write, ctx, revision.parent, &|args| {
        args.title.clone_from(&title);
        args.content.clone_from(&content);
        args.excerpt.clone_from(&excerpt);
    })
    .await?;
    let user = Value::Int(ctx.user.cast_signed());
    update_metadata(write, Meta::Post, id, "_edit_last", &user, &Value::Null).await?;
    // The `wp_restore_post_revision` action: wp_restore_post_revision_meta().
    let none = Value::String(Vec::new());
    delete_metadata(write, Meta::Post, id, "footnotes", &none, false).await?;
    for value in get_metadata(write, Meta::Post, revision_id, "footnotes").await? {
        let value = hooks::slash(value);
        add_metadata(write, Meta::Post, id, "footnotes", &value, false).await?;
    }
    Ok(id)
}

/// `wp_publish_post( $post )`, which cron's `publish_future_post` runs.
pub async fn wp_publish_post(write: &mut Write<'_>, ctx: &Context, id: u64) -> Result<()> {
    let Some(post) = write.post(id).await? else {
        return Ok(());
    };
    if post.status == "publish" {
        return Ok(());
    }
    // Only categories have a default term on a default install.
    let kind = Kind::of(&post.post_type)?;
    if kind == Kind::Post && write.object_terms(id, "category").await?.is_empty() {
        let default = site::option_int(write, "default_category").await?;
        if default != 0 {
            let terms = TermList::List(&[TermRef::Id(default)]);
            wp_set_post_terms(write, id, terms, Taxonomy::Category, false).await?;
        }
    }
    write.set_post_status(id, "publish").await?;
    hooks::transition(write, ctx, kind, ("publish", &post.status), id).await?;
    hooks::save_revision(write, ctx, kind, id).await
}

/// `sanitize_post( $postarr, 'db' )` with a default install's filters:
/// kses only for a user who may not post unfiltered HTML.
fn sanitize(mut args: PostArgs, ctx: &Context) -> Result<PostArgs> {
    args.title = hyscribe_phpser::php_trim(&args.title).to_vec();
    args.content = crate::convert_invalid_entities(&args.content);
    args.excerpt = crate::convert_invalid_entities(&args.excerpt);
    if !ctx.unfiltered_html {
        let post_kses = |text: &[u8]| hyscribe_render::kses::wp_kses_post(text, &ctx.upload_host);
        // wp_filter_global_styles_post() rewrites a global styles post first.
        let styles = |text: &[u8]| {
            let marker = b"isGlobalStylesUserThemeJSON";
            if text.windows(marker.len()).any(|w| w == marker) {
                return Err(Error::Unported("global styles content"));
            }
            Ok(post_kses(text))
        };
        args.title = hyscribe_render::kses::wp_kses_data(&args.title);
        args.content = styles(&args.content)?;
        args.content_filtered = styles(&args.content_filtered)?;
        args.excerpt = post_kses(&args.excerpt);
    }
    for key in [
        &mut args.status,
        &mut args.post_type,
        &mut args.comment_status,
        &mut args.ping_status,
    ] {
        *key = hyscribe_kses::sanitize_key(key.as_bytes());
    }
    args.mime_type
        .retain(|c| c.is_ascii_alphanumeric() || "-+*./".contains(c));
    Ok(args)
}

/// `sanitize_trackback_urls( $to_ping )`.
fn sanitize_trackback_urls(to_ping: &[u8]) -> Vec<u8> {
    let urls: Vec<Vec<u8>> = hyscribe_phpser::php_trim(to_ping)
        .split(|b| matches!(b, b'\r' | b'\n' | b'\t' | b' '))
        .filter(|url| {
            let lower = url.to_ascii_lowercase();
            let rest = lower
                .strip_prefix(b"http://")
                .or_else(|| lower.strip_prefix(b"https://"));
            rest.is_some_and(|rest| !rest.is_empty())
        })
        .map(hyscribe_kses::esc_url_raw)
        .collect();
    urls.join(&b'\n')
}

/// `wp_insert_post()` up to its `wp_insert_post` action: the post's ID and
/// kind, and whether it was an update.
async fn insert(write: &mut Write<'_>, ctx: &Context, args: PostArgs) -> Result<(u64, Kind, bool)> {
    let args = sanitize(args, ctx)?;
    let before = match args.id {
        0 => None,
        id => Some(existing(write, id).await?),
    };
    let post_id = args.id;
    let previous = before
        .as_ref()
        .map_or("new", |post| post.status.as_str())
        .to_string();
    let post_type = if args.post_type.is_empty() {
        "post".to_string()
    } else {
        args.post_type.clone()
    };
    let kind = Kind::of(&post_type)?;
    // The types that support the editor, a title and an excerpt.
    if matches!(kind, Kind::Post | Kind::Block)
        && !truthy(&args.content)
        && !truthy(&args.title)
        && !truthy(&args.excerpt)
    {
        return Err(Error::Refused("empty_content"));
    }
    let mut status = if args.status.is_empty() {
        "draft".to_string()
    } else {
        args.status.clone()
    };
    // An update keeps its categories, a new post (ID 0) has none; with
    // none, wp_set_post_categories() gives the default one.
    let category: Vec<u64> = match &args.category {
        Some(given) => given.iter().copied().filter(|&term| term != 0).collect(),
        None => {
            let terms = write.object_terms(post_id, "category").await?;
            terms.into_iter().map(|(term, _)| term).collect()
        }
    };
    let mut name = match (&args.name, &before) {
        (Some(name), _) => name.clone(),
        (None, Some(before)) => before.name.clone(),
        (None, None) => Vec::new(),
    };
    if status == "pending" && !ctx.can_publish {
        name.clear();
    }
    name = if truthy(&name) {
        // The old sanitization a kept name passes gives the same slug.
        sanitize_title(&name)
    } else if UNSLUGGED.contains(&status.as_str()) {
        Vec::new()
    } else {
        sanitize_title(&args.title)
    };
    let dates = date::Dates::read(write, ctx.now).await?;
    let post_date = dates.resolve(&args.date, &args.date_gmt)?;
    let post_date_gmt = if args.date_gmt.is_empty() || args.date_gmt == ZERO_DATE {
        if UNSLUGGED.contains(&status.as_str()) {
            ZERO_DATE.to_string()
        } else {
            dates.gmt_from(&post_date)
        }
    } else {
        date::canonical(&args.date_gmt)?.to_string()
    };
    let (modified, modified_gmt) = if before.is_some() || post_date == ZERO_DATE {
        (dates.local_now(), dates.gmt_now())
    } else {
        (post_date.clone(), post_date_gmt.clone())
    };
    if status == "publish" || status == "future" {
        let future = date::Dates::unix_gmt(&post_date_gmt) - ctx.now >= 60;
        status = if future { "future" } else { "publish" }.into();
    }
    let comment_status = match (args.comment_status.as_str(), &before) {
        ("", Some(_)) => "closed".to_string(),
        ("", None) => site::default_status(write, kind, "default_comment_status").await?,
        (given, _) => given.to_string(),
    };
    let ping_status = match args.ping_status.as_str() {
        "" => site::default_status(write, kind, "default_ping_status").await?,
        given => given.to_string(),
    };
    let password = if status == "private" {
        Vec::new()
    } else {
        args.password.clone()
    };
    let parent = hooks::parent_without_loop(write, args.parent, post_id).await?;
    if previous == "trash" && status != "trash" {
        let key = "_wp_desired_post_slug";
        let desired = get_metadata(write, Meta::Post, post_id, key).await?;
        if let Some(desired) = desired.first().filter(|slug| !slug.is_empty()) {
            name = desired.to_php_string();
            let any = Value::String(Vec::new());
            delete_metadata(write, Meta::Post, post_id, key, &any, false).await?;
        }
    }
    if status != "trash" && truthy(&name) {
        slug::suffix_trashed_namesakes(write, &name, post_id).await?;
    }
    if status == "trash" && previous != "trash" && previous != "new" {
        name = slug::suffix_trashed(write, post_id).await?;
    }
    let name = slug::unique(write, name, post_id, (&status, kind, &post_type), parent).await?;
    let mut fields = PostFields {
        author: args.author.unwrap_or(ctx.user),
        date: post_date,
        date_gmt: post_date_gmt,
        content: args.content,
        content_filtered: args.content_filtered,
        title: args.title,
        excerpt: args.excerpt,
        status,
        post_type,
        comment_status,
        ping_status,
        password,
        name,
        to_ping: sanitize_trackback_urls(&args.to_ping),
        pinged: args.pinged,
        modified,
        modified_gmt,
        parent,
        menu_order: args.menu_order,
        mime_type: args.mime_type,
        guid: before
            .as_ref()
            .map(|post| post.guid.clone())
            .unwrap_or_default(),
    };
    let id = if before.is_some() {
        write.update_post(post_id, &fields).await?;
        post_id
    } else if args.import_id != 0 && write.post(args.import_id).await?.is_none() {
        write.insert_post_with_id(args.import_id, &fields).await?
    } else {
        write.insert_post(&fields).await?
    };
    if !truthy(&fields.name) && !UNSLUGGED.contains(&fields.status.as_str()) {
        let title = Some(sanitize_title(&fields.title))
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| id.to_string().into_bytes());
        let status = (fields.status.as_str(), kind, fields.post_type.as_str());
        fields.name = slug::unique(write, title, id, status, parent).await?;
        write.set_post_name(id, &fields.name).await?;
    }
    if kind == Kind::Post {
        let terms: Vec<TermRef<'_>> = category.iter().map(|&term| TermRef::Id(term)).collect();
        wp_set_post_categories(write, id, &terms, false).await?;
    }
    if before.is_none() {
        let guid = link::permalink(write, ctx, id, &fields).await?;
        write.set_post_guid(id, &guid).await?;
    }
    if truthy(&args.page_template) {
        if args.page_template != b"default" {
            return Err(Error::Unported("a page template of the theme"));
        }
        let template = Value::String(args.page_template);
        let any = Value::String(Vec::new());
        update_metadata(write, Meta::Post, id, "_wp_page_template", &template, &any).await?;
    }
    hooks::transition(write, ctx, kind, (&fields.status, &previous), id).await?;
    if let Some(before) = before {
        hooks::post_updated(write, kind, id, &before).await?;
    }
    Ok((id, kind, args.id != 0))
}

#[cfg(test)]
pub(crate) mod tests;
