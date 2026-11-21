// SPDX-License-Identifier: AGPL-3.0-or-later

//! Post template tags from `wp-includes/post-template.php`: the title, and
//! the classes of a post and of the page body.

use hyscribe_kses::esc_attr;
use hyscribe_model::hooks::apply_filters;
use hyscribe_model::{Post, PostQuery};
use hyscribe_phpser::{Value, decode, is_empty_string, php_trim};

use crate::formatting::{CORE_SHORTCODE_TAGS, capital_p_dangit_title, convert_chars, wptexturize};
use crate::site::{Archive, Request, Site, Taxonomy};
use crate::template::replace;

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

/// `get_the_title( $post )` in the admin, which prefixes no state.
pub fn admin_title(post: &Post) -> Vec<u8> {
    title_filters(&post.title)
}

/// `get_the_title( $post )` with the `the_title` filters.
pub fn the_title(post: &Post) -> Vec<u8> {
    let title = if !post.password.is_empty() {
        [b"Protected: ", &post.title[..]].concat()
    } else if post.status == "private" {
        [b"Private: ", &post.title[..]].concat()
    } else {
        post.title.clone()
    };
    title_filters(&title)
}

/// [`the_title`], or `(no title)` for one that is `empty()`, as post
/// lists show it.
pub(crate) fn listed_title(post: &Post) -> Vec<u8> {
    let title = the_title(post);
    if is_empty_string(&title) {
        return b"(no title)".to_vec();
    }
    title
}

/// The `the_title` filters: `wptexturize`, `convert_chars`, `trim` and
/// `capital_P_dangit`.
pub(crate) fn title_filters(title: &[u8]) -> Vec<u8> {
    let title = convert_chars(&wptexturize(title, &CORE_SHORTCODE_TAGS));
    capital_p_dangit_title(php_trim(&title))
}

pub use hyscribe_kses::sanitize_html_class;

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

/// `get_post_class( $class, $post )`, joined by spaces, for `request` or
/// the admin (`None`); only `post` has post formats among core types.
pub fn post_class(site: &dyn Site, request: Option<&Request>, class: &str, post: &Post) -> Vec<u8> {
    let id = post.id;
    let mut classes: Vec<Vec<u8>> = class
        .split_whitespace()
        .map(|c| c.as_bytes().to_vec())
        .collect();
    classes.push(format!("post-{id}").into_bytes());
    if request.is_some() {
        classes.push(post.post_type.clone().into_bytes());
    }
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
    match request {
        Some(request) if is_sticky && request.home && !request.is_paged() => {
            classes.push(b"sticky".to_vec());
        }
        None if is_sticky => classes.push(b"status-sticky".to_vec()),
        _ => {}
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
    for class in classes.iter().map(|class| esc_attr(class).into_bytes()) {
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

/// `is_privacy_policy()` of the page `post` shown on its own, `option`
/// the `wp_page_for_privacy_policy` option: it names the page by ID, title
/// or name. A path naming it is not told apart.
pub fn is_privacy_policy(option: &[u8], post: &Post) -> bool {
    !is_empty_string(option)
        && post.post_type == "page"
        && (option == post.id.to_string().as_bytes() || option == post.title || option == post.name)
}

/// `get_page_template_slug( $post )`: none for `default`.
pub(crate) fn page_template_slug(site: &dyn Site, post: u64) -> Option<Vec<u8>> {
    let slug = site.post_meta(post, "_wp_page_template")?;
    (!is_empty_string(&slug) && slug != b"default").then_some(slug)
}

/// `get_body_class()` for a visitor, with `template` as `get_template()` and
/// `stylesheet` as `get_stylesheet()`, which differs for a child theme.
pub fn body_class(
    site: &dyn Site,
    request: &Request,
    (template, stylesheet): (&str, &str),
) -> Vec<u8> {
    let mut classes: Vec<Vec<u8>> = Vec::new();
    let mut push = |class: &[u8]| classes.push(class.to_vec());
    let singular = request.posts.first().filter(|_| request.singular);
    for (on, class) in [
        (request.front_page, &b"home"[..]),
        (request.home, b"blog"),
        (
            singular.is_some_and(|post| {
                is_privacy_policy(
                    &site
                        .option("wp_page_for_privacy_policy")
                        .unwrap_or_default(),
                    post,
                )
            }),
            b"privacy-policy",
        ),
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
    let attachment = singular.filter(|_| request.attachment);
    if attachment.is_some() {
        push(b"attachment");
    }
    if request.not_found {
        push(b"error404");
    }
    if let Some(post) = singular {
        let kind = &post.post_type;
        push(b"wp-singular");
        match page_template_slug(site, post.id) {
            Some(slug) => {
                let class = |name: &[u8]| {
                    let name = sanitize_html_class(&replace(name, b".", b"-"), b"");
                    [format!("{kind}-template-").as_bytes(), &name].concat()
                };
                push(format!("{kind}-template").as_bytes());
                for part in slug.split(|&b| b == b'/') {
                    // basename( $part, '.php' )
                    let base = part.strip_suffix(b".php").filter(|base| !base.is_empty());
                    push(&class(base.unwrap_or(part)));
                }
                push(&class(&slug));
            }
            None => push(format!("{kind}-template-default").as_bytes()),
        }
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
        }
        if attachment.is_some() {
            push(format!("attachmentid-{}", post.id).as_bytes());
            push(format!("attachment-{}", mime_class(&post.mime_type)).as_bytes());
        } else if kind == "page" {
            push(b"page");
            push(format!("page-id-{}", post.id).as_bytes());
            // get_pages( array( 'parent' => $page_id, 'number' => 1 ) )
            let children = site.query(&PostQuery {
                post_types: vec!["page".into()],
                parents: vec![post.id],
                limit: 1,
                ..PostQuery::default()
            });
            if !children.is_empty() {
                push(b"page-parent");
            }
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
    if template != stylesheet {
        push(
            &[
                b"wp-child-theme-",
                &sanitize_html_class(stylesheet.as_bytes(), b"")[..],
            ]
            .concat(),
        );
    }
    let escaped = classes.iter().map(|class| esc_attr(class).into_bytes());
    // No caller passes `$css_class`.
    let none: Vec<Vec<u8>> = Vec::new();
    let filtered = apply_filters("body_class", escaped.collect::<Vec<_>>(), &[&none]);
    let mut unique: Vec<Vec<u8>> = Vec::with_capacity(filtered.len());
    for class in filtered {
        if !unique.contains(&class) {
            unique.push(class);
        }
    }
    // `body_class()` escapes them again, joined.
    esc_attr(&unique.join(&b' ')).into_bytes()
}

/// The mime type without the `application/`, `image/`, `text/`, `audio/`,
/// `video/` or `music/` prefix that `get_body_class()` takes off it.
fn mime_class(mime: &str) -> String {
    let mut class = mime.to_string();
    for prefix in [
        "application/",
        "image/",
        "text/",
        "audio/",
        "video/",
        "music/",
    ] {
        class = class.replace(prefix, "");
    }
    class
}

#[cfg(test)]
mod tests;
