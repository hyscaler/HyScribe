// SPDX-License-Identifier: AGPL-3.0-or-later

//! The RSS2 feeds of the main query's posts and of comments, as
//! `feed-rss2.php` and `feed-rss2-comments.php` print them, with the
//! templates' whitespace.
//!
//! The generator names HyScribe, as the page head does. Not ported
//! yet: the Atom, RDF and RSS 0.92 feeds, comment feeds of searches and
//! archives, comment links to a later comment page, a post's second and
//! later enclosures, `wp_staticize_emoji()` and oEmbed in feed content.

use std::sync::OnceLock;

use hyscribe_kses::{esc_attr, esc_html, esc_url, strip_tags};
use hyscribe_model::{Comment, Post};
use hyscribe_phpser::{Value, add_query_arg, is_empty_string, php_trim};

use crate::content::password_form;
use crate::core_blocks::{comment_author, comment_text, comments_open};
use crate::date::{self, Zone};
use crate::formatting::{contains, convert_chars, ent2ncr, html_entity_decode};
use crate::link::host;
use crate::post::the_title;
use crate::site::{Request, Site};
use crate::template::{Renderer, replace};
use crate::theme::Theme;

/// `feed-rss2.php` for the main query's posts. `title` is
/// `wp_get_document_title()`; `last_modified` is
/// `get_lastpostmodified( 'GMT' )`, the build date of a feed without
/// posts.
pub fn rss2(
    theme: &Theme,
    site: &dyn Site,
    request: &Request,
    title: &[u8],
    last_modified: Option<&str>,
) -> Vec<u8> {
    let option = |name| site.option(name).unwrap_or_default();
    let charset = option("blog_charset");
    let built = build_date(request, &[], last_modified, "r");
    let mut out = [
        b"<?xml version=\"1.0\" encoding=\"".as_slice(),
        &charset,
        b"\"?><rss version=\"2.0\"\n\
          \txmlns:content=\"http://purl.org/rss/1.0/modules/content/\"\n\
          \txmlns:wfw=\"http://wellformedweb.org/CommentAPI/\"\n\
          \txmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
          \txmlns:atom=\"http://www.w3.org/2005/Atom\"\n\
          \txmlns:sy=\"http://purl.org/rss/1.0/modules/syndication/\"\n\
          \txmlns:slash=\"http://purl.org/rss/1.0/modules/slash/\"\n\
          \t>\n\n<channel>\n\t<title>",
        title,
        b"</title>\n\t<atom:link href=\"",
        &esc_url(&self_link(site, request)),
        b"\" rel=\"self\" type=\"application/rss+xml\" />\n\t<link>",
        &bloginfo(&site.home_url()),
        b"</link>\n\t<description>",
        &bloginfo(&option("blogdescription")),
        b"</description>\n\t<lastBuildDate>",
        built.as_bytes(),
        b"</lastBuildDate>\n\t<language>en-US</language>\n\
          \t<sy:updatePeriod>\n\thourly\t</sy:updatePeriod>\n\
          \t<sy:updateFrequency>\n\t1\t</sy:updateFrequency>\n\t",
        &generator(),
        b"\n",
    ]
    .concat();
    // is_feed(): posts show in full.
    let request = &Request {
        feed: true,
        ..request.clone()
    };
    let mut renderer = Renderer::new(theme, site, request);
    for post in &request.posts {
        renderer.context.post_id = Some(post.id);
        out.extend(item(&mut renderer, post));
    }
    out.extend_from_slice(b"</channel>\n</rss>\n");
    out
}

/// `feed-rss2-comments.php` for `comments`, newest first: a singular
/// request's, with its post (if any) as the main query's, or the site's.
/// `title` is `wp_get_document_title()` for the site's feed; `single` is
/// `is_single()`, a post (not a page) asked for; `last_modified` is
/// `get_lastpostmodified( 'GMT' )`.
pub fn rss2_comments(
    theme: &Theme,
    site: &dyn Site,
    request: &Request,
    title: &[u8],
    single: bool,
    comments: &[Comment],
    last_modified: Option<&str>,
) -> Vec<u8> {
    let option = |name| site.option(name).unwrap_or_default();
    let post = request.posts.first();
    let channel_title = if request.singular {
        [
            b"Comments on: ".as_slice(),
            &post.map(title_rss).unwrap_or_default(),
        ]
        .concat()
    } else {
        [b"Comments for ".as_slice(), title].concat()
    };
    let renderer = Renderer::new(theme, site, request);
    let link = if single {
        post.map(|post| esc_url(&renderer.context.permalink(post)))
            .unwrap_or_default()
    } else {
        bloginfo(&site.home_url())
    };
    let built = build_date(request, comments, last_modified, "r");
    let mut out = [
        b"<?xml version=\"1.0\" encoding=\"".as_slice(),
        &option("blog_charset"),
        b"\"?><rss version=\"2.0\"\n\
          \txmlns:content=\"http://purl.org/rss/1.0/modules/content/\"\n\
          \txmlns:dc=\"http://purl.org/dc/elements/1.1/\"\n\
          \txmlns:atom=\"http://www.w3.org/2005/Atom\"\n\
          \txmlns:sy=\"http://purl.org/rss/1.0/modules/syndication/\"\n\
          \t\n\t>\n<channel>\n\t<title>\n\t",
        &channel_title,
        b"\t</title>\n\t<atom:link href=\"",
        &esc_url(&self_link(site, request)),
        b"\" rel=\"self\" type=\"application/rss+xml\" />\n\t<link>",
        &link,
        b"</link>\n\t<description>",
        &bloginfo(&option("blogdescription")),
        b"</description>\n\t<lastBuildDate>",
        built.as_bytes(),
        b"</lastBuildDate>\n\
          \t<sy:updatePeriod>\n\thourly\t</sy:updatePeriod>\n\
          \t<sy:updateFrequency>\n\t1\t</sy:updateFrequency>\n\t",
        &generator(),
        b"\n",
    ]
    .concat();
    for comment in comments {
        out.extend(comment_item(&renderer, comment));
    }
    out.extend_from_slice(b"</channel>\n</rss>\n");
    out
}

/// One `<item>` of the comment loop, with the template's whitespace.
fn comment_item(renderer: &Renderer<'_>, comment: &Comment) -> Vec<u8> {
    let context = &renderer.context;
    let site = context.site;
    let post = site.post(comment.post_id).unwrap_or_default();
    let author = ent2ncr(&comment_author(context, comment));
    let title = if context.request.singular {
        [b"By: ".as_slice(), &author].concat()
    } else {
        [
            b"Comment on ".as_slice(),
            &title_rss(&post),
            b" by ",
            &author,
        ]
        .concat()
    };
    let anchor = format!("#comment-{}", comment.id);
    let body = if post.password.is_empty() {
        [
            b"\t\t\t<description><![CDATA[".as_slice(),
            esc_html(&ent2ncr(&comment.content)).as_bytes(),
            b"]]></description>\n\t\t\t<content:encoded><![CDATA[",
            &comment_text(site, &comment.content),
            b"]]></content:encoded>\n\t\t",
        ]
        .concat()
    } else {
        [
            b"\t\t\t<description>Protected Comments: Please enter your password to view comments.</description>\n\t\t\t<content:encoded><![CDATA[".as_slice(),
            &password_form(renderer, &post),
            b"]]></content:encoded>\n\t\t",
        ]
        .concat()
    };
    [
        b"\t<item>\n\t\t<title>\n\t\t".as_slice(),
        &title,
        b"\t\t</title>\n\t\t<link>",
        &esc_url(&[&context.permalink(&post)[..], anchor.as_bytes()].concat()),
        b"</link>\n\n\t\t<dc:creator><![CDATA[",
        &author,
        b"]]></dc:creator>\n\t\t<pubDate>",
        date::format("D, d M Y H:i:s +0000", &comment.date_gmt, Zone::Utc).as_bytes(),
        b"</pubDate>\n\t\t<guid isPermaLink=\"false\">",
        &esc_url(&[&post.guid[..], anchor.as_bytes()].concat()),
        b"</guid>\n\n\t\t",
        &body,
        b"\n\t\t\t</item>\n\t",
    ]
    .concat()
}

/// `get_feed_build_date( $format )`: the latest change among the main
/// query's posts and a comment feed's `comments`, else `last_modified`.
fn build_date(
    request: &Request,
    comments: &[Comment],
    last_modified: Option<&str>,
    format: &str,
) -> String {
    request
        .posts
        .iter()
        .map(|post| post.modified_gmt.as_str())
        .chain(comments.iter().map(|c| c.date_gmt.as_str()))
        .max()
        .or(last_modified)
        .map(|stored| date::format(format, stored, Zone::Utc))
        .unwrap_or_default()
}

/// `get_bloginfo_rss()` of a value.
fn bloginfo(text: &[u8]) -> Vec<u8> {
    convert_chars(&strip_tags(text))
}

/// `get_the_title_rss()`.
fn title_rss(post: &Post) -> Vec<u8> {
    esc_html(&strip_tags(&ent2ncr(&the_title(post)))).into_bytes()
}

/// Atom's date format.
const ATOM_DATE: &str = "Y-m-d\\TH:i:s\\Z";

/// The MIME types a visitor may upload, `get_allowed_mime_types()`.
const MIME_TYPES: &[u8] = include_bytes!("../../../wordpress/wp-includes/mime-types.json");

/// `feed-atom.php` for the main query's posts. `title` is
/// `wp_get_document_title()`; `last_modified` is
/// `get_lastpostmodified( 'GMT' )`, the update time of a feed without
/// posts.
pub fn atom(
    theme: &Theme,
    site: &dyn Site,
    request: &Request,
    title: &[u8],
    last_modified: Option<&str>,
) -> Vec<u8> {
    let option = |name| site.option(name).unwrap_or_default();
    let updated = build_date(request, &[], last_modified, ATOM_DATE);
    let mut out = [
        b"<?xml version=\"1.0\" encoding=\"".as_slice(),
        &option("blog_charset"),
        b"\"?><feed\n\txmlns=\"http://www.w3.org/2005/Atom\"\n\
          \txmlns:thr=\"http://purl.org/syndication/thread/1.0\"\n\
          \txml:lang=\"en-US\"\n\t>\n\t<title type=\"text\">",
        title,
        b"</title>\n\t<subtitle type=\"text\">",
        &bloginfo(&option("blogdescription")),
        b"</subtitle>\n\n\t<updated>",
        updated.as_bytes(),
        b"</updated>\n\n\t<link rel=\"alternate\" type=\"",
        &bloginfo(&option("html_type")),
        b"\" href=\"",
        &bloginfo(&site.home_url()),
        b"\" />\n\t<id>",
        &[&site.home_url()[..], b"/?feed=atom"].concat(),
        b"</id>\n\t<link rel=\"self\" type=\"application/atom+xml\" href=\"",
        &esc_url(&self_link(site, request)),
        b"\" />\n\n\t",
        format!(
            "<generator uri=\"{}\" version=\"{}\">HyScribe</generator>",
            env!("CARGO_PKG_REPOSITORY"),
            env!("CARGO_PKG_VERSION")
        )
        .as_bytes(),
        b"\n",
    ]
    .concat();
    let request = &Request {
        feed: true,
        ..request.clone()
    };
    let mut renderer = Renderer::new(theme, site, request);
    for post in &request.posts {
        renderer.context.post_id = Some(post.id);
        out.extend(entry(&mut renderer, post));
    }
    out.extend_from_slice(b"</feed>\n");
    out
}

/// One `<entry>` of the Atom loop, with the template's whitespace.
fn entry(renderer: &mut Renderer<'_>, post: &Post) -> Vec<u8> {
    let context = &renderer.context;
    let site = context.site;
    let option = |name| site.option(name).unwrap_or_default();
    let html_type = bloginfo(&option("html_type"));
    // html_type_rss()
    let kind: &[u8] = if contains(&option("html_type"), b"xhtml") {
        b"xhtml"
    } else {
        b"html"
    };
    let permalink = esc_url(&context.permalink(post));
    let user = site.user(post.author).unwrap_or_default();
    let uri = if user.url.is_empty() {
        Vec::new()
    } else {
        [b"\t\t\t\t<uri>".as_slice(), &user.url, b"</uri>\n\t\t\t\t"].concat()
    };
    let scheme = esc_attr(&bloginfo(&site.home_url()));
    let categories: Vec<u8> = category_names(site, post, <[u8]>::to_vec)
        .iter()
        .flat_map(|name| {
            format!(
                "<category scheme=\"{scheme}\" term=\"{}\" />",
                esc_attr(name)
            )
            .into_bytes()
        })
        .collect();
    let count = post.comment_count;
    let replies = if count != 0 || comments_open(context, post) {
        let feed = esc_url(&post_comments_feed_link(site, post, b"atom"));
        [
            b"\t\t\t<link rel=\"replies\" type=\"".as_slice(),
            &html_type,
            b"\" href=\"",
            &permalink,
            format!("#comments\" thr:count=\"{count}\" />\n\t\t\t<link rel=\"replies\" type=\"application/atom+xml\" href=\"").as_bytes(),
            &feed,
            format!("\" thr:count=\"{count}\" />\n\t\t\t<thr:total>{count}</thr:total>\n\t\t").as_bytes(),
        ]
        .concat()
    } else {
        Vec::new()
    };
    let enclosure = atom_enclosure(site, post);
    let use_excerpt = !is_empty_string(&option("rss_use_excerpt"));
    let excerpt = convert_chars(&ent2ncr(&renderer.get_the_excerpt(post)));
    let content = if use_excerpt {
        Vec::new()
    } else {
        let content = replace(&renderer.the_content(post), b"]]>", b"]]&gt;");
        [
            b"\t\t\t<content type=\"".as_slice(),
            kind,
            b"\" xml:base=\"",
            &permalink,
            b"\"><![CDATA[",
            &content,
            b"]]></content>\n\t\t",
        ]
        .concat()
    };
    [
        b"\t<entry>\n\t\t<author>\n\t\t\t<name>".as_slice(),
        &user.display_name,
        b"</name>\n\t\t\t",
        &uri,
        b"\t\t</author>\n\n\t\t<title type=\"",
        kind,
        b"\"><![CDATA[",
        &title_rss(post),
        b"]]></title>\n\t\t<link rel=\"alternate\" type=\"",
        &html_type,
        b"\" href=\"",
        &permalink,
        b"\" />\n\n\t\t<id>",
        &esc_url(&post.guid),
        b"</id>\n\t\t<updated>",
        date::format(ATOM_DATE, &post.modified_gmt, Zone::Utc).as_bytes(),
        b"</updated>\n\t\t<published>",
        date::format(ATOM_DATE, &post.date_gmt, Zone::Utc).as_bytes(),
        b"</published>\n\t\t",
        &categories,
        b"\n\t\t<summary type=\"",
        kind,
        b"\"><![CDATA[",
        &excerpt,
        b"]]></summary>\n\n\t\t",
        &content,
        b"\n\t\t",
        &enclosure,
        &replies,
        b"\t</entry>\n\t",
    ]
    .concat()
}

/// `atom_enclosure()` for the post's first `enclosure` field: the URL,
/// then a numeric length and an allowed type in either order.
fn atom_enclosure(site: &dyn Site, post: &Post) -> Vec<u8> {
    if !post.password.is_empty() {
        return Vec::new();
    }
    let Some(field) = site.post_meta(post.id, "enclosure") else {
        return Vec::new();
    };
    static TYPES: OnceLock<Value> = OnceLock::new();
    let types = TYPES.get_or_init(|| hyscribe_phpser::json_decode(MIME_TYPES).unwrap_or_default());
    let allowed = |line: &[u8]| {
        types
            .as_array()
            .unwrap_or_default()
            .iter()
            .any(|(_, kind)| kind.as_bytes() == Some(line))
    };
    let lines: Vec<&[u8]> = field.split(|&b| b == b'\n').collect();
    let (mut length, mut kind): (&[u8], &[u8]) = (b"0", b"");
    for line in lines.iter().skip(1).take(2) {
        if Value::String(line.to_vec()).is_numeric() {
            length = php_trim(line);
        } else if allowed(line) {
            kind = php_trim(line);
        }
    }
    let url = lines.first().map_or(&[][..], |url| php_trim(url));
    format!(
        "<link href=\"{}\" rel=\"enclosure\" length=\"{}\" type=\"{}\" />\n",
        String::from_utf8_lossy(&esc_url(url)),
        Value::String(esc_attr(length).into_bytes()).to_int(),
        esc_attr(kind)
    )
    .into_bytes()
}

/// `get_the_generator( 'rss2' )`, naming HyScribe and its version.
fn generator() -> Vec<u8> {
    format!(
        "<generator>{}?v={}</generator>",
        env!("CARGO_PKG_REPOSITORY"),
        env!("CARGO_PKG_VERSION")
    )
    .into_bytes()
}

/// One `<item>` of the loop, with the template's whitespace.
fn item(renderer: &mut Renderer<'_>, post: &Post) -> Vec<u8> {
    let context = &renderer.context;
    let site = context.site;
    let permalink = context.permalink(post);
    let count = post.comment_count;
    let discussed = count != 0 || comments_open(context, post);
    let comments = if discussed {
        let hash: &[u8] = if count != 0 {
            b"#comments"
        } else {
            b"#respond"
        };
        [
            b"\t\t\t<comments>".as_slice(),
            &esc_url(&[&permalink[..], hash].concat()),
            b"</comments>\n\t\t",
        ]
        .concat()
    } else {
        Vec::new()
    };
    let author = site
        .user(post.author)
        .map(|user| user.display_name)
        .unwrap_or_default();
    let use_excerpt = !is_empty_string(&site.option("rss_use_excerpt").unwrap_or_default());
    let excerpt = renderer.get_the_excerpt(post);
    let excerpt = convert_chars(&ent2ncr(&excerpt));
    let description = [
        b"\t\t\t<description><![CDATA[".as_slice(),
        &excerpt,
        b"]]></description>\n",
    ]
    .concat();
    let body = if use_excerpt {
        [&description[..], b"\t\t"].concat()
    } else {
        let content = replace(&renderer.the_content(post), b"]]>", b"]]&gt;");
        let encoded = if content.is_empty() {
            &excerpt
        } else {
            &content
        };
        [
            &description[..],
            b"\t\t\t\t\t\t\t\t\t\t<content:encoded><![CDATA[",
            encoded,
            b"]]></content:encoded>\n\t\t\t\t\t",
        ]
        .concat()
    };
    let comment_feed = if discussed {
        [
            b"\t\t\t<wfw:commentRss>".as_slice(),
            &esc_url(&post_comments_feed_link(site, post, b"rss2")),
            b"</wfw:commentRss>\n\t\t\t<slash:comments>",
            count.to_string().as_bytes(),
            b"</slash:comments>\n\t\t",
        ]
        .concat()
    } else {
        Vec::new()
    };
    [
        b"\t<item>\n\t\t<title>".as_slice(),
        &title_rss(post),
        b"</title>\n\t\t<link>",
        &esc_url(&permalink),
        b"</link>\n\t\t",
        &comments,
        b"\n\t\t<dc:creator><![CDATA[",
        &author,
        b"]]></dc:creator>\n\t\t<pubDate>",
        date::format("D, d M Y H:i:s +0000", &post.date_gmt, Zone::Utc).as_bytes(),
        b"</pubDate>\n\t\t",
        &categories(site, post),
        b"\t\t<guid isPermaLink=\"false\">",
        &esc_url(&post.guid),
        b"</guid>\n\n\t\t",
        &body,
        b"\n\t\t",
        &comment_feed,
        b"\n\t\t",
        &enclosure(site, post),
        b"\n\t\t\t</item>\n\t",
    ]
    .concat()
}

/// `get_the_category_rss( 'rss2' )`: the post's categories, then its tags,
/// each name once.
fn categories(site: &dyn Site, post: &Post) -> Vec<u8> {
    // sanitize_term_field( 'name', ..., 'rss' )
    category_names(site, post, convert_chars)
        .iter()
        .flat_map(|name| {
            [
                b"\t\t<category><![CDATA[".as_slice(),
                &html_entity_decode(name),
                b"]]></category>\n",
            ]
            .concat()
        })
        .collect()
}

/// The names of the post's categories, then its tags, as `filter` leaves
/// them, each once.
fn category_names(site: &dyn Site, post: &Post, filter: fn(&[u8]) -> Vec<u8>) -> Vec<Vec<u8>> {
    let mut names: Vec<Vec<u8>> = Vec::new();
    for taxonomy in ["category", "post_tag"] {
        for term in site.post_terms(post.id, taxonomy) {
            let name = filter(&term.name);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

/// `rss_enclosure()` for the post's first `enclosure` field: URL, length
/// and type on lines of their own.
fn enclosure(site: &dyn Site, post: &Post) -> Vec<u8> {
    if !post.password.is_empty() {
        return Vec::new();
    }
    let Some(field) = site.post_meta(post.id, "enclosure") else {
        return Vec::new();
    };
    let lines: Vec<&[u8]> = field.split(|&b| b == b'\n').collect();
    let [url, length, kind, ..] = lines.as_slice() else {
        return Vec::new();
    };
    let kind = php_trim(kind)
        .split(|&b| b == b' ' || b == b'\t')
        .next()
        .unwrap_or_default();
    let length = Value::String(php_trim(length).to_vec())
        .to_int()
        .unsigned_abs();
    [
        b"<enclosure url=\"".as_slice(),
        &esc_url(php_trim(url)),
        b"\" length=\"",
        length.to_string().as_bytes(),
        b"\" type=\"",
        esc_attr(kind).as_bytes(),
        b"\" />\n",
    ]
    .concat()
}

/// `get_self_link()`: the requested URI on the home URL's host.
fn self_link(site: &dyn Site, request: &Request) -> Vec<u8> {
    [b"http://".as_slice(), &host(&site.home_url()), &request.uri].concat()
}

/// `get_post_comments_feed_link( $post, $feed )` with plain permalinks.
pub(crate) fn post_comments_feed_link(site: &dyn Site, post: &Post, feed: &[u8]) -> Vec<u8> {
    let key: &[u8] = match post.post_type.as_str() {
        "page" => b"page_id",
        _ => b"p",
    };
    let id = post.id.to_string();
    add_query_arg(
        &[(b"feed", Some(feed)), (key, Some(id.as_bytes()))],
        &[&site.home_url()[..], b"/"].concat(),
    )
}

#[cfg(test)]
mod tests;
