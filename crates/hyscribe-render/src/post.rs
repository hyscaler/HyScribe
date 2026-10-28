// SPDX-License-Identifier: AGPL-3.0-or-later

//! Post template tags from `wp-includes/post-template.php`: the title and
//! the classes of a post.

use hyscribe_model::Post;
use hyscribe_phpser::{Value, decode, php_trim};

use crate::formatting::{CORE_SHORTCODE_TAGS, capital_p_dangit_title, convert_chars, wptexturize};
use crate::site::{Request, Site};

/// `get_the_title( $post )` with the `the_title` filters.
pub(crate) fn the_title(post: &Post) -> Vec<u8> {
    let title = if !post.password.is_empty() {
        [b"Protected: ", &post.title[..]].concat()
    } else if post.status == "private" {
        [b"Private: ", &post.title[..]].concat()
    } else {
        post.title.clone()
    };
    let title = convert_chars(&wptexturize(&title, &CORE_SHORTCODE_TAGS));
    capital_p_dangit_title(php_trim(&title))
}

/// `sanitize_html_class( $class, $fallback )`.
fn sanitize_html_class(class: &[u8], fallback: &[u8]) -> Vec<u8> {
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
        let format = site
            .post_terms(id, "post_format")
            .first()
            .map(|term| {
                term.slug
                    .strip_prefix(b"post-format-")
                    .unwrap_or(&term.slug)
                    .to_vec()
            })
            .map(|format| sanitize_html_class(&format, b""))
            .filter(|format| !format.is_empty());
        classes.push([b"format-", format.as_deref().unwrap_or(b"standard")].concat());
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
    if is_sticky && request.home && !request.paged {
        classes.push(b"sticky".to_vec());
    }
    classes.push(b"hentry".to_vec());
    for taxonomy in taxonomies(&post.post_type) {
        for term in site.post_terms(id, taxonomy) {
            if term.slug.is_empty() {
                continue;
            }
            let mut term_class = sanitize_html_class(&term.slug, term.id.to_string().as_bytes());
            let trimmed = term_class.iter().all(|&b| b == b'-');
            if Value::String(term_class.clone()).is_numeric() || trimmed {
                term_class = term.id.to_string().into_bytes();
            }
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

#[cfg(test)]
mod tests;
