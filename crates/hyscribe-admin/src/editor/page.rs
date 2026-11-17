// SPDX-License-Identifier: AGPL-3.0-or-later

//! The block editor's page, `edit-form-blocks.php` with the admin around
//! it: its REST preload paths, and the page once their answers are known.

use std::sync::OnceLock;

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, add_query_arg, is_empty_string, json_decode, json_encode};
use hyscribe_render::head::json_hex_tag;
use hyscribe_render::post::admin_title;

use super::form::{self, Editing};
use super::{EditorSite, settings};
use crate::admin_bar::BarSite;
use crate::assets::{Assets, Urls};
use crate::menu::Screen;
use crate::page::{Extras, Page, Request, admin_page_with};

const THEME_REGISTERED: &[u8] =
    include_bytes!("../../../../themes/twentytwentyfive/registered.json");

/// The fields of the REST index the editor's site entity reads, in its order.
const INDEX_FIELDS: &str = "description,gmt_offset,home,image_max_bit_depth,image_sizes,image_size_threshold,image_strip_meta,name,site_icon,site_icon_url,site_logo,timezone_string,url,page_for_posts,page_on_front,show_on_front";

/// `rest_get_route_for_post_type_items()` of the post types the page names.
fn items_route(post_type: &str) -> &'static str {
    match post_type {
        "attachment" => "/wp/v2/media",
        "page" => "/wp/v2/pages",
        "wp_block" => "/wp/v2/blocks",
        "wp_template" => "/wp/v2/templates",
        _ => "/wp/v2/posts",
    }
}

/// `edit-form-blocks.php`'s `$preload_paths` for `post`: each method and
/// path. `styles` is the theme's global styles post, `edit_theme` whether
/// the user may edit the theme's options.
pub fn preload_paths(
    post: &Post,
    styles: u64,
    edit_theme: bool,
    stylesheet: &str,
) -> Vec<(&'static str, String)> {
    let kind = &post.post_type;
    let route = format!("{}/{}", items_route(kind), post.id);
    let mut lookup = if kind == "page" {
        "page".to_string()
    } else {
        format!("single-{kind}")
    };
    if !post.name.is_empty() {
        lookup = format!("{lookup}-{}", String::from_utf8_lossy(&post.name));
    }
    let context = if edit_theme { "edit" } else { "view" };
    let get = |path: String| ("GET", path);
    let options = |path: &str| ("OPTIONS", path.to_string());
    let mut paths = vec![
        get("/wp/v2/types?context=view".into()),
        get("/wp/v2/taxonomies?context=view".into()),
        get(format!("{route}?context=edit")),
        get(format!("/wp/v2/types/{kind}?context=edit")),
        get("/wp/v2/users/me".into()),
        options(items_route("attachment")),
        options(items_route("page")),
        options(items_route("wp_block")),
        options(items_route("wp_template")),
        get(format!("{route}/autosaves?context=edit")),
        get("/wp/v2/settings".into()),
        options("/wp/v2/settings"),
        get(format!(
            "/wp/v2/global-styles/themes/{stylesheet}?context=view"
        )),
        get(format!(
            "/wp/v2/global-styles/themes/{stylesheet}/variations?context=view"
        )),
        get("/wp/v2/themes?context=edit&status=active".into()),
        options(&format!("/wp/v2/global-styles/{styles}")),
        get(format!("/wp/v2/global-styles/{styles}?context={context}")),
        get("/wp/v2/block-patterns/categories".into()),
        get(format!("/?_fields={INDEX_FIELDS}")),
        get(format!("/wp/v2/templates/lookup?slug={lookup}")),
        get("/wp/v2/templates/lookup?slug=front-page".into()),
        get("/wp/v2/taxonomies?context=edit".into()),
        options(items_route(kind)),
    ];
    // Posts and pages support authors.
    if post.author > 0 {
        let author = post.author;
        paths.push(get(format!(
            "/wp/v2/users/{author}?context=view&_fields=id,name"
        )));
    }
    paths
}

/// The answer to one preloaded request: `None` where it is not ported,
/// else a 200 answer's data and headers, if it is one.
pub type Answered = Option<Option<(Value, Value)>>;

/// `rest_preload_api_request()`'s memo of what `answered` gives each of
/// `paths`, a 200 answer's data and headers; `None` where one is not
/// ported.
pub fn preloaded(
    paths: &[(&'static str, String)],
    answered: &dyn Fn(&str, &str) -> Answered,
) -> Option<Value> {
    let mut memo: Vec<(Key, Value)> = Vec::new();
    let mut options: Vec<(Key, Value)> = Vec::new();
    for (method, path) in paths {
        let Some((body, headers)) = answered(method, path)? else {
            continue;
        };
        let entry = Value::Array(vec![
            (Key::String(b"body".to_vec()), body),
            (Key::String(b"headers".to_vec()), headers),
        ]);
        let key = Key::String(path.clone().into_bytes());
        let list = if *method == "OPTIONS" {
            if options.is_empty() {
                // The `OPTIONS` entry keeps the place of its first path.
                memo.push((Key::String(b"OPTIONS".to_vec()), Value::Null));
            }
            &mut options
        } else {
            &mut memo
        };
        match list.iter_mut().find(|(k, _)| *k == key) {
            Some((_, old)) => *old = entry,
            None => list.push((key, entry)),
        }
    }
    let at = memo
        .iter_mut()
        .find(|(k, _)| k.to_php_string() == b"OPTIONS");
    if let Some((_, value)) = at {
        *value = Value::Array(options);
    }
    Some(Value::Array(memo))
}

/// `enqueue_editor_block_styles_assets()`'s script: the block styles the
/// theme registers, its own and its style partials'.
fn block_styles_script() -> &'static str {
    static SCRIPT: OnceLock<String> = OnceLock::new();
    SCRIPT.get_or_init(|| {
        let registered = json_decode(THEME_REGISTERED).unwrap_or_default();
        let lists = ["block_styles", "partial_styles"].map(|name| {
            let list = registered.get_str(name).and_then(Value::as_array);
            list.unwrap_or_default().to_vec()
        });
        let lines: String = lists
            .concat()
            .iter()
            .map(|(_, entry)| {
                let block = entry
                    .get(&Key::Int(0))
                    .map(Value::to_php_string)
                    .unwrap_or_default();
                let style = entry.get(&Key::Int(1)).cloned().unwrap_or_default();
                let field = |name: &str| style.get_str(name).cloned().unwrap_or_default();
                let style = Value::Array(vec![
                    (Key::String(b"name".to_vec()), field("name")),
                    (Key::String(b"label".to_vec()), field("label")),
                ]);
                let json = json_encode(&style).unwrap_or_default();
                format!(
                    "\twp.blocks.registerBlockStyle( '{}', {} );\n",
                    String::from_utf8_lossy(&block),
                    String::from_utf8_lossy(&json)
                )
            })
            .collect();
        format!("( function() {{\n{lines}}} )();")
    })
}

/// `wp_tinymce_inline_scripts()`'s settings for the classic block.
const TINYMCE: &str = "window.wpEditorL10n = {\n\t\ttinymce: {\n\t\t\tbaseURL: \"%site_url%/wp-includes/js/tinymce\",\n\t\t\tsuffix: \".min\",\n\t\t\tsettings: {plugins:\"charmap,colorpicker,hr,lists,media,paste,tabfocus,textcolor,fullscreen,wordpress,wpautoresize,wpeditimage,wpemoji,wpgallery,wplink,wpdialogs,wptextpattern,wpview\",toolbar1:\"formatselect,bold,italic,bullist,numlist,blockquote,alignleft,aligncenter,alignright,link,unlink,wp_more,spellchecker,wp_add_media,wp_adv\",toolbar2:\"strikethrough,hr,forecolor,pastetext,removeformat,charmap,outdent,indent,undo,redo,wp_help\",toolbar3:\"\",toolbar4:\"\",external_plugins:[],classic_block_editor:true},\n\t\t}\n\t}";

/// `_WP_Editors::print_default_editor_scripts()`, without and with rich
/// editing, which loads TinyMCE.
const DEFAULT_EDITOR: &str = include_str!("default-editor.html");

/// [`DEFAULT_EDITOR`] with rich editing.
const RICH_EDITOR: &str = include_str!("rich-editor.html");

/// The block editor's page of `editing`, with `preloads`, the memo
/// [`preloaded`] gives; `None` for a user the menu turns away.
pub fn editor_page(
    site: &dyn EditorSite,
    editing: &Editing,
    preloads: &Value,
    request: &Request,
) -> Option<String> {
    let post = editing.post;
    let settings =
        settings::get_block_editor_settings(site, form::custom_settings(site, editing), post);
    let edits = form::initial_edits(post);
    let after = form::scripts_after(&settings, &edits, post);
    let (templates, media) =
        crate::media::enqueue_media(site, request.browser, Some((post.id, Value::Int(-1))));
    // `wp_heartbeat_set_suspension()`: the editors keep the heartbeat.
    let heartbeat = format!(
        "var heartbeatSettings = {{\"nonce\":\"{}\",\"suspension\":\"disable\"}};",
        site.nonce("heartbeat-nonce")
    );
    let mut script_data = media.to_vec();
    // `wp_enqueue_media()` names a page's labels where a post's are.
    if post.post_type == "page" {
        for (_, data) in &mut script_data {
            *data = data
                .replace("\"Insert into post\"", "\"Insert into page\"")
                .replace("\"Uploaded to this post\"", "\"Uploaded to this page\"");
        }
    }
    script_data.push(("heartbeat", heartbeat));
    let new = post.status == "auto-draft";
    let kind = if post.post_type == "page" {
        "Page"
    } else {
        "Post"
    };
    let mut title = format!("{} {kind}", if new { "Add" } else { "Edit" });
    // `admin-header.php` names the post edited.
    let shown = admin_title(post);
    if !new && !is_empty_string(&shown) {
        let shown = hyscribe_kses::strip_tags(&shown);
        title = format!("{title} &#8220;{}&#8221;", String::from_utf8_lossy(&shown));
    }
    let meta_boxes = crate::page::text(&format!(
        "var _wpMetaBoxUrl = {};",
        String::from_utf8_lossy(&json_hex_tag(&crate::page::text(&format!(
            "{}?post={}&action=edit&meta-box-loader=1&meta-box-loader-nonce={}",
            BarSite::admin_url(site, "post.php"),
            post.id,
            site.nonce("meta-box-loader")
        ))))
    ));
    let site_url = Urls::site_url(site);
    let variables = site.theme().global_styles().stylesheet_of("variables");
    let preload = format!(
        "wp.apiFetch.use( wp.apiFetch.createPreloadingMiddleware( {} ) );",
        String::from_utf8_lossy(&json_hex_tag(preloads))
    );
    let more = |assets: &mut Assets| {
        assets.set_style_data(
            "global-styles-css-custom-properties",
            "after",
            Value::Array(vec![(Key::Int(0), crate::page::text(&variables))]),
        );
        // Added again as the editor's canvas collects its assets.
        let css = super::assets::block_style_css()
            .iter()
            .map(|(_, css)| css.clone());
        let css = css
            .clone()
            .chain(css)
            .zip(0..)
            .map(|(css, i)| (Key::Int(i), css));
        let css = Value::Array(css.collect());
        assets.set_style_data("wp-block-library", "after", css);
        assets.add_inline_script("wp-api-fetch", "after", &preload);
        assets.add_inline_script(
            "wp-block-library",
            "before",
            &TINYMCE.replace("%site_url%", &site_url),
        );
        assets.add_inline_script(
            "wp-editor",
            "before",
            &String::from_utf8_lossy(&meta_boxes.to_php_string()),
        );
        assets.add_inline_script("wp-block-styles", "after", block_styles_script());
        for (handle, script) in &after {
            assets.add_inline_script(handle, "after", script);
        }
    };
    let modules = ["@wordpress/latex-to-mathml/loader".to_string()];
    let rich = form::rich_editing(site, request.browser);
    let editor = if rich { RICH_EDITOR } else { DEFAULT_EDITOR };
    let editor = editor.replace("%site_url%", &site_url);
    let footer = format!("{editor}{}", crate::comments::link_dialog(site));
    let extras = Extras {
        body_class: " is-fullscreen-mode",
        block_editor: true,
        modules: &modules,
        assets: Some(&more),
        footer: Some(&footer),
    };
    // `edit-form-blocks.php` leaves PHP with a line break, and the footer
    // starts with the one that ends the markup.
    let body = form::body(site, editing);
    let content = format!("\n{}", body.strip_suffix('\n').unwrap_or(&body));
    // `wp_admin_bar_edit_menu()`: a draft's preview, else the post.
    let link = site.permalink(post);
    let view = (!new && site.can_post("read_post", post)).then(|| match post.status.as_str() {
        "draft" => {
            let preview = add_query_arg(&[(b"preview", Some(b"true"))], link.as_bytes());
            (
                format!("Preview {kind}"),
                String::from_utf8_lossy(&preview).into_owned(),
            )
        }
        _ => (format!("View {kind}"), link),
    });
    let (parent, add) = match post.post_type.as_str() {
        "page" => ("edit.php?post_type=page", "post-new.php?post_type=page"),
        _ => ("edit.php", "post-new.php"),
    };
    let page = Page {
        screen: Screen {
            pagenow: if new { "post-new.php" } else { "post.php" },
            typenow: &post.post_type,
            taxonomy: "",
            parent_file: Some(parent),
            submenu_file: Some(if new { add } else { parent }),
        },
        id: &post.post_type,
        title: &title,
        help: &[],
        sidebar: "",
        screen_options: "",
        styles: &form::STYLES,
        scripts: &form::SCRIPTS,
        late_scripts: &[],
        script_data: &script_data,
        script_after: &[],
        late_styles: &[],
        admin_head: "",
        admin_footer: &templates,
        footer_scripts: "",
        content: &content,
        canonical: request.canonical,
        origin: request.origin,
        view: view
            .as_ref()
            .map(|(label, url)| (label.as_str(), url.as_str())),
    };
    admin_page_with(site, &page, request.browser, &extras)
}
