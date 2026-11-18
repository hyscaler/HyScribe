// SPDX-License-Identifier: AGPL-3.0-or-later

//! The posts controller's writes (MVP 4.3) for posts, pages, navigation
//! menus and synced patterns: `create_item()`, `update_item()` and
//! `delete_item()`. A request whose checks pass gives the [`Plan`]
//! `hyscribe-http` writes in one action, then its answer is made from the
//! site as the write left it, as PHP prepares the post it has just written.
//! What is not ported answers 501 before anything is written: `meta`,
//! pattern categories, a template that is a list, and content whose answer
//! would be 501.

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, is_empty_string, lookup};
use hyscribe_render::attachment::image_downsize;
use hyscribe_render::date::{Zone, format_unix, stored_unix};
use hyscribe_render::rest::Rendering;

use super::prepare::writes_or_unported;
use super::{Kind, get_post, is_sticky, prepare, render_request, set};
use crate::request::{Arg, Params};
use crate::response::{Response, RestError, object, text};
use crate::{Context, schema};

/// What a create or update saves: `wp_insert_post()`'s fields where the
/// request gives them, and what the controller does after it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Save {
    /// The post to update, 0 to create one.
    pub id: u64,
    /// `post_type`.
    pub post_type: String,
    /// `post_title`.
    pub title: Option<Vec<u8>>,
    /// `post_content`.
    pub content: Option<Vec<u8>>,
    /// `post_excerpt`.
    pub excerpt: Option<Vec<u8>>,
    /// `post_status`.
    pub status: Option<String>,
    /// `post_date` and `post_date_gmt`, both empty to reset them.
    pub date: Option<(String, String)>,
    /// `edit_date`.
    pub edit_date: bool,
    /// `post_name`.
    pub slug: Option<Vec<u8>>,
    /// The parent `wp_unique_post_slug()` makes the slug unique under, as
    /// if published, where the controller does.
    pub unique_slug: Option<u64>,
    /// `post_author`.
    pub author: Option<u64>,
    /// `post_password`.
    pub password: Option<Vec<u8>>,
    /// `post_parent`.
    pub parent: Option<u64>,
    /// `menu_order`.
    pub menu_order: Option<i64>,
    /// `comment_status`.
    pub comment_status: Option<String>,
    /// `ping_status`.
    pub ping_status: Option<String>,
    /// After the save: `stick_post()` or `unstick_post()`.
    pub sticky: Option<bool>,
    /// After the save: `set_post_thumbnail()` of this image, or for 0
    /// `delete_post_thumbnail()`.
    pub thumbnail: Option<u64>,
    /// After the save: `set_post_format()`, the format's term slug, empty
    /// for none.
    pub format: Option<Vec<u8>>,
    /// After the save: `update_post_meta( $id, '_wp_page_template', $template )`.
    pub template: Option<Vec<u8>>,
    /// After the save: `wp_set_object_terms()` of these categories' IDs.
    pub categories: Option<Vec<u64>>,
    /// After the save: `wp_set_object_terms()` of these tags' IDs.
    pub tags: Option<Vec<u64>>,
}

/// What an upload to the media collection saves besides its file: the
/// attachment's fields the request gives.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Upload {
    /// `post_title`.
    pub title: Option<Vec<u8>>,
    /// `post_excerpt`, from `caption`.
    pub caption: Option<Vec<u8>>,
    /// `post_content`, from `description`.
    pub description: Option<Vec<u8>>,
    /// `_wp_attachment_image_alt`, from `alt_text`.
    pub alt_text: Option<Vec<u8>>,
    /// `post_parent`, from `post`.
    pub post: u64,
}

/// What a write does.
#[derive(Clone, Debug, PartialEq)]
pub enum Write {
    /// Creates or updates a post.
    Save(Box<Save>),
    /// Uploads a file to the media collection.
    Upload(Box<Upload>),
    /// `wp_trash_post( $id )`.
    Trash(u64),
    /// `wp_delete_post( $id, true )`.
    Delete(u64),
}

/// A write whose checks passed.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    /// The current user.
    pub user: u64,
    /// Whether the user may publish the post (`wp_insert_post()`'s check
    /// of a pending post's slug).
    pub can_publish: bool,
    /// `current_user_can( 'unfiltered_html' )`, which leaves the post's
    /// fields unfiltered.
    pub unfiltered_html: bool,
    /// What to write.
    pub write: Write,
    /// A deleted post and its answer, prepared before it is deleted.
    previous: Option<(Post, Value)>,
}

/// What a write came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The post written: saved, trashed or deleted.
    Written(u64),
    /// What PHP refuses without writing, by its error code:
    /// `wp_insert_post()`'s `empty_content` (its `invalid_date` needs a day
    /// the validated dates never have), or
    /// `rest_cannot_delete` where trashing or deleting did nothing.
    Refused(&'static str),
    /// An upload's refusal: its code, message and status.
    Failed(&'static str, String, u16),
    /// A save `wpdb` refused for text that is not valid UTF-8, with its
    /// `$last_error`.
    InvalidText(String),
}

/// Makes a write's answer once it is written.
pub(crate) type Finish = fn(&Context, &mut Params, Plan, Outcome) -> Result<Response, RestError>;

/// A ported write endpoint: the arguments it does not port, its argument
/// checks for the request, its plan and its answer.
pub(crate) struct Writing {
    pub(crate) unported: fn(&Params) -> bool,
    pub(crate) args: fn(&Context, &Params, Vec<Arg>) -> Vec<Arg>,
    pub(crate) plan: fn(&Context, &Params) -> Result<Plan, RestError>,
    pub(crate) finish: Finish,
}

/// The ported write of the route `regex`'s endpoint `index`, if any.
pub(crate) fn writing(regex: &str, index: usize) -> Option<Writing> {
    let kind = Kind::of(regex);
    let collection = format!("/wp/v2/{}", kind.base());
    let item = format!("{collection}/(?P<id>[\\d]+)");
    let save = Writing {
        // `check_template()` takes a list as an array key, which PHP refuses.
        unported: |params| {
            params
                .get("template")
                .is_some_and(|t| t.as_array().is_some_and(|t| !t.is_empty()))
        },
        args: status_args,
        plan: plan_save,
        finish,
    };
    // The media collection's create uploads.
    if regex == "/wp/v2/media" && index == 1 {
        return Some(Writing {
            // The fields an upload does not port are the route's own unported arguments.
            unported: |_| false,
            args: |_, _, args| args,
            plan: plan_upload,
            finish,
        });
    }
    if !kind.writes() || regex != collection && regex != item {
        return None;
    }
    // Only writes ask: the second endpoint saves, an item's third deletes.
    Some(match index {
        1 => save,
        _ => Writing {
            unported: |_| false,
            args: |_, _, args| args,
            plan: plan_delete,
            finish,
        },
    })
}

/// `create_item()` of the media collection up to its upload: the parent
/// checked, and the fields `prepare_item_for_database()` takes.
fn plan_upload(context: &Context, params: &Params) -> Result<Plan, RestError> {
    let post = params.get("post").map_or(0, Value::to_int).unsigned_abs();
    let parent = (post != 0).then(|| context.site.post(post)).flatten();
    if parent.is_some_and(|parent| ["revision", "attachment"].contains(&parent.post_type.as_str()))
    {
        return Err(invalid("rest_invalid_param", "Invalid parent type."));
    }
    let upload = Upload {
        title: raw(params, "title")?,
        caption: raw(params, "caption")?,
        description: raw(params, "description")?,
        alt_text: params.get("alt_text").map(Value::to_php_string),
        post,
    };
    Ok(Plan {
        user: context.user,
        can_publish: false,
        unfiltered_html: context.can("unfiltered_html"),
        write: Write::Upload(Box::new(upload)),
        previous: None,
    })
}

/// `check_status()`: the post's own status is valid whatever the schema
/// says.
fn status_args(context: &Context, params: &Params, mut args: Vec<Arg>) -> Vec<Arg> {
    let post = set(params, "id")
        .then(|| get_post(context, params, Kind::of(params.route)).ok())
        .flatten();
    let own = post.is_some_and(|post| params.get("status") == Some(&text(post.status)));
    if own {
        args.iter_mut()
            .filter(|arg| arg.name == "status")
            .for_each(|arg| arg.validate = false);
    }
    args
}

/// Whether the type's schema has the field `name`, without which
/// `prepare_item_for_database()` and the callbacks leave it alone.
fn field(kind: Kind, name: &str) -> bool {
    super::args::save(kind).iter().any(|arg| arg.name == name)
}

/// The block `update_ignored_hooked_blocks_postmeta()` wraps the type's
/// content in.
fn wrapper(kind: Kind) -> &'static [u8] {
    match kind {
        Kind::Navigation => b"navigation",
        Kind::Block => b"block",
        _ => b"post-content",
    }
}

/// `WP_Error( $code, $message, array( 'status' => 400 ) )`.
fn invalid(code: &'static str, message: &str) -> RestError {
    RestError::status(code, message, 400)
}

/// The text of `title`, `content` or `excerpt`: a string, or an array's
/// `raw`, for `title` only when not empty. A `raw` that is not a string
/// is not ported.
fn raw(params: &Params, name: &str) -> Result<Option<Vec<u8>>, RestError> {
    let raw = match params.get(name) {
        Some(Value::String(text)) => return Ok(Some(text.clone())),
        Some(Value::Array(parts)) => lookup(parts, "raw"),
        _ => None,
    };
    match raw {
        Some(raw) if name == "title" && raw.is_empty() => Ok(None),
        Some(Value::String(raw)) => Ok(Some(raw.clone())),
        Some(Value::Null) | None => Ok(None),
        Some(_) => Err(RestError::not_ported()),
    }
}

/// `rest_get_date_with_gmt( $date, $is_utc )`: the local and UTC dates.
fn date_with_gmt(date: &[u8], utc: bool, zone: Zone) -> (String, String) {
    const MYSQL: &str = "Y-m-d H:i:s";
    let sign = |b: &u8| matches!(b, b'+' | b'-');
    let digits = |digits: &[&u8]| digits.iter().all(|b| b.is_ascii_digit());
    // Validated as a date and time, which parses.
    let unix = schema::parse_date(date).unwrap_or_default();
    // `#(Z|[+-]\d{2}(:\d{2})?)$#`, of a date sanitized of new lines.
    let tail = |n: usize| date.get(date.len().saturating_sub(n)..).unwrap_or_default();
    let zoned = date.ends_with(b"Z")
        || matches!(tail(3), [s, a, b] if sign(s) && digits(&[a, b]))
        || matches!(tail(6), [s, a, b, b':', c, d] if sign(s) && digits(&[a, b, c, d]));
    if !utc && !zoned {
        let local = format_unix(MYSQL, unix, Zone::Utc);
        let gmt = format_unix(MYSQL, stored_unix(&local, zone), Zone::Utc);
        (local, gmt)
    } else {
        (
            format_unix(MYSQL, unix, zone),
            format_unix(MYSQL, unix, Zone::Utc),
        )
    }
}

/// `handle_status_param()` of a validated status.
fn status(context: &Context, kind: Kind, status: String) -> Result<String, RestError> {
    let message = match status.as_str() {
        "private" => "Sorry, you are not allowed to create private posts in this post type.",
        "publish" | "future" => "Sorry, you are not allowed to publish posts in this post type.",
        _ => return Ok(status),
    };
    if context.can(&kind.cap("publish_posts")) {
        return Ok(status);
    }
    Err(context.forbidden("rest_cannot_publish", message))
}

/// `create_item()` and `update_item()` up to their writes, with
/// `prepare_item_for_database()`.
fn plan_save(context: &Context, params: &Params) -> Result<Plan, RestError> {
    let kind = Kind::of(params.route);
    let existing = match params.get("id") {
        Some(_) => Some(get_post(context, params, kind)?),
        None => None,
    };
    let id = existing.as_ref().map_or(0, |post| post.id);
    let mut save = Save {
        id,
        post_type: kind.name().into(),
        title: raw(params, "title")?,
        content: raw(params, "content")?,
        excerpt: match field(kind, "excerpt") {
            true => raw(params, "excerpt")?,
            false => None,
        },
        ..Save::default()
    };
    let can_publish = if id == 0 {
        context.can(&kind.cap("publish_posts"))
    } else {
        context.can_for("publish_post", id)
    };
    let current = existing.as_ref().map(|post| post.status.as_str());
    if let Some(asked) = params.get("status") {
        let asked = String::from_utf8_lossy(&asked.to_php_string()).into_owned();
        if current != Some(asked.as_str()) {
            save.status = Some(status(context, kind, asked)?);
        }
    }
    dates(context, params, existing.as_ref(), &mut save);
    save.slug = params.get("slug").map(Value::to_php_string);
    if field(kind, "author") && set(params, "author") {
        let author = params.get("author").map_or(0, Value::to_int);
        let author = u64::try_from(author)
            .ok()
            .filter(|&author| author == context.user || context.site.user(author).is_some());
        let invalid = || invalid("rest_invalid_author", "Invalid author ID.");
        save.author = Some(author.ok_or_else(invalid)?);
    }
    protection(context, params, (kind, id), &mut save)?;
    if kind == Kind::Page {
        if let Some(parent) = params.get("parent").map(Value::to_int) {
            let found = u64::try_from(parent)
                .ok()
                .filter(|&parent| parent == 0 || context.site.post(parent).is_some());
            let message = "Invalid post parent ID.";
            save.parent = Some(found.ok_or_else(|| invalid("rest_post_invalid_id", message))?);
        }
        save.menu_order = params.get("menu_order").map(Value::to_int);
    }
    let status_of = |name| {
        let given = params
            .get(name)
            .filter(|status| field(kind, name) && !status.is_empty());
        given.map(|status| String::from_utf8_lossy(&status.to_php_string()).into_owned())
    };
    save.comment_status = status_of("comment_status");
    save.ping_status = status_of("ping_status");
    if let (Some(post), Some(content)) = (&existing, &save.content) {
        // `update_ignored_hooked_blocks_postmeta()`, where no block is hooked.
        let ignored = context.site.post_meta(post.id, "_wp_ignored_hooked_blocks");
        if ignored.is_some_and(|ignored| !is_empty_string(&ignored)) {
            return Err(RestError::not_ported());
        }
        let theme = context.site.theme().slug().as_bytes();
        save.content = Some(crate::templates::content::in_parent(
            content,
            wrapper(kind),
            theme,
        ));
    }
    let slugged = current.map_or(save.status.as_deref(), |current| {
        Some(save.status.as_deref().unwrap_or(current))
    });
    let slug = save
        .slug
        .as_deref()
        .is_some_and(|slug| !is_empty_string(slug));
    if slug && matches!(slugged, Some("draft" | "pending")) {
        save.unique_slug = Some(save.parent.unwrap_or_default());
    }
    after_save(context, params, kind, &mut save);
    let hook = format!("rest_pre_insert_{}", kind.name());
    let save = crate::hooks::apply_filters(&hook, save, &[params]);
    let unfiltered_html = context.can("unfiltered_html");
    answerable(context, &save, existing.as_ref(), unfiltered_html)?;
    Ok(Plan {
        user: context.user,
        can_publish,
        unfiltered_html,
        write: Write::Save(Box::new(save)),
        previous: None,
    })
}

/// The dates of `prepare_item_for_database()`: `date`, else `date_gmt`,
/// where it changes the post's; either sent as null resets both.
fn dates(context: &Context, params: &Params, existing: Option<&Post>, save: &mut Save) {
    let zone = context.zone();
    let given = |name| params.get(name).filter(|date| !date.is_empty());
    let changed = match (given("date"), given("date_gmt")) {
        (Some(date), _) => Some(date_with_gmt(&date.to_php_string(), false, zone))
            .filter(|(local, _)| existing.is_none_or(|post| post.date != *local)),
        (None, Some(date)) => Some(date_with_gmt(&date.to_php_string(), true, zone))
            .filter(|(_, gmt)| existing.is_none_or(|post| post.date_gmt != *gmt)),
        (None, None) => None,
    };
    if let Some(date) = changed {
        save.date = Some(date);
        save.edit_date = true;
    }
    let null = |name| params.has(name) && params.get(name).is_none();
    if null("date_gmt") || null("date") {
        save.date = Some((String::new(), String::new()));
    }
}

/// The password and stickiness checks of `prepare_item_for_database()`.
/// A password protected post made sticky is not ported:
/// `post_password_required()` reads the post password cookie.
fn protection(
    context: &Context,
    params: &Params,
    (kind, id): (Kind, u64),
    save: &mut Save,
) -> Result<(), RestError> {
    let sticky = kind == Kind::Post && set(params, "sticky");
    if let Some(password) = params.get("password").map(Value::to_php_string) {
        if !password.is_empty() && sticky {
            let message = "A post can not be sticky and have a password.";
            return Err(invalid("rest_invalid_field", message));
        }
        if !password.is_empty() && id != 0 && is_sticky(context, id) {
            let message = "A sticky post can not be password protected.";
            return Err(invalid("rest_invalid_field", message));
        }
        save.password = Some(password);
    }
    let protected = context
        .site
        .post(id)
        .is_some_and(|post| !post.password.is_empty());
    if sticky && protected {
        return Err(RestError::not_ported());
    }
    Ok(())
}

/// What `create_item()` and `update_item()` do once the post is saved:
/// stickiness (a create always), the featured image, the template and the
/// terms.
fn after_save(context: &Context, params: &Params, kind: Kind, save: &mut Save) {
    if kind == Kind::Post {
        let sticky = params.get("sticky").map(|sticky| !sticky.is_empty());
        save.sticky = sticky.or((save.id == 0).then_some(false));
        let ids = |name| {
            let ids = params
                .get(name)
                .and_then(Value::as_array)
                .unwrap_or_default();
            // The terms check refused 0, which no user may assign.
            let ids = ids.iter();
            ids.filter_map(|(_, id)| u64::try_from(id.to_int()).ok())
                .collect()
        };
        save.categories = params.get("categories").map(|_| ids("categories"));
        save.tags = params.get("tags").map(|_| ids("tags"));
    }
    // `set_post_thumbnail()` keeps an image `wp_get_attachment_image()`
    // shows and deletes the meta for any other post, as
    // `delete_post_thumbnail()` does for none.
    let media = params
        .get("featured_media")
        .filter(|_| field(kind, "featured_media"));
    if let Some(media) = media.map(|media| media.to_int().unsigned_abs()) {
        let shown = image_downsize(context.site.reads(), media, "thumbnail").is_some();
        save.thumbnail = match context.site.post(media) {
            Some(_) if shown => Some(media),
            Some(_) => Some(0),
            None => (media == 0).then_some(0),
        };
    }
    if kind == Kind::Post {
        // Validated as one of `get_post_format_slugs()`, so never empty.
        save.format = params
            .get("format")
            .map(|format| match format.to_php_string() {
                standard if standard == b"standard" => Vec::new(),
                format => [&b"post-format-"[..], &format].concat(),
            });
    }
    save.template = params.get("template").map(Value::to_php_string);
}

/// `check_template()`: no template, the post's own, or one of
/// `get_page_templates()` for the post's type; the theme's PHP templates
/// are not ported, as the active theme is a block theme.
pub(super) fn check_template(
    context: &Context,
    params: &Params,
    template: &Value,
) -> Result<(), RestError> {
    if template.is_empty() {
        return Ok(());
    }
    let post = params
        .get("id")
        .filter(|id| !id.is_empty())
        .and_then(|id| u64::try_from(id.to_int()).ok())
        .and_then(|id| context.site.post(id));
    if let Some(post) = &post {
        // `get_page_template_slug()`.
        let current = context.site.post_meta(post.id, "_wp_page_template");
        let current = current.filter(|current| !is_empty_string(current) && current != b"default");
        if *template == text(current.unwrap_or_default()) {
            return Ok(());
        }
    }
    let post_type = match &post {
        Some(post) => post.post_type.clone(),
        None => Kind::of(params.route).name().to_string(),
    };
    let allowed = crate::templates::post_templates(context, post_type.as_bytes())?;
    let args: [&dyn std::any::Any; 2] = [&post, &post_type];
    let allowed = crate::hooks::apply_filters("theme_templates", allowed, &args);
    let hook = format!("theme_{post_type}_templates");
    let allowed = crate::hooks::apply_filters(&hook, allowed, &args);
    if allowed.contains(&template.to_php_string()) {
        return Ok(());
    }
    let names: Vec<_> = allowed
        .iter()
        .map(|slug| String::from_utf8_lossy(slug))
        .collect();
    Err(RestError {
        code: "rest_invalid_param",
        message: format!("template is not one of {}.", names.join(", ")),
        data: Value::Null,
    })
}

/// The answer after the save renders the content as stored, for the
/// logged in user every write has, which must be ported: kses filters it
/// unless the user may post unfiltered HTML.
fn answerable(
    context: &Context,
    save: &Save,
    existing: Option<&Post>,
    unfiltered_html: bool,
) -> Result<(), RestError> {
    let stored = existing.map(|post| post.content.clone());
    let content = save.content.clone().or(stored).unwrap_or_default();
    let content = if unfiltered_html {
        content
    } else {
        let site = context.option("siteurl").unwrap_or_default();
        let host = hyscribe_render::link::host(&site);
        hyscribe_render::kses::wp_kses_post(&content, &host)
    };
    if writes_or_unported(&content, true) {
        return Err(RestError::not_ported());
    }
    Ok(())
}

/// `delete_item()` up to its write: `force` deletes, else the post goes
/// to the trash, which keeps it for `EMPTY_TRASH_DAYS` unless that is 0.
fn plan_delete(context: &Context, params: &Params) -> Result<Plan, RestError> {
    let kind = Kind::of(params.route);
    // The permission check found the post.
    let post = get_post(context, params, kind).unwrap_or_default();
    let force = params.get("force").is_some_and(|force| !force.is_empty());
    let (write, previous) = if force {
        let mut edit = params.clone();
        edit.set("context", text("edit"));
        (
            Write::Delete(post.id),
            Some((post.clone(), prepared(context, &edit, kind, &post)?.data)),
        )
    } else if !context.site.uses_trash() {
        let message = "The post does not support trashing. Set 'force=true' to delete.";
        return Err(RestError::status("rest_trash_not_supported", message, 501));
    } else if post.status == "trash" {
        let message = "The post has already been deleted.";
        return Err(RestError::status("rest_already_trashed", message, 410));
    } else if writes_or_unported(&post.content, true) {
        return Err(RestError::not_ported());
    } else {
        (Write::Trash(post.id), None)
    };
    Ok(Plan {
        user: context.user,
        can_publish: false,
        unfiltered_html: false,
        write,
        previous,
    })
}

/// `prepare_item_for_response()` of `post`.
fn prepared(
    context: &Context,
    params: &Params,
    kind: Kind,
    post: &Post,
) -> Result<Response, RestError> {
    let request = render_request(context);
    let mut rendering = Rendering::new(context.site.theme(), context.site.reads(), &request);
    prepare::post(context, params, kind, post, &mut rendering)
}

/// The answer once written: the post as saved or trashed, in the `edit`
/// context, a new one with `201` and its `Location`; a deleted post's
/// answer as it was; or PHP's error where it refused. The type's REST
/// actions fire as the write leaves the post, before its answer.
fn finish(
    context: &Context,
    params: &mut Params,
    plan: Plan,
    outcome: Outcome,
) -> Result<Response, RestError> {
    let id = match outcome {
        Outcome::Written(id) => id,
        Outcome::Refused("empty_content") => {
            let message = "Content, title, and excerpt are empty.";
            return Err(invalid("empty_content", message));
        }
        Outcome::Refused(_) => {
            let message = "The post cannot be deleted.";
            return Err(RestError::status("rest_cannot_delete", message, 500));
        }
        Outcome::Failed(code, message, status) => {
            return Err(RestError::status(code, &message, status));
        }
        // `WP_Error::add_data()` keeps wpdb's error as additional data.
        Outcome::InvalidText(last_error) => {
            let (code, message) = match &plan.write {
                Write::Save(save) if save.id == 0 => (
                    "db_insert_error",
                    "Could not insert post into the database.",
                ),
                _ => ("db_update_error", "Could not update post in the database."),
            };
            let data = object([
                ("code", text(code)),
                ("message", text(message)),
                ("data", object([("status", Value::Int(500))])),
                (
                    "additional_data",
                    Value::Array(vec![(Key::Int(0), text(last_error))]),
                ),
            ]);
            return Ok(Response::new(data, 500));
        }
    };
    let kind = Kind::of(params.route);
    let action = |name: &str, args: &[&dyn std::any::Any]| {
        crate::hooks::do_action(&format!("{name}_{}", kind.name()), args);
    };
    if let (Write::Delete(_), Some((post, previous))) = (&plan.write, plan.previous) {
        let deleted = object([("deleted", Value::Bool(true)), ("previous", previous)]);
        let response = Response::new(deleted, 200);
        action("rest_delete", &[&post, &response, params]);
        return Ok(response);
    }
    params.set("context", text("edit"));
    let post = context.site.post(id).unwrap_or_default();
    let created = match &plan.write {
        Write::Save(save) => save.id == 0,
        Write::Upload(_) => true,
        Write::Trash(_) | Write::Delete(_) => false,
    };
    // `rest_insert_attachment` fires at the upload's insert, in PHP before its sizes.
    if let Write::Save(_) | Write::Upload(_) = plan.write {
        action("rest_insert", &[&post, params, &created]);
        action("rest_after_insert", &[&post, params, &created]);
    }
    let location = context.rest_url(&format!("/wp/v2/{}/{id}", kind.base()));
    // Its content was found ported before the write.
    prepared(context, params, kind, &post).map(|mut response| {
        if created {
            response.status = 201;
            response.header("Location", &location);
        }
        if let Write::Trash(_) = plan.write {
            action("rest_delete", &[&post, &response, params]);
        }
        response
    })
}
