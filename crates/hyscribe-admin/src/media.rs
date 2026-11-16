// SPDX-License-Identifier: AGPL-3.0-or-later

//! The Media screens: the library, `upload.php`, in its grid and list
//! modes, Add New Media, `media-new.php`, and an attachment's edit screen;
//! with the media library `wp_enqueue_media()` brings to a screen.

use hyscribe_phpser::{Key, Value};
use hyscribe_render::date::MONTHS;

use crate::admin_bar::{BarSite, Browser};
use crate::assets::{cookie_path, registered_script_data};
use crate::list_table::fill;
use crate::page::{esc_js, json, object, text};
use crate::posts::PostsSite;

pub(crate) mod attachment;
pub(crate) mod image;
mod library;
mod upload_new;

pub use attachment::{AttachmentArgs, AttachmentSite, attachment_edit, size_format};
pub use image::{icon_name, site_icon_url};
pub use library::{LibraryArgs, upload};
pub use upload_new::{NewArgs, media_new};

/// What the Media screens ask of the site beyond the posts list's.
pub trait MediaSite: PostsSite {
    /// `get_available_post_mime_types( 'attachment' )`: the types of the
    /// attachments, each once.
    fn attachment_mime_types(&self) -> Vec<String>;
    /// Whether a post other than `id` has the slug `slug`.
    fn slug_taken(&self, slug: &[u8], id: u64) -> bool;
}

const MEDIA_TEMPLATES: &str = include_str!("media/media.html");
const MEDIA_VIEWS: &str = include_str!("media/media-views.js");

/// The uploader's buttons and limits, for a browser that can upload.
const UPLOAD: &str = "\t\t\t\t<h2 class=\"upload-instructions drop-instructions\">Drop files to upload</h2>\n\t\t\t\t<p class=\"upload-instructions drop-instructions\">or</p>\n\t\t\t\t<button type=\"button\" class=\"browser button button-hero\" aria-describedby=\"post-upload-info\">Select Files</button>\n\t\t\t</div>\n\n\t\t\t<div class=\"upload-inline-status\"></div>\n\n\t\t\t<div class=\"post-upload-ui\" id=\"post-upload-info\">\n\t\t\t\t\n\t\t\t\t<p class=\"max-upload-size\">\n\t\t\t\tMaximum upload file size: 2 MB.\t\t\t\t</p>\n\n\t\t\t\t<# if ( data.suggestedWidth && data.suggestedHeight ) { #>\n\t\t\t\t\t<p class=\"suggested-dimensions\">\n\t\t\t\t\t\tSuggested image dimensions: {{data.suggestedWidth}} by {{data.suggestedHeight}} pixels.\t\t\t\t\t</p>\n\t\t\t\t<# } #>\n\n\t\t\t\t\t\t\t</div>\n";

/// What the uploader says to a browser that cannot upload.
const NO_UPLOAD: &str = "\t\t\t\t<h2 class=\"upload-instructions\">Your browser cannot upload files</h2>\n\t\t\t\t<p>\n\t\t\t\tThe web browser on your device cannot be used to upload files. You may be able to use the <a href=\"https://apps.wordpress.org/\">native app for your device</a> instead.\t\t\t\t</p>\n\t\t\t</div>\n";

/// The extensions `get_allowed_mime_types()` gives a user who may not post
/// unfiltered HTML.
const EXTENSIONS: &str = "jpg,jpeg,jpe,gif,png,bmp,tiff,tif,webp,avif,ico,heic,heif,heics,heifs,asf,asx,wmv,wmx,wm,avi,divx,flv,mov,qt,mpeg,mpg,mpe,mp4,m4v,ogv,webm,mkv,3gp,3gpp,3g2,3gp2,txt,asc,c,cc,h,srt,csv,tsv,ics,rtx,css,vtt,dfxp,mp3,m4a,m4b,aac,ra,ram,wav,x-wav,ogg,oga,flac,mid,midi,wma,wax,mka,rtf,pdf,class,tar,zip,gz,gzip,rar,7z,psd,xcf,doc,pot,pps,ppt,wri,xla,xls,xlt,xlw,mdb,mpp,docx,docm,dotx,dotm,xlsx,xlsm,xlsb,xltx,xltm,xlam,pptx,pptm,ppsx,ppsm,potx,potm,ppam,sldx,sldm,onetoc,onetoc2,onetmp,onepkg,oxps,xps,odt,odp,ods,odg,odc,odb,odf,wp,wpd,key,numbers,pages";

/// `get_allowed_mime_types()`'s extensions for the current user: `htm`,
/// `html` and `js` too when they may post unfiltered HTML.
fn extensions(site: &dyn PostsSite) -> String {
    if !site.can("unfiltered_html") {
        return EXTENSIONS.into();
    }
    EXTENSIONS
        .replace(",css,", ",css,htm,html,")
        .replace(",rtf,", ",rtf,js,")
}

/// `wp_max_upload_size()`, the oracle's `upload_max_filesize`.
const MAX_UPLOAD: &str = "2097152b";

/// `_device_can_upload()`: not on iOS before 6.
pub(crate) fn can_upload(browser: &Browser) -> bool {
    let agent = String::from_utf8_lossy(browser.user_agent);
    if !browser.is_mobile() || !["iPhone", "iPad", "iPod"].iter().any(|d| agent.contains(d)) {
        return true;
    }
    agent
        .split_once("OS ")
        .filter(|(_, rest)| rest.contains(" like Mac OS X"))
        .and_then(|(_, rest)| {
            rest.split(['_', ' '])
                .next()
                .unwrap_or_default()
                .parse::<u32>()
                .ok()
        })
        .is_some_and(|major| major >= 6)
}

/// Whether plupload must pick one file at a time: iOS 7 in Safari.
fn single_file(browser: &Browser) -> bool {
    let agent = String::from_utf8_lossy(browser.user_agent);
    browser.is_mobile() && agent.contains("OS 7_") && agent.contains("like Mac OS X")
}

/// `wp_plupload_default_settings()`'s data after the registered strings.
fn plupload(site: &dyn PostsSite, browser: &Browser) -> String {
    let ajax = cookie_path(&BarSite::admin_url(site, "async-upload.php"));
    let mut defaults = vec![
        ("file_data_name", text("async-upload")),
        ("url", text(ajax.strip_suffix('/').unwrap_or(&ajax))),
        (
            "filters",
            object(vec![
                ("max_file_size", text(MAX_UPLOAD)),
                (
                    "mime_types",
                    Value::Array(vec![(
                        Key::Int(0),
                        object(vec![("extensions", text(&extensions(site)))]),
                    )]),
                ),
            ]),
        ),
    ];
    if single_file(browser) {
        defaults.push(("multi_selection", Value::Bool(false)));
    }
    defaults.push((
        "multipart_params",
        object(vec![
            ("action", text("upload-attachment")),
            ("_wpnonce", text(&site.nonce("media-form"))),
        ]),
    ));
    let settings = object(vec![
        ("defaults", object(defaults)),
        (
            "browser",
            object(vec![
                ("mobile", Value::Bool(browser.is_mobile())),
                ("supported", Value::Bool(can_upload(browser))),
            ]),
        ),
        ("limitExceeded", Value::Bool(false)),
    ]);
    format!(
        "{}\nvar _wpPluploadSettings = {};",
        registered_script_data("wp-plupload", "data"),
        json(&settings)
    )
}

/// `get_option( $name )` as text, empty when unset.
fn option(site: &dyn PostsSite, name: &str) -> String {
    String::from_utf8_lossy(&site.option(name).unwrap_or_default()).into_owned()
}

/// `wp_enqueue_media()`'s `_wpMediaViewsL10n`, its settings from the
/// site: the months with attachments newest first among them, and the
/// `post` it is given.
fn media_views(site: &dyn PostsSite, post: Option<(u64, Value)>) -> String {
    let mut months = site.months("attachment", false);
    months.extend(site.months("attachment", true));
    months.sort_by(|a, b| b.cmp(a));
    months.dedup();
    let months = Value::Array(
        months
            .into_iter()
            .enumerate()
            .map(|(i, (year, month))| {
                let name = MONTHS.get((month as usize).wrapping_sub(1)).copied();
                let name = name.unwrap_or_default();
                let entry = object(vec![
                    ("year", text(&year.to_string())),
                    ("month", text(&month.to_string())),
                    ("text", text(&format!("{name} {year}"))),
                ]);
                (Key::Int(i as i64), entry)
            })
            .collect(),
    );
    let props = object(vec![
        ("link", text(&option(site, "image_default_link_type"))),
        ("align", text(&option(site, "image_default_align"))),
        ("size", text(&option(site, "image_default_size"))),
    ]);
    let scrolling = site.user_option("infinite_scrolling").as_deref() != Some(b"false");
    let post = match post {
        None => object(vec![("id", Value::Int(0))]),
        Some((id, featured)) => object(vec![
            ("id", Value::Int(id.cast_signed())),
            ("nonce", text(&site.nonce(&format!("update-post_{id}")))),
            ("featuredImageId", featured),
        ]),
    };
    fill(
        MEDIA_VIEWS,
        &[
            (
                "tab_url",
                json(&text(&BarSite::admin_url(
                    site,
                    "media-upload.php?chromeless=1",
                ))),
            ),
            (
                "nonce:media-send-to-editor",
                site.nonce("media-send-to-editor"),
            ),
            (
                "nonce:set-attachment-thumbnail",
                site.nonce("set-attachment-thumbnail"),
            ),
            ("props", json(&props)),
            (
                "proxy",
                json(&text(&site.fill("%rest_url%oembed/1.0/proxy"))),
            ),
            ("months", json(&months)),
            ("post", json(&post)),
            ("scrolling", u8::from(scrolling).to_string()),
        ],
    )
}

/// The scripts `wp_enqueue_media()` adds, in its order.
pub(crate) const MEDIA_SCRIPTS: [&str; 4] =
    ["media-editor", "media-audiovideo", "mce-view", "image-edit"];

/// The styles `wp_enqueue_media()` adds.
pub(crate) const MEDIA_STYLES: [&str; 2] = ["media-views", "imgareaselect"];

/// `wp_enqueue_media()` for a `post` with featured images (ID and image):
/// the footer's media templates and the data of the scripts it adds.
pub(crate) fn enqueue_media(
    site: &dyn PostsSite,
    browser: &Browser,
    post: Option<(u64, Value)>,
) -> (String, [(&'static str, String); 3]) {
    let upload = if can_upload(browser) {
        UPLOAD
    } else {
        NO_UPLOAD
    };
    let templates = site.fill(&fill(
        MEDIA_TEMPLATES,
        &[
            ("title", esc_js(&option(site, "blogname"))),
            ("upload", upload.into()),
        ],
    ));
    let script_data = [
        ("media-views", media_views(site, post)),
        ("wp-plupload", plupload(site, browser)),
        // `wp_default_scripts()` lists the shortcodes core adds by then.
        (
            "mce-view",
            "var mceViewL10n = {\"shortcodes\":[\"wp_caption\",\"caption\",\"gallery\",\"playlist\",\"audio\",\"video\",\"embed\"]};".into(),
        ),
    ];
    (templates, script_data)
}

#[cfg(test)]
pub(crate) mod content;
#[cfg(test)]
mod tests;
