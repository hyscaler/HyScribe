// SPDX-License-Identifier: AGPL-3.0-or-later

//! The embed template (`wp-includes/theme-compat/embed.php`): the page an
//! `embed` request gets, with its head, the post's card or the 404 one,
//! the sharing dialog and the scripts.
//!
//! Not ported yet: featured images and the site icon, which the default
//! install has none of, and the excerpt's `shortcode_unautop()` and
//! attachment filters.

use hyscribe_kses::{esc_attr, esc_html, esc_textarea, esc_url, esc_url_raw};
use hyscribe_model::Post;
use hyscribe_phpser::add_query_arg;

use super::footer::emoji_detection_script;
use super::{EMOJI_CSS, Head, Style, block_style, inline_script, script_source, tag};
use crate::autop::wpautop;
use crate::core_blocks::{comments_open, number_format};
use crate::formatting::{CORE_SHORTCODE_TAGS, convert_chars, wptexturize};
use crate::post::{body_class, post_class, the_title};
use crate::site::{Request, Site};
use crate::template::Renderer;
use crate::theme::Theme;

const TEMPLATE_CSS: &str =
    include_str!("../../../../wordpress/wp-includes/css/wp-embed-template.min.css");
const TEMPLATE_JS: &str =
    include_str!("../../../../wordpress/wp-includes/js/wp-embed-template.min.js");
const HOST_JS: &str = include_str!("../../../../wordpress/wp-includes/js/wp-embed.min.js");

/// What `get_post_embed_html()` and `print_embed_sharing_dialog()` draw at
/// random: the secret the embedding page and the frame share, and the
/// dialog's ID suffix.
pub struct Share {
    /// `wp_generate_password( 10, false )`.
    pub secret: String,
    /// `wp_rand()`.
    pub suffix: String,
}

/// The site as the embed page names and links it.
struct Blog {
    siteurl: String,
    /// `home_url()`, escaped.
    home: Vec<u8>,
    /// `get_bloginfo( 'name' )`.
    name: Vec<u8>,
}

/// The whole embed page for `request`, whose document title is `title`.
pub fn embed_page(
    theme: &Theme,
    site: &dyn Site,
    request: &Request,
    title: &[u8],
    share: &Share,
) -> Vec<u8> {
    let mut renderer = Renderer::new(theme, site, request);
    let blog = Blog {
        siteurl: String::from_utf8_lossy(&site.option("siteurl").unwrap_or_default()).into_owned(),
        home: esc_url(&site.home_url()),
        name: site.option("blogname").unwrap_or_default(),
    };
    let post = request.posts.first().filter(|_| request.singular);
    let body = match post {
        Some(post) => content(&mut renderer, &blog, post),
        None => not_found(&blog),
    };
    let head = Head {
        theme,
        site,
        request,
        context: &renderer.context,
    };
    let siteurl = &blog.siteurl;
    let mut out = [
        &b"<!DOCTYPE html>\n<html lang=\"en-US\" class=\"no-js\">\n<head>\n\t<title>"[..],
        title,
        b"</title>\n\t<meta http-equiv=\"X-UA-Compatible\" content=\"IE=edge\">\n\t",
    ]
    .concat();
    out.extend(head.robots());
    out.extend(head.canonical(false));
    out.extend(Style::inline("wp-emoji-styles", EMOJI_CSS).print());
    out.extend(Style::inline("wp-embed-template", TEMPLATE_CSS).print());
    out.extend_from_slice(b"</head>\n<body class=\"");
    out.extend(body_class(site, request, theme.slug()));
    out.extend_from_slice(b"\">\n");
    out.extend(body);
    if let Some(post) = post {
        out.extend(sharing_dialog(&renderer, &blog, post, share));
    }
    out.extend(file_script(siteurl, "wp-embed-template", TEMPLATE_JS));
    // wp_print_footer_scripts(): the styles rendering the excerpt enqueued.
    for handle in &renderer.assets.styles {
        out.extend(block_style(handle, siteurl).print());
    }
    out.extend(emoji_detection_script(siteurl));
    out.extend_from_slice(b"</body>\n</html>\n");
    out
}

/// `embed-content.php` for the loop's `post`.
fn content(renderer: &mut Renderer, blog: &Blog, post: &Post) -> Vec<u8> {
    let site = renderer.context.site;
    let permalink = esc_url(&renderer.context.permalink(post));
    let title = the_title(post);
    // wp_embed_excerpt_more()
    let more = [
        &b" &hellip; <a href=\""[..],
        &permalink,
        b"\" class=\"wp-embed-more\" target=\"_top\">Continue reading <span class=\"screen-reader-text\">",
        &title,
        b"</span></a>",
    ]
    .concat();
    let excerpt = renderer.excerpt(post, 55, &more);
    let excerpt = wpautop(
        &convert_chars(&wptexturize(&excerpt, &CORE_SHORTCODE_TAGS)),
        true,
    );
    let classes = post_class(site, Some(renderer.context.request), "wp-embed", post);
    let mut out = [
        &b"\t<div class=\""[..],
        &classes,
        b"\">\n\t\t\n\t\t<p class=\"wp-embed-heading\">\n\t\t\t<a href=\"",
        &permalink,
        b"\" target=\"_top\">\n\t\t\t\t",
        &title,
        b"\t\t\t</a>\n\t\t</p>\n\n\t\t\n\t\t<div class=\"wp-embed-excerpt\">",
        &excerpt,
        b"</div>\n\n\t\t\n\t\t<div class=\"wp-embed-footer\">\n\t\t\t",
        &site_title(blog),
        b"\n\t\t\t<div class=\"wp-embed-meta\">\n\t\t\t\t",
    ]
    .concat();
    // print_embed_comments_button()
    let count = post.comment_count;
    if count > 0 || comments_open(&renderer.context, post) {
        let hash: &[u8] = if count > 0 { b"#comments" } else { b"#respond" };
        let noun = if count == 1 { "Comment" } else { "Comments" };
        out.extend(
            [
                &b"\t<div class=\"wp-embed-comments\">\n\t\t<a href=\""[..],
                &esc_url(&[&renderer.context.permalink(post)[..], hash].concat()),
                b"\" target=\"_top\">\n\t\t\t<span class=\"dashicons dashicons-admin-comments\"></span>\n\t\t\t",
                format!(
                    "{} <span class=\"screen-reader-text\">{noun}</span>",
                    number_format(count)
                )
                .as_bytes(),
                b"\t\t</a>\n\t</div>\n\t",
            ]
            .concat(),
        );
    }
    out.extend_from_slice(
        b"\t<div class=\"wp-embed-share\">\n\t\t<button type=\"button\" class=\"wp-embed-share-dialog-open\" aria-label=\"Open sharing dialog\">\n\t\t\t<span class=\"dashicons dashicons-share\"></span>\n\t\t</button>\n\t</div>\n\t\t\t\t</div>\n\t\t</div>\n\t</div>\n",
    );
    out
}

/// `embed-404.php`.
fn not_found(blog: &Blog) -> Vec<u8> {
    [
        &b"<div class=\"wp-embed\">\n\t<p class=\"wp-embed-heading\">Oops! That embed cannot be found.</p>\n\n\t<div class=\"wp-embed-excerpt\">\n\t\t<p>\n\t\t\tIt looks like nothing was found at this location. Maybe try visiting <strong><a href=\""[..],
        &blog.home,
        b"\">",
        esc_html(&blog.name).as_bytes(),
        b"</a></strong> directly?\t\t</p>\n\t</div>\n\n\t\n\t<div class=\"wp-embed-footer\">\n\t\t",
        &site_title(blog),
        b"\t</div>\n</div>\n",
    ]
    .concat()
}

/// `the_embed_site_title()`, with WordPress's icon: the site icon is not
/// read yet.
fn site_title(blog: &Blog) -> Vec<u8> {
    let icon = format!(
        "{}/wp-includes/images/w-logo-gray-white-bg.svg",
        blog.siteurl
    );
    [
        &b"<div class=\"wp-embed-site-title\"><a href=\""[..],
        &blog.home,
        b"\" target=\"_top\"><img src=\"",
        &esc_url(icon.as_bytes()),
        b"\" width=\"32\" height=\"32\" alt=\"\" class=\"wp-embed-site-icon\" /><span>",
        esc_html(&blog.name).as_bytes(),
        b"</span></a></div>",
    ]
    .concat()
}

/// `print_embed_sharing_dialog()`.
fn sharing_dialog(renderer: &Renderer, blog: &Blog, post: &Post, share: &Share) -> Vec<u8> {
    let suffix = format!("{}-{}", post.id, share.suffix);
    let permalink = renderer.context.permalink(post);
    format!(
        "\t<div class=\"wp-embed-share-dialog hidden\" role=\"dialog\" aria-label=\"Sharing options\">\n\t\t<div class=\"wp-embed-share-dialog-content\">\n\t\t\t<div class=\"wp-embed-share-dialog-text\">\n\t\t\t\t<ul class=\"wp-embed-share-tabs\" role=\"tablist\">\n\t\t\t\t\t<li class=\"wp-embed-share-tab-button wp-embed-share-tab-button-wordpress\" role=\"presentation\">\n\t\t\t\t\t\t<button type=\"button\" role=\"tab\" aria-controls=\"wp-embed-share-tab-wordpress-{suffix}\" aria-selected=\"true\" tabindex=\"0\">WordPress Embed</button>\n\t\t\t\t\t</li>\n\t\t\t\t\t<li class=\"wp-embed-share-tab-button wp-embed-share-tab-button-html\" role=\"presentation\">\n\t\t\t\t\t\t<button type=\"button\" role=\"tab\" aria-controls=\"wp-embed-share-tab-html-{suffix}\" aria-selected=\"false\" tabindex=\"-1\">HTML Embed</button>\n\t\t\t\t\t</li>\n\t\t\t\t</ul>\n\t\t\t\t<div id=\"wp-embed-share-tab-wordpress-{suffix}\" class=\"wp-embed-share-tab\" role=\"tabpanel\" aria-hidden=\"false\">\n\t\t\t\t\t<input type=\"text\" value=\"{}\" class=\"wp-embed-share-input\" aria-label=\"URL\" aria-describedby=\"wp-embed-share-description-wordpress-{suffix}\" tabindex=\"0\" readonly/>\n\n\t\t\t\t\t<p class=\"wp-embed-share-description\" id=\"wp-embed-share-description-wordpress-{suffix}\">\n\t\t\t\t\t\tCopy and paste this URL into your WordPress site to embed\t\t\t\t\t</p>\n\t\t\t\t</div>\n\t\t\t\t<div id=\"wp-embed-share-tab-html-{suffix}\" class=\"wp-embed-share-tab\" role=\"tabpanel\" aria-hidden=\"true\">\n\t\t\t\t\t<textarea class=\"wp-embed-share-input\" aria-label=\"HTML\" aria-describedby=\"wp-embed-share-description-html-{suffix}\" tabindex=\"0\" readonly>{}</textarea>\n\n\t\t\t\t\t<p class=\"wp-embed-share-description\" id=\"wp-embed-share-description-html-{suffix}\">\n\t\t\t\t\t\tCopy and paste this code into your site to embed\t\t\t\t\t</p>\n\t\t\t\t</div>\n\t\t\t</div>\n\n\t\t\t<button type=\"button\" class=\"wp-embed-share-dialog-close\" aria-label=\"Close sharing dialog\">\n\t\t\t\t<span class=\"dashicons dashicons-no\"></span>\n\t\t\t</button>\n\t\t</div>\n\t</div>\n\t",
        String::from_utf8_lossy(&esc_url(&permalink)),
        String::from_utf8_lossy(&esc_textarea(&embed_html(blog, post, &permalink, &share.secret))),
    )
    .into_bytes()
}

/// `get_post_embed_html( 600, 400 )`: the blockquote and frame another
/// site embeds the post with, and the script that sizes the frame.
fn embed_html(blog: &Blog, post: &Post, permalink: &[u8], secret: &str) -> Vec<u8> {
    let title = the_title(post);
    // get_post_embed_url() with plain permalinks.
    let url = esc_url_raw(&add_query_arg(&[(b"embed", Some(b"true"))], permalink));
    let src = esc_url(&[&url[..], b"#?secret=", secret.as_bytes()].concat());
    let frame_title =
        esc_attr(&[&b"&#8220;"[..], &title, b"&#8221; &#8212; ", &blog.name].concat());
    let secret = esc_attr(secret.as_bytes());
    [
        format!("<blockquote class=\"wp-embedded-content\" data-secret=\"{secret}\"><a href=\"").as_bytes(),
        &esc_url(permalink),
        b"\">",
        &title,
        format!("</a></blockquote><iframe sandbox=\"allow-scripts\" security=\"restricted\" src=\"{}\" width=\"600\" height=\"400\" title=\"{frame_title}\" data-secret=\"{secret}\" frameborder=\"0\" marginwidth=\"0\" marginheight=\"0\" scrolling=\"no\" class=\"wp-embedded-content\"></iframe>", String::from_utf8_lossy(&src)).as_bytes(),
        &file_script(&blog.siteurl, "wp-embed", HOST_JS),
    ]
    .concat()
}

/// `wp_get_inline_script_tag()` of a script file of `wp-includes/js`.
fn file_script(siteurl: &str, name: &str, js: &str) -> Vec<u8> {
    tag(
        b"script",
        &[],
        Some(&inline_script(&script_source(siteurl, name, js))),
    )
}
