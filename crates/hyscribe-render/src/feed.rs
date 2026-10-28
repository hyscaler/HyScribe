// SPDX-License-Identifier: AGPL-3.0-or-later

//! The RSS2 feed of the main query's posts, as `feed-rss2.php` prints it,
//! with the template's whitespace.
//!
//! The generator names HyScribe, as the page head does. Not ported
//! yet: the Atom, RDF and RSS 0.92 feeds, comment feeds, a post's second
//! and later enclosures, `wp_staticize_emoji()` and oEmbed in feed content.

use hyscribe_kses::{esc_attr, esc_html, esc_url, strip_tags};
use hyscribe_model::Post;
use hyscribe_phpser::{Value, add_query_arg, is_empty_string, php_trim};

use crate::core_blocks::comments_open;
use crate::date::{self, Zone};
use crate::formatting::{convert_chars, ent2ncr, html_entity_decode};
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
    let bloginfo = |text: &[u8]| convert_chars(&strip_tags(text));
    let built = request
        .posts
        .iter()
        .map(|post| post.modified_gmt.as_str())
        .max()
        .or(last_modified)
        .map(|stored| date::format("r", stored, Zone::Utc))
        .unwrap_or_default();
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
            &esc_url(&post_comments_feed_link(site, post)),
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
        esc_html(&strip_tags(&ent2ncr(&the_title(post)))).as_bytes(),
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
    let mut names: Vec<Vec<u8>> = Vec::new();
    for taxonomy in ["category", "post_tag"] {
        for term in site.post_terms(post.id, taxonomy) {
            // sanitize_term_field( 'name', ..., 'rss' )
            let name = convert_chars(&term.name);
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
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
    let home = site.home_url();
    let host = home
        .splitn(2, |&b| b == b':')
        .nth(1)
        .and_then(|rest| rest.strip_prefix(b"//"))
        .map(|rest| rest.split(|&b| b == b'/').next().unwrap_or_default())
        .unwrap_or_default();
    [b"http://".as_slice(), host, &request.uri].concat()
}

/// `get_post_comments_feed_link( $post, 'rss2' )` with plain permalinks.
pub(crate) fn post_comments_feed_link(site: &dyn Site, post: &Post) -> Vec<u8> {
    let key: &[u8] = match post.post_type.as_str() {
        "page" => b"page_id",
        _ => b"p",
    };
    let id = post.id.to_string();
    add_query_arg(
        &[(b"feed", Some(b"rss2")), (key, Some(id.as_bytes()))],
        &[&site.home_url()[..], b"/"].concat(),
    )
}

#[cfg(test)]
mod tests;
