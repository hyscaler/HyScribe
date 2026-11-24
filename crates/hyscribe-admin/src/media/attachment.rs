// SPDX-License-Identifier: AGPL-3.0-or-later

//! An attachment's edit screen, `post.php?post=N&action=edit`:
//! `edit-form-advanced.php` with `edit_form_image_editor()` and meta boxes.

use std::cell::Cell;

use hyscribe_kses::{esc_attr, esc_html, esc_textarea, esc_url};
use hyscribe_model::{Post, User};
use hyscribe_phpser::{Float, Value, add_query_arg, is_empty_string, urldecode, wp_basename};
use hyscribe_render::attachment::is_image;
use hyscribe_render::attachment::prepend::{
    audio_shortcode, embedded, is_audio, is_video, video_shortcode,
};
use hyscribe_render::date::{self, Zone};
use hyscribe_render::link::front_page;
use hyscribe_render::permalinks::Permalinks;
use hyscribe_render::post::admin_title;
use hyscribe_render::template::replace;
use hyscribe_render::user_avatar;

use super::image::{attachment_image, attachment_url, image_src, metadata};
use super::{MEDIA_SCRIPTS, MEDIA_STYLES, MediaSite, enqueue_media, size_format};
use crate::admin_bar::{BarSite, Browser};
use crate::assets::cookie_path;
use crate::comments::{CommentsSite, comment_reply, editor_css, editor_footer};
use crate::dashboard::{Widget, listed, postbox, sorted};
use crate::list_table::{fill, wp_referer_field};
use crate::menu::Screen;
use crate::page::query_strings;
use crate::page::{HelpTab, Page, Request, admin_page, wp_admin_notice};
use crate::posts::{Answer, lock_holder, preview_link};
use crate::profile::{ProfileSite, wp_get_referer};
use crate::text;

const FORM: &str = include_str!("attachment.html");

const HELP: [HelpTab; 1] = [HelpTab {
    id: "overview",
    title: "Overview",
    content: "<p>This screen allows you to edit fields for metadata in a file within the media library.</p><p>For images only, you can click on Edit Image under the thumbnail to expand out an inline image editor with icons for cropping, rotating, or flipping the image as well as for undoing and redoing. The boxes on the right give you more options for scaling the image, for cropping it, and for cropping the thumbnail in a different way than you crop the original image. You can click on Help in those boxes to get more information.</p><p>Note that you crop the image by clicking on it (the Crop icon is already selected) and dragging the cropping frame to select the desired part. Then click Save to retain the cropping.</p><p>Remember to click Update to save metadata entered or changed.</p>",
}];

const SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/edit-media/\">Documentation on Edit Media</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// `wp_just_in_time_script_localization()`'s data of the word counter.
const WORD_COUNT: &str = "var wordCountL10n = {\"type\":\"words\",\"shortcodes\":[\"wp_caption\",\"caption\",\"gallery\",\"playlist\",\"audio\",\"video\",\"embed\"]};";

const ALT: &str = "\t\t\t<p class=\"attachment-alt-text\">\n\t\t\t<label for=\"attachment_alt\"><strong>Alternative Text</strong></label><br />\n\t\t\t<textarea class=\"widefat\" name=\"_wp_attachment_image_alt\" id=\"attachment_alt\" aria-describedby=\"alt-text-description\">%alt%</textarea>\n\t\t</p>\n\t\t<p class=\"attachment-alt-text-description\" id=\"alt-text-description\">\n\t\t<a href=\"https://www.w3.org/WAI/tutorials/images/decision-tree/\" target=\"_blank\">Learn how to describe the purpose of the image<span class=\"screen-reader-text\"> (opens in a new tab)</span><span aria-hidden=\"true\" class=\"dashicons dashicons-external\"></span></a>. Leave empty if the image is purely decorative.\t\t</p>\n\t\n";

/// The image types the oracle's image editor, Imagick, edits.
const EDITABLE: [&str; 9] = [
    "image/jpeg",
    "image/png",
    "image/gif",
    "image/webp",
    "image/avif",
    "image/heic",
    "image/bmp",
    "image/tiff",
    "image/x-icon",
];

/// The meta boxes hidden until the user shows them: `get_hidden_meta_boxes()`'s.
const HIDDEN: [&str; 8] = [
    "slugdiv",
    "trackbacksdiv",
    "postcustom",
    "postexcerpt",
    "commentstatusdiv",
    "commentsdiv",
    "authordiv",
    "revisionsdiv",
];

/// What `wp_attachment_is()` finds a file to be, in the order the screen
/// asks.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Image,
    Audio,
    Video,
    File,
}

impl Kind {
    fn of(file: &[u8], mime: &str) -> Kind {
        if is_image(file, mime) {
            Kind::Image
        } else if is_audio(file, mime) {
            Kind::Audio
        } else if is_video(file, mime) {
            Kind::Video
        } else {
            Kind::File
        }
    }

    /// Whether the screen has featured images and the media library.
    fn plays(self) -> bool {
        matches!(self, Kind::Audio | Kind::Video)
    }
}

/// What the screen asks of the site: the media's, the comments' and the
/// users'.
pub trait AttachmentSite: MediaSite + CommentsSite + ProfileSite {}

impl<T: MediaSite + CommentsSite + ProfileSite> AttachmentSite for T {}

/// The screen's arguments: `post.php`'s post and the notice a save sends
/// back to it, and the referer the browser sent.
pub struct AttachmentArgs {
    /// `$_GET['post']`.
    pub post: String,
    /// `$_GET['message']`, if given.
    pub message: Option<String>,
    /// The `Referer` header, if sent.
    pub referer: Option<String>,
}

impl AttachmentArgs {
    /// The arguments of the query string `query`, `None` unless it asks to
    /// edit a post and takes no argument that is not ported.
    pub fn parse(query: &str, referer: Option<String>) -> Option<AttachmentArgs> {
        let (mut post, mut message, mut edit) = (None, None, false);
        for (key, value) in query_strings(query)? {
            match key.as_str() {
                "post" => post = Some(value),
                "message" => message = Some(value),
                "action" => edit = value == "edit",
                _ => return None,
            }
        }
        let post = post.filter(|_| edit)?;
        Some(AttachmentArgs {
            post,
            message,
            referer,
        })
    }
}

/// `get_post_status()` of an attachment: its parent's (before the Trash,
/// as its trash meta keeps it), `publish` without one, or its own private
/// status.
fn post_status(site: &dyn AttachmentSite, post: &Post) -> String {
    if post.status == "private" {
        return post.status.clone();
    }
    let parent = (post.parent != post.id).then(|| site.post(post.parent));
    match parent.flatten() {
        None => "publish".into(),
        Some(parent) if parent.status == "trash" => {
            let kept = site.meta(&parent, "_wp_trash_meta_status");
            let kept = kept.filter(|status| !Value::String(status.clone()).is_empty());
            kept.map_or_else(|| "publish".into(), |status| text(&status))
        }
        Some(parent) => parent.status,
    }
}

/// `user_can_richedit()`: the user's choice, in a browser the visual
/// editor works in.
fn rich_editing(site: &dyn AttachmentSite, browser: &Browser) -> bool {
    let agent = String::from_utf8_lossy(browser.user_agent);
    let lower = agent.to_ascii_lowercase();
    let webkit = || {
        let version = agent
            .split_once("AppleWebKit/")
            .map_or("", |(_, version)| version);
        let digits: String = version.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().is_ok_and(|version| version >= 534)
    };
    // `wp-includes/vars.php`'s browser, in its order; Chrome Frame is Chrome
    // in the admin.
    let capable = if agent.contains("Lynx") {
        false
    } else if agent.contains("Edg") {
        true
    } else if agent.contains("Opera") || agent.contains("OPR/") {
        !browser.is_mobile()
    } else if lower.contains("chrome") {
        true
    } else if lower.contains("safari") {
        !browser.is_mobile() || webkit()
    } else if (agent.contains("MSIE") || agent.contains("Trident")) && agent.contains("Win") {
        agent.contains("Trident/7.0;")
    } else {
        !(agent.contains("MSIE") && agent.contains("Mac")) && agent.contains("Gecko")
    };
    capable && site.user_option("rich_editing").as_deref() == Some(b"true")
}

/// The attachment's preview: its image, with the image editor's button
/// when the editor supports its type, its player, or its type's icon; and
/// whether a player plays it.
fn preview(site: &dyn AttachmentSite, post: &Post, kind: Kind, meta: &Value) -> (String, bool) {
    let id = post.id;
    let url = attachment_url(site, post).unwrap_or_default();
    let player = match kind {
        Kind::Audio => Some(audio_shortcode(id, &url, &Cell::new(0)).ok_or("audio")),
        Kind::Video => {
            let side = |key| {
                meta.get_str(key)
                    .filter(|value| !value.is_empty())
                    .map_or(0, Value::to_int)
            };
            let poster = featured(site, post)
                .and_then(|thumbnail| attachment_url(site, &thumbnail))
                .unwrap_or_default();
            let size = (side("width"), side("height"));
            Some(video_shortcode(id, &url, size, &poster).ok_or("video"))
        }
        Kind::Image | Kind::File => None,
    };
    if let Some(player) = player {
        let played = player.is_ok();
        let html = player.unwrap_or_else(|kind| embedded(kind, &url));
        return (format!("\t{}", String::from_utf8_lossy(&html)), played);
    }
    let (src, _, _) = image_src(site, post, (900, 450));
    let src = String::from_utf8_lossy(&src).into_owned();
    if kind == Kind::File {
        let html = format!(
            "\t\t\t<div class=\"wp_attachment_image wp-clearfix\" id=\"media-head-{id}\">\n\t\t\t<p id=\"thumbnail-head-{id}\">\n\t\t\t\t<img class=\"thumbnail\" src=\"{src}\" style=\"max-width:100%\" alt=\"\" />\n\t\t\t</p>\n\t\t</div>\n\t\t"
        );
        return (html, false);
    }
    let button = if EDITABLE.contains(&post.mime_type.as_str()) {
        format!(
            "<input type='button' id='imgedit-open-btn-{id}' onclick='imageEdit.open( {id}, \"{}\" )' class='button' value='Edit Image' /> <span class='spinner'></span>",
            site.nonce(&format!("image_editor-{id}"))
        )
    } else {
        String::new()
    };
    let html = format!(
        "\t\t\t<div class=\"imgedit-response\" id=\"imgedit-response-{id}\"></div>\n\n\t\t<div class=\"wp_attachment_image wp-clearfix\" id=\"media-head-{id}\">\n\t\t\t<p id=\"thumbnail-head-{id}\"><img class=\"thumbnail\" src=\"{src}\" style=\"max-width:100%\" alt=\"\" /></p>\n\t\t\t<p>{button}</p>\n\t\t</div>\n\t\t<div style=\"display:none\" class=\"image-editor\" id=\"image-editor-{id}\">\n\t\t\t\t</div>\n\t\t"
    );
    (html, false)
}

/// The stored `_thumbnail_id`, unless empty.
fn thumbnail_id(site: &dyn AttachmentSite, post: &Post) -> Option<Vec<u8>> {
    let stored = site.meta(post, "_thumbnail_id")?;
    (!Value::String(stored.clone()).is_empty()).then_some(stored)
}

/// The post `get_post_thumbnail_id()` names, if there is one.
fn featured(site: &dyn AttachmentSite, post: &Post) -> Option<Post> {
    let thumbnail = Value::String(thumbnail_id(site, post)?).to_int();
    u64::try_from(thumbnail).ok().and_then(|id| site.post(id))
}

/// Whether `id` is the page on the front.
fn on_front(site: &dyn AttachmentSite, id: u64) -> bool {
    let option = |name| site.option(name).unwrap_or_default();
    front_page(&option("show_on_front"), &option("page_on_front")) == Some(id)
}

/// A song's or a video's type, as `preg_match( '#^(audio|video)/#' )` finds it.
fn plays_by_type(post: &Post) -> bool {
    ["audio/", "video/"]
        .iter()
        .any(|group| post.mime_type.starts_with(group))
}

/// `post_thumbnail_meta_box()`, `None` for a featured file that is not an
/// image, whose image is not ported.
fn featured_box(site: &dyn AttachmentSite, post: &Post) -> Option<String> {
    let link = esc_url(
        BarSite::admin_url(
            site,
            &format!(
                "media-upload.php?post_id={}&type=image&TB_iframe=1",
                post.id
            ),
        )
        .as_bytes(),
    );
    let link = String::from_utf8_lossy(&link);
    let image = match featured(site, post) {
        Some(thumbnail) if thumbnail.post_type == "attachment" => {
            let file = site
                .meta(&thumbnail, "_wp_attached_file")
                .unwrap_or_default();
            if !is_image(&file, &thumbnail.mime_type) {
                return None;
            }
            Some(attachment_image(site, &thumbnail, (266, 266)))
        }
        _ => None,
    };
    let anchor = |described: &str, text: &str| {
        format!(
            "<p class=\"hide-if-no-js\"><a href=\"{link}\" id=\"set-post-thumbnail\"{described} class=\"thickbox\" role=\"button\" aria-haspopup=\"dialog\" aria-controls=\"wp-media-modal\">{text}</a></p>"
        )
    };
    let mut html = match image {
        Some(image) => format!(
            "{}<p class=\"hide-if-no-js howto\" id=\"set-post-thumbnail-desc\">Click the image to edit or update</p><p class=\"hide-if-no-js\"><a href=\"#\" id=\"remove-post-thumbnail\" role=\"button\">Remove featured image</a></p>",
            anchor(" aria-describedby=\"set-post-thumbnail-desc\"", &image)
        ),
        None => anchor("", "Set featured image"),
    };
    let value = thumbnail_id(site, post).unwrap_or(b"-1".to_vec());
    html.push_str(&format!(
        "<input type=\"hidden\" id=\"_thumbnail_id\" name=\"_thumbnail_id\" value=\"{}\" />",
        esc_attr(&value)
    ));
    Some(html)
}

/// `attachment_id3_data_meta_box()`.
fn id3_box(meta: &Value) -> String {
    [("artist", "Artist"), ("album", "Album")]
        .iter()
        .map(|(key, label)| {
            let value = meta.get_str(key).filter(|value| !value.is_empty());
            let value = value.map(Value::to_php_string).unwrap_or_default();
            format!(
                "\t<p>\n\t\t<label for=\"title\">{label}</label><br />\n\t\t<input type=\"text\" name=\"id3_{key}\" id=\"id3_{key}\" class=\"large-text\" value=\"{}\" />\n\t</p>\n\t\t",
                esc_attr(&value)
            )
        })
        .collect()
}

/// `human_readable_duration()`: `[H:]M:S` in words, empty for any other.
pub fn human_readable_duration(duration: &Value) -> String {
    let Value::String(duration) = duration else {
        return String::new();
    };
    let duration = String::from_utf8_lossy(duration.trim_ascii());
    let duration = duration.strip_prefix('-').unwrap_or(&duration);
    let parts: Vec<&str> = duration.split(':').collect();
    let digits = |part: &str, sixty: bool| {
        !part.is_empty()
            && part.bytes().all(|b| b.is_ascii_digit())
            && (!sixty
                || part.len() == 1
                || part.len() == 2 && part.as_bytes().first() < Some(&b'6'))
    };
    let valid = match parts[..] {
        [hour, minute, second] => {
            digits(hour, false) && digits(minute, true) && digits(second, true)
        }
        [minute, second] => digits(minute, true) && digits(second, true),
        _ => false,
    };
    if !valid {
        return String::new();
    }
    let units = [
        ("hour", "hours"),
        ("minute", "minutes"),
        ("second", "seconds"),
    ];
    parts
        .iter()
        .zip(units.iter().skip(3 - parts.len()))
        .map(|(part, (one, many))| {
            let n = Value::String(part.as_bytes().to_vec()).to_int();
            format!("{n} {}", if n == 1 { one } else { many })
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `attachment_submitbox_metadata()`'s length, bitrate and audio details
/// of a song or video.
fn media_details(meta: &Value) -> String {
    let mut out = String::new();
    for (key, label) in [("length_formatted", "Length:"), ("bitrate", "Bitrate:")] {
        let Some(value) = meta.get_str(key).filter(|value| !value.is_empty()) else {
            continue;
        };
        let shown = if key == "bitrate" {
            let rate = Value::Float(Float::from_f64((value.to_float() / 1000.0).round()));
            let rate = rate.to_php_string();
            let mode = meta.get_str("bitrate_mode").filter(|mode| !mode.is_empty());
            let mode = mode.map_or(String::new(), |mode| {
                format!(" {}", esc_html(&mode.to_php_string()).to_ascii_uppercase())
            });
            format!("{}kb/s{mode}", String::from_utf8_lossy(&rate))
        } else {
            human_readable_duration(value)
        };
        out.push_str(&format!(
            "\t\t\t<div class=\"misc-pub-section misc-pub-mime-meta misc-pub-{key}\">\n\t\t\t\t{label}\t\t\t\t<strong>\n\t\t\t\t{shown}\t\t\t\t</strong>\n\t\t\t</div>\n\t\t\t"
        ));
    }
    for (key, label) in [("dataformat", "Audio Format:"), ("codec", "Audio Codec:")] {
        let value = meta
            .get_path(&["audio", key])
            .filter(|value| !value.is_empty());
        if let Some(value) = value {
            out.push_str(&format!(
                "\t\t\t<div class=\"misc-pub-section misc-pub-audio misc-pub-{key}\">\n\t\t\t\t{label} <strong>{}</strong>\n\t\t\t</div>\n\t\t\t",
                esc_html(&value.to_php_string())
            ));
        }
    }
    out
}

/// `get_permalink()` of an attachment's parent, `_get_page_link()` of a
/// page; `None` for the page on the front, not ported.
fn parent_link(site: &dyn AttachmentSite, parent: &Post) -> Option<String> {
    if parent.post_type == "page" && on_front(site, parent.id) {
        return None;
    }
    // A private parent of a shown attachment, which the user may read, has
    // the link it would have published.
    if parent.status == "private" {
        let published = Post {
            status: "publish".into(),
            ..parent.clone()
        };
        return Some(site.permalink(&published));
    }
    Some(site.permalink(parent))
}

/// `get_attachment_link()` of an attachment of `status`, and the sample with
/// `%postname%` left in; `None` where [`parent_link`] is, or under a file.
fn attachment_links(
    site: &dyn AttachmentSite,
    post: &Post,
    status: &str,
) -> Option<(String, String)> {
    let plain = site.permalink(post);
    let structure = site.option("permalink_structure").unwrap_or_default();
    if is_empty_string(&structure) {
        return Some((plain.clone(), plain));
    }
    let parent = (post.parent != 0).then(|| site.post(post.parent)).flatten();
    let attached = |parent: &Post| parent.id != post.id && parent.post_type == "attachment";
    if parent.as_ref().is_some_and(attached) {
        return None;
    }
    // `wp_force_plain_post_permalink()`, and a parent that is there.
    let shown = status == "publish" || status == "private" && site.can_post("read_post", post);
    let viewable = |parent: &Post| {
        parent.id != post.id && ["post", "page"].contains(&parent.post_type.as_str())
    };
    let valid = post.parent == 0 || parent.as_ref().is_some_and(viewable);
    if !shown || !valid {
        return Some((plain.clone(), plain));
    }
    let parent_link = match parent {
        Some(parent) => Some(parent_link(site, &parent)?),
        None => None,
    };
    let option = |name| text(&site.option(name).unwrap_or_default());
    let links = Permalinks::new(
        &text(&structure),
        &option("category_base"),
        &option("tag_base"),
    );
    let home = BarSite::home_url(site);
    let home = home.strip_suffix('/').unwrap_or(&home);
    let link = |sample| {
        let link = links.attachment_link(home, post, parent_link.as_deref(), sample);
        link.unwrap_or_else(|| plain.clone())
    };
    Some((link(false), link(true)))
}

/// `wp_unique_post_slug()` of an attachment's slug: suffixed while another
/// post has it; `None` where `_truncate_post_slug()` would cut it, not ported.
fn unique_slug(site: &dyn AttachmentSite, post: &Post) -> Option<Vec<u8>> {
    let slug = &post.name;
    let reserved = ["feed", "rdf", "rss", "rss2", "atom", "embed"]
        .iter()
        .any(|name| name.as_bytes() == slug.as_slice());
    if !reserved && !site.slug_taken(slug, post.id) {
        return Some(slug.clone());
    }
    (2u64..)
        .find_map(|n| {
            let suffix = format!("-{n}");
            if slug.len() + suffix.len() > 200 {
                return Some(None);
            }
            let alt = [&slug[..], suffix.as_bytes()].concat();
            (!site.slug_taken(&alt, post.id)).then_some(Some(alt))
        })
        .flatten()
}

/// `get_sample_permalink_html()` of the attachment's `view` and sample
/// links; `None` where [`unique_slug`] is, or editable without a slug.
fn sample_permalink(
    site: &dyn AttachmentSite,
    post: &Post,
    (view, permalink): (&str, &str),
) -> Option<String> {
    let (view, target) = match site.can_post("read_post", post) {
        false => (None, String::new()),
        true if is_empty_string(&post.name) => {
            let preview = add_query_arg(&[(b"preview", Some(b"true"))], view.as_bytes());
            (
                Some(text(&preview)),
                format!(" target='wp-preview-{}'", post.id),
            )
        }
        true => (Some(view.to_string()), String::new()),
    };
    let mut html = String::from("<strong>Permalink:</strong>\n");
    if !permalink.contains("%postname%") {
        match view {
            Some(link) => html.push_str(&format!(
                "<a id=\"sample-permalink\" href=\"{}\"{target}>{}</a>\n",
                text(&esc_url(link.as_bytes())),
                esc_html(&urldecode(link.as_bytes()))
            )),
            None => html.push_str(&format!(
                "<span id=\"sample-permalink\">{permalink}</span>\n"
            )),
        }
        let plain = site.option("permalink_structure").unwrap_or_default();
        if is_empty_string(&plain) && site.can("manage_options") && !on_front(site, post.id) {
            html.push_str("<span id=\"change-permalinks\"><a href=\"options-permalink\" class=\"button button-small\">Change Permalink Structure</a></span>\n");
        }
        return Some(html);
    }
    if is_empty_string(&post.name) {
        return None;
    }
    let name = text(&urldecode(&unique_slug(site, post)?));
    let chars: Vec<char> = name.chars().collect();
    let abridged = if chars.len() > 34 {
        let start: String = chars.iter().take(16).collect();
        let end: String = chars.iter().skip(chars.len() - 16).collect();
        format!("{start}&hellip;{end}")
    } else {
        name.clone()
    };
    let display = esc_html(&urldecode(permalink.as_bytes())).replace(
        "%postname%",
        &format!(
            "<span id=\"editable-post-name\">{}</span>",
            esc_html(abridged.as_bytes())
        ),
    );
    html.push_str(&format!(
        "<span id=\"sample-permalink\"><a href=\"{}\"{target}>{display}</a></span>\n&lrm;<span id=\"edit-slug-buttons\"><button type=\"button\" class=\"edit-slug button button-small hide-if-no-js\" aria-label=\"Edit permalink\">Edit</button></span>\n<span id=\"editable-post-name-full\">{}</span>\n",
        text(&esc_url(view.unwrap_or_default().as_bytes())),
        esc_html(name.as_bytes())
    ));
    Some(html)
}

/// The extension `/^.*?\.(\w+)$/` finds in a file's name.
fn extension(name: &[u8]) -> Option<&[u8]> {
    let dot = name.iter().rposition(|b| *b == b'.')?;
    let ext = name.get(dot + 1..).unwrap_or_default();
    let word = |b: &u8| b.is_ascii_alphanumeric() || *b == b'_';
    (!ext.is_empty() && ext.iter().all(word)).then_some(ext)
}

/// `attachment_submitbox_metadata()`'s uploader, parent and file details.
fn file_details(site: &dyn AttachmentSite, post: &Post, meta: &Value, size: i64) -> String {
    let id = post.id;
    let uploader = match site.profile_user(post.author) {
        Some(user) => {
            let name = if is_empty_string(&user.display_name) {
                site.user_option_of(user.id, "nickname").unwrap_or_default()
            } else {
                user.display_name
            };
            let name = String::from_utf8_lossy(&name).into_owned();
            if !site.can_for("edit_user", user.id) {
                format!("<strong>{name}</strong>")
            } else if user.id == site.user_id() {
                let link = BarSite::admin_url(site, "profile.php");
                format!("<a href=\"{link}\"><strong>{name}</strong></a>")
            } else {
                let link = BarSite::admin_url(site, &format!("user-edit.php?user_id={}", user.id));
                format!("<a href=\"{link}\"><strong>{name}</strong></a>")
            }
        }
        None => "<strong>(no author)</strong>".into(),
    };
    let mut out = format!(
        "\t\t<div class=\"misc-pub-section misc-pub-uploadedby word-wrap-break-word\">\n\t\t\t\t\tUploaded by: {uploader}\n\t\t\t</div>\n\n\t"
    );
    let parent = (post.parent != 0).then(|| site.post(post.parent)).flatten();
    if let Some(parent) = parent.filter(|parent| site.can_post("read_post", parent)) {
        let title = if is_empty_string(&parent.title) {
            "(no title)".to_string()
        } else {
            String::from_utf8_lossy(&parent.title).into_owned()
        };
        let link = if site.can_post("edit_post", &parent) {
            let link =
                BarSite::admin_url(site, &format!("post.php?post={}&action=edit", parent.id));
            format!("<a href=\"{link}\"><strong>{title}</strong></a>")
        } else {
            format!("<strong>{title}</strong>")
        };
        out.push_str(&format!(
            "\t\t\t<div class=\"misc-pub-section misc-pub-uploadedto\">\n\t\t\t\t\t\t\t\t\tUploaded to: {link}\n\t\t\t\t\t\t\t</div>\n\t\t\t"
        ));
    }
    let url = attachment_url(site, post).unwrap_or_default();
    let file = site.meta(post, "_wp_attached_file").unwrap_or_default();
    let name = wp_basename(&file);
    let kind = match extension(&name) {
        Some(ext) => {
            let mut kind = esc_html(&ext.to_ascii_uppercase());
            let group = post.mime_type.split('/').next().unwrap_or_default();
            let stored = meta
                .get_str("mime_type")
                .map(Value::to_php_string)
                .unwrap_or_default();
            let named = format!(
                "{group}/{}",
                String::from_utf8_lossy(&ext.to_ascii_lowercase())
            );
            if group != "image" && !is_empty_string(&stored) && named.as_bytes() != stored {
                kind.push_str(&format!(" ({})", String::from_utf8_lossy(&stored)));
            }
            kind
        }
        None => post.mime_type.replace("image/", "").to_ascii_uppercase(),
    };
    out.push_str(&format!(
        "\n\t<div class=\"misc-pub-section misc-pub-attachment\">\n\t\t<label for=\"attachment_url\">File URL:</label>\n\t\t<input type=\"text\" class=\"widefat urlfield ltr\" readonly=\"readonly\" name=\"attachment_url\" id=\"attachment_url\" value=\"{}\" />\n\t\t<span class=\"copy-to-clipboard-container\">\n\t\t\t<button type=\"button\" class=\"button copy-attachment-url edit-media\" data-clipboard-target=\"#attachment_url\">Copy URL to clipboard</button>\n\t\t\t<span class=\"success hidden\" aria-hidden=\"true\">Copied!</span>\n\t\t</span>\n\t</div>\n\t<div class=\"misc-pub-section misc-pub-download\">\n\t\t<a href=\"{}\" download>Download file</a>\n\t</div>\n\t<div class=\"misc-pub-section misc-pub-filename\">\n\t\tFile name: <strong>{}</strong>\n\t</div>\n\t<div class=\"misc-pub-section misc-pub-filetype\">\n\t\tFile type:\t\t<strong>\n\t\t{kind}\t\t</strong>\n\t</div>\n\n\t\t\t<div class=\"misc-pub-section misc-pub-filesize\">\n\t\t\tFile size: <strong>{}</strong>\n\t\t</div>\n\t\t",
        esc_attr(&url),
        String::from_utf8_lossy(&esc_url(&url)),
        esc_html(&name),
        size_format(size)
    ));
    if plays_by_type(post) {
        out.push_str(&media_details(meta));
    }
    if let (Some(width), Some(height)) = (meta.get_str("width"), meta.get_str("height")) {
        out.push_str(&format!(
            "\t\t<div class=\"misc-pub-section misc-pub-dimensions\">\n\t\t\tDimensions: <strong><span id='media-dims-{id}'>{} by {} pixels</span></strong>\n\t\t</div>\n\t\t",
            String::from_utf8_lossy(&width.to_php_string()),
            String::from_utf8_lossy(&height.to_php_string())
        ));
    }
    let original = meta.get_str("original_image").map(Value::to_php_string);
    if let Some(original) = original.filter(|original| !is_empty_string(original)) {
        // `wp_get_original_image_url()` and its path; only images get here.
        let link = replace(&url, &wp_basename(&url), &original);
        let name = wp_basename(&original);
        out.push_str(&format!(
            "		<div class=\"misc-pub-section misc-pub-original-image word-wrap-break-word\">\n\t\t\tOriginal image:\t\t\t<a href=\"{}\">\n\t\t\t\t<strong>{}</strong>\n\t\t\t</a>\n\t\t</div>\n\t\t",
            String::from_utf8_lossy(&esc_url(&link)),
            esc_html(&name)
        ));
    }
    out
}

/// `attachment_submit_meta_box()`.
fn submit_box(site: &dyn AttachmentSite, post: &Post, meta: &Value, size: i64) -> String {
    let uploaded = format!(
        "{} at {}",
        date::format("M j, Y", &post.date, Zone::Utc),
        date::format("H:i", &post.date, Zone::Utc)
    );
    let delete = if site.can_post("delete_post", post) {
        let link = BarSite::admin_url(
            site,
            &format!(
                "post.php?post={}&amp;action=delete&amp;_wpnonce={}",
                post.id,
                site.nonce(&format!("delete-post_{}", post.id))
            ),
        );
        format!(
            "<a class=\"submitdelete deletion\" onclick='return showNotice.warn();' href=\"{link}\">Delete permanently</a>"
        )
    } else {
        String::new()
    };
    format!(
        "<div class=\"submitbox\" id=\"submitpost\">\n\n<div id=\"minor-publishing\">\n\n\t<div style=\"display:none;\">\n\t<p class=\"submit\"><input type=\"submit\" name=\"save\" id=\"save\" class=\"button\" value=\"Save\"  /></p></div>\n\n\n<div id=\"misc-publishing-actions\">\n\t<div class=\"misc-pub-section curtime misc-pub-curtime\">\n\t\t<span id=\"timestamp\">\n\t\t\tUploaded on: <b>{uploaded}</b>\t\t</span>\n\t</div><!-- .misc-pub-section -->\n\n{}</div><!-- #misc-publishing-actions -->\n<div class=\"clear\"></div>\n</div><!-- #minor-publishing -->\n\n<div id=\"major-publishing-actions\">\n\t<div id=\"delete-action\">\n\t{delete}\t</div>\n\n\t<div id=\"publishing-action\">\n\t\t<span class=\"spinner\"></span>\n\t\t<input name=\"original_publish\" type=\"hidden\" id=\"original_publish\" value=\"Update\" />\n\t\t<input name=\"save\" type=\"submit\" class=\"button button-primary button-large\" id=\"publish\" value=\"Update\" />\n\t</div>\n\t<div class=\"clear\"></div>\n</div><!-- #major-publishing-actions -->\n\n</div>\n\n\t",
        file_details(site, post, meta, size)
    )
}

/// `checked( $value, 'open' )`.
fn checked(value: &str) -> &'static str {
    if value == "open" {
        " checked='checked'"
    } else {
        ""
    }
}

/// `post_comment_status_meta_box()`.
fn discussion_box(post: &Post) -> String {
    format!(
        "<input name=\"advanced_view\" type=\"hidden\" value=\"1\" />\n<p class=\"meta-options\">\n\t<label for=\"comment_status\" class=\"selectit\"><input name=\"comment_status\" type=\"checkbox\" id=\"comment_status\" value=\"open\" {} /> Allow comments</label><br />\n\t<label for=\"ping_status\" class=\"selectit\"><input name=\"ping_status\" type=\"checkbox\" id=\"ping_status\" value=\"open\" {} />\n\t\tAllow <a href=\"https://wordpress.org/documentation/article/introduction-to-blogging/#managing-comments\">trackbacks and pingbacks</a>\t</label>\n\t</p>\n\t",
        checked(&post.comment_status),
        checked(&post.ping_status)
    )
}

/// `post_comment_meta_box()`, with the list its scripts load when `shown`;
/// `trash_notice` is `wp_comment_trashnotice()`.
fn comments_box(
    site: &dyn AttachmentSite,
    post: &Post,
    request: &Request,
    shown: bool,
    trash_notice: &str,
) -> String {
    let total = site.post_comment_counts(post.id).all();
    let more = if total < 1 {
        "\t\t<p id=\"no-comments\">No comments yet.</p>".to_string()
    } else {
        let script = if shown {
            format!(
                "\t\t\t\t\t<script>jQuery(function(){{commentsBox.get({total}, 10);}});</script>\n\t"
            )
        } else {
            String::new()
        };
        format!(
            "{script}\t\t\t\t<p class=\"hide-if-no-js\" id=\"show-comments\"><a href=\"#commentstatusdiv\" onclick=\"commentsBox.load({total});return false;\">Show comments</a> <span class=\"spinner\"></span></p>\n\t\t"
        )
    };
    format!(
        "<input type=\"hidden\" id=\"add_comment_nonce\" name=\"add_comment_nonce\" value=\"{}\" />\t<p class=\"hide-if-no-js\" id=\"add-new-comment\"><button type=\"button\" class=\"button\" onclick=\"window.commentReply && commentReply.addcomment({});\">Add Comment</button></p>\n\t<input type=\"hidden\" id=\"_ajax_fetch_list_nonce\" name=\"_ajax_fetch_list_nonce\" value=\"{}\" />{}<table class=\"widefat fixed striped table-view-list comments wp-list-table comments-box\" style=\"display:none;\">\n\t<tbody id=\"the-comment-list\"\n\t\t data-wp-lists='list:comment'\t\t>\n\t\t\t</tbody>\n</table>\n{more}{trash_notice}\t",
        site.nonce("get-comments"),
        post.id,
        site.nonce("fetch-list-WP_Post_Comments_List_Table"),
        wp_referer_field(request.uri)
    )
}

/// `post_author_meta_box()`: the users who may edit posts, and the author.
fn author_box(site: &dyn AttachmentSite, post: &Post) -> String {
    let mut users = site.authors("edit_posts");
    if !users.iter().any(|user| user.id == post.author) {
        users.extend(site.profile_user(post.author));
    }
    let options: String = users
        .iter()
        .map(|user| {
            let label = [&user.display_name[..], b" (", &user.login, b")"].concat();
            let selected = if user.id == post.author {
                " selected='selected'"
            } else {
                ""
            };
            format!(
                "\t<option value='{}'{selected}>{}</option>\n",
                user.id,
                esc_html(&label)
            )
        })
        .collect();
    format!(
        "<label class=\"screen-reader-text\" for=\"post_author_override\">\n\tAuthor</label>\n\t<select name='post_author_override' id='post_author_override' class=''>\n{options}</select>"
    )
}

/// `_admin_notice_post_locked()` for a lock `holder` has, the user
/// sent back to `referer` when they came from another screen.
fn lock_dialog(
    site: &dyn AttachmentSite,
    post: &Post,
    (holder, view): (&User, &str),
    referer: Option<&str>,
) -> String {
    // The editors, `post.php` and `post-new.php`, are at `/hs-admin/post` here.
    let editor = |r: &&str| {
        ["post.php", "post-new.php", "/hs-admin/post"]
            .iter()
            .any(|e| r.contains(e))
    };
    let (back, text) = match referer.filter(|r| !editor(r)) {
        Some(referer) => (referer.to_string(), "Go back"),
        None => (
            BarSite::admin_url(site, "edit.php?post_type=attachment"),
            "Media",
        ),
    };
    let (id, nonce) = (
        post.id.to_string(),
        site.nonce(&format!("post_preview_{}", post.id)),
    );
    let mut args: Vec<(&[u8], Option<&[u8]>)> = Vec::new();
    if holder.id != post.author {
        args.push((b"preview_id", Some(id.as_bytes())));
        args.push((b"preview_nonce", Some(nonce.as_bytes())));
    }
    let preview = preview_link(site, view, &args);
    let take_over = BarSite::admin_url(
        site,
        &format!(
            "post.php?post={}&action=edit&_wpnonce={}&get-post-lock=1",
            post.id,
            site.nonce(&format!("lock-post_{}", post.id))
        ),
    );
    let avatar = user_avatar(&|name| site.option(name), holder, 64);
    format!(
        "\t<div id=\"post-lock-dialog\" class=\"notification-dialog-wrap\">\n\t<div class=\"notification-dialog-background\"></div>\n\t<div class=\"notification-dialog\">\n\t\t\t<div class=\"post-locked-message\">\n\t\t<div class=\"post-locked-avatar\">{}</div>\n\t\t<p class=\"currently-editing wp-tab-first\" tabindex=\"0\">\n\t\t{} is currently editing this post. Do you want to take over?\t\t</p>\n\t\t\t\t<p>\n\t\t<a class=\"button\" href=\"{}\">{text}</a>\n\t\t\t\t<a class=\"button\" href=\"{}\">Preview</a>\n\t\t\t\t<a class=\"button button-primary wp-tab-last\" href=\"{}\">Take over</a>\n\t\t\t\t\t</p>\n\t\t</div>\n\t\t\t</div>\n\t</div>\n\t",
        String::from_utf8_lossy(&avatar),
        esc_html(&holder.display_name),
        String::from_utf8_lossy(&esc_url(back.as_bytes())),
        String::from_utf8_lossy(&esc_url(&preview)),
        String::from_utf8_lossy(&esc_url(take_over.as_bytes())),
    )
}

/// The screen for the attachment `args` names, or PHP's refusals for a
/// post or page: `None` for what is not ported, which a view that would
/// write its own edit lock and the block editor's page are.
pub fn attachment_edit(
    site: &dyn AttachmentSite,
    args: &AttachmentArgs,
    request: &Request,
) -> Option<Answer> {
    let id = Value::String(args.post.clone().into_bytes()).to_int();
    if id == 0 {
        return None;
    }
    let Some(post) = u64::try_from(id).ok().and_then(|id| site.post(id)) else {
        return Some(Answer::Missing(
            "You attempted to edit an item that does not exist. Perhaps it was deleted?",
        ));
    };
    // The block editor's page is not ported, but its refusals are.
    let editor = ["post", "page"].contains(&post.post_type.as_str());
    if post.post_type != "attachment" && !editor {
        return None;
    }
    if !site.can_post("edit_post", &post) {
        return Some(Answer::Denied(
            "Sorry, you are not allowed to edit this item.",
        ));
    }
    if post.status == "trash" {
        return Some(Answer::Conflict(
            "You cannot edit this item because it is in the Trash. Please restore it and try again.",
        ));
    }
    if editor {
        return None;
    }
    let meta = metadata(site, &post);
    let size = meta.get_str("filesize").map_or(0, Value::to_int);
    // Not ported: a file size read from disk, a status but inherit or
    // private, and a scaled original of a file that is not an image.
    let file = site.meta(&post, "_wp_attached_file").unwrap_or_default();
    let kind = Kind::of(&file, &post.mime_type);
    let original = meta
        .get_str("original_image")
        .is_some_and(|o| !o.is_empty());
    let unported = !["inherit", "private"].contains(&post.status.as_str())
        || original && kind != Kind::Image
        || size <= 0;
    if unported {
        return None;
    }
    let status = post_status(site, &post);
    let holder = lock_holder(site, &post)?;
    let (view, sample) = attachment_links(site, &post, &status)?;
    let state = State {
        meta,
        size,
        kind,
        status,
        holder,
        view,
        sample,
    };
    page(site, args, request, &post, &state)
}

/// What the screen shows of an attachment beyond its post: metadata, size,
/// kind, `get_post_status()`, the lock's holder, its link and sample link.
struct State {
    meta: Value,
    size: i64,
    kind: Kind,
    status: String,
    holder: User,
    view: String,
    sample: String,
}

/// The form, its meta boxes and the lock dialog; `None` where the parts are.
fn page(
    site: &dyn AttachmentSite,
    args: &AttachmentArgs,
    request: &Request,
    post: &Post,
    state: &State,
) -> Option<Answer> {
    let (id, meta, size) = (post.id, &state.meta, state.size);
    // Without the right to publish, a pending attachment's slug is not shown.
    let slug_shown = state.status != "pending" || site.can("publish_posts");
    let image = state.kind == Kind::Image;
    // Songs and videos have featured images, for a user who may upload.
    let featured = state.kind.plays() && site.can("upload_files");
    let featured = match featured {
        true => Some(featured_box(site, post)?),
        false => None,
    };
    let permalink_box = match slug_shown {
        true => format!(
            "\t\t<div id=\"edit-slug-box\" class=\"hide-if-no-js\">\n\t\t\t{}\t</div>\n\t\t",
            sample_permalink(site, post, (&state.view, &state.sample))?
        ),
        false => String::new(),
    };
    let referer = wp_get_referer(site, args.referer.as_deref(), request.uri);
    let columns = site.user_option("screen_layout_attachment");
    let columns = if columns.is_some_and(|c| Value::String(c).to_int() == 1) {
        1
    } else {
        2
    };
    let stored = site.user_option("metaboxhidden_attachment");
    let hidden: Vec<String> = match stored {
        Some(_) => listed(site, "metaboxhidden_attachment"),
        None => HIDDEN.iter().map(|id| id.to_string()).collect(),
    };
    let closed = listed(site, "closedpostboxes_attachment");
    let reply = comment_reply(site, Some(2), (false, "single"), 1, false);
    let (reply, trash_notice) = reply.split_once("\t</form>\n\t").unwrap_or_default();
    let mut boxes = vec![Widget {
        id: "submitdiv",
        title: "Save",
        context: "side".into(),
        inside: submit_box(site, post, meta, size),
    }];
    if let Some(inside) = featured {
        boxes.push(Widget {
            id: "postimagediv",
            title: "Featured image",
            context: "side".into(),
            inside,
        });
    }
    let mut add = |id, title, inside| {
        boxes.push(Widget {
            id,
            title,
            context: "normal".into(),
            inside,
        })
    };
    if state.kind == Kind::Audio {
        add("attachment-id3", "Metadata", id3_box(meta));
    }
    add("commentstatusdiv", "Discussion", discussion_box(post));
    // Only a published or private attachment's comments are listed.
    let commented = ["publish", "private"].contains(&state.status.as_str());
    if commented {
        let shown = !hidden.iter().any(|h| h == "commentsdiv");
        add(
            "commentsdiv",
            "Comments",
            comments_box(site, post, request, shown, trash_notice),
        );
    }
    let slug = format!(
        "<label class=\"screen-reader-text\" for=\"post_name\">\n\tSlug</label><input name=\"post_name\" type=\"text\" class=\"large-text\" id=\"post_name\" value=\"{}\" />\n\t",
        esc_attr(&urldecode(&post.name))
    );
    if slug_shown {
        add("slugdiv", "Slug", slug);
    }
    if site.can("edit_others_posts") {
        add("authordiv", "Author", author_box(site, post));
    }
    let labels: String = boxes
        .iter()
        .filter(|b| b.id != "submitdiv")
        .map(|b| {
            let checked = if hidden.iter().any(|h| h == b.id) { "" } else { " checked='checked'" };
            format!(
                "<label for=\"{0}-hide\"><input class=\"hide-postbox-tog\" name=\"{0}-hide\" type=\"checkbox\" id=\"{0}-hide\" value=\"{0}\" {checked} />{1}</label>",
                b.id, b.title
            )
        })
        .collect();
    let boxes = sorted(site, "meta-box-order_attachment", boxes);
    // `edit-form-advanced.php` sets `$_GET['message']` to its `absint()`.
    let message = args.message.as_deref().map_or(0, |m| {
        Value::String(m.as_bytes().to_vec()).to_int().unsigned_abs()
    });
    let mut containers = String::new();
    for (number, context) in [(1, "side"), (2, "normal")] {
        containers.push_str(&format!(
            "<div id=\"postbox-container-{number}\" class=\"postbox-container\">\n<div id=\"{context}-sortables\" class=\"meta-box-sortables\">"
        ));
        for b in boxes.iter().filter(|b| b.context == context) {
            let id = b.id.to_string();
            containers.push_str(&postbox(
                b.id,
                b.title,
                closed.contains(&id),
                hidden.contains(&id),
                &b.inside,
            ));
        }
        containers.push_str("</div>");
        if context == "normal" {
            // `WP_Embed::maybe_run_ajax_cache()`, after a save.
            if message != 0 {
                let ajax = cookie_path(&BarSite::admin_url(site, "admin-ajax.php"));
                containers.push_str(&format!(
                    "<script>\n\tjQuery( function($) {{\n\t\t$.get(\"{}?action=oembed-cache&post={}\");\n\t}} );\n</script>\n\t\t",
                    String::from_utf8_lossy(&esc_url(ajax.strip_suffix('/').unwrap_or(&ajax).as_bytes())),
                    post.id
                ));
            }
            containers
                .push_str("<div id=\"advanced-sortables\" class=\"meta-box-sortables\"></div>");
        }
        containers.push_str("</div>\n");
    }
    containers.push_str(&format!(
        "</div><!-- /post-body -->\n<br class=\"clear\" />\n</div><!-- /poststuff -->\n</form>\n</div>\n\n{reply}\t</form>\n\t\n"
    ));
    if is_empty_string(&post.title) && !request.browser.is_mobile() {
        containers.push_str("<script>\ntry{document.post.title.focus();}catch(e){}\n</script>\n");
    }
    let message = Some(message)
        .filter(|m| (1..=10).contains(m))
        .map_or(String::new(), |_| {
            wp_admin_notice(
                "Media file updated.",
                "message",
                "notice-success is-dismissible updated",
                true,
                "",
            )
        });
    let new = if site.can("upload_files") {
        format!(
            " <a href=\"{}\" class=\"page-title-action\">Add Media File</a>",
            String::from_utf8_lossy(&esc_url(
                BarSite::admin_url(site, "media-new.php").as_bytes()
            ))
        )
    } else {
        String::new()
    };
    let raw_title = esc_textarea(&post.title);
    let description = esc_textarea(&post.content);
    let description = if rich_editing(site, request.browser) {
        description
    } else {
        esc_textarea(&description)
    };
    let referer_url = referer.as_deref().unwrap_or_default();
    // `wp_original_referer_field()`, which a draft's form goes without.
    let original = if state.status == "draft" {
        String::new()
    } else {
        format!(
            "<input type=\"hidden\" name=\"_wp_original_http_referer\" value=\"{}\" />",
            esc_attr(&hyscribe_phpser::stripslashes(referer_url.as_bytes()))
        )
    };
    let alt = if post.mime_type.starts_with("image") {
        let alt = site
            .meta(post, "_wp_attachment_image_alt")
            .unwrap_or_default();
        ALT.replace("%alt%", &esc_attr(&alt))
    } else {
        "\t\n".into()
    };
    let (preview, played) = preview(site, post, state.kind, meta);
    let content = fill(
        FORM,
        &[
            ("new", new),
            ("message", message),
            ("nonce", site.nonce(&format!("update-post_{id}"))),
            ("referer", wp_referer_field(request.uri)),
            ("user", site.user_id().to_string()),
            ("author", post.author.to_string()),
            ("status", esc_attr(post.status.as_bytes())),
            (
                "referredby",
                String::from_utf8_lossy(&esc_url(referer_url.as_bytes())).into_owned(),
            ),
            ("original", original),
            ("id", id.to_string()),
            ("order_nonce", site.nonce("meta-box-order")),
            ("closed_nonce", site.nonce("closedpostboxes")),
            ("columns", columns.to_string()),
            ("title", esc_attr(&raw_title)),
            ("permalink", permalink_box),
            ("permalink_nonce", site.nonce("samplepermalink")),
            ("preview", preview),
            ("alt", alt),
            (
                "caption_label",
                if image {
                    "Image Caption"
                } else {
                    "Short Description"
                }
                .into(),
            ),
            (
                "caption",
                String::from_utf8_lossy(&esc_textarea(&post.excerpt)).into_owned(),
            ),
            ("editor_css", editor_css(site)),
            (
                "displayed",
                if plays_by_type(post) {
                    ": Displayed on attachment pages."
                } else {
                    ""
                }
                .into(),
            ),
            (
                "description",
                String::from_utf8_lossy(&description).into_owned(),
            ),
            ("boxes", containers),
        ],
    );
    let screen_options = format!(
        "<div id=\"screen-options-wrap\" class=\"hidden\" tabindex=\"-1\" aria-label=\"Screen Options Tab\">\n<form id='adv-settings' method='post'>\n\t\t<fieldset class=\"metabox-prefs\">\n\t\t<legend>Screen elements</legend>\n\t\t<p>\n\t\t\tSome screen elements can be shown or hidden by using the checkboxes.\t\t\tExpand or collapse the elements by clicking on their headings, and arrange them by dragging their headings or by clicking on the up and down arrows.\t\t</p>\n\t\t<div class=\"metabox-prefs-container\">\n\t\t{labels}\t\t</div>\n\t\t</fieldset>\n\t\t\t\t<fieldset class='columns-prefs'>\n\t\t<legend class=\"screen-layout\">Layout</legend>\n\t\t\t\t\t<label class=\"columns-prefs-1\">\n\t\t\t<input type='radio' name='screen_columns' value='1' {} />\n\t\t\t1 column\t\t\t</label>\n\t\t\t\t\t<label class=\"columns-prefs-2\">\n\t\t\t<input type='radio' name='screen_columns' value='2' {} />\n\t\t\t2 columns\t\t\t</label>\n\t\t\t\t</fieldset>\n\t\t<fieldset class=\"editor-expand hidden\"><legend>Additional settings</legend><label for=\"editor-expand-toggle\"><input type=\"checkbox\" id=\"editor-expand-toggle\" checked='checked' />Enable full-height editor and distraction-free functionality.</label></fieldset>\n<input type=\"hidden\" id=\"screenoptionnonce\" name=\"screenoptionnonce\" value=\"{}\" />\n</form>\n</div>",
        if columns == 1 {
            " checked='checked'"
        } else {
            ""
        },
        if columns == 2 {
            " checked='checked'"
        } else {
            ""
        },
        site.nonce("screen-options-nonce")
    );
    let users = Value::String(site.option("user_count").unwrap_or(b"-1".to_vec())).to_int();
    let mut footer = if users > 1 {
        lock_dialog(site, post, (&state.holder, &state.view), referer.as_deref())
    } else {
        String::new()
    };
    // `wp_enqueue_media()` for songs and videos, with their featured image.
    let library = state.kind.plays().then(|| {
        let featured = thumbnail_id(site, post).map_or(Value::Int(-1), Value::String);
        enqueue_media(site, request.browser, Some((id, featured)))
    });
    if let Some((templates, _)) = &library {
        footer.push_str(templates);
    }
    footer.push_str("<div id=\"local-storage-notice\" class=\"notice is-dismissible hidden\"><p class=\"local-restore\">The backup of this post in your browser is different from the version below.<button type=\"button\" class=\"button restore-backup\">Restore the backup</button></p><p class=\"help\">This will replace the current editor content with the last backup version. You can use undo and redo in the editor to get the old content back or to return to the restored version.</p></div>");
    // `_js_vars()` of the comments box's list table and the reply form's.
    footer.push_str(&"<script>list_args = {\"class\":\"WP_Post_Comments_List_Table\",\"screen\":{\"id\":\"attachment\",\"base\":\"post\"}};</script>\n".repeat(1 + usize::from(commented)));
    let editor = editor_footer(site).replacen(
        "qtInit: {",
        "qtInit: {'attachment_content':{id:\"attachment_content\",buttons:\"strong,em,link,block,del,ins,img,ul,ol,li,code,close\"},",
        1,
    );
    let shown_title = admin_title(post);
    let title = if is_empty_string(&shown_title) {
        "Edit Media".to_string()
    } else {
        format!(
            "Edit Media &#8220;{}&#8221;",
            String::from_utf8_lossy(&hyscribe_kses::strip_tags(&shown_title))
        )
    };
    let mut scripts = vec!["admin-comments", "post"];
    if request.browser.is_mobile() {
        scripts.push("jquery-touch-punch");
    }
    let mut styles = vec!["imgareaselect"];
    let mut script_data = vec![("word-count", WORD_COUNT.to_string())];
    // The media library's scripts end with the image editor's.
    match library {
        Some((_, data)) => {
            scripts.push("thickbox");
            scripts.extend(MEDIA_SCRIPTS);
            styles = [&["thickbox"][..], &MEDIA_STYLES].concat();
            script_data.extend(data);
        }
        None => scripts.push("image-edit"),
    }
    // The video player's script, enqueued as the preview prints; the media
    // library has the players' others.
    let mut late_scripts = Vec::new();
    if played && state.kind == Kind::Video {
        late_scripts.push("mediaelement-vimeo");
    }
    late_scripts.extend(["quicktags", "wplink", "jquery-ui-autocomplete"]);
    // `wp_heartbeat_set_suspension()`: the editors keep the heartbeat.
    let heartbeat = format!(
        "var heartbeatSettings = {{\"nonce\":\"{}\",\"suspension\":\"disable\"}};",
        site.nonce("heartbeat-nonce")
    );
    script_data.push(("heartbeat", heartbeat));
    let page = Page {
        screen: Screen {
            pagenow: "post.php",
            typenow: "attachment",
            taxonomy: "",
            parent_file: Some("upload.php"),
            submenu_file: Some("upload.php"),
            plugin_page: None,
        },
        id: "attachment",
        title: &title,
        help: &HELP,
        sidebar: SIDEBAR,
        screen_options: &screen_options,
        styles: &styles,
        scripts: &scripts,
        late_scripts: &late_scripts,
        script_data: &script_data,
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: &footer,
        footer_scripts: &editor,
        content: &content,
        canonical: request.canonical,
        origin: request.origin,
        // `wp_admin_bar_edit_menu()`, for a user who may read it.
        view: site
            .can_post("read_post", post)
            .then_some(("View Media File", state.view.as_str())),
    };
    Some(Answer::from_page(admin_page(site, &page, request.browser)))
}

#[cfg(test)]
pub(crate) mod content;
#[cfg(test)]
mod tests;
