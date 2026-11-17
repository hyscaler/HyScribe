// SPDX-License-Identifier: AGPL-3.0-or-later

//! Add New Media, `media-new.php`: the uploader's form, whose files the
//! upload handler takes.

use hyscribe_kses::esc_url;
use hyscribe_phpser::{Value, is_empty_string, remove_query_arg};

use super::{MediaSite, can_upload, max_file_size, max_upload_line, single_file};
use crate::admin_bar::BarSite;
use crate::list_table::fill;
use crate::menu::Screen;
use crate::page::user_setting;
use crate::page::{HelpTab, Page, Request, admin_page, json, object, query_strings, text};
use crate::posts::Answer;

const CONTENT: &str = include_str!("new.html");
const UPLOADER: &str = include_str!("uploader.html");

const HELP: [HelpTab; 1] = [HelpTab {
    id: "overview",
    title: "Overview",
    content: "<p>You can upload media files here without creating a post first. This allows you to upload files to use with posts and pages later and/or to get a web link for a particular file that you can share. There are three options for uploading files:</p><ul><li><strong>Drag and drop</strong> your files into the area below. Multiple files are allowed.</li><li>Clicking <strong>Select Files</strong> opens a navigation window showing you files in your operating system. Selecting <strong>Open</strong> after clicking on the file you want activates a progress bar on the uploader screen.</li><li>Revert to the <strong>Browser Uploader</strong> by clicking the link below the drag and drop box.</li></ul>",
}];

const SIDEBAR: &str = "<p><strong>For more information:</strong></p><p><a href=\"https://wordpress.org/documentation/article/media-add-new-screen/\">Documentation on Uploading Media Files</a></p><p><a href=\"https://wordpress.org/support/forums/\">Support forums</a></p>";

/// What the uploader says to a browser that cannot upload.
const NO_UPLOAD: &str = "<p>The web browser on your device cannot be used to upload files. You may be able to use the <a href=\"https://apps.wordpress.org/\">native app for your device</a> instead.</p>";

/// `media-new.php`'s query arguments.
#[derive(Default)]
pub struct NewArgs {
    /// `$_REQUEST['post_id']`, if given.
    pub post_id: Option<String>,
    /// `$_GET['browser-uploader']` is set.
    pub browser_uploader: bool,
    /// The user's `wp-settings-{ID}` cookie, if sent.
    pub settings: Option<String>,
}

impl NewArgs {
    /// The arguments of the query string `query` with the settings cookie
    /// `settings`; `None` for one that is not a string.
    pub fn parse(query: &str, settings: Option<String>) -> Option<NewArgs> {
        let args = query_strings(query)?;
        let get = |name: &str| {
            args.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
        };
        Some(NewArgs {
            post_id: get("post_id"),
            browser_uploader: get("browser-uploader").is_some(),
            settings,
        })
    }
}

/// An option's `absint()`, 1024 when that is 0.
fn large_size(site: &dyn MediaSite, name: &str) -> String {
    let size = Value::String(site.option(name).unwrap_or_default())
        .to_int()
        .unsigned_abs();
    if size == 0 { 1024 } else { size }.to_string()
}

/// `media_upload_form()` for the post `post_id` asks for.
fn uploader(site: &dyn MediaSite, post_id: i64, request: &Request) -> String {
    if !can_upload(request.browser) {
        return NO_UPLOAD.into();
    }
    let mut init = vec![
        ("browse_button", text("plupload-browse-button")),
        ("container", text("plupload-upload-ui")),
        ("drop_element", text("drag-drop-area")),
        ("file_data_name", text("async-upload")),
        ("url", text(&BarSite::admin_url(site, "async-upload.php"))),
        (
            "filters",
            object(vec![("max_file_size", max_file_size(site))]),
        ),
        (
            "multipart_params",
            object(vec![
                ("post_id", Value::Int(post_id)),
                ("_wpnonce", text(&site.nonce("media-form"))),
                ("type", text("")),
                ("tab", text("")),
                ("short", text("1")),
            ]),
        ),
    ];
    if single_file(request.browser) {
        init.push(("multi_selection", Value::Bool(false)));
    }
    fill(
        UPLOADER,
        &[
            ("height", large_size(site, "large_size_h")),
            ("width", large_size(site, "large_size_w")),
            ("init", json(&object(init))),
            ("max_upload", max_upload_line(site)),
        ],
    )
}

/// `media-new.php`'s form to upload files, for the post `post_id` names
/// if the user may edit it.
pub fn media_new(site: &dyn MediaSite, args: &NewArgs, request: &Request) -> Answer {
    // `admin.php` turns away a user the menu does not let in first.
    if !site.can("upload_files") {
        return Answer::Denied("Sorry, you are not allowed to access this page.");
    }
    let asked = args
        .post_id
        .as_deref()
        .map_or(0, |id| Value::String(id.into()).to_int());
    let post_id = site
        .post(asked.unsigned_abs())
        .filter(|post| site.can_post("edit_post", post))
        .map_or(0, |post| post.id);
    let mut class = "wp-upload-form media-upload-form type-form validate".to_string();
    let uploader_setting = user_setting(site, args.settings.as_deref(), "uploader");
    if uploader_setting.is_some_and(|s| !is_empty_string(s.as_bytes())) || args.browser_uploader {
        class.push_str(" html-uploader");
    }
    let referer = remove_query_arg(&[b"_wp_http_referer"], request.uri.as_bytes());
    let content = fill(
        CONTENT,
        &[
            (
                "action",
                String::from_utf8_lossy(&esc_url(
                    BarSite::admin_url(site, "media-new.php").as_bytes(),
                ))
                .into_owned(),
            ),
            ("class", class),
            ("uploader", uploader(site, asked, request)),
            ("post_id", post_id.to_string()),
            ("nonce", site.nonce("media-form")),
            (
                "referer",
                String::from_utf8_lossy(&esc_url(&referer)).into_owned(),
            ),
        ],
    );
    let page = Page {
        screen: Screen {
            pagenow: "media-new.php",
            typenow: "",
            taxonomy: "",
            parent_file: Some("upload.php"),
            submenu_file: None,
        },
        id: "media",
        title: "Upload Media",
        help: &HELP,
        sidebar: SIDEBAR,
        screen_options: "",
        styles: &[],
        scripts: &["plupload-handlers"],
        late_scripts: &[],
        script_data: &[],
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: "",
        content: &content,
        canonical: request.canonical,
        origin: request.origin,
        view: None,
    };
    admin_page(site, &page, request.browser).map_or(
        Answer::Denied("Sorry, you are not allowed to access this page."),
        Answer::Page,
    )
}
