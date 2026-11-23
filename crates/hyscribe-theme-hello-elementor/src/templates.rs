// SPDX-License-Identifier: AGPL-3.0-or-later

//! Hello's PHP templates, file for file: each computes what it prints in
//! PHP's order of calls, then renders its askama template of the same path.
//! A template part PHP includes inline is rendered and appended where it
//! is included, as the SDK keeps markup it did not answer out of templates.

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use askama::Template;
use hyscribe_sdk::assets::{self, AssetKind};
use hyscribe_sdk::i18n;
use hyscribe_sdk::page::{self, Html, Text};
use hyscribe_sdk::{Value, hooks, options, template};

use crate::{Hello, allowed, bytes_value, display_header_footer, esc_html__, truthy, why};

type Made = Result<Vec<u8>, String>;

/// Renders Hello's template `file`.
pub(crate) fn render(file: &str) -> Made {
    match file {
        "index.php" => index(),
        "header.php" => header(),
        "footer.php" => footer(),
        // Its only content is PHP that prints nothing.
        "sidebar.php" => Ok(Vec::new()),
        "comments.php" => comments(),
        "template-parts/header.php" => header_part(),
        "template-parts/footer.php" => footer_part(),
        "template-parts/single.php" => single(),
        "template-parts/archive.php" => archive(),
        "template-parts/search.php" => search(),
        "template-parts/404.php" => not_found(),
        _ => Err(format!("no template {file}")),
    }
}

fn text(text: &str) -> Value {
    Value::Text(text.into())
}

/// A tag that prints markup.
fn html(name: &str, args: &[Value]) -> Result<Html, String> {
    page::print(name, args).map_err(why)
}

/// A tag that answers bytes.
fn bytes(name: &str, args: &[Value]) -> Result<Vec<u8>, String> {
    template::print(name, args).map_err(why)
}

fn is(name: &str, args: &[Value]) -> Result<bool, String> {
    template::is(name, args).map_err(why)
}

/// `esc_url( $url )`.
fn esc_url(url: &[u8]) -> Result<Text, String> {
    bytes("esc_url", &[bytes_value(url)]).map(Text)
}

/// The file a part function found, rendered; nothing for none.
fn part(found: Option<String>) -> Made {
    found.map_or(Ok(Vec::new()), |file| render(&file))
}

/// PHP's truthiness of bytes.
fn filled(bytes: &[u8]) -> bool {
    !bytes.is_empty() && bytes != b"0"
}

/// A filter's value as PHP prints it.
fn shown(value: &Value) -> String {
    match value {
        Value::Text(text) => text.clone(),
        Value::Int(n) => format!("{n}"),
        Value::Bool(true) => "1".into(),
        _ => String::new(),
    }
}

/// `wp_nav_menu()` of `location` with `echo` false, as Hello asks it;
/// none for `false`.
fn menu(location: &str) -> Result<Option<Html>, String> {
    let args = format!(
        r#"{{"theme_location":"{location}","fallback_cb":false,"container":false,"echo":false}}"#
    );
    let menu = html("wp_nav_menu", &[Value::Json(args)])?;
    Ok((!menu.0.is_empty()).then_some(menu))
}

/// `apply_filters( 'hello_elementor_page_title', true )`.
fn page_title() -> bool {
    allowed("hello_elementor_page_title")
}

fn index() -> Made {
    let mut page = part(template::get_header(None, &[]))?;
    let slug = if is("is_singular", &[])? {
        "single"
    } else if is("is_archive", &[])? || is("is_home", &[])? {
        "archive"
    } else if is("is_search", &[])? {
        "search"
    } else {
        "404"
    };
    let slug = format!("template-parts/{slug}");
    page.extend(part(template::get_template_part(&slug, None, &[]))?);
    page.extend(part(template::get_footer(None, &[]))?);
    Ok(page)
}

#[derive(Template)]
#[template(path = "header.html")]
struct Header {
    language: Html,
    charset: Html,
    viewport: String,
    head: Html,
    body_class: Html,
    body_open: Html,
    skip_url: Option<Text>,
    skip_label: String,
}

fn header() -> Made {
    let viewport = text("width=device-width, initial-scale=1");
    let viewport = hooks::filter::<Hello>("hello_elementor_viewport_content", viewport, &[]);
    let skip = allowed("hello_elementor_enable_skip_link");
    let url = hooks::filter::<Hello>("hello_elementor_skip_link_url", text("#content"), &[]);
    let language = html("language_attributes", &[])?;
    let charset = html("bloginfo", &[text("charset")])?;
    let head = html("wp_head", &[])?;
    let body_class = html("body_class", &[])?;
    let body_open = html("wp_body_open", &[])?;
    let skip_url = match skip {
        true => Some(bytes("esc_url", &[url]).map(Text)?),
        false => None,
    };
    let header = Header {
        language,
        charset,
        viewport: shown(&viewport),
        head,
        body_class,
        body_open,
        skip_url,
        skip_label: i18n::translate("Skip to content"),
    };
    page::render(&header).and_then(|mut page| {
        if display_header_footer() {
            let found = template::get_template_part("template-parts/header", None, &[]);
            page.extend(part(found)?);
        }
        Ok(page)
    })
}

#[derive(Template)]
#[template(path = "footer.html")]
struct Footer {
    footer: Html,
}

fn footer() -> Made {
    let mut page = Vec::new();
    if display_header_footer() {
        let found = template::get_template_part("template-parts/footer", None, &[]);
        page.extend(part(found)?);
    }
    let footer = html("wp_footer", &[])?;
    page::render(&Footer { footer }).map(|tail| [page, tail].concat())
}

#[derive(Template)]
#[template(path = "template-parts/header.html")]
struct HeaderPart {
    logo: Option<Html>,
    site_name: Option<Text>,
    home: Text,
    home_label: String,
    tagline: Option<Text>,
    nav: Option<Html>,
    nav_label: String,
}

fn header_part() -> Made {
    let name = bytes("get_bloginfo", &[text("name")])?;
    let tagline = bytes("get_bloginfo", &[text("description"), text("display")])?;
    let nav = menu("menu-1")?;
    let mut part = HeaderPart {
        logo: None,
        site_name: None,
        home: Text(Vec::new()),
        home_label: i18n::translate("Home"),
        tagline: None,
        nav,
        nav_label: i18n::translate("Main menu"),
    };
    if is("has_custom_logo", &[])? {
        part.logo = Some(html("the_custom_logo", &[])?);
    } else if filled(&name) {
        part.home = esc_url(&bytes("home_url", &[text("/")])?)?;
        part.site_name = Some(Text(name));
        part.tagline = filled(&tagline).then_some(Text(tagline));
    }
    page::render(&part)
}

#[derive(Template)]
#[template(path = "template-parts/footer.html")]
struct FooterPart {
    nav: Option<Html>,
    nav_label: String,
}

fn footer_part() -> Made {
    let nav = menu("menu-2")?;
    let nav_label = i18n::translate("Footer menu");
    page::render(&FooterPart { nav, nav_label })
}

#[derive(Template)]
#[template(path = "template-parts/single.html")]
struct Single {
    post_class: Html,
    title: Option<Html>,
    content: Html,
    link_pages: Html,
    tags: Option<Html>,
}

fn single() -> Made {
    let mut page = Vec::new();
    while template::have_posts() {
        template::the_post();
        let post_class = html("post_class", &[text("site-main")])?;
        let title = match page_title() {
            true => Some(html(
                "the_title",
                &[text("<h1 class=\"entry-title\">"), text("</h1>")],
            )?),
            false => None,
        };
        let content = html("the_content", &[])?;
        let link_pages = html("wp_link_pages", &[])?;
        let tags = match is("has_tag", &[])? {
            true => {
                let before = format!("<span class=\"tag-links\">{}", esc_html__("Tagged "));
                Some(html(
                    "the_tags",
                    &[text(&before), text(", "), text("</span>")],
                )?)
            }
            false => None,
        };
        let single = Single {
            post_class,
            title,
            content,
            link_pages,
            tags,
        };
        let rendered = page::render(&single)
            .and_then(|top| Ok([top, comments_template()?, b"\n</main>\n\n\t".to_vec()].concat()));
        page.extend(rendered?);
    }
    Ok(page)
}

/// `comments_template()`: the host sets the post's comments up and names
/// the file to render, none when the page shows no comments.
fn comments_template() -> Made {
    match template::get("comments_template", &[]).map_err(why)? {
        Value::Text(file) => render(&file),
        _ => Ok(Vec::new()),
    }
}

struct Heading {
    title: Html,
    description: Html,
}

struct Thumbnail {
    url: Text,
    image: Html,
}

/// A post of an archive or search page.
struct Entry {
    url: Text,
    title: Html,
    thumb: Option<Thumbnail>,
    excerpt: Html,
}

struct Pagination {
    previous: Html,
    next: Html,
}

#[derive(Template)]
#[template(path = "template-parts/archive.html")]
struct Archive {
    heading: Option<Heading>,
    posts: Vec<Entry>,
    pagination: Option<Pagination>,
}

fn archive() -> Made {
    let heading = match page_title() {
        true => {
            let title = [text("<h1 class=\"entry-title\">"), text("</h1>")];
            let description = [text("<p class=\"archive-description\">"), text("</p>")];
            Some(Heading {
                title: html("the_archive_title", &title)?,
                description: html("the_archive_description", &description)?,
            })
        }
        false => None,
    };
    let posts = entries()?;
    let pagination = pagination()?;
    page::render(&Archive {
        heading,
        posts,
        pagination,
    })
}

/// The loop of an archive or search page.
fn entries() -> Result<Vec<Entry>, String> {
    let mut posts = Vec::new();
    while template::have_posts() {
        template::the_post();
        let link = bytes("get_permalink", &[])?;
        let url = esc_url(&link)?;
        let title = bytes("get_the_title", &[])?;
        let title = html("wp_kses_post", &[bytes_value(&title)])?;
        // PHP escapes the link again for the thumbnail: the same answer.
        let thumb = match is("has_post_thumbnail", &[])? {
            true => Some(Thumbnail {
                url: Text(url.0.clone()),
                image: html("get_the_post_thumbnail", &[Value::Null, text("large")])?,
            }),
            false => None,
        };
        let excerpt = html("the_excerpt", &[])?;
        posts.push(Entry {
            url,
            title,
            thumb,
            excerpt,
        });
    }
    Ok(posts)
}

/// The previous and next links when the query has more than one page.
fn pagination() -> Result<Option<Pagination>, String> {
    let pages = template::get("max_num_pages", &[]).map_err(why)?;
    if pages.int().unwrap_or(0) <= 1 {
        return Ok(None);
    }
    // PHP asks `is_rtl()` for each arrow: the same answer.
    let rtl = is("is_rtl", &[])?;
    let (previous_arrow, next_arrow) = if rtl {
        ("&rarr;", "&larr;")
    } else {
        ("&larr;", "&rarr;")
    };
    let label = |text: &str, arrow: &str| {
        let arrow = format!("<span class=\"meta-nav\">{arrow}</span>");
        esc_html__(text).replacen("%s", &arrow, 1)
    };
    let previous = html(
        "previous_posts_link",
        &[text(&label("%s Previous", previous_arrow))],
    )?;
    let next = html("next_posts_link", &[text(&label("Next %s", next_arrow))])?;
    Ok(Some(Pagination { previous, next }))
}

#[derive(Template)]
#[template(path = "template-parts/search.html")]
struct Search {
    query: Option<Html>,
    results_for: String,
    posts: Vec<Entry>,
    nothing: String,
    pagination: Option<Pagination>,
}

fn search() -> Made {
    let query = match page_title() {
        true => Some(html("get_search_query", &[])?),
        false => None,
    };
    // The loop runs only past `have_posts()`, as the loop's own end fires
    // `loop_no_results` once more.
    let posts = match template::have_posts() {
        true => entries()?,
        false => Vec::new(),
    };
    let pagination = pagination()?;
    page::render(&Search {
        query,
        results_for: i18n::translate("Search results for: "),
        posts,
        nothing: i18n::translate("It seems we can't find what you're looking for."),
        pagination,
    })
}

#[derive(Template)]
#[template(path = "template-parts/404.html")]
struct NotFound {
    title: Option<String>,
    nothing: String,
}

fn not_found() -> Made {
    let title = page_title().then(|| i18n::translate("The page can&rsquo;t be found."));
    let nothing = i18n::translate("It looks like nothing was found at this location.");
    page::render(&NotFound { title, nothing })
}

/// The navigation is printed before and after the list, as PHP prints it
/// twice with the same answer.
struct CommentList {
    title: String,
    nav: Html,
    comments: Html,
}

#[derive(Template)]
#[template(path = "comments.html")]
struct Comments {
    list: Option<CommentList>,
    form: Html,
}

fn comments() -> Made {
    let post_type = template::get("get_post_type", &[]).map_err(why)?;
    if !is("post_type_supports", &[post_type, text("comments")])? {
        return Ok(Vec::new());
    }
    // PHP asks each of these twice, with the same answers.
    let have = is("have_comments", &[])?;
    let open = is("comments_open", &[])?;
    if !have && !open {
        return Ok(Vec::new());
    }
    if open {
        let threaded = options::get("thread_comments").map_err(why)?;
        if threaded.as_ref().is_some_and(truthy) {
            assets::enqueue(AssetKind::Script, "comment-reply").map_err(why)?;
        }
    }
    let list = match have {
        true => Some(comment_list()?),
        false => None,
    };
    let args = r#"{"title_reply_before":"<h2 id=\"reply-title\" class=\"comment-reply-title\">","title_reply_after":"</h2>"}"#;
    let form = html("comment_form", &[Value::Json(args.into())])?;
    page::render(&Comments { list, form })
}

/// The title, navigation and list of a post that has comments.
fn comment_list() -> Result<CommentList, String> {
    let number = template::get("get_comments_number", &[]).map_err(why)?;
    let title = if number == text("1") {
        i18n::translate_with_context("One Response", "comments title")
    } else {
        let count = shown(&number);
        let n = count.parse::<u64>().unwrap_or(0);
        let label = i18n::plural_with_context("%s Response", "%s Responses", n, "comments title");
        label.replacen("%s", &number_format_i18n(n), 1)
    };
    let list = r#"{"style":"ol","short_ping":true,"avatar_size":42}"#;
    Ok(CommentList {
        title,
        nav: html("the_comments_navigation", &[])?,
        comments: html("wp_list_comments", &[Value::Json(list.into())])?,
    })
}

/// `number_format_i18n( $number )` of a whole number in English.
pub(crate) fn number_format_i18n(n: u64) -> String {
    let digits = format!("{n}");
    let mut out = String::new();
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at) % 3 == 0 {
            out.push(',');
        }
        out.push(digit);
    }
    out
}
