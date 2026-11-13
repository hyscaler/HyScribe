// SPDX-License-Identifier: AGPL-3.0-or-later

//! `upload.php`: the media library, as the grid's shell its scripts fill
//! over `admin-ajax.php`, or as `WP_Media_List_Table`.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_model::{OrderBy, Post, PostQuery, StatusFilter, TimeCondition};
use hyscribe_phpser::{
    Key, Maybe, Value, add_query_arg, addslashes, is_empty_string, maybe_unserialize,
    remove_query_arg, wp_basename,
};
use hyscribe_render::date::{self, human_time_diff};
use hyscribe_render::formatting::{ent2ncr, html_entity_decode};
use hyscribe_render::{number_format, order_by, search_count, search_terms};

use super::image::{attachment_url, thumbnail};
use super::{MEDIA_SCRIPTS, MEDIA_STYLES, MediaSite, enqueue_media};
use crate::admin_bar::BarSite;
use crate::assets::cookie_path;
use crate::list_table::{
    Column, Order, Sort, bulk_actions, column_headers, fill, months_dropdown, pagenum, pagination,
    row_actions, screen_options, table_description,
};
use crate::menu::{Screen, esc_link, link};
use crate::page::Request;
use crate::page::{HelpTab, Page, admin_page, json, text};
use crate::posts::{
    Answer, author_ids, author_lists, comments_bubble, draft_or_post_title, edit_link,
    hidden_columns, per_page,
};

const GRID: &str = include_str!("grid.html");
const LIST: &str = include_str!("list.html");

const SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/media-library-screen/\">Documentation on Media Library</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

const GRID_HELP: [HelpTab; 2] = [
    HelpTab {
        id: "overview",
        title: "Overview",
        content: "<p>All the files you&#8217;ve uploaded are listed in the Media Library, with the most recent uploads listed first.</p><p>You can view your media in a simple visual grid or a list with columns. Switch between these views using the icons to the left above the media.</p><p>To delete media items, click the <strong>Bulk select</strong> button at the top of the screen. Select any items you wish to delete, then click the <strong>Delete permanently</strong> button. Clicking the <strong>Cancel</strong> button takes you back to viewing your media.</p>",
    },
    HelpTab {
        id: "attachment-details",
        title: "Attachment Details",
        content: "<p>Clicking an item will display an Attachment Details dialog, which allows you to preview media and make quick edits. Any changes you make to the attachment details will be automatically saved.</p><p>Use the buttons at the top of the dialog, or <code>alt</code>/<code>option</code> plus the left or right arrow keys on your keyboard, to navigate between media items quickly.</p><p>You can also delete individual items and access the extended edit screen from the details dialog.</p>",
    },
];

const LIST_HELP: [HelpTab; 3] = [
    HelpTab {
        id: "overview",
        title: "Overview",
        content: "<p>All the files you&#8217;ve uploaded are listed in the Media Library, with the most recent uploads listed first. You can use the Screen Options tab to customize the display of this screen.</p><p>You can narrow the list by file type/status or by date using the dropdown menus above the media table.</p><p>You can view your media in a simple visual grid or a list with columns. Switch between these views using the icons to the left above the media.</p>",
    },
    HelpTab {
        id: "actions-links",
        title: "Available Actions",
        content: "<p>Hovering over a row reveals action links that allow you to manage media items. You can perform the following actions:</p><ul><li><strong>Edit</strong> takes you to a simple screen to edit that individual file&#8217;s metadata. You can also reach that screen by clicking on the media file name or thumbnail.</li><li><strong>Delete Permanently</strong> will delete the file from the media library (as well as from any posts to which it is currently attached).</li><li><strong>View</strong> will take you to a public display page for that file.</li><li><strong>Copy URL</strong> copies the URL for the media file to your clipboard.</li><li><strong>Download file</strong> downloads the original media file to your device.</li></ul>",
    },
    HelpTab {
        id: "attaching-files",
        title: "Attaching Files",
        content: "<p>If a media file has not been attached to any content, you will see that in the Uploaded To column, and can click on Attach to launch a small popup that will allow you to search for existing content and attach the file.</p>",
    },
];

/// `get_post_mime_types()`: each type's MIME types and label; the groups
/// beyond images, audio and video list `wp_get_mime_types()` of their
/// extensions.
const MIME_TYPES: [(&str, &str); 6] = [
    ("image", "Images"),
    ("audio", "Audio"),
    ("video", "Video"),
    (
        "application/msword,application/vnd.openxmlformats-officedocument.wordprocessingml.document,application/vnd.ms-word.document.macroEnabled.12,application/vnd.ms-word.template.macroEnabled.12,application/vnd.oasis.opendocument.text,application/vnd.apple.pages,application/pdf,application/vnd.ms-xpsdocument,application/oxps,application/rtf,application/wordperfect,application/octet-stream",
        "Documents",
    ),
    (
        "application/vnd.apple.numbers,application/vnd.oasis.opendocument.spreadsheet,application/vnd.ms-excel,application/vnd.openxmlformats-officedocument.spreadsheetml.sheet,application/vnd.ms-excel.sheet.macroEnabled.12,application/vnd.ms-excel.sheet.binary.macroEnabled.12",
        "Spreadsheets",
    ),
    (
        "application/x-gzip,application/rar,application/x-tar,application/zip,application/x-7z-compressed",
        "Archives",
    ),
];

/// The post types a parent links to the editor from: those with `show_ui`
/// that `post.php` edits.
const EDITED_TYPES: [&str; 4] = ["post", "page", "attachment", "wp_block"];

/// The post types the site editor edits.
const SITE_EDITED: [&str; 3] = ["wp_navigation", "wp_template", "wp_template_part"];

/// The other post types core registers without `show_ui`, whose parents
/// show their title to a reader.
const HIDDEN_TYPES: [&str; 9] = [
    "revision",
    "nav_menu_item",
    "custom_css",
    "customize_changeset",
    "oembed_cache",
    "user_request",
    "wp_global_styles",
    "wp_font_family",
    "wp_font_face",
];

/// The arguments the list takes; any other is not ported.
const LIST_ARGS: [&str; 26] = [
    "mode",
    "attachment-filter",
    "post_mime_type",
    "detached",
    "mine",
    "m",
    "cat",
    "s",
    "orderby",
    "order",
    "paged",
    "author",
    "filter_action",
    "action",
    "action2",
    "bulk_action",
    "_wpnonce",
    "_wp_http_referer",
    "posted",
    "attached",
    "detach",
    "deleted",
    "trashed",
    "untrashed",
    "message",
    "ids",
];

/// `upload.php`'s query: `$_GET`'s strings in order, unslashed.
#[derive(Default)]
pub struct LibraryArgs {
    /// The arguments, each name once.
    pub args: Vec<(String, String)>,
}

impl LibraryArgs {
    /// The arguments of the query string `query`; `None` for one that is
    /// not a string.
    pub fn parse(query: &str) -> Option<LibraryArgs> {
        let args = query_strings(query)?;
        Some(LibraryArgs { args })
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.args
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }

    /// `! empty( $_GET[ $name ] )`.
    fn given(&self, name: &str) -> bool {
        self.get(name)
            .is_some_and(|v| !is_empty_string(v.as_bytes()))
    }

    /// `absint( $_GET[ $name ] )` when it is not empty and not 0.
    fn count(&self, name: &str) -> Option<u64> {
        let value = self.get(name).filter(|_| self.given(name))?;
        let count = Value::String(value.into()).to_int().unsigned_abs();
        (count > 0).then_some(count)
    }
}

/// `$_GET` of `query`: each name and its string; `None` if one is not a
/// string or not text.
pub(crate) fn query_strings(query: &str) -> Option<Vec<(String, String)>> {
    let parsed = hyscribe_phpser::parse_str(query.as_bytes());
    let mut args = Vec::new();
    for (key, value) in parsed.as_array().unwrap_or_default() {
        let Value::String(value) = value else {
            return None;
        };
        let key = match key {
            Key::String(key) => String::from_utf8(key.clone()).ok()?,
            Key::Int(key) => key.to_string(),
        };
        args.push((key, String::from_utf8(value.clone()).ok()?));
    }
    Some(args)
}

fn bytes(text: &[u8]) -> String {
    String::from_utf8_lossy(text).into_owned()
}

/// `_n()` of a count: the message for one, else for `count` with `%s`.
fn counted(count: u64, one: &str, many: &str) -> String {
    if count == 1 {
        one.into()
    } else {
        many.replace("%s", &number_format(count))
    }
}

/// The Undo link of a move to the Trash, `wp_kses_post()` normalizing its
/// entities.
fn undo(site: &dyn MediaSite, args: &LibraryArgs) -> String {
    let ids = addslashes(args.get("ids").unwrap_or_default().as_bytes());
    let url = [&b"upload.php?doaction=undo&action=untrash&ids="[..], &ids].concat();
    format!(
        " <a href=\"{}\">Undo</a>",
        nonce_url(&url, &site.nonce("bulk-media")).replace("&#038;", "&amp;")
    )
}

/// `esc_url( wp_nonce_url( $url ) )` with `nonce`, under hs-admin's URLs.
fn nonce_url(url: &[u8], nonce: &str) -> String {
    // The nonce goes in after the escaping, a stand-in holding its place.
    let url = add_query_arg(&[(b"_wpnonce", Some(b"0"))], url);
    let url = esc_link(&esc_html(&url));
    format!("{}{nonce}", url.strip_suffix('0').unwrap_or(&url))
}

/// The notice the arguments of a finished action ask for, and the
/// arguments `upload.php` takes out of the request for it.
fn notice(site: &dyn MediaSite, args: &LibraryArgs) -> (String, Vec<&'static [u8]>) {
    let mut message = String::new();
    let mut removed: Vec<&'static [u8]> = Vec::new();
    if args.given("posted") {
        message = "Media file updated.".into();
        removed.push(b"posted");
    }
    // An attach unsets the detach.
    let (attached, detached) = match args.count("attached") {
        Some(n) => (Some(n), None),
        None => (None, args.count("detach")),
    };
    if let Some(n) = attached {
        message = counted(n, "Media file attached.", "%s media files attached.");
    }
    if let Some(n) = detached {
        message = counted(n, "Media file detached.", "%s media files detached.");
    }
    if attached.or(detached).is_some() {
        removed.extend([&b"detach"[..], b"attached"]);
    }
    if let Some(n) = args.count("deleted") {
        message = counted(
            n,
            "Media file permanently deleted.",
            "%s media files permanently deleted.",
        );
        removed.push(b"deleted");
    }
    if let Some(n) = args.count("trashed") {
        message = counted(
            n,
            "Media file moved to the Trash.",
            "%s media files moved to the Trash.",
        ) + &undo(site, args);
        removed.push(b"trashed");
    }
    if let Some(n) = args.count("untrashed") {
        message = counted(
            n,
            "Media file restored from the Trash.",
            "%s media files restored from the Trash.",
        );
        removed.push(b"untrashed");
    }
    let chosen = match args.get("message").filter(|_| args.given("message")) {
        Some("1") => Some("Media file updated.".to_string()),
        Some("2") => Some("Media file permanently deleted.".into()),
        Some("3") => Some("Error saving media file.".into()),
        Some("4") => Some(format!(
            "Media file moved to the Trash.{}",
            undo(site, args)
        )),
        Some("5") => Some("Media file restored from the Trash.".into()),
        _ => None,
    };
    if let Some(chosen) = chosen {
        message = chosen;
        removed.push(b"message");
    }
    let html = if message.is_empty() {
        message
    } else {
        format!(
            "<div id=\"message\" class=\"notice is-dismissible updated\"><p>{message}</p></div>"
        )
    };
    (html, removed)
}

/// `wp_match_mime_types( $wildcard, $real )` without wildcards: a type
/// of the list equal to `real`, or a group `real` starts or contains.
fn mime_matches(wildcard: &str, real: &str) -> bool {
    wildcard.split(',').map(str::trim).any(|mime| {
        // A group's `^group/` matches nothing its bare `group` does not.
        mime == real || !mime.contains('/') && real.contains(mime)
    })
}

/// `wp_post_mime_type_where()`'s types, each a `LIKE` pattern with a
/// wildcard; none for a type that matches everything.
fn mime_patterns(types: &str) -> Vec<String> {
    let keep = |text: &str, extra: &str| -> String {
        text.chars()
            .filter(|c| c.is_ascii_alphanumeric() || "-*.".contains(*c) || extra.contains(*c))
            .collect()
    };
    let mut patterns = Vec::new();
    for mime in types.split(',') {
        let mime: String = mime.trim().chars().filter(|c| !c.is_whitespace()).collect();
        if ["", "%", "%/%"].contains(&mime.as_str()) {
            return Vec::new();
        }
        let pattern = match mime.split_once('/') {
            Some((group, sub)) => {
                let sub = keep(sub, "+");
                let sub = if sub.is_empty() { "*".into() } else { sub };
                format!("{}/{sub}", keep(group, ""))
            }
            None => {
                let kept = keep(&mime, "");
                if kept.contains('*') {
                    kept
                } else {
                    format!("{kept}/*")
                }
            }
        };
        let mut collapsed = String::new();
        for c in pattern.chars() {
            if c != '*' {
                collapsed.push(c);
            } else if !collapsed.ends_with('%') {
                collapsed.push('%');
            }
        }
        patterns.push(collapsed);
    }
    patterns
}

/// The attachments `File_Upload_Upgrader` deletes soon, which the list
/// leaves out: the first argument of each `upgrader_scheduled_cleanup`.
fn scheduled_cleanups(cron: &[u8]) -> Vec<u64> {
    let crons = match maybe_unserialize(cron) {
        Maybe::Value(crons) => crons,
        _ => Value::Null,
    };
    crons
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(|(_, cron)| {
            let details = cron.get_str("upgrader_scheduled_cleanup")?;
            let first = details.as_array()?.first()?;
            let id = first.1.get_path(&["args", "0"])?;
            (!id.is_empty()).then(|| id.to_int().unsigned_abs())
        })
        .collect()
}

/// The `post_date` conditions of `m`'s digits, as `WP_Query` reads them.
fn date_filter(query: &mut PostQuery, m: &str) {
    let digits = Value::String(m.into()).to_int().unsigned_abs().to_string();
    if digits == "0" {
        return;
    }
    let part = |from: usize, len: usize| {
        digits
            .get(from..from + len)
            .and_then(|part| part.parse::<u64>().ok())
    };
    query.year = part(0, 4);
    query.month = part(4, 2);
    query.day = part(6, 2);
    let time = [
        part(8, 2).map(TimeCondition::Hour),
        part(10, 2).map(TimeCondition::Minute),
        part(12, 2).map(TimeCondition::Second),
    ];
    query.time = time.into_iter().flatten().collect();
}

/// The list's state, as `prepare_items()` leaves it.
struct Listing {
    posts: Vec<Post>,
    total: u64,
    pages: u64,
    page: u64,
    per_page: u64,
    trash: bool,
    detached: bool,
    filter: String,
    columns: Vec<Column>,
    hidden: Vec<String>,
}

/// `wp_edit_attachments_query()`'s query of `args`; `None` for an
/// author or category filter not ported.
fn query(
    site: &dyn MediaSite,
    args: &LibraryArgs,
    trash: bool,
    per_page: u64,
) -> Option<PostQuery> {
    let mut query = PostQuery {
        post_types: vec!["attachment".into()],
        limit: u32::try_from(per_page).unwrap_or(u32::MAX),
        exclude: scheduled_cleanups(&site.option("cron").unwrap_or_default()),
        ..PostQuery::default()
    };
    query.statuses = if trash {
        vec!["trash"]
    } else if site.can("read_private_posts") {
        vec!["inherit", "private"]
    } else {
        vec!["inherit"]
    }
    .into_iter()
    .map(|status| StatusFilter {
        status: status.into(),
        author: None,
    })
    .collect();
    if Value::String(args.get("cat").unwrap_or_default().into()).to_int() != 0 {
        return None;
    }
    let filter = args.get("attachment-filter").unwrap_or_default();
    let mut mime = args
        .get("post_mime_type")
        .filter(|mime| MIME_TYPES.iter().any(|(types, _)| types == mime));
    if let Some((types, _)) = MIME_TYPES
        .iter()
        .find(|(types, _)| filter.strip_prefix("post_mime_type:") == Some(types))
    {
        mime = Some(types);
    }
    query.mime_types = mime_patterns(mime.unwrap_or_default());
    if args.get("detached").is_some() || filter == "detached" {
        query.parents = vec![0];
    }
    let author = if args.get("mine").is_some() || filter == "mine" {
        site.user_id().to_string()
    } else {
        args.get("author").unwrap_or_default().into()
    };
    (query.authors, query.excluded_authors) =
        author_lists(&author_ids(&author).unwrap_or_default());
    date_filter(&mut query, args.get("m").unwrap_or_default());
    let search = args.get("s").unwrap_or_default();
    // `parse_query()` empties a search over 1600 bytes.
    if !search.is_empty() && search.len() <= 1600 {
        query.search = search_terms(search);
        query.sentence = search.into();
        query.search_count = search_count(search);
        query.filenames = true;
        query.logged_in = true;
    }
    let orderby = args.get("orderby").unwrap_or_default();
    let ascending = args
        .get("order")
        .is_some_and(|order| order.eq_ignore_ascii_case("asc"));
    query.order = order_by(&Value::String(orderby.into()), ascending, &query);
    // A search with a term not left out is ordered by relevance first,
    // unless another order is asked.
    let titled = query.search.iter().any(|term| !term.starts_with('-'));
    if titled && (orderby.is_empty() || orderby == "relevance") {
        query.order.insert(0, (OrderBy::Relevance, false));
    }
    let page = pagenum(args.get("paged"));
    query.offset = u32::try_from((page - 1).saturating_mul(per_page)).unwrap_or(u32::MAX);
    Some(query)
}

/// `get_columns()`: the comments only with attachment pages, and neither
/// they nor the parent for the unattached.
fn columns(site: &dyn MediaSite, detached: bool) -> Vec<Column> {
    let sort = |orderby, desc_first, abbr, caption, initial| {
        Some(Sort {
            orderby,
            desc_first,
            abbr,
            caption,
            initial,
        })
    };
    let column = |key, label: &str, sort| Column {
        key,
        label: label.into(),
        sort,
        primary: key == "title",
    };
    let mut columns = vec![
        column("cb", "<input type=\"checkbox\" />", None),
        column(
            "title",
            "File",
            sort("title", false, "File", "Table ordered by File Name.", None),
        ),
        column(
            "author",
            "Author",
            sort("author", false, "Author", "Table ordered by Author.", None),
        ),
    ];
    if !detached {
        columns.push(column(
            "parent",
            "Uploaded to",
            sort(
                "parent",
                false,
                "Uploaded to",
                "Table ordered by Uploaded To.",
                None,
            ),
        ));
        let pages = site
            .option("wp_attachment_pages_enabled")
            .unwrap_or_default();
        if !Value::String(pages).is_empty() {
            columns.push(column(
                "comments",
                "<span class=\"vers comment-grey-bubble\" title=\"Comments\" aria-hidden=\"true\"></span><span class=\"screen-reader-text\">Comments</span>",
                sort("comment_count", true, "", "Table ordered by Comments.", None),
            ));
        }
    }
    columns.push(column(
        "date",
        "Date",
        sort("date", true, "Date", "Table ordered by Date.", Some("desc")),
    ));
    columns
}

/// `get_views()`: the type filter's options, those of types the library
/// holds.
fn type_options(site: &dyn MediaSite, listing: &Listing) -> String {
    let filter = listing.filter.as_str();
    let selected = |chosen: bool| if chosen { " selected='selected'" } else { "" };
    let mut options = vec![format!(
        "<option value=\"\"{}>All media items</option>",
        selected(filter == "1")
    )];
    let available = site.attachment_mime_types();
    for (types, label) in MIME_TYPES {
        if !available.iter().any(|real| mime_matches(types, real)) {
            continue;
        }
        let chosen = filter
            .strip_prefix("post_mime_type:")
            .is_some_and(|real| !filter.is_empty() && mime_matches(types, real));
        options.push(format!(
            "<option value=\"post_mime_type:{}\"{}>{label}</option>",
            esc_attr(types.as_bytes()),
            selected(chosen)
        ));
    }
    let detached = if listing.detached {
        " selected=\"selected\""
    } else {
        ""
    };
    options.push(format!(
        "<option value=\"detached\"{detached}>Unattached</option>"
    ));
    options.push(format!(
        "<option value=\"mine\"{}>Mine</option>",
        selected(filter == "mine")
    ));
    if listing.trash {
        options.push("<option value=\"trash\" selected='selected'>Trash</option>".into());
    }
    options
        .iter()
        .map(|option| format!("\t{option}\n"))
        .collect()
}

/// `view_switcher()`: the list and grid links on the request `uri`.
fn view_switch(mode: &str, uri: &str) -> String {
    let links: String = [("list", "List view"), ("grid", "Grid view")]
        .iter()
        .map(|(name, title)| {
            let url = add_query_arg(&[(b"mode", Some(name.as_bytes()))], uri.as_bytes());
            let url = remove_query_arg(&[b"attachment-filter"], &url);
            let (class, current) = if *name == mode {
                (" current", " aria-current=\"page\"")
            } else {
                ("", "")
            };
            format!(
                "<a href='{}' class='view-{name}{class}' id='view-switch-{name}'{current}><span class='screen-reader-text'>{title}</span></a>\n",
                bytes(&esc_url(&url))
            )
        })
        .collect();
    format!(
        "\t\t<input type=\"hidden\" name=\"mode\" value=\"{}\" />\n\t\t<div class=\"view-switch\">\n\t\t{links}\t\t</div>\n\t\t",
        esc_attr(mode.as_bytes())
    )
}

/// `media_states()`: the custom logo's, as the screen's theme mods give it.
fn media_states(site: &dyn MediaSite, post: &Post) -> String {
    let stylesheet = bytes(&site.option("stylesheet").unwrap_or_default());
    let mods = site
        .option(&format!("theme_mods_{stylesheet}"))
        .unwrap_or_default();
    let logo = match maybe_unserialize(&mods) {
        Maybe::Value(mods) => mods.get_str("custom_logo").map_or(0, Value::to_int),
        _ => 0,
    };
    if logo == post.id as i64 {
        " &mdash; <span class='post-state'>Logo</span>".into()
    } else {
        String::new()
    }
}

/// `column_title()`: the thumbnail and title, linked to the editor, and
/// the file's name.
fn column_title(site: &dyn MediaSite, post: &Post, trash: bool) -> String {
    let mime = post.mime_type.split('/').next().unwrap_or_default();
    let thumb_id = site
        .meta(post, "_thumbnail_id")
        .map_or(0, |id| Value::String(id).to_int().unsigned_abs());
    let thumb = thumbnail(site, if thumb_id == 0 { post.id } else { thumb_id });
    let (start, end) = if site.can_post("edit_post", post) && !trash {
        (format!("<a href=\"{}\">", edit_link(site, post)), "</a>")
    } else {
        (String::new(), "")
    };
    let class = hyscribe_render::post::sanitize_html_class(format!("{mime}-icon").as_bytes(), b"");
    let file = wp_basename(&site.meta(post, "_wp_attached_file").unwrap_or_default());
    format!(
        "\t\t<strong class=\"has-media-icon\">\n\t\t\t{start}\t\t\t\t<span class=\"media-icon {}\">{thumb}</span>\n\t\t\t\t{}{end}{}\t\t</strong>\n\t\t<p class=\"filename\">\n\t\t\t<span class=\"screen-reader-text\">\n\t\t\t\tFile name:\t\t\t</span>\n\t\t\t{}\t\t</p>\n\t\t",
        bytes(&class),
        draft_or_post_title(post),
        media_states(site, post),
        esc_html(&file)
    )
}

/// `_get_row_actions()` of `post` titled `title`.
fn actions(site: &dyn MediaSite, post: &Post, title: &str, listing: &Listing) -> String {
    let id = post.id;
    let label = |text: &str| esc_attr(text.replace("%s", title).as_bytes());
    let nonced = |action: &str| {
        let url = format!("post.php?action={action}&post={id}");
        nonce_url(url.as_bytes(), &site.nonce(&format!("{action}-post_{id}")))
    };
    let mut actions: Vec<(&str, String)> = Vec::new();
    let can_edit = site.can_post("edit_post", post);
    if !listing.trash && can_edit {
        actions.push((
            "edit",
            format!(
                "<a href=\"{}\" aria-label=\"{}\">Edit</a>",
                bytes(&esc_url(edit_link(site, post).as_bytes())),
                label("Edit &#8220;%s&#8221;")
            ),
        ));
    }
    if site.can_post("delete_post", post) {
        if listing.trash {
            actions.push((
                "untrash",
                format!(
                    "<a href=\"{}\" class=\"submitdelete aria-button-if-js\" aria-label=\"{}\">Restore</a>",
                    nonced("untrash"),
                    label("Restore &#8220;%s&#8221; from the Trash")
                ),
            ));
        }
        // Without `MEDIA_TRASH`, files are deleted at once, after a warning.
        let confirm = if listing.trash {
            ""
        } else {
            " onclick='return showNotice.warn();'"
        };
        actions.push((
            "delete",
            format!(
                "<a href=\"{}\" class=\"submitdelete aria-button-if-js\"{confirm} aria-label=\"{}\">Delete Permanently</a>",
                nonced("delete"),
                label("Delete &#8220;%s&#8221; permanently")
            ),
        ));
    }
    let url = attachment_url(site, post).map(|url| bytes(&esc_url(&url)));
    // `get_permalink()` fails only for a post that is not there.
    if !listing.trash {
        actions.push((
            "view",
            format!(
                "<a href=\"{}\" aria-label=\"{}\" rel=\"bookmark\">View</a>",
                bytes(&esc_url(site.permalink(post).as_bytes())),
                label("View &#8220;%s&#8221;")
            ),
        ));
        if let Some(url) = &url {
            actions.push((
                "copy",
                format!(
                    "<span class=\"copy-to-clipboard-container\"><button type=\"button\" class=\"button-link copy-attachment-url media-library\" data-clipboard-text=\"{url}\" aria-label=\"{}\">Copy URL</button><span class=\"success hidden\" aria-hidden=\"true\">Copied!</span></span>",
                    label("Copy &#8220;%s&#8221; URL to clipboard")
                ),
            ));
        }
    }
    if let Some(url) = &url {
        actions.push((
            "download",
            format!(
                "<a href=\"{url}\" aria-label=\"{}\" download>Download file</a>",
                label("Download &#8220;%s&#8221;")
            ),
        ));
    }
    if listing.detached && can_edit {
        actions.push(("attach", attach_link(id, title)));
    }
    row_actions(&actions, false)
}

/// The link that opens the dialog to attach `id`, titled `title`.
fn attach_link(id: u64, title: &str) -> String {
    format!(
        "<a href=\"#the-list\" onclick=\"findPosts.open( 'media[]', '{id}' ); return false;\" class=\"hide-if-no-js aria-button-if-js\" aria-label=\"{}\">Attach</a>",
        esc_attr(format!("Attach &#8220;{title}&#8221; to existing content").as_bytes())
    )
}

/// `column_author()`.
fn column_author(site: &dyn MediaSite, post: &Post) -> String {
    match site
        .display_name(post.author)
        .filter(|name| !name.is_empty())
    {
        Some(name) => format!(
            "<a href=\"{}\">{}</a>",
            esc_link(&format!("upload.php?author={}", post.author)),
            esc_html(&ent2ncr(&name))
        ),
        None => "<span aria-hidden=\"true\">&#8212;</span><span class=\"screen-reader-text\">(no author)</span>".into(),
    }
}

/// `column_parent()`: the post the file is attached to, `parent`, with a
/// link to detach it, or one to attach it.
fn column_parent(site: &dyn MediaSite, post: &Post, parent: Option<&Post>) -> String {
    let can_edit = site.can_post("edit_post", post);
    let Some(parent) = parent else {
        let attach = if can_edit {
            format!("<br />{}", attach_link(post.id, &draft_or_post_title(post)))
        } else {
            String::new()
        };
        return format!("(Unattached)\t\t\t{attach}");
    };
    let title = draft_or_post_title(parent);
    let kind = parent.post_type.as_str();
    let edited = EDITED_TYPES.contains(&kind);
    let mut out = if edited && site.can_post("edit_post", parent) {
        format!(
            "<strong><a href=\"{}\">{title}</a></strong>",
            edit_link(site, parent)
        )
    } else if (edited || HIDDEN_TYPES.contains(&kind)) && site.can_post("read_post", parent) {
        format!("<strong>{title}</strong>")
    } else {
        "(Private post)".into()
    };
    if can_edit {
        let url = format!(
            "upload.php?parent_post_id={}&media[]={}&_wpnonce={}",
            post.parent,
            post.id,
            site.nonce("bulk-media")
        );
        out.push_str(&format!(
            "<br /><a href=\"{}\" class=\"hide-if-no-js detach-from-parent\" aria-label=\"{}\">Detach</a>",
            link(&url),
            esc_attr(format!("Detach from &#8220;{title}&#8221;").as_bytes())
        ));
    }
    out
}

/// The post `post` is attached to, if any.
fn parent_of(site: &dyn MediaSite, post: &Post) -> Option<Post> {
    // No post has ID 0.
    site.post(post.parent)
}

/// `column_date()`: how long ago within a day, else the day.
fn column_date(site: &dyn MediaSite, post: &Post) -> String {
    if post.date == "0000-00-00 00:00:00" {
        return "Unpublished".into();
    }
    let (zone, now) = site.clock();
    let time = date::stored_unix(&post.date, zone);
    let diff = now - time;
    if time != 0 && diff > 0 && diff < 86_400 {
        format!("{} ago", human_time_diff(time, now))
    } else {
        date::format("Y/m/d", &post.date, zone)
    }
}

/// `single_row()` of `post`.
fn row(site: &dyn MediaSite, post: &Post, listing: &Listing) -> String {
    let owner = if site.user_id() == post.author {
        "self"
    } else {
        "other"
    };
    let title = draft_or_post_title(post);
    let mut out = format!(
        "\t\t\t<tr id=\"post-{}\" class=\"author-{owner} status-{}\">\n\t\t\t\t",
        post.id, post.status
    );
    for column in &listing.columns {
        let key = column.key;
        let hidden = if listing.hidden.iter().any(|h| h == key) {
            " hidden"
        } else {
            ""
        };
        let colname = esc_attr(&hyscribe_kses::wp_strip_all_tags(
            column.label.as_bytes(),
            false,
        ));
        let attributes = format!("class='{key} column-{key}{hidden}' data-colname=\"{colname}\"");
        let cell = match key {
            "cb" => {
                let cb = if site.can_post("edit_post", post) {
                    format!(
                        "\t\t\t<input type=\"checkbox\" name=\"media[]\" id=\"cb-select-{0}\" value=\"{0}\" />\n\t\t\t<label for=\"cb-select-{0}\">\n\t\t\t\t<span class=\"screen-reader-text\">\n\t\t\t\tSelect {title}\t\t\t\t</span>\n\t\t\t</label>\n\t\t\t",
                        post.id
                    )
                } else {
                    String::new()
                };
                out.push_str(&format!("<td class=\"check-column\">{cb}</td>"));
                continue;
            }
            "title" => {
                let name =
                    hyscribe_kses::wp_strip_all_tags(&html_entity_decode(title.as_bytes()), false);
                let label = if name.is_empty() {
                    String::new()
                } else {
                    format!(" aria-label=\"{}\"", esc_attr(&name))
                };
                out.push_str(&format!(
                    "<th class='title column-title has-row-actions column-primary{hidden}' data-colname=\"{colname}\" scope=\"row\"{label}>{}{}</th>",
                    column_title(site, post, listing.trash),
                    actions(site, post, &title, listing)
                ));
                continue;
            }
            "author" => column_author(site, post),
            "parent" => column_parent(site, post, parent_of(site, post).as_ref()),
            "comments" => format!(
                "<div class=\"post-com-count-wrapper\">{}</div>",
                comments_bubble(site, post, site.pending_comments(post))
            ),
            _ => column_date(site, post),
        };
        out.push_str(&format!("<td {attributes}>{cell}</td>"));
    }
    out.push_str("\t\t\t</tr>\n\t\t\t");
    out
}

/// `display_tablenav( $which )`: the bulk actions and the pages.
fn tablenav(listing: &Listing, which: &str, url: &str) -> String {
    let mut actions = vec![("delete", "Delete permanently")];
    if listing.detached {
        actions.push(("attach", "Attach"));
    }
    let hidden = if listing.posts.is_empty() {
        " hidden"
    } else {
        ""
    };
    let current = listing.page.min(listing.pages).max(1);
    let pages = pagination(
        which,
        "Media items list navigation",
        listing.total,
        listing.pages,
        current,
        url,
    );
    format!(
        "\t<div class=\"tablenav {which}\">\n\n\t\t\t\t<div class=\"alignleft actions bulkactions{hidden}\">\n\t\t\t{}\t\t</div>\n\t\t{pages}\n\t\t<br class=\"clear\" />\n\t</div>\n",
        bulk_actions(which, &actions)
    )
}

/// What the request asks of the grid's scripts: `$_GET` but the search,
/// through `wp_edit_attachments_query_vars()`, less what is empty.
fn query_vars(site: &dyn MediaSite, args: &LibraryArgs, removed: &[&[u8]]) -> Value {
    let mut vars: Vec<(String, Value)> = args
        .args
        .iter()
        // `upload.php` unsets the notices' arguments but the message.
        .filter(|(key, _)| key != "s" && (key == "message" || !removed.contains(&key.as_bytes())))
        .map(|(key, value)| (key.clone(), Value::String(addslashes(value.as_bytes()))))
        .collect();
    let set = |vars: &mut Vec<(String, Value)>, key: &str, value: Value| match vars
        .iter_mut()
        .find(|(name, _)| name == key)
    {
        Some(found) => found.1 = value,
        None => vars.push((key.into(), value)),
    };
    for key in ["m", "cat"] {
        let int = Value::Int(Value::String(args.get(key).unwrap_or_default().into()).to_int());
        set(&mut vars, key, int);
    }
    let filter = args.get("attachment-filter").unwrap_or_default();
    let valid = |mime: &str| MIME_TYPES.iter().any(|(types, _)| *types == mime);
    if args.get("post_mime_type").is_some_and(|mime| !valid(mime)) {
        vars.retain(|(name, _)| name != "post_mime_type");
    }
    if let Some((types, _)) = MIME_TYPES
        .iter()
        .find(|(types, _)| filter.strip_prefix("post_mime_type:") == Some(types))
    {
        set(
            &mut vars,
            "post_mime_type",
            Value::String(types.as_bytes().to_vec()),
        );
    }
    if args.get("mine").is_some() || filter == "mine" {
        set(&mut vars, "author", Value::Int(site.user_id() as i64));
    }
    vars.retain(|(key, value)| {
        !value.is_empty()
            && !["mode", "post_type", "post_status", "posts_per_page"].contains(&key.as_str())
    });
    Value::Array(
        vars.into_iter()
            .map(|(key, value)| (Key::String(key.into_bytes()), value))
            .collect(),
    )
}

/// The grid's shell: the scripts list and edit the files.
fn grid(
    site: &dyn MediaSite,
    args: &LibraryArgs,
    request: &Request,
    notice: &str,
    removed: &[&[u8]],
) -> Answer {
    // Only `error` is removable here, as `upload.php` filters the list.
    let mut removable = vec![&b"error"[..]];
    removable.extend_from_slice(removed);
    let url = format!("{}{}", request.origin, request.uri);
    let canonical = bytes(&remove_query_arg(&removable, url.as_bytes()));
    let content = fill(
        GRID,
        &[
            (
                "search",
                esc_attr(args.get("s").unwrap_or_default().as_bytes()),
            ),
            (
                "add",
                bytes(&esc_url(
                    BarSite::admin_url(site, "media-new.php").as_bytes(),
                )),
            ),
            ("notice", notice.into()),
        ],
    );
    let admin = cookie_path(&BarSite::admin_url(site, ""));
    // `(object)` of no variables is an empty object.
    let vars = match query_vars(site, args, removed) {
        Value::Array(vars) if vars.is_empty() => "{}".to_string(),
        vars => json(&vars),
    };
    let admin = json(&text(admin.strip_suffix('/').unwrap_or(&admin)));
    let (templates, media_data) = enqueue_media(site, request.browser);
    let mut script_data = media_data.to_vec();
    script_data.push((
        "media-grid",
        format!("var _wpMediaGridSettings = {{\"adminUrl\":{admin},\"queryVars\":{vars}}};"),
    ));
    let mut scripts = MEDIA_SCRIPTS.to_vec();
    scripts.extend(["media-grid", "media"]);
    let page = Page {
        screen: Screen {
            pagenow: "upload.php",
            typenow: "attachment",
            taxonomy: "",
            parent_file: Some("upload.php"),
            submenu_file: None,
        },
        id: "upload",
        title: "Media Library",
        help: &GRID_HELP,
        sidebar: SIDEBAR,
        screen_options: "",
        styles: &MEDIA_STYLES,
        scripts: &scripts,
        late_scripts: &[],
        script_data: &script_data,
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: &templates,
        footer_scripts: "",
        content: &content,
        canonical: &canonical,
        origin: request.origin,
        view: None,
    };
    page_answer(site, &page, request)
}

fn page_answer(site: &dyn MediaSite, page: &Page, request: &Request) -> Answer {
    admin_page(site, page, request.browser).map_or(
        Answer::Denied("Sorry, you are not allowed to access this page."),
        Answer::Page,
    )
}

/// The list, after `current_action()` found no action to take.
fn list(
    site: &dyn MediaSite,
    args: &LibraryArgs,
    request: &Request,
    notice: &str,
    removed: &[&[u8]],
) -> Option<Answer> {
    let uri = bytes(&remove_query_arg(removed, request.uri.as_bytes()));
    let canonical = bytes(&remove_query_arg(removed, request.canonical.as_bytes()));
    let filter = args.get("attachment-filter").unwrap_or_default();
    let trash = filter == "trash";
    let detached = filter == "detached";
    let per_page = per_page(site, "upload_per_page", 20);
    let query = query(site, args, trash, per_page)?;
    let (posts, found) = site.list_posts(&query);
    // The site editor edits the templates and menus some files belong to.
    let site_edited = |post: &Post| {
        parent_of(site, post).is_some_and(|parent| SITE_EDITED.contains(&parent.post_type.as_str()))
    };
    if posts.iter().any(site_edited) {
        return None;
    }
    // `WP_Query` counts nothing when the page is past the last.
    let total = if posts.is_empty() { 0 } else { found };
    let columns = columns(site, detached);
    let listing = Listing {
        posts,
        total,
        pages: total.div_ceil(per_page),
        page: pagenum(args.get("paged")),
        per_page,
        trash,
        detached,
        filter: filter.into(),
        columns,
        hidden: hidden_columns(site, "upload"),
    };
    let rows = if listing.posts.is_empty() {
        let visible = listing
            .columns
            .iter()
            .filter(|column| !listing.hidden.iter().any(|h| h == column.key))
            .count();
        let in_trash = if trash { " in Trash" } else { "" };
        format!(
            "<tr class=\"no-items\"><td class=\"colspanchange\" colspan=\"{visible}\">No media files found{in_trash}.</td></tr>"
        )
    } else {
        listing
            .posts
            .iter()
            .map(|post| row(site, post, &listing))
            .collect()
    };
    let order = Order {
        orderby: args.get("orderby"),
        desc: args.get("order") == Some("desc"),
    };
    // The headers link to the request's URL, its removable arguments kept.
    let current = format!("{}{uri}", request.origin);
    let unpaged = bytes(&remove_query_arg(&[b"paged"], current.as_bytes()));
    let hidden: Vec<&str> = listing.hidden.iter().map(String::as_str).collect();
    let search = args.get("s").unwrap_or_default();
    let subtitle = if search.is_empty() {
        String::new()
    } else {
        let shown = if search.len() > 1600 {
            String::new()
        } else {
            esc_attr(search.replace(['\r', '\n'], "").as_bytes())
        };
        format!("<span class=\"subtitle\">Search results for: <strong>{shown}</strong></span>")
    };
    let mode = args
        .get("mode")
        .filter(|m| !is_empty_string(m.as_bytes()))
        .unwrap_or("list");
    let mut filters = if trash {
        String::new()
    } else {
        months_dropdown(
            &site.months("attachment", false),
            args.get("m").unwrap_or_default(),
            "Filter by date",
        )
    };
    filters.push_str("<input type=\"submit\" name=\"filter_action\" id=\"post-query-submit\" class=\"button button-compact\" value=\"Filter\"  />");
    if trash && !listing.posts.is_empty() && site.can("edit_others_posts") {
        filters.push_str("<input type=\"submit\" name=\"delete_all\" id=\"delete_all\" class=\"button apply button-compact\" value=\"Empty Trash\"  />");
    }
    let referer = remove_query_arg(&[b"_wp_http_referer"], uri.as_bytes());
    let top = format!(
        "<input type=\"hidden\" id=\"_wpnonce\" name=\"_wpnonce\" value=\"{}\" /><input type=\"hidden\" name=\"_wp_http_referer\" value=\"{}\" />{}",
        site.nonce("bulk-media"),
        bytes(&esc_url(&referer)),
        tablenav(&listing, "top", &canonical)
    );
    let bottom = if listing.posts.is_empty() {
        "\t\t".to_string()
    } else {
        format!("\t\t{}\t\t", tablenav(&listing, "bottom", &canonical))
    };
    let content = fill(
        LIST,
        &[
            (
                "add",
                bytes(&esc_url(
                    BarSite::admin_url(site, "media-new.php").as_bytes(),
                )),
            ),
            ("subtitle", subtitle),
            ("notice", notice.into()),
            ("switch", view_switch(mode, &uri)),
            ("types", type_options(site, &listing)),
            ("filters", filters),
            ("search", esc_attr(search.as_bytes())),
            ("top", top),
            ("mode", esc_attr(mode.as_bytes())),
            ("caption", table_description(&listing.columns, &order)),
            (
                "head",
                column_headers(&listing.columns, &hidden, &order, &unpaged, 1, true),
            ),
            ("rows", rows),
            (
                "foot",
                column_headers(&listing.columns, &hidden, &order, &unpaged, 2, false),
            ),
            ("bottom", bottom),
            ("find_posts", site.nonce("find-posts")),
        ],
    );
    let options = screen_options(
        &listing.columns,
        &listing.hidden,
        "upload_per_page",
        listing.per_page,
        "",
        site.nonce("screen-options-nonce"),
    );
    let page = Page {
        screen: Screen {
            pagenow: "upload.php",
            typenow: "attachment",
            taxonomy: "",
            parent_file: Some("upload.php"),
            submenu_file: None,
        },
        id: "upload",
        title: "Media Library",
        help: &LIST_HELP,
        sidebar: SIDEBAR,
        screen_options: &options,
        styles: &[],
        scripts: &["media"],
        late_scripts: &[],
        script_data: &[],
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical: &canonical,
        origin: request.origin,
        view: None,
    };
    Some(page_answer(site, &page, request))
}

/// `upload.php` for `args`; `None` for an action or an argument not
/// ported.
pub fn upload(site: &dyn MediaSite, args: &LibraryArgs, request: &Request) -> Option<Answer> {
    // `admin.php` turns away a user the menu does not let in first.
    if !site.can("upload_files") {
        return Some(Answer::Denied(
            "Sorry, you are not allowed to access this page.",
        ));
    }
    let (notice, removed) = notice(site, args);
    let stored = site.user_option("media_library_mode").unwrap_or_default();
    let mode = match args.get("mode") {
        Some(mode @ ("grid" | "list")) => mode.to_string(),
        _ if !Value::String(stored.clone()).is_empty() => bytes(&stored),
        _ => "grid".into(),
    };
    if mode == "grid" {
        return Some(grid(site, args, request, &notice, &removed));
    }
    if args
        .args
        .iter()
        .any(|(key, _)| !LIST_ARGS.contains(&key.as_str()))
    {
        return None;
    }
    let no_action = |name| {
        args.get(name)
            .is_none_or(|action| Value::String(action.into()).to_float() == -1.0)
    };
    if !args.given("filter_action") && !(no_action("action") && no_action("action2")) {
        return None;
    }
    if args.given("_wp_http_referer") {
        let uri = remove_query_arg(&[b"_wp_http_referer", b"_wpnonce"], request.uri.as_bytes());
        return Some(Answer::Redirect(bytes(&hyscribe_phpser::stripslashes(
            &uri,
        ))));
    }
    list(site, args, request, &notice, &removed)
}

#[cfg(test)]
mod tests;
