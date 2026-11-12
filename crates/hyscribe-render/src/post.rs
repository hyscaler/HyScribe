// SPDX-License-Identifier: AGPL-3.0-or-later

//! Post template tags from `wp-includes/post-template.php`: the title, and
//! the classes of a post and of the page body.

use hyscribe_model::Post;
use hyscribe_phpser::{Value, decode, php_trim};

use crate::formatting::{CORE_SHORTCODE_TAGS, capital_p_dangit_title, convert_chars, wptexturize};
use crate::site::{Archive, Request, Site, Taxonomy};

/// `get_post_format_strings()`: each format's slug, which
/// `_post_format_request()` prefixes, and its name.
pub const POST_FORMATS: [(&str, &str); 10] = [
    ("standard", "Standard"),
    ("aside", "Aside"),
    ("chat", "Chat"),
    ("gallery", "Gallery"),
    ("link", "Link"),
    ("image", "Image"),
    ("quote", "Quote"),
    ("status", "Status"),
    ("video", "Video"),
    ("audio", "Audio"),
];

/// `get_post_format_string()` of a format term's slug without
/// `post-format-`: the name `_post_format_get_term()` gives the term,
/// `Standard` for none, empty for an unknown one.
pub fn post_format_string(slug: &[u8]) -> &'static str {
    let format = crate::template::replace(slug, b"post-format-", b"");
    if format.is_empty() {
        return "Standard";
    }
    POST_FORMATS
        .iter()
        .find(|(slug, _)| slug.as_bytes() == format)
        .map_or("", |(_, name)| name)
}

/// `get_the_title( $post )` with the `the_title` filters.
pub(crate) fn the_title(post: &Post) -> Vec<u8> {
    let title = if !post.password.is_empty() {
        [b"Protected: ", &post.title[..]].concat()
    } else if post.status == "private" {
        [b"Private: ", &post.title[..]].concat()
    } else {
        post.title.clone()
    };
    title_filters(&title)
}

/// The `the_title` filters: `wptexturize`, `convert_chars`, `trim` and
/// `capital_P_dangit`.
pub(crate) fn title_filters(title: &[u8]) -> Vec<u8> {
    let title = convert_chars(&wptexturize(title, &CORE_SHORTCODE_TAGS));
    capital_p_dangit_title(php_trim(&title))
}

/// `sanitize_html_class( $class, $fallback )`.
pub fn sanitize_html_class(class: &[u8], fallback: &[u8]) -> Vec<u8> {
    let mut kept = Vec::with_capacity(class.len());
    let mut rest = class;
    for _ in 0..=class.len() {
        match rest {
            [b'%', a, b, after @ ..] if a.is_ascii_hexdigit() && b.is_ascii_hexdigit() => {
                rest = after
            }
            [first, after @ ..] => {
                if first.is_ascii_alphanumeric() || matches!(first, b'_' | b'-') {
                    kept.push(*first);
                }
                rest = after;
            }
            [] => break,
        }
    }
    if kept.is_empty() && !fallback.is_empty() {
        return sanitize_html_class(fallback, b"");
    }
    kept
}

/// `get_post_format()` as a class: the format's slug, or `standard`.
fn post_format(site: &dyn Site, id: u64) -> Vec<u8> {
    site.post_terms(id, "post_format")
        .first()
        .map(|term| {
            sanitize_html_class(
                term.slug
                    .strip_prefix(b"post-format-")
                    .unwrap_or(&term.slug),
                b"",
            )
        })
        .filter(|format| !format.is_empty())
        .unwrap_or_else(|| b"standard".to_vec())
}

/// The taxonomies of a post type `get_post_class()` reads, public ones only.
fn taxonomies(post_type: &str) -> &'static [&'static str] {
    match post_type {
        "post" => &["category", "post_tag", "post_format"],
        _ => &[],
    }
}

/// `get_post_class( $class, $post )`, joined by spaces. Only `post`
/// supports post formats among the core types.
pub(crate) fn post_class(site: &dyn Site, request: &Request, class: &str, post: &Post) -> Vec<u8> {
    let id = post.id;
    let mut classes: Vec<Vec<u8>> = class
        .split_whitespace()
        .map(|c| c.as_bytes().to_vec())
        .collect();
    classes.push(format!("post-{id}").into_bytes());
    classes.push(post.post_type.clone().into_bytes());
    classes.push(format!("type-{}", post.post_type).into_bytes());
    classes.push(format!("status-{}", post.status).into_bytes());
    if post.post_type == "post" {
        classes.push([b"format-", &post_format(site, id)[..]].concat());
    }
    // A visitor never has the password cookie.
    let password_required = !post.password.is_empty();
    if password_required {
        classes.push(b"post-password-required".to_vec());
    }
    let thumbnail = site
        .post_meta(id, "_thumbnail_id")
        .is_some_and(|meta| Value::String(meta).to_int() != 0);
    if thumbnail && post.post_type != "attachment" && !password_required {
        classes.push(b"has-post-thumbnail".to_vec());
    }
    let sticky = site
        .option("sticky_posts")
        .and_then(|stored| decode(&stored).ok());
    let is_sticky = sticky
        .as_ref()
        .and_then(Value::as_array)
        .is_some_and(|ids| ids.iter().any(|(_, v)| v.to_int() == id as i64));
    if is_sticky && request.home && !request.is_paged() {
        classes.push(b"sticky".to_vec());
    }
    classes.push(b"hentry".to_vec());
    for taxonomy in taxonomies(&post.post_type) {
        for term in site.post_terms(id, taxonomy) {
            if term.slug.is_empty() {
                continue;
            }
            let term_class = term_class(&term.slug, term.id);
            classes.push(if *taxonomy == "post_tag" {
                [b"tag-", &term_class[..]].concat()
            } else {
                let fallback = format!("{taxonomy}-{}", term.id);
                sanitize_html_class(
                    &[taxonomy.as_bytes(), b"-", &term_class].concat(),
                    fallback.as_bytes(),
                )
            });
        }
    }
    let mut unique: Vec<Vec<u8>> = Vec::with_capacity(classes.len());
    for class in classes {
        if !unique.contains(&class) {
            unique.push(class);
        }
    }
    unique.join(&b' ')
}

/// The class a term gives: its sanitized slug, or its id when that is
/// numeric or only dashes.
fn term_class(slug: &[u8], id: u64) -> Vec<u8> {
    let class = sanitize_html_class(slug, id.to_string().as_bytes());
    if Value::String(class.clone()).is_numeric() || class.iter().all(|&b| b == b'-') {
        id.to_string().into_bytes()
    } else {
        class
    }
}

/// The body classes' name for an archive of `taxonomy`.
fn archive_class(taxonomy: Taxonomy) -> &'static str {
    match taxonomy {
        Taxonomy::Category => "category",
        Taxonomy::Tag => "tag",
        Taxonomy::Format => "term",
    }
}

/// `get_body_class()` for a visitor, with `template` as `get_template()`.
/// Pages with child pages and page templates other than the default are
/// not told apart yet.
pub fn body_class(site: &dyn Site, request: &Request, template: &str) -> Vec<u8> {
    let mut classes: Vec<Vec<u8>> = Vec::new();
    let mut push = |class: &[u8]| classes.push(class.to_vec());
    let singular = request.posts.first().filter(|_| request.singular);
    for (on, class) in [
        (request.front_page, &b"home"[..]),
        (request.home, b"blog"),
        (request.archive.is_some(), b"archive"),
        (request.date, b"date"),
    ] {
        if on {
            push(class);
        }
    }
    if request.search.is_some() {
        push(b"search");
        push(if request.posts.is_empty() {
            b"search-no-results"
        } else {
            b"search-results"
        });
    }
    if request.is_paged() {
        push(b"paged");
    }
    if request.not_found {
        push(b"error404");
    }
    if let Some(post) = singular {
        let kind = &post.post_type;
        push(b"wp-singular");
        push(format!("{kind}-template-default").as_bytes());
        if kind != "page" {
            push(b"single");
            push(
                &[
                    b"single-",
                    &sanitize_html_class(kind.as_bytes(), post.id.to_string().as_bytes())[..],
                ]
                .concat(),
            );
            push(format!("postid-{}", post.id).as_bytes());
            if kind == "post" {
                push(&[b"single-format-", &post_format(site, post.id)[..]].concat());
            }
        } else {
            push(b"page");
            push(format!("page-id-{}", post.id).as_bytes());
            if post.parent != 0 {
                push(b"page-child");
                push(format!("parent-pageid-{}", post.parent).as_bytes());
            }
        }
    } else if let Some(Archive::Term(taxonomy, term)) = &request.archive {
        let prefix = archive_class(*taxonomy);
        if *taxonomy != Taxonomy::Format {
            push(prefix.as_bytes());
        }
        if let Some(term) = term {
            if *taxonomy == Taxonomy::Format {
                push(b"tax-post_format");
            }
            push(
                &[
                    prefix.as_bytes(),
                    b"-",
                    &term_class(&term.slug, term.id)[..],
                ]
                .concat(),
            );
            push(format!("{prefix}-{}", term.id).as_bytes());
        }
    }
    // Block themes support responsive embeds.
    push(b"wp-embed-responsive");
    // `page`, or `paged` when `page` is not past the first page.
    let page = if request.page > 1 {
        request.page
    } else {
        request.paged
    };
    if page > 1 && !request.not_found {
        push(format!("paged-{page}").as_bytes());
        let kind = match (singular.map(|p| p.post_type.as_str()), &request.archive) {
            (Some("page"), _) => "page",
            (Some(_), _) => "single",
            (None, Some(Archive::Term(taxonomy, _))) if *taxonomy != Taxonomy::Format => {
                archive_class(*taxonomy)
            }
            _ if request.date => "date",
            _ if request.search.is_some() => "search",
            _ => "",
        };
        if !kind.is_empty() {
            push(format!("{kind}-paged-{page}").as_bytes());
        }
    }
    push(
        &[
            b"wp-theme-",
            &sanitize_html_class(template.as_bytes(), b"")[..],
        ]
        .concat(),
    );
    let mut unique: Vec<Vec<u8>> = Vec::with_capacity(classes.len());
    for class in classes {
        if !unique.contains(&class) {
            unique.push(class);
        }
    }
    unique.join(&b' ')
}

#[cfg(test)]
mod tests;
