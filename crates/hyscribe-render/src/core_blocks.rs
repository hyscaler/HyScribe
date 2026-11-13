// SPDX-License-Identifier: AGPL-3.0-or-later

//! The dynamic core blocks: the `render_block_core_*()` callbacks from
//! `wp-includes/blocks/`, run on the attributes as
//! `prepare_attributes_for_render()` leaves them.

use hyscribe_kses::{esc_attr, esc_html, esc_url, esc_url_protocols, url_host, url_scheme};
use hyscribe_model::{Comment, Post};
use hyscribe_phpser::{Key, Value, add_query_arg, is_empty_string, isset, php_trim, urlencode};

use crate::autop::wpautop;
use crate::balance::force_balance_tags;
use crate::clickable::make_clickable;
use crate::date::{self, Zone};
use crate::excerpt::wp_trim_words;
use crate::formatting::{CORE_SHORTCODE_TAGS, capital_p_dangit, convert_chars, wptexturize};
use crate::kses::wp_kses_post;
use crate::link;
use crate::media::Loading;
use crate::post::{the_title, title_filters};
use crate::query_loop::{inherits, own_counts, own_page, query_posts};
use crate::registry::block_type;
use crate::site::{Request, Site};
use crate::supports::get_block_wrapper_attributes;
use crate::template::{replace, site_smilies};
use crate::theme::Theme;
use crate::title::archive_title;

/// What a render callback reads besides its block.
#[derive(Clone)]
pub(crate) struct Context<'a> {
    pub(crate) theme: &'a Theme,
    pub(crate) site: &'a dyn Site,
    pub(crate) request: &'a Request,
    /// The `postId` block context.
    pub(crate) post_id: Option<u64>,
    /// The context `core/query` provides.
    pub(crate) query: Option<QueryContext>,
    /// The comment the comment template is rendering, its `commentId`.
    pub(crate) comment: Option<Comment>,
    /// How deep that comment is in its thread, 1 for a top-level comment,
    /// as `get_comment_reply_link()` counts it through its parents.
    pub(crate) comment_depth: usize,
    /// The loop and image counts images' loading attributes depend on.
    pub(crate) loading: Loading,
    /// The terms query block's context and the term template's.
    pub(crate) terms: TermContext,
}

/// The block context of a terms query block's descendants.
#[derive(Clone, Debug, Default)]
pub(crate) struct TermContext {
    /// `termQuery`.
    pub(crate) query: Option<Value>,
    /// `taxonomy` and `termId`, which the term template gives each term.
    pub(crate) term: Option<(String, u64)>,
}

/// The block context of a query block's descendants: `query`,
/// `displayLayout` and `enhancedPagination`.
#[derive(Clone, Debug, Default)]
pub(crate) struct QueryContext {
    /// `queryId`, which names the query's page in the request.
    pub(crate) id: Option<Value>,
    pub(crate) query: Value,
    pub(crate) display_layout: Option<Value>,
    pub(crate) enhanced_pagination: bool,
}

/// One block being rendered: its name, parsed and prepared attributes, and
/// its inner blocks rendered, as `WP_Block::render()` passes `$content`.
struct Block<'a> {
    name: &'a str,
    parsed: &'a [(Key, Value)],
    attributes: Value,
    content: &'a [u8],
}

/// The attributes of `name` as `prepare_attributes_for_render()` leaves
/// them.
pub(crate) fn prepared(name: &str, parsed: &[(Key, Value)]) -> Value {
    Value::Array(
        block_type(name)
            .map(|t| t.prepare(parsed))
            .unwrap_or_default(),
    )
}

/// `array_merge()` of the prepared attributes and those block bindings
/// computed, which skip validation.
pub(crate) fn merged(attributes: Value, computed: &[(Key, Value)]) -> Value {
    let mut attributes = attributes.as_array().unwrap_or_default().to_vec();
    for (key, value) in computed {
        match attributes.iter_mut().find(|(k, _)| k == key) {
            Some((_, old)) => *old = value.clone(),
            None => attributes.push((key.clone(), value.clone())),
        }
    }
    Value::Array(attributes)
}

/// The output of `name`'s render callback, `None` for a block that renders
/// its saved markup.
pub(crate) fn render(
    context: &Context<'_>,
    name: &str,
    parsed: &[(Key, Value)],
    content: &[u8],
) -> Option<Vec<u8>> {
    render_bound(context, name, parsed, &[], content)
}

/// [`render`], with the attributes block bindings computed.
pub(crate) fn render_bound(
    context: &Context<'_>,
    name: &str,
    parsed: &[(Key, Value)],
    computed: &[(Key, Value)],
    content: &[u8],
) -> Option<Vec<u8>> {
    let callback = match name {
        "core/avatar" => avatar::avatar,
        "core/comment-author-name" => comment_author_name,
        "core/comment-content" => comment_content,
        "core/comment-date" => comment_date,
        "core/comment-edit-link" => comment_edit_link,
        "core/comment-reply-link" => comment_reply_link,
        "core/comments" | "core/post-comments" => comments,
        "core/comments-pagination" => comments_pagination,
        "core/comments-title" => comments_title,
        "core/cover" => cover::cover,
        "core/breadcrumbs" => breadcrumbs::breadcrumbs,
        "core/button" => button::button,
        "core/calendar" => calendar::calendar,
        "core/footnotes" => footnotes,
        "core/heading" => heading_list::heading,
        "core/image" => image::image,
        "core/list" => heading_list::list,
        "core/icon" => icon::icon,
        "core/latest-comments" => latest_comments::latest_comments,
        "core/loginout" => loginout::loginout,
        "core/navigation-overlay-close" => navigation_overlay_close,
        "core/post-author" => author::post_author,
        "core/post-author-biography" => author::post_author_biography,
        "core/post-author-name" => author::post_author_name,
        "core/post-comments-form" => comment_form::post_comments_form,
        "core/post-comments-count" => post_comments_count,
        "core/post-comments-link" => post_comments_link,
        "core/post-date" => post_date,
        "core/post-excerpt" => post_excerpt,
        "core/post-featured-image" => featured_image::post_featured_image,
        "core/post-navigation-link" => post_navigation_link,
        "core/post-terms" => post_terms,
        "core/post-time-to-read" => time_to_read::post_time_to_read,
        "core/post-title" => post_title,
        "core/query-no-results" => query_no_results,
        "core/query-pagination" => query_pagination,
        "core/query-title" => query_title,
        "core/query-total" => query_total,
        "core/read-more" => read_more,
        "core/shortcode" => shortcode,
        "core/site-logo" => site_logo::site_logo,
        "core/site-tagline" => site_tagline,
        "core/tag-cloud" => tag_cloud::tag_cloud,
        "core/term-count" => term::term_count,
        "core/term-description" => term::term_description,
        "core/term-name" => term::term_name,
        "core/video" => video::video,
        "core/site-title" => site_title,
        _ => return None,
    };
    let block = Block {
        name,
        parsed,
        attributes: merged(prepared(name, parsed), computed),
        content,
    };
    Some(callback(context, &block))
}

impl Block<'_> {
    fn attribute(&self, name: &str) -> Option<&Value> {
        self.attributes.get_str(name).filter(|v| **v != Value::Null)
    }

    /// `get_block_wrapper_attributes( array( 'class' => $class ) )`.
    fn wrapper(&self, context: &Context<'_>, class: &[u8]) -> String {
        let extra = [("class", Value::String(class.to_vec()))];
        get_block_wrapper_attributes(self.name, self.parsed, &extra, &context.theme.global)
    }

    /// `isset( $attributes['style']['elements']['link']['color']['text'] )`.
    fn has_link_color(&self) -> bool {
        isset(
            self.attributes
                .get_path(&["style", "elements", "link", "color", "text"]),
        )
    }

    /// `has-text-align-<textAlign>`, or empty without an alignment.
    fn text_align_class(&self) -> Vec<u8> {
        match self.attribute("textAlign").filter(|v| !v.is_empty()) {
            Some(align) => [b"has-text-align-", &align.to_php_string()[..]].concat(),
            None => Vec::new(),
        }
    }
}

impl Context<'_> {
    /// The post in the `postId` context.
    pub(crate) fn post(&self) -> Option<Post> {
        self.post_id.and_then(|id| self.site.post(id))
    }

    /// The global post: the main query's.
    pub(crate) fn global_post(&self) -> Option<Post> {
        self.request.post.and_then(|id| self.site.post(id))
    }

    /// `get_permalink( $post )`.
    pub(crate) fn permalink(&self, post: &Post) -> Vec<u8> {
        crate::permalinks::permalink(self.site, post)
    }

    /// `site_url( $path )`.
    pub(crate) fn site_url(&self, path: &[u8]) -> Vec<u8> {
        [&self.site.option("siteurl").unwrap_or_default()[..], path].concat()
    }

    /// The host, with `:port`, of `wp_upload_dir()`'s URL, which lives
    /// under `siteurl`: where `wp_kses_post()` lets embedded PDFs come from.
    pub(crate) fn upload_host(&self) -> Vec<u8> {
        link::host(&self.site.option("siteurl").unwrap_or_default())
    }

    /// `wp_timezone()`.
    pub(crate) fn zone(&self) -> Zone {
        let option = |name| self.site.option(name).unwrap_or_default();
        date::site_zone(&option("timezone_string"), &option("gmt_offset"))
    }
}

/// `render_block_core_comment_content()`: the comment's text through the
/// `comment_text` filters. Only approved comments are read, so there is
/// never a moderation note.
fn comment_content(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(comment) = &context.comment else {
        return Vec::new();
    };
    if is_empty_string(&comment.content) {
        return Vec::new();
    }
    let text = comment_text(context.site, &comment.content);
    [
        format!(
            "<div {}>",
            block.wrapper(context, &align_link_classes(block, Vec::new()))
        )
        .as_bytes(),
        &text,
        b"</div>",
    ]
    .concat()
}

/// The `comment_text` filters over a comment's content.
pub(crate) fn comment_text(site: &dyn Site, content: &[u8]) -> Vec<u8> {
    let text = make_clickable(content, &site.home_url(), true);
    let text = convert_chars(&wptexturize(&text, &CORE_SHORTCODE_TAGS));
    let text = force_balance_tags(&site_smilies(site, &text));
    capital_p_dangit(&wpautop(&text, true))
}

/// `render_block_core_comment_date()`: the comment's date in `format`,
/// or the site's, or how long ago, linked to the comment if `isLink`.
fn comment_date(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(comment) = &context.comment else {
        return Vec::new();
    };
    let class: &[u8] = if block.has_link_color() {
        b"has-link-color"
    } else {
        b""
    };
    let zone = context.zone();
    let format = block
        .attribute("format")
        .filter(|v| !v.is_empty())
        .map(Value::to_php_string);
    let mut date = match format.as_deref() {
        Some(b"human-diff") => {
            let from = date::stored_unix(&comment.date, zone);
            format!("{} ago", date::human_time_diff(from, context.request.now)).into_bytes()
        }
        Some(format) => {
            date::format(&String::from_utf8_lossy(format), &comment.date, zone).into_bytes()
        }
        None => {
            let site_format = context.site.option("date_format").unwrap_or_default();
            date::format(&String::from_utf8_lossy(&site_format), &comment.date, zone).into_bytes()
        }
    };
    if block.attribute("isLink").is_some_and(|v| !v.is_empty()) {
        date = [
            &b"<a href=\""[..],
            &esc_url(&comment_link(context, comment)),
            b"\">",
            &date,
            b"</a>",
        ]
        .concat();
    }
    let datetime = esc_attr(date::format("c", &comment.date, zone).as_bytes());
    [
        format!(
            "<div {}><time datetime=\"{datetime}\">",
            block.wrapper(context, class)
        )
        .as_bytes(),
        &date,
        b"</time></div>",
    ]
    .concat()
}

/// `get_comment_link( $comment )` with comment paging off: the post's
/// permalink and the comment's anchor.
pub(crate) fn comment_link(context: &Context<'_>, comment: &Comment) -> Vec<u8> {
    let permalink = context
        .site
        .post(comment.post_id)
        .map(|post| context.permalink(&post))
        .unwrap_or_default();
    [permalink, format!("#comment-{}", comment.id).into_bytes()].concat()
}

/// `get_comment_author( $comment )`: the author, else the commenting
/// user's display name, else "Anonymous".
pub(crate) fn comment_author(context: &Context<'_>, comment: &Comment) -> Vec<u8> {
    if !is_empty_string(&comment.author) {
        return comment.author.clone();
    }
    let user = (comment.user_id != 0)
        .then(|| context.site.user(comment.user_id))
        .flatten();
    user.map_or(b"Anonymous".to_vec(), |user| user.display_name)
}

/// `get_comment_author_url( $comment )`: the author's site, with `http://`
/// alone counting as none.
pub(crate) fn comment_author_url(comment: &Comment) -> Vec<u8> {
    let url: &[u8] = if comment.author_url == b"http://" {
        b""
    } else {
        &comment.author_url
    };
    esc_url_protocols(url, &["http", "https"])
}

/// `get_comment_author_link( $comment )`: the author, linked to their site
/// when there is one (an escaped `http://` is none), `external nofollow` unless `wp_is_internal_link()`
/// finds the site's own host behind a scheme (the URL allows only http and
/// https, so a protocol relative one is external).
pub(crate) fn comment_author_link(context: &Context<'_>, comment: &Comment) -> Vec<u8> {
    let author = comment_author(context, comment);
    let url = comment_author_url(comment);
    if url.is_empty() || url == b"http://" {
        return author;
    }
    let lower = url.to_ascii_lowercase();
    let home = url_host(&context.site.home_url()).map(|host| host.to_ascii_lowercase());
    let internal = url_scheme(&lower).is_some() && url_host(&lower) == home;
    let rel: &[u8] = if internal {
        b"ugc"
    } else {
        b"ugc external nofollow"
    };
    [
        &b"<a href=\""[..],
        &url,
        b"\" class=\"url\" rel=\"",
        rel,
        b"\">",
        &author,
        b"</a>",
    ]
    .concat()
}

/// `render_block_core_comment_author_name()`: the author, linked to their
/// site when `isLink` and `linkTarget` are set.
fn comment_author_name(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(comment) = &context.comment else {
        return Vec::new();
    };
    let mut name = comment_author(context, comment);
    let url = comment_author_url(comment);
    let target = block.attribute("linkTarget").filter(|v| !v.is_empty());
    if let (false, true, Some(target)) = (
        url.is_empty(),
        block.attribute("isLink").is_some_and(|v| !v.is_empty()),
        target,
    ) {
        name = [
            &b"<a rel=\"external nofollow ugc\" href=\""[..],
            &esc_url(&url),
            b"\" target=\"",
            esc_attr(&target.to_php_string()).as_bytes(),
            b"\" >",
            &name,
            b"</a>",
        ]
        .concat();
    }
    [
        format!(
            "<div {}>",
            block.wrapper(context, &align_link_classes(block, Vec::new()))
        )
        .as_bytes(),
        &name,
        b"</div>",
    ]
    .concat()
}

/// `render_block_core_comment_edit_link()`: only for those who may edit the
/// comment, and visitors may not.
fn comment_edit_link(_context: &Context<'_>, _block: &Block<'_>) -> Vec<u8> {
    vec![]
}

/// `render_block_core_comment_reply_link()` with
/// `get_comment_reply_link()`: a reply link while the thread can go deeper
/// and the post takes comments.
fn comment_reply_link(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(comment) = &context.comment else {
        return Vec::new();
    };
    let option = |name| context.site.option(name).unwrap_or_default();
    if is_empty_string(&option("thread_comments")) {
        return Vec::new();
    }
    let max_depth = Value::String(option("thread_comments_depth")).to_int();
    if max_depth <= i64::try_from(context.comment_depth).unwrap_or(i64::MAX) {
        return Vec::new();
    }
    let Some(post) = context
        .site
        .post(comment.post_id)
        .filter(|post| comments_open(context, post))
    else {
        return Vec::new();
    };
    let permalink = context.permalink(&post);
    let author = comment_author(context, comment);
    let reply_to = [&b"Reply to "[..], &author].concat();
    let link = if Value::String(option("comment_registration")).is_empty() {
        let separator: &[u8] = if permalink.contains(&b'?') {
            b"&"
        } else {
            b"?"
        };
        let href = esc_url(
            &[
                &permalink[..],
                separator,
                format!("replytocom={}", comment.id).as_bytes(),
            ]
            .concat(),
        );
        let data = format!(
            "data-commentid=\"{}\" data-postid=\"{}\" data-belowelement=\"comment-{}\" data-respondelement=\"respond\" data-replyto=\"{}\"",
            comment.id,
            post.id,
            comment.id,
            esc_attr(&reply_to)
        );
        [
            &b"<a rel=\"nofollow\" class=\"comment-reply-link\" href=\""[..],
            &href,
            b"#respond\" ",
            data.as_bytes(),
            b" aria-label=\"",
            esc_attr(&reply_to).as_bytes(),
            b"\">Reply</a>",
        ]
        .concat()
    } else {
        // wp_login_url( get_permalink() ), for the global post: the main query's
        let current = context.global_post();
        let login = wp_login_url(
            context,
            &current
                .map(|post| context.permalink(&post))
                .unwrap_or_default(),
        );
        [
            &b"<a rel=\"nofollow\" class=\"comment-reply-login\" href=\""[..],
            &esc_url(&login),
            b"\">Log in to Reply</a>",
        ]
        .concat()
    };
    [
        format!(
            "<div {}>",
            block.wrapper(context, &align_link_classes(block, Vec::new()))
        )
        .as_bytes(),
        &link,
        b"</div>",
    ]
    .concat()
}

/// `comments_open( $post )`, with `_close_comments_for_old_post()`.
pub(crate) fn comments_open(context: &Context<'_>, post: &Post) -> bool {
    let option = |name| context.site.option(name).unwrap_or_default();
    if post.comment_status != "open" {
        return false;
    }
    if is_empty_string(&option("close_comments_for_old_posts")) {
        return true;
    }
    let days = Value::String(option("close_comments_days_old")).to_int();
    if days == 0 || post.post_type != "post" || post.date_gmt == "0000-00-00 00:00:00" {
        return true;
    }
    let age = context.request.now - date::stored_unix(&post.date_gmt, Zone::Utc);
    age <= days.saturating_mul(86_400)
}

/// `wp_login_url( $redirect )`.
fn wp_login_url(context: &Context<'_>, redirect: &[u8]) -> Vec<u8> {
    let login = context.site_url(b"/wp-login.php");
    if is_empty_string(redirect) {
        return login;
    }
    add_query_arg(&[(b"redirect_to", Some(&urlencode(redirect)))], &login)
}

/// `render_block_core_comments()`: the saved markup, where the post takes
/// comments or has some. The legacy form, which runs `comments_template()`,
/// is not ported and renders nothing.
fn comments(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(post) = context.post_id.map(|id| context.site.post(id)) else {
        return Vec::new();
    };
    // comments_open() and get_comments_number()
    let open = post
        .as_ref()
        .is_some_and(|post| comments_open(context, post));
    let count = post.as_ref().map_or(0, |post| post.comment_count);
    if !open && count == 0 {
        return Vec::new();
    }
    let legacy = block.name == "core/post-comments"
        || block.attribute("legacy").is_some_and(|v| !v.is_empty());
    if legacy {
        return Vec::new();
    }
    block.content.to_vec()
}

/// `render_block_core_comments_title()`: how many responses the post has,
/// with its title.
fn comments_title(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let post = context.post();
    // post_password_required() for a visitor without the cookie
    if post.as_ref().is_some_and(|post| !post.password.is_empty()) {
        return Vec::new();
    }
    // Without a post the count is the integer 0, which PHP's `'0' ===`
    // check lets through.
    let count = post.as_ref().map_or(0, |post| post.comment_count);
    if post.is_some() && count == 0 {
        return Vec::new();
    }
    let flag = |name| block.attribute(name).is_some_and(|v| !v.is_empty());
    let title =
        String::from_utf8_lossy(&post.as_ref().map(the_title).unwrap_or_default()).into_owned();
    let number = number_format(count);
    let text = match (flag("showCommentsCount"), flag("showPostTitle"), count == 1) {
        (true, true, true) => format!("One response to &#8220;{title}&#8221;"),
        (true, true, false) => format!("{number} responses to &#8220;{title}&#8221;"),
        (true, false, true) => "One response".to_string(),
        (true, false, false) => format!("{number} responses"),
        (false, true, true) => format!("Response to &#8220;{title}&#8221;"),
        (false, true, false) => format!("Responses to &#8220;{title}&#8221;"),
        (false, false, true) => "Response".to_string(),
        (false, false, false) => "Responses".to_string(),
    };
    // `level` always has its default of 2.
    let level = block
        .attribute("level")
        .map(Value::to_php_string)
        .unwrap_or_default();
    let tag = [&b"h"[..], &level].concat();
    let class = match block.attribute("textAlign").filter(|v| !v.is_empty()) {
        Some(align) => [&b"has-text-align-"[..], &align.to_php_string()].concat(),
        None => Vec::new(),
    };
    [
        &b"<"[..],
        &tag,
        b" id=\"comments\" ",
        block.wrapper(context, &class).as_bytes(),
        b">",
        text.as_bytes(),
        b"</",
        &tag,
        b">",
    ]
    .concat()
}

/// `number_format_i18n( $number )` in English: thousands separated by
/// commas.
pub fn number_format(number: impl Into<i128>) -> String {
    let number = number.into();
    let digits = number.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    if number < 0 { format!("-{out}") } else { out }
}

/// `render_block_core_post_navigation_link()`: on a single post, a link to
/// the previous or next post, labelled, titled and with an arrow as set.
fn post_navigation_link(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    if !context.request.singular {
        return Vec::new();
    }
    let kind = block
        .attribute("type")
        .map_or(b"next".to_vec(), Value::to_php_string);
    let previous = match kind.as_slice() {
        b"next" => false,
        b"previous" => true,
        _ => return Vec::new(),
    };
    let mut classes = [&b"post-navigation-link-"[..], &kind].concat();
    if let Some(align) = block.attribute("textAlign") {
        classes.extend_from_slice(b" has-text-align-");
        classes.extend(align.to_php_string());
    }
    let upload_host = context.upload_host();
    let span = |class: &str, text: &[u8]| {
        [
            format!("<span class=\"post-navigation-link__{class}\">").as_bytes(),
            text,
            b"</span>",
        ]
        .concat()
    };
    let mut format = b"%link".to_vec();
    let mut link: Vec<u8> = if previous {
        b"Previous".to_vec()
    } else {
        b"Next".to_vec()
    };
    let mut label = Vec::new();
    if let Some(given) = block.attribute("label").filter(|v| !v.is_empty()) {
        label = given.to_php_string();
        link = label.clone();
    }
    if block.attribute("showTitle").is_some_and(|v| !v.is_empty()) {
        if block.attribute("linkLabel").is_none_or(Value::is_empty) {
            if !is_empty_string(&label) {
                format = [
                    &span("label", &wp_kses_post(&label, &upload_host))[..],
                    b" %link",
                ]
                .concat();
            }
            link = b"%title".to_vec();
        } else {
            if is_empty_string(&label) {
                label = if previous {
                    b"Previous:".to_vec()
                } else {
                    b"Next:".to_vec()
                };
            }
            link = [
                &span("label", &wp_kses_post(&label, &upload_host))[..],
                b" ",
                &span("title", b"%title"),
            ]
            .concat();
        }
    }
    let arrow = block.attribute("arrow").map(Value::to_php_string);
    let arrow_text: Option<&str> = match (arrow.as_deref(), previous) {
        (Some(b"arrow"), false) => Some("→"),
        (Some(b"arrow"), true) => Some("←"),
        (Some(b"chevron"), false) => Some("»"),
        (Some(b"chevron"), true) => Some("«"),
        _ => None,
    };
    if let (Some(text), Some(arrow)) = (arrow_text, &arrow) {
        let arrow = String::from_utf8_lossy(arrow);
        format = if previous {
            format!("<span class=\"wp-block-post-navigation-link__arrow-previous is-arrow-{arrow}\" aria-hidden=\"true\">{text}</span>%link").into_bytes()
        } else {
            format!("%link<span class=\"wp-block-post-navigation-link__arrow-next is-arrow-{arrow}\" aria-hidden=\"true\">{text}</span>").into_bytes()
        };
    }
    let taxonomy = block
        .attribute("taxonomy")
        .filter(|v| !v.is_empty())
        .map(Value::to_php_string);
    let content = adjacent_post_link(context, &format, &link, previous, taxonomy.as_deref());
    [
        format!("<div {}>", block.wrapper(context, &classes)).as_bytes(),
        &content,
        b"</div>",
    ]
    .concat()
}

/// `get_adjacent_post_link()`: `format` with `%link` as the linked `link`,
/// whose `%title` and `%date` are the adjacent post's.
fn adjacent_post_link(
    context: &Context<'_>,
    format: &[u8],
    link: &[u8],
    previous: bool,
    taxonomy: Option<&[u8]>,
) -> Vec<u8> {
    let Some(current) = context.post() else {
        return Vec::new();
    };
    // The previous link of an attachment page goes to its parent, but
    // attachment pages are not routed yet, so is_attachment() is false.
    let Some(post) = adjacent_post(context, &current, previous, taxonomy) else {
        return Vec::new();
    };
    let title: &[u8] = match (is_empty_string(&post.title), previous) {
        (false, _) => &post.title,
        (true, true) => b"Previous Post",
        (true, false) => b"Next Post",
    };
    let title = title_filters(title);
    let date_format = context.site.option("date_format").unwrap_or_default();
    let date = date::format(
        &String::from_utf8_lossy(&date_format),
        &post.date,
        context.zone(),
    );
    let rel = if previous { "prev" } else { "next" };
    let anchor = [
        &b"<a href=\""[..],
        &context.permalink(&post),
        format!("\" rel=\"{rel}\">").as_bytes(),
    ]
    .concat();
    let inlink = replace(&replace(link, b"%title", &title), b"%date", date.as_bytes());
    replace(format, b"%link", &[&anchor[..], &inlink, b"</a>"].concat())
}

/// `get_adjacent_post()`: nothing for a taxonomy that does not exist, or,
/// within the same terms, one the post is not in.
fn adjacent_post(
    context: &Context<'_>,
    post: &Post,
    previous: bool,
    taxonomy: Option<&[u8]>,
) -> Option<Post> {
    let Some(taxonomy) = taxonomy else {
        return context.site.adjacent_post(post, previous, None);
    };
    let taxonomy = String::from_utf8_lossy(taxonomy);
    // taxonomy_exists() and is_object_in_taxonomy()
    let object_type = match taxonomy.as_ref() {
        "category" | "post_tag" | "post_format" => "post",
        "nav_menu" => "nav_menu_item",
        "link_category" => "link",
        "wp_template_part_area" => "wp_template_part",
        "wp_pattern_category" => "wp_block",
        "wp_theme" => {
            return matches!(
                post.post_type.as_str(),
                "wp_template" | "wp_template_part" | "wp_global_styles"
            )
            .then(|| same_terms(context, post, previous, &taxonomy))
            .flatten();
        }
        _ => return None,
    };
    if post.post_type != object_type {
        return None;
    }
    same_terms(context, post, previous, &taxonomy)
}

/// The adjacent post sharing one of `post`'s terms in `taxonomy`.
fn same_terms(context: &Context<'_>, post: &Post, previous: bool, taxonomy: &str) -> Option<Post> {
    let ids: Vec<u64> = context
        .site
        .post_terms(post.id, taxonomy)
        .iter()
        .map(|term| term.id)
        .collect();
    if ids.is_empty() {
        return None;
    }
    context
        .site
        .adjacent_post(post, previous, Some((taxonomy, &ids)))
}

/// `render_block_core_post_terms()`: the post's terms in a taxonomy that
/// is publicly viewable, linked, between the prefix and suffix.
fn post_terms(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let (Some(post_id), Some(taxonomy)) = (context.post_id, block.attribute("term")) else {
        return Vec::new();
    };
    let taxonomy = String::from_utf8_lossy(&taxonomy.to_php_string()).into_owned();
    // is_taxonomy_viewable()
    if !matches!(taxonomy.as_str(), "category" | "post_tag" | "post_format") {
        return Vec::new();
    }
    let terms = context.site.post_terms(post_id, &taxonomy);
    if terms.is_empty() {
        return Vec::new();
    }
    let classes = align_link_classes(
        block,
        vec![[&b"taxonomy-"[..], taxonomy.as_bytes()].concat()],
    );
    let separator = match block.attribute("separator").filter(|v| !v.is_empty()) {
        Some(separator) => separator.to_php_string(),
        None => b" ".to_vec(),
    };
    let affix = |name, class: &str| match block.attribute(name).filter(|v| !v.is_empty()) {
        Some(text) => [
            format!("<span class=\"wp-block-post-terms__{class}\">").as_bytes(),
            &text.to_php_string(),
            b"</span>",
        ]
        .concat(),
        None => Vec::new(),
    };
    let upload_host = context.upload_host();
    let before = wp_kses_post(
        &[
            format!("<div {}>", block.wrapper(context, &classes)).as_bytes(),
            &affix("prefix", "prefix"),
        ]
        .concat(),
        &upload_host,
    );
    let after = wp_kses_post(
        &[&affix("suffix", "suffix")[..], b"</div>"].concat(),
        &upload_host,
    );
    let separator = [
        &b"<span class=\"wp-block-post-terms__separator\">"[..],
        esc_html(&separator).as_bytes(),
        b"</span>",
    ]
    .concat();
    let links: Vec<Vec<u8>> = terms
        .iter()
        .map(|term| {
            let link = esc_url(&crate::permalinks::term_link(context.site, &taxonomy, term));
            [
                &b"<a href=\""[..],
                &link,
                b"\" rel=\"tag\">",
                &term.name,
                b"</a>",
            ]
            .concat()
        })
        .collect();
    [before, links.join(&separator[..]), after].concat()
}

/// The classes post and comment blocks share: `has-text-align-*` when `textAlign` is
/// set (`isset()`, where the site blocks test `empty()`), and
/// `has-link-color` when the link has a color.
fn align_link_classes(block: &Block<'_>, mut classes: Vec<Vec<u8>>) -> Vec<u8> {
    classes.extend(align_class(block));
    if block.has_link_color() {
        classes.push(b"has-link-color".to_vec());
    }
    classes.join(&b' ')
}

/// `render_block_core_post_excerpt()` around `block.content`, the excerpt
/// `get_the_excerpt()` gave.
fn post_excerpt(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let more_text = block
        .attribute("moreText")
        .filter(|text| !text.is_empty())
        .map(|text| {
            let href = context
                .post()
                .map(|post| esc_url(&context.permalink(&post)));
            let label = wp_kses_post(&text.to_php_string(), &context.upload_host());
            [
                b"<a class=\"wp-block-post-excerpt__more-link\" href=\"".as_slice(),
                &href.unwrap_or_default(),
                b"\">",
                &label,
                b"</a>",
            ]
            .concat()
        })
        .unwrap_or_default();
    let length = block.attribute("excerptLength").map_or(0, Value::to_int);
    let excerpt = wp_trim_words(block.content, length, b"&hellip;");
    let wrapper = block.wrapper(context, &align_link_classes(block, Vec::new()));
    let new_line = block
        .attribute("showMoreOnNewLine")
        .is_none_or(|v| !v.is_empty());
    let end = if new_line && !more_text.is_empty() {
        [
            b"</p><p class=\"wp-block-post-excerpt__more-text\">".as_slice(),
            &more_text,
            b"</p>",
        ]
        .concat()
    } else {
        [b" ".as_slice(), &more_text, b"</p>"].concat()
    };
    let content = [
        b"<p class=\"wp-block-post-excerpt__excerpt\">".as_slice(),
        &excerpt,
        &end,
    ]
    .concat();
    element(b"div", &wrapper, &content)
}

/// `render_block_core_read_more()`: a link to the `postId` context's post.
fn read_more(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(id) = context.post_id else {
        return Vec::new();
    };
    let post = context.post();
    // get_the_title() is empty for a post there is not.
    let mut title = post.as_ref().map(the_title).unwrap_or_default();
    if title.is_empty() {
        title = format!("untitled post {id}").into_bytes();
    }
    let justify = block
        .attribute("justifyContent")
        .filter(|v| !v.is_empty())
        .map(|v| [b"is-justified-", &v.to_php_string()[..]].concat())
        .unwrap_or_default();
    let text = block
        .attribute("content")
        .filter(|v| !v.is_empty())
        .map_or_else(
            || b"Read more".to_vec(),
            |v| wp_kses_post(&v.to_php_string(), &context.upload_host()),
        );
    let href = post.map(|post| esc_url(&context.permalink(&post)));
    let target = block.attribute("linkTarget").map(Value::to_php_string);
    [
        b"<a ".as_slice(),
        block.wrapper(context, &justify).as_bytes(),
        b" href=\"",
        &href.unwrap_or_default(),
        b"\" target=\"",
        esc_attr(&target.unwrap_or_default()).as_bytes(),
        b"\">",
        &text,
        b"<span class=\"screen-reader-text\">: ",
        &title,
        b"</span></a>",
    ]
    .concat()
}

/// `render_block_core_post_comments_count()`.
fn post_comments_count(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    if context.post_id.is_none() {
        return Vec::new();
    }
    let wrapper = block.wrapper(context, &align_class(block).unwrap_or_default());
    // get_comments_number() is 0 for a post there is not.
    let count = context
        .post()
        .map_or(0, |post| post.comment_count)
        .to_string();
    element(b"div", &wrapper, count.as_bytes())
}

/// `render_block_core_post_comments_link()`: the comment count, linked to
/// the comments, while comments are open.
fn post_comments_link(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(post) = context.post().filter(|post| comments_open(context, post)) else {
        return Vec::new();
    };
    let (count, anchor) = match post.comment_count {
        0 => ("No comments".to_string(), "#respond"),
        1 => ("1 comment".to_string(), "#comments"),
        n => (format!("{} comments", number_format(n)), "#comments"),
    };
    let link = [&context.permalink(&post)[..], anchor.as_bytes()].concat();
    let wrapper = block.wrapper(context, &align_link_classes(block, Vec::new()));
    let content = [
        b"<a href=".as_slice(),
        &esc_url(&link),
        b">",
        count.as_bytes(),
        b"<span class=\"screen-reader-text\"> on ",
        &the_title(&post),
        b"</span></a>",
    ]
    .concat();
    element(b"div", &wrapper, &content)
}

/// `has-text-align-<textAlign>` when `textAlign` is set (`isset()`).
fn align_class(block: &Block<'_>) -> Option<Vec<u8>> {
    let align = block.attribute("textAlign")?;
    Some([b"has-text-align-", &align.to_php_string()[..]].concat())
}

/// `render_block_core_post_title()`.
fn post_title(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(post) = context.post() else {
        return Vec::new();
    };
    let mut title = the_title(&post);
    if is_empty_string(&title) {
        return Vec::new();
    }
    let tag = tag(block.attribute("level"), b"h2");
    if block.attribute("isLink").is_some_and(|v| !v.is_empty()) {
        let rel = match block.attribute("rel").filter(|v| !v.is_empty()) {
            Some(rel) => format!("rel=\"{}\"", esc_attr(&rel.to_php_string())),
            None => String::new(),
        };
        let target = block
            .attribute("linkTarget")
            .map(Value::to_php_string)
            .unwrap_or_default();
        title = [
            b"<a href=\"".as_slice(),
            &esc_url(&context.permalink(&post)),
            b"\" target=\"",
            esc_attr(&target).as_bytes(),
            b"\" ",
            rel.as_bytes(),
            b">",
            &title,
            b"</a>",
        ]
        .concat();
    }
    element(
        &tag,
        &block.wrapper(context, &align_link_classes(block, Vec::new())),
        &title,
    )
}

/// The post, if public and without a password, as a visitor may read it.
pub(crate) fn public(post: Option<Post>) -> Option<Post> {
    post.filter(|post| post.status == "publish" && post.password.is_empty())
}

/// `_block_bindings_post_data_get_value()` for `post`: `None` unless it is
/// public and has no password.
pub(crate) fn post_data(
    context: &Context<'_>,
    post: Option<Post>,
    field: &[u8],
) -> Option<Vec<u8>> {
    let post = public(post)?;
    let zone = context.zone();
    let iso = |stored: &str| esc_attr(date::format("c", stored, zone).as_bytes()).into_bytes();
    match field {
        b"date" => Some(iso(&post.date)),
        // Only a modified date later than the publish date shows.
        b"modified" => Some(
            if date::stored_unix(&post.modified, zone) > date::stored_unix(&post.date, zone) {
                iso(&post.modified)
            } else {
                Vec::new()
            },
        ),
        b"link" => Some(esc_url(&context.permalink(&post))),
        _ => None,
    }
}

/// `render_block_core_post_date()`, with the post data binding resolved as
/// `WP_Block` resolves it before rendering.
fn post_date(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let mut classes = Vec::new();
    // Bound, `datetime` is what block bindings computed, if anything.
    let bound = block
        .attributes
        .get_path(&["metadata", "bindings", "datetime"])
        .is_some_and(|b| isset(b.get_str("source")) && isset(b.get_str("args")));
    let datetime = match block.attribute("datetime") {
        Some(datetime) => Some(datetime.to_php_string()),
        None if bound => None,
        None => {
            let modified = block
                .attribute("displayType")
                .map(Value::to_php_string)
                .as_deref()
                == Some(b"modified");
            if modified {
                classes.push(b"wp-block-post-date__modified-date".to_vec());
            }
            post_data(
                context,
                context.post(),
                if modified { b"modified" } else { b"date" },
            )
        }
    };
    let Some(datetime) = datetime.filter(|d| !is_empty_string(d)) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&datetime).into_owned();
    let timestamp = date::strtotime(&text);
    let format = block
        .attribute("format")
        .filter(|v| !v.is_empty())
        .map(Value::to_php_string);
    let formatted = match format.as_deref() {
        Some(b"human-diff") => {
            // A time `strtotime()` cannot read counts as 0, as PHP's false.
            let timestamp = timestamp.unwrap_or(0);
            let diff = date::human_time_diff(timestamp, context.request.now);
            if timestamp > context.request.now {
                format!("{diff} from now")
            } else {
                format!("{diff} ago")
            }
        }
        _ => {
            let format =
                format.unwrap_or_else(|| context.site.option("date_format").unwrap_or_default());
            timestamp
                .map(|t| date::format_unix(&String::from_utf8_lossy(&format), t, context.zone()))
                .unwrap_or_default()
        }
    };
    let mut time = [
        b"<time datetime=\"".as_slice(),
        esc_attr(&datetime).as_bytes(),
        b"\">",
        esc_html(formatted.as_bytes()).as_bytes(),
        b"</time>",
    ]
    .concat();
    if block.attribute("isLink").is_some_and(|v| !v.is_empty()) && context.post_id.is_some() {
        let link = context
            .post()
            .map(|post| context.permalink(&post))
            .unwrap_or_default();
        time = [
            b"<a href=\"".as_slice(),
            &esc_url(&link),
            b"\">",
            &time,
            b"</a>",
        ]
        .concat();
    }
    let wrapper = block.wrapper(context, &align_link_classes(block, classes));
    [
        b"<div ".as_slice(),
        wrapper.as_bytes(),
        b">",
        &time,
        b"</div>",
    ]
    .concat()
}

/// `has-link-color` when the block's link has a color, else empty.
fn link_color_class(block: &Block<'_>) -> &'static [u8] {
    if block.has_link_color() {
        b"has-link-color"
    } else {
        b""
    }
}

/// `render_block_core_query_no_results()`: the content, when the query
/// finds nothing.
fn query_no_results(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    if php_trim(block.content).is_empty() || !query_posts(context).is_empty() {
        return Vec::new();
    }
    let wrapper = block.wrapper(context, link_color_class(block));
    [
        b"<div ".as_slice(),
        wrapper.as_bytes(),
        b">",
        block.content,
        b"</div>",
    ]
    .concat()
}

/// `render_block_core_query_total()`: the main query's count, or that of a
/// query of its own at its page.
fn query_total(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let request = context.request;
    // PHP's integers: `(int) get( 'posts_per_page' )` may be -1.
    let (found, per_page, page) = if inherits(context) {
        let option = context.site.option("posts_per_page").unwrap_or_default();
        let page = i64::try_from(request.paged.max(1)).unwrap_or(i64::MAX);
        (request.found_posts, Value::String(option).to_int(), page)
    } else {
        // `(int) ( $_GET[ $key ] ?? 1 )`: an empty page is page 0 here.
        let (key, _) = own_page(context);
        let key = String::from_utf8_lossy(&key).into_owned();
        let page = request.get.get_str(&key).map_or(1, Value::to_int);
        let (found, per_page, _) = own_counts(context, page);
        (found, per_page, page)
    };
    let found = i64::try_from(found).unwrap_or(i64::MAX);
    let start = if found == 0 {
        0
    } else {
        (page - 1) * per_page + 1
    };
    let end = (start + per_page - 1).min(found);
    let text = match block
        .attribute("displayType")
        .map(Value::to_php_string)
        .as_deref()
    {
        Some(b"range-display") if start == end => format!("Displaying {start} of {found}"),
        Some(b"range-display") => format!("Displaying {start} – {end} of {found}"),
        _ if found == 1 => "1 result found".to_string(),
        _ => format!("{found} results found"),
    };
    element(b"div", &block.wrapper(context, b""), text.as_bytes())
}

/// `render_block_core_query_pagination()`.
fn query_pagination(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    pagination_nav(context, block, b"Pagination")
}

/// `render_block_core_comments_pagination()`.
fn comments_pagination(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    // post_password_required() for the global post
    if context
        .global_post()
        .is_some_and(|post| !post.password.is_empty())
    {
        return Vec::new();
    }
    pagination_nav(context, block, b"Comments pagination")
}

/// A pagination block's content in a `<nav>` labelled `label`; nothing
/// when the content is blank.
fn pagination_nav(context: &Context<'_>, block: &Block<'_>, label: &[u8]) -> Vec<u8> {
    if php_trim(block.content).is_empty() {
        return Vec::new();
    }
    let extra = [
        ("aria-label", Value::String(label.to_vec())),
        ("class", Value::String(link_color_class(block).to_vec())),
    ];
    let wrapper =
        get_block_wrapper_attributes(block.name, block.parsed, &extra, &context.theme.global);
    [
        b"<nav ".as_slice(),
        wrapper.as_bytes(),
        b">",
        block.content,
        b"</nav>",
    ]
    .concat()
}

/// `get_post_type_object( $type )->labels->singular_name` for the core
/// post types.
const POST_TYPE_NAMES: [(&[u8], &[u8]); 16] = [
    (b"post", b"Post"),
    (b"page", b"Page"),
    (b"attachment", b"Media"),
    (b"revision", b"Revision"),
    (b"nav_menu_item", b"Navigation Menu Item"),
    (b"custom_css", b"Custom CSS"),
    (b"customize_changeset", b"Changeset"),
    (b"oembed_cache", b"oEmbed Response"),
    (b"user_request", b"User Request"),
    (b"wp_block", b"Pattern"),
    (b"wp_template", b"Template"),
    (b"wp_template_part", b"Template Part"),
    (b"wp_global_styles", b"Global Styles"),
    (b"wp_navigation", b"Navigation Menu"),
    (b"wp_font_family", b"Font Family"),
    (b"wp_font_face", b"Font Face"),
];

/// `render_block_core_query_title()`.
fn query_title(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let request = context.request;
    let kind = block.attribute("type").map(Value::to_php_string);
    let post_type = context
        .query
        .as_ref()
        .and_then(|q| q.query.get_str("postType"))
        .filter(|v| **v != Value::Null)
        .map(Value::to_php_string)
        .or_else(|| context.post().map(|post| post.post_type.into_bytes()));
    let shown = match kind.as_deref() {
        None => false,
        Some(b"archive") => request.archive.is_some(),
        Some(b"search") => request.search.is_some(),
        Some(b"post-type") => post_type.as_deref().is_some_and(|t| !is_empty_string(t)),
        Some(_) => true,
    };
    if !shown {
        return Vec::new();
    }
    let show_prefix = block.attribute("showPrefix").is_none_or(|v| !v.is_empty());
    let mut title = Vec::new();
    if let Some(archive) = &request.archive {
        let date_format = context.site.option("date_format").unwrap_or_default();
        let posted = request.posts.first().map_or("", |post| post.date.as_str());
        title = archive_title(archive, posted, context.zone(), &date_format, show_prefix);
    }
    if let Some(search) = &request.search {
        title = b"Search results".to_vec();
        if block
            .attribute("showSearchTerm")
            .is_some_and(|v| !v.is_empty())
        {
            title = [
                b"Search results for: &#8220;".as_slice(),
                esc_attr(search).as_bytes(),
                b"&#8221;",
            ]
            .concat();
        }
    }
    if kind.as_deref() == Some(b"post-type") {
        let Some((_, name)) = POST_TYPE_NAMES
            .iter()
            .find(|(slug, _)| Some(*slug) == post_type.as_deref())
        else {
            return Vec::new();
        };
        let name = *name;
        title = if show_prefix {
            [b"Post Type: &#8220;", name, b"&#8221;"].concat()
        } else {
            name.to_vec()
        };
    }
    let tag = tag(block.attribute("level"), b"h1");
    element(
        &tag,
        &block.wrapper(context, &block.text_align_class()),
        &title,
    )
}

/// The tag for a `level` attribute: `p` for 0, else `'h' . (int) $level`,
/// and `default` without one.
fn tag(level: Option<&Value>, default: &[u8]) -> Vec<u8> {
    match level {
        Some(Value::Int(0)) => b"p".to_vec(),
        // Every caller registers a default level, so `default` is a backstop.
        _ => level.map_or(default.to_vec(), |level| {
            format!("h{}", level.to_int()).into_bytes()
        }),
    }
}

/// `render_block_core_shortcode()`: the content in paragraphs; shortcodes
/// themselves are not run.
fn shortcode(_: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    wpautop(block.content, true)
}

/// `render_block_core_navigation_overlay_close()`: a close button with an
/// icon, its text, or both.
fn navigation_overlay_close(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let text = block
        .attribute("text")
        .filter(|text| !text.is_empty())
        .map_or(b"Close".to_vec(), Value::to_php_string);
    let mode = block
        .attribute("displayMode")
        .filter(|mode| !mode.is_empty())
        .map_or(b"icon".to_vec(), Value::to_php_string);
    let show_icon = mode == b"both" || mode == b"icon";
    let show_text = mode == b"both" || mode == b"text";
    let mut button = Vec::new();
    if show_icon {
        button.extend_from_slice(br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="M13 11.8l6.1-6.3-1.1-1-6.1 6.2-6.1-6.2-1.1 1 6.1 6.3-6.5 6.7 1.1 1 6.5-6.6 6.5 6.6 1.1-1z" /></svg>"#);
    }
    if show_text {
        let text = crate::kses::wp_kses_post(&text, &context.upload_host());
        button.extend_from_slice(b"<span class=\"wp-block-navigation-overlay-close__text\">");
        button.extend_from_slice(&text);
        button.extend_from_slice(b"</span>");
    }
    let label = if show_text {
        ""
    } else {
        "aria-label=\"Close\""
    };
    [
        format!(
            "<button {} type=\"button\" {label} >",
            block.wrapper(context, b"")
        )
        .as_bytes(),
        &button,
        b"</button>",
    ]
    .concat()
}

/// `sprintf( '<%1$s %2$s>%3$s</%1$s>', $tag, $wrapper, $content )`.
fn element(tag: &[u8], wrapper: &str, content: &[u8]) -> Vec<u8> {
    [
        b"<",
        tag,
        b" ",
        wrapper.as_bytes(),
        b">",
        content,
        b"</",
        tag,
        b">",
    ]
    .concat()
}

/// `render_block_core_site_title()`.
fn site_title(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let title = context.site.option("blogname").unwrap_or_default();
    if is_empty_string(php_trim(&title)) {
        return Vec::new();
    }
    let mut classes = block.text_align_class();
    if block.has_link_color() {
        classes.extend_from_slice(b" has-link-color");
    }
    let tag = tag(block.attribute("level"), b"h1");
    let content = if block.attribute("isLink").is_some_and(|v| !v.is_empty()) {
        let request = context.request;
        let current = !request.is_paged() && request.is_site_home(context.site);
        let target = block
            .attribute("linkTarget")
            .filter(|v| !v.is_empty())
            .map_or(b"_self".to_vec(), Value::to_php_string);
        [
            b"<a href=\"".as_slice(),
            &esc_url(&context.site.home_url()),
            b"\" target=\"",
            esc_attr(&target).as_bytes(),
            b"\" rel=\"home\"",
            if current {
                b" aria-current=\"page\""
            } else {
                b""
            },
            b">",
            esc_html(&title).as_bytes(),
            b"</a>",
        ]
        .concat()
    } else {
        esc_html(&title).into_bytes()
    };
    element(&tag, &block.wrapper(context, php_trim(&classes)), &content)
}

/// `render_block_core_site_tagline()`. The tagline prints as stored.
fn site_tagline(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let tagline = context.site.option("blogdescription").unwrap_or_default();
    if is_empty_string(&tagline) {
        return Vec::new();
    }
    let wrapper = block.wrapper(context, &block.text_align_class());
    let tag = tag(block.attribute("level"), b"p");
    element(&tag, &wrapper, &tagline)
}

/// `render_block_core_footnotes()`: the post's `footnotes` meta, a JSON
/// list of notes, each with a link back to its reference.
fn footnotes(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let Some(post) = context.post().filter(|post| post.password.is_empty()) else {
        return Vec::new();
    };
    let meta = context
        .site
        .post_meta(post.id, "footnotes")
        .unwrap_or_default();
    if is_empty_string(&meta) {
        return Vec::new();
    }
    // An array with entries, as json_decode( $meta, true ) gives it.
    let decoded = hyscribe_phpser::json_decode(&meta);
    let Some(notes) = decoded
        .as_ref()
        .and_then(Value::as_array)
        .filter(|n| !n.is_empty())
    else {
        return Vec::new();
    };
    let upload_host = context.upload_host();
    let mut items = Vec::new();
    for (index, (_, note)) in notes.iter().enumerate() {
        let field = |name| {
            note.get_str(name)
                .map(Value::to_php_string)
                .unwrap_or_default()
        };
        let id = esc_attr(&field("id"));
        items.extend(format!("<li id=\"{id}\">").into_bytes());
        items.extend(wp_kses_post(&field("content"), &upload_host));
        items.extend(
            format!(
                " <a href=\"#{id}-link\" aria-label=\"Jump to footnote reference {}\">↩︎</a></li>",
                index + 1
            )
            .into_bytes(),
        );
    }
    [
        format!("<ol {}>", block.wrapper(context, b"")).as_bytes(),
        &items,
        b"</ol>",
    ]
    .concat()
}

mod author;
mod avatar;
pub use avatar::{comment_avatar, user_avatar};
mod breadcrumbs;
mod button;
pub(crate) mod calendar;
mod comment_form;
mod cover;
mod featured_image;
mod heading_list;
mod icon;
mod image;
mod latest_comments;
mod loginout;
mod site_logo;
pub(crate) mod tag_cloud;
mod term;
mod time_to_read;
mod video;

#[cfg(test)]
mod tests;
