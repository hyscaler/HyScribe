// SPDX-License-Identifier: AGPL-3.0-or-later

//! Posts: `wp_insert_post()`, `wp_update_post()`, `wp_trash_post()`,
//! `wp_untrash_post()` and `wp_delete_post()`, with the hooks a default
//! install runs on them. Each runs inside the caller's [`Write`], which the
//! caller commits. Posts and pages are ported; other types, and what a
//! default install never reaches, answer [`Error::Unported`], and the
//! caller's action rolls back.

use hyscribe_config::Constants;
use hyscribe_db::{PostFields, Write};
use hyscribe_phpser::{Key, Value};
use hyscribe_render::formatting::sanitize_title;

use crate::options::{Meta, add_metadata, delete_metadata, get_metadata, update_metadata};
use crate::terms::{
    Taxonomy, TermError, TermList, TermRef, wp_set_post_categories, wp_set_post_terms,
};
use crate::{Error, cron};

mod autosave;
mod date;
mod hooks;
mod link;
mod site;
mod slug;
mod trash;

pub use autosave::{Revisioned, wp_create_post_autosave};
pub use date::checkdate as wp_checkdate;
pub use trash::{
    AttachmentDeleted, stick_post, unstick_post, wp_delete_attachment, wp_delete_post,
    wp_trash_post, wp_untrash_post,
};

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
    /// `tax_input`: the terms of a flat taxonomy as `wp_set_post_terms()`
    /// takes them, slashed; the caller passes only taxonomies whose terms
    /// the user may assign.
    pub tax_input: Vec<(Taxonomy, Vec<u8>)>,
    /// `meta_input`: meta values `update_post_meta()` writes, slashed.
    pub meta_input: Vec<(&'static str, Value)>,
    /// `guid`: a new attachment's URL; other posts' is their permalink.
    pub guid: Vec<u8>,
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
    /// `wp_global_styles`, which only the theme's first one is created of.
    GlobalStyles,
    /// A privacy request, which `_wp_personal_data_cleanup_requests()` updates.
    UserRequest,
    /// `wp_navigation`, the navigation block's menus.
    Navigation,
    /// `wp_block`, a synced pattern.
    Block,
    /// `wp_template`, a template the site editor saved.
    Template,
    /// `wp_template_part`.
    TemplatePart,
    /// An attachment, which only [`wp_insert_attachment`] makes.
    Attachment,
}

impl Kind {
    fn of(post_type: &str) -> Result<Kind> {
        match post_type {
            "post" => Ok(Kind::Post),
            "page" => Ok(Kind::Page),
            "revision" => Ok(Kind::Revision),
            "wp_global_styles" => Ok(Kind::GlobalStyles),
            "user_request" => Ok(Kind::UserRequest),
            "wp_navigation" => Ok(Kind::Navigation),
            "wp_block" => Ok(Kind::Block),
            "wp_template" => Ok(Kind::Template),
            "wp_template_part" => Ok(Kind::TemplatePart),
            "attachment" => Ok(Kind::Attachment),
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
            Kind::Template | Kind::GlobalStyles => &["wp_theme"],
            Kind::TemplatePart => &["wp_theme", "wp_template_part_area"],
            Kind::Page
            | Kind::Revision
            | Kind::UserRequest
            | Kind::Navigation
            | Kind::Attachment => &[],
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
    let (id, kind, update) = insert(write, ctx, args, None).await?;
    // wp_after_insert_post: wp_save_post_revision_on_insert(), which an
    // attachment's insert returns before.
    if update && kind != Kind::Attachment {
        hooks::save_revision(write, ctx, kind, id).await?;
    }
    Ok(id)
}

/// `wp_after_insert_post( $post, true, ... )` of a post already updated: the
/// revision `wp_save_post_revision_on_insert()` makes where the post differs
/// from its last, footnotes included.
pub async fn wp_after_insert_post(write: &mut Write<'_>, ctx: &Context, id: u64) -> Result<()> {
    let post = existing(write, id).await?;
    // The post was just written, so `insert()` took its type.
    let kind = Kind::of(&post.post_type).unwrap_or(Kind::Post);
    hooks::save_revision(write, ctx, kind, id).await
}

/// `wp_update_post( $postarr )`: `change` sets the fields to change on the
/// post's own, as the keys of `$postarr` would.
pub async fn wp_update_post(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    change: &dyn Fn(&mut PostArgs),
) -> Result<u64> {
    let args = update_args(write, ctx, id, change).await?;
    wp_insert_post(write, ctx, args).await
}

/// `wp_update_post( $postarr, true, false )`: the post updated without its
/// after hooks, which the caller fires with [`wp_after_insert_post`].
pub async fn wp_update_post_quietly(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    change: &dyn Fn(&mut PostArgs),
) -> Result<u64> {
    let args = update_args(write, ctx, id, change).await?;
    let (id, ..) = insert(write, ctx, args, None).await?;
    Ok(id)
}

/// `wp_update_post()`'s arguments for `wp_insert_post()`: the post's own,
/// changed by `change`.
async fn update_args(
    write: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    change: &dyn Fn(&mut PostArgs),
) -> Result<PostArgs> {
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
        tax_input: Vec::new(),
        meta_input: Vec::new(),
        guid: Vec::new(),
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
    Ok(args)
}

/// `wp_insert_attachment( $args, $file )` of a new attachment for the file
/// at `file` within the uploads folder: its ID, before the `add_attachment`
/// action the caller fires.
pub async fn wp_insert_attachment(
    write: &mut Write<'_>,
    ctx: &Context,
    args: PostArgs,
    file: &str,
) -> Result<u64> {
    let args = PostArgs {
        id: 0,
        post_type: "attachment".into(),
        ..args
    };
    let (id, ..) = insert(write, ctx, args, Some(file)).await?;
    Ok(id)
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

/// `get_default_post_to_edit( $post_type, true )`: a new `auto-draft`, with
/// the daily deletion of old ones scheduled. A default post format is not ported.
pub async fn get_default_post_to_edit(
    write: &mut Write<'_>,
    ctx: &Context,
    post_type: &str,
) -> Result<u64> {
    let args = PostArgs {
        title: b"Auto Draft".to_vec(),
        status: "auto-draft".into(),
        post_type: post_type.into(),
        ..PostArgs::default()
    };
    let id = wp_insert_post(write, ctx, args).await?;
    let format = write.option_value("default_post_format").await?;
    if post_type == "post" && format.is_some_and(|format| truthy(&format)) {
        return Err(Error::Unported("a default post format"));
    }
    const HOOK: &str = "wp_scheduled_auto_draft_delete";
    let none = Value::Array(Vec::new());
    if cron::wp_next_scheduled(write, HOOK, &none).await?.is_none() {
        cron::wp_schedule_event(write, ctx.now, "daily", HOOK, &none).await?;
    }
    Ok(id)
}

/// The content a theme's first global styles post is created with.
const GLOBAL_STYLES: &[u8] = br#"{"version": 3, "isGlobalStylesUserThemeJSON": true }"#;

/// `WP_Theme_JSON_Resolver::get_user_global_styles_post_id()` for the theme
/// `stylesheet`: its newest published `wp_global_styles` post, created
/// when it has none.
pub async fn get_user_global_styles_post_id(
    write: &mut Write<'_>,
    ctx: &Context,
    stylesheet: &str,
) -> Result<u64> {
    if let Some(term) = write.term_by_slug("wp_theme", Some(stylesheet)).await? {
        let mut newest: Option<(String, u64)> = None;
        for id in write.term_object_ids(term.term_taxonomy_id).await? {
            let post = existing(write, id).await?;
            let styles = post.post_type == "wp_global_styles" && post.status == "publish";
            if styles && newest.as_ref().is_none_or(|(date, _)| post.date > *date) {
                newest = Some((post.date, id));
            }
        }
        if let Some((_, id)) = newest {
            return Ok(id);
        }
    }
    let args = PostArgs {
        content: GLOBAL_STYLES.to_vec(),
        status: "publish".into(),
        title: b"Custom Styles".to_vec(),
        post_type: "wp_global_styles".into(),
        name: Some(
            format!(
                "wp-global-styles-{}",
                String::from_utf8_lossy(&hyscribe_phpser::urlencode(stylesheet.as_bytes()))
            )
            .into_bytes(),
        ),
        ..PostArgs::default()
    };
    let id = wp_insert_post(write, ctx, args).await?;
    // `tax_input`, which every user who may edit posts may assign.
    let theme = [TermRef::Text(stylesheet.as_bytes())];
    wp_set_post_terms(write, id, TermList::List(&theme), Taxonomy::WpTheme, false).await?;
    Ok(id)
}

/// `wp_get_custom_css_post()`'s lookup for the active theme `stylesheet`
/// (named `name`), kept in its theme mod `custom_css_post_id`: `-1` once
/// it found no `custom_css` post. Finding one is not ported.
pub async fn custom_css_post_lookup(
    write: &mut Write<'_>,
    stylesheet: &str,
    name: &str,
) -> Result<()> {
    let option = format!("theme_mods_{stylesheet}");
    let mut mods = match crate::options::get_option(write, &option).await? {
        Value::Array(mods) => mods,
        Value::Bool(false) => {
            // get_theme_mods() moves the mods kept under the theme's name.
            let current = crate::options::get_option(write, "current_theme").await?;
            let current = match current {
                Value::Bool(false) => name.as_bytes().to_vec(),
                current => current.to_php_string(),
            };
            let legacy = format!("mods_{}", String::from_utf8_lossy(&current));
            if write.option_value(&legacy).await?.is_some() {
                return Err(Error::Unported("theme mods kept under the theme's name"));
            }
            Vec::new()
        }
        _ => return Err(Error::Unported("theme mods that are not an array")),
    };
    let key = Key::String(b"custom_css_post_id".to_vec());
    let found = mods.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone());
    let kept = match found {
        Some(Value::Int(-1)) => true,
        // No post has the ID 0, so `$post_id > 0` is the conversion's.
        Some(Value::Int(id)) => match u64::try_from(id) {
            Ok(id) => write.post(id).await?.is_some(),
            Err(_) => false,
        },
        _ => false,
    };
    if kept {
        return Ok(());
    }
    if write
        .post_name_used(&sanitize_title(stylesheet.as_bytes()), "custom_css", 0)
        .await?
    {
        return Err(Error::Unported("a custom CSS post"));
    }
    match mods.iter_mut().find(|(k, _)| *k == key) {
        Some((_, value)) => *value = Value::Int(-1),
        None => mods.push((key, Value::Int(-1))),
    }
    crate::options::update_option(write, &option, &Value::Array(mods), None).await?;
    Ok(())
}

/// The post a block editor's view is of.
#[derive(Clone, Copy, Debug)]
pub enum Edited<'a> {
    /// `post-new.php` of a post type: a new auto-draft.
    New(&'a str),
    /// `post.php?action=edit` of a post.
    Existing(u64),
}

/// What a view of the block editor writes, in PHP's order: a new post's
/// auto-draft, the theme's global styles post its preloads name, the edit
/// lock, and the custom CSS lookup; the edited post's ID and the global
/// styles post's. `theme` is the active theme's stylesheet and name.
pub async fn block_editor_view(
    write: &mut Write<'_>,
    ctx: &Context,
    edited: Edited<'_>,
    (stylesheet, name): (&str, &str),
) -> Result<(u64, u64)> {
    // The theme pattern cache transient PHP also writes is skipped.
    let id = match edited {
        Edited::New(post_type) => get_default_post_to_edit(write, ctx, post_type).await?,
        Edited::Existing(id) => id,
    };
    let styles = get_user_global_styles_post_id(write, ctx, stylesheet).await?;
    // `wp_set_post_lock()`.
    let lock = Value::String(format!("{}:{}", ctx.now, ctx.user).into_bytes());
    update_metadata(
        write,
        Meta::Post,
        id,
        "_edit_lock",
        &lock,
        &Value::String(Vec::new()),
    )
    .await?;
    custom_css_post_lookup(write, stylesheet, name).await?;
    Ok((id, styles))
}

/// What a view of the site editor writes, in PHP's order: the custom CSS
/// lookup of its settings, then the theme's global styles post its
/// preloads name, whose ID it gives. `theme` is as for [`block_editor_view`].
pub async fn site_editor_view(
    write: &mut Write<'_>,
    ctx: &Context,
    (stylesheet, name): (&str, &str),
) -> Result<u64> {
    custom_css_post_lookup(write, stylesheet, name).await?;
    get_user_global_styles_post_id(write, ctx, stylesheet).await
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
    if kind == Kind::Attachment {
        return Err(Error::Unported("publishing an attachment"));
    }
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

/// The theme's global styles, which global styles posts are filtered
/// with: the site's own change only how fluid font sizes print, never
/// whether a value is safe.
fn styles() -> &'static hyscribe_render::theme::Theme {
    static THEME: std::sync::OnceLock<hyscribe_render::theme::Theme> = std::sync::OnceLock::new();
    THEME.get_or_init(hyscribe_render::theme::twentytwentyfive)
}

/// `sanitize_post( $postarr, 'db' )` with a default install's filters:
/// kses only for a user who may not post unfiltered HTML.
fn sanitize(mut args: PostArgs, ctx: &Context) -> PostArgs {
    args.title = hyscribe_phpser::php_trim(&args.title).to_vec();
    args.content = crate::convert_invalid_entities(&args.content);
    args.excerpt = crate::convert_invalid_entities(&args.excerpt);
    if !ctx.unfiltered_html {
        let post_kses = |text: &[u8]| hyscribe_render::kses::wp_kses_post(text, &ctx.upload_host);
        // content_save_pre: wp_filter_global_styles_post(), then kses.
        let styles = |text: &[u8]| {
            let filtered = styles().global_styles().filter_global_styles_post(text);
            post_kses(filtered.as_deref().unwrap_or(text))
        };
        args.title = hyscribe_render::kses::wp_kses_data(&args.title);
        args.content = styles(&args.content);
        args.content_filtered = styles(&args.content_filtered);
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
    args
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

/// `wpdb::process_fields()`'s check of a post row: each text column must be
/// valid UTF-8, as `strip_invalid_text()` would leave it, else PHP writes nothing.
fn valid_text(fields: &PostFields) -> Result<()> {
    let columns: [(&str, &[u8]); 9] = [
        ("post_content", &fields.content),
        ("post_content_filtered", &fields.content_filtered),
        ("post_title", &fields.title),
        ("post_excerpt", &fields.excerpt),
        ("post_password", &fields.password),
        ("post_name", &fields.name),
        ("to_ping", &fields.to_ping),
        ("pinged", &fields.pinged),
        ("guid", &fields.guid),
    ];
    let invalid: Vec<&str> = columns
        .iter()
        .filter(|(_, text)| std::str::from_utf8(text).is_err())
        .map(|(column, _)| *column)
        .collect();
    let last_error = match invalid.as_slice() {
        [] => return Ok(()),
        [column] => format!(
            "WordPress database error: Processing the value for the following field failed: \
             {column}. The supplied value may be too long or contains invalid data."
        ),
        columns => format!(
            "WordPress database error: Processing the values for the following fields failed: \
             {}. The supplied values may be too long or contain invalid data.",
            columns.join(", ")
        ),
    };
    Err(Error::InvalidText(last_error))
}

/// `wp_insert_post()` up to its `wp_insert_post` action, or for the upload
/// at `file` up to `add_attachment`: the post's ID and kind, and whether it
/// was an update.
async fn insert(
    write: &mut Write<'_>,
    ctx: &Context,
    args: PostArgs,
    file: Option<&str>,
) -> Result<(u64, Kind, bool)> {
    let args = sanitize(args, ctx);
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
    let kind = match file {
        Some(_) => Kind::Attachment,
        None => Kind::of(&post_type)?,
    };
    if kind == Kind::Attachment && before.is_none() && file.is_none() {
        return Err(Error::Unported("a new attachment but by an upload"));
    }
    // The types that support the editor, a title and an excerpt.
    if matches!(
        kind,
        Kind::Post | Kind::Block | Kind::Template | Kind::TemplatePart
    ) && !truthy(&args.content)
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
    let kept = ["inherit", "private", "trash", "auto-draft"];
    if kind == Kind::Attachment && !kept.contains(&status.as_str()) {
        status = "inherit".into();
    }
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
    if kind != Kind::Attachment && (status == "publish" || status == "future") {
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
        guid: before.as_ref().map_or(args.guid, |post| post.guid.clone()),
    };
    valid_text(&fields)?;
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
    for (taxonomy, terms) in &args.tax_input {
        wp_set_post_terms(write, id, TermList::Text(terms), *taxonomy, false).await?;
    }
    for (key, value) in &args.meta_input {
        update_metadata(write, Meta::Post, id, key, value, &Value::Null).await?;
    }
    if before.is_none() && fields.guid.is_empty() {
        let guid = link::permalink(write, ctx, id, &fields).await?;
        write.set_post_guid(id, &guid).await?;
    }
    if let Some(file) = file {
        // update_attached_file(); an attachment fires `add_attachment` alone.
        let file = Value::String(file.as_bytes().to_vec());
        let any = Value::String(Vec::new());
        update_metadata(write, Meta::Post, id, "_wp_attached_file", &file, &any).await?;
        return Ok((id, kind, false));
    }
    // An attachment's update fires `attachment_updated` alone.
    if let (Kind::Attachment, Some(before)) = (kind, &before) {
        hooks::post_updated(write, kind, id, before).await?;
        return Ok((id, kind, true));
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

#[cfg(test)]
mod editor_tests;
