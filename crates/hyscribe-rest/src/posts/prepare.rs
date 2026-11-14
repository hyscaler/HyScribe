// SPDX-License-Identifier: AGPL-3.0-or-later

//! `prepare_item_for_response()`, `prepare_links()` and
//! `get_available_actions()` of the posts controller.

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Value, add_query_arg, php_trim};
use hyscribe_render::date::{format_unix, site_zone, stored_unix};
use hyscribe_render::permalinks::permalink;
use hyscribe_render::post::{admin_title, post_class};
use hyscribe_render::rest::Rendering;
use hyscribe_render::site::Request;

use super::{Kind, is_sticky, meta, revisions};
use crate::Context;
use crate::fields::{Properties, fields_for_response, filter_by_context, is_field_included};
use crate::request::Params;
use crate::response::{Response, RestError, list, object, text};

const POST: &Properties = &[
    ("date", &["view", "edit", "embed"]),
    ("date_gmt", &["view", "edit"]),
    ("guid", &["view", "edit"]),
    ("id", &["view", "edit", "embed"]),
    ("link", &["view", "edit", "embed"]),
    ("modified", &["view", "edit"]),
    ("modified_gmt", &["view", "edit"]),
    ("slug", &["view", "edit", "embed"]),
    ("status", &["view", "edit"]),
    ("type", &["view", "edit", "embed"]),
    ("password", &["edit"]),
    ("permalink_template", &["edit"]),
    ("generated_slug", &["edit"]),
    ("class_list", &["view", "edit"]),
    ("title", &["view", "edit", "embed"]),
    ("content", &["view", "edit"]),
    ("author", &["view", "edit", "embed"]),
    ("excerpt", &["view", "edit", "embed"]),
    ("featured_media", &["view", "edit", "embed"]),
    ("comment_status", &["view", "edit"]),
    ("ping_status", &["view", "edit"]),
    ("format", &["view", "edit"]),
    ("meta", &["view", "edit"]),
    ("sticky", &["view", "edit"]),
    ("template", &["view", "edit"]),
    ("categories", &["view", "edit"]),
    ("tags", &["view", "edit"]),
];

const PAGE: &Properties = &[
    ("date", &["view", "edit", "embed"]),
    ("date_gmt", &["view", "edit"]),
    ("guid", &["view", "edit"]),
    ("id", &["view", "edit", "embed"]),
    ("link", &["view", "edit", "embed"]),
    ("modified", &["view", "edit"]),
    ("modified_gmt", &["view", "edit"]),
    ("slug", &["view", "edit", "embed"]),
    ("status", &["view", "edit"]),
    ("type", &["view", "edit", "embed"]),
    ("password", &["edit"]),
    ("permalink_template", &["edit"]),
    ("generated_slug", &["edit"]),
    ("class_list", &["view", "edit"]),
    ("parent", &["view", "edit"]),
    ("title", &["view", "edit", "embed"]),
    ("content", &["view", "edit"]),
    ("author", &["view", "edit", "embed"]),
    ("excerpt", &["view", "edit", "embed"]),
    ("featured_media", &["view", "edit", "embed"]),
    ("comment_status", &["view", "edit"]),
    ("ping_status", &["view", "edit"]),
    ("menu_order", &["view", "edit"]),
    ("meta", &["view", "edit"]),
    ("template", &["view", "edit"]),
];

/// The contexts of the parts of `title`, `content`, `excerpt` and `guid`.
const PARTS: &Properties = &[
    ("raw", &["edit"]),
    ("rendered", &["view", "edit", "embed"]),
    ("protected", &["view", "edit", "embed"]),
    ("block_version", &["edit"]),
];

/// `mysql_to_rfc3339()` of a date WordPress has set.
fn rfc3339(date: &str) -> Value {
    text(date.replacen(' ', "T", 1))
}

const UNSET: &str = "0000-00-00 00:00:00";

/// A draft's unset GMT date: `get_gmt_from_date( $post_date )` for the
/// date, `$post_modified` less `gmt_offset` for the modified date.
fn gmt_of(context: &Context, local: &str, modified: bool) -> Value {
    let option = |name| context.site.option(name).unwrap_or_default();
    let utc = site_zone(b"", b"0");
    let unix = if modified {
        let offset = Value::String(option("gmt_offset")).to_float();
        stored_unix(local, utc) - (offset * 3600.0) as i64
    } else {
        stored_unix(
            local,
            site_zone(&option("timezone_string"), &option("gmt_offset")),
        )
    };
    text(format_unix("Y-m-d\\TH:i:s", unix, utc))
}

fn key(name: &str) -> Key {
    Key::String(name.as_bytes().to_vec())
}

fn int(n: u64) -> Value {
    Value::Int(n.cast_signed())
}

/// `get_sample_permalink( $post->ID, $post->post_title, '' )`: the link
/// with the slug left as a token, and the slug made from the title.
fn sample_permalink(context: &Context, post: &Post) -> (Vec<u8>, Vec<u8>) {
    let token: &[u8] = match post.post_type.as_str() {
        "page" => b"%pagename%",
        _ => b"%postname%",
    };
    let sample = Post {
        status: "publish".into(),
        name: token.to_vec(),
        ..post.clone()
    };
    let link = permalink(context.site.reads(), &sample);
    let slug = hyscribe_render::formatting::sanitize_title(&post.title);
    let slug = if slug.is_empty() {
        post.id.to_string().into_bytes()
    } else {
        slug
    };
    // The `editable_slug` filter's `urldecode()`.
    (link, hyscribe_phpser::urldecode(&slug))
}

/// `prepare_item_for_response()`.
pub(super) fn post(
    context: &Context,
    params: &Params,
    kind: Kind,
    post: &Post,
    rendering: &mut Rendering,
) -> Result<Response, RestError> {
    if context.head {
        return Ok(Response::new(Value::Array(Vec::new()), 200));
    }
    let properties = match kind {
        Kind::Post => POST,
        Kind::Page => PAGE,
    };
    let fields = fields_for_response(params, properties);
    let has = |field: &str| is_field_included(field, &fields);
    let reads = context.site.reads();
    let edit = params.context() == b"edit";
    let mut data: Vec<(Key, Value)> = Vec::new();
    let mut put = |name: &str, value: Value| data.push((key(name), value));
    let part = |parts: Vec<(&str, Value)>, prefix: &str| {
        let kept = parts
            .into_iter()
            .filter(|(name, _)| has(&format!("{prefix}.{name}")))
            .map(|(name, value)| (key(name), value))
            .collect();
        Value::Array(kept)
    };
    // `id` is in every context and kept with any `_fields`.
    put("id", int(post.id));
    if has("date") {
        put("date", rfc3339(&post.date));
    }
    if has("date_gmt") {
        let date = if post.date_gmt == UNSET {
            gmt_of(context, &post.date, false)
        } else {
            rfc3339(&post.date_gmt)
        };
        put("date_gmt", date);
    }
    if has("guid") {
        let guid = vec![
            ("rendered", text(post.guid.clone())),
            ("raw", text(post.guid.clone())),
        ];
        put(
            "guid",
            Value::Array(guid.into_iter().map(|(n, v)| (key(n), v)).collect()),
        );
    }
    if has("modified") {
        put("modified", rfc3339(&post.modified));
    }
    if has("modified_gmt") {
        let date = if post.modified_gmt == UNSET {
            gmt_of(context, &post.modified, true)
        } else {
            rfc3339(&post.modified_gmt)
        };
        put("modified_gmt", date);
    }
    if has("password") {
        put("password", text(post.password.clone()));
    }
    if has("slug") {
        put("slug", text(post.name.clone()));
    }
    if has("status") {
        put("status", text(post.status.clone()));
    }
    if has("type") {
        put("type", text(post.post_type.clone()));
    }
    if has("link") {
        put("link", text(permalink(reads, post)));
    }
    if has("title") {
        put(
            "title",
            part(
                vec![
                    ("raw", text(post.title.clone())),
                    ("rendered", text(admin_title(post))),
                ],
                "title",
            ),
        );
    }
    // `post_password_required()`: with the `edit` context's filter, or the
    // password given, the content shows.
    let given = params.get("password").map(Value::to_php_string);
    let unlocked =
        post.password.is_empty() || edit || given.is_some_and(|given| given == post.password);
    let open = Post {
        password: Vec::new(),
        ..post.clone()
    };
    let protected = Value::Bool(!post.password.is_empty());
    let from_content = php_trim(&post.excerpt).is_empty();
    let renders = has("content.rendered") || has("excerpt") && from_content;
    if unlocked && renders && writes_or_unported(&post.content, context.user != 0) {
        return Err(RestError::not_ported());
    }
    // `the_content` renders the content, then the excerpt's filters; in
    // the edit context `insert_hooked_blocks_into_rest_response()`
    // renders the content again.
    let rendered = (unlocked && has("content.rendered")).then(|| rendering.the_content(&open));
    let length = params.get("excerpt_length").map_or(55, Value::to_int);
    let excerpt = (unlocked && has("excerpt")).then(|| rendering.the_excerpt(&open, length));
    let hooked = edit && has("content.raw") && !text(post.content.clone()).is_empty();
    let rendered = match rendered {
        Some(first) if hooked && !text(first.clone()).is_empty() => {
            Some(rendering.the_content(&open))
        }
        rendered => rendered,
    };
    if has("content") {
        let version = i64::from(post.content.windows(8).any(|w| w == b"<!-- wp:"));
        put(
            "content",
            part(
                vec![
                    ("raw", text(post.content.clone())),
                    ("rendered", text(rendered.unwrap_or_default())),
                    ("protected", protected.clone()),
                    ("block_version", Value::Int(version)),
                ],
                "content",
            ),
        );
    }
    if has("excerpt") {
        let excerpt = vec![
            ("raw", text(post.excerpt.clone())),
            ("rendered", text(excerpt.unwrap_or_default())),
            ("protected", protected),
        ];
        put(
            "excerpt",
            Value::Array(excerpt.into_iter().map(|(n, v)| (key(n), v)).collect()),
        );
    }
    let thumbnail = meta(context, post.id, "_thumbnail_id").map_or(0, |id| id.to_int().max(0));
    if has("author") {
        put("author", int(post.author));
    }
    if has("featured_media") {
        put("featured_media", Value::Int(thumbnail));
    }
    if has("parent") {
        put("parent", int(post.parent));
    }
    if has("menu_order") {
        put("menu_order", Value::Int(post.menu_order));
    }
    if has("comment_status") {
        put("comment_status", text(post.comment_status.clone()));
    }
    if has("ping_status") {
        put("ping_status", text(post.ping_status.clone()));
    }
    if has("sticky") {
        put("sticky", Value::Bool(is_sticky(context, post.id)));
    }
    if has("template") {
        let template = meta(context, post.id, "_wp_page_template")
            .map(|template| template.to_php_string())
            .filter(|template| template != b"default");
        put("template", text(template.unwrap_or_default()));
    }
    if has("format") {
        let format = context
            .site
            .post_terms(post.id, "post_format")
            .first()
            .map(|term| {
                let slug = term
                    .slug
                    .strip_prefix(b"post-format-")
                    .unwrap_or(&term.slug);
                slug.to_vec()
            })
            .filter(|format| !format.is_empty());
        put(
            "format",
            text(format.unwrap_or_else(|| b"standard".to_vec())),
        );
    }
    if has("meta") {
        // The `footnotes` meta is a string, as registered.
        let footnotes = text(
            context
                .site
                .post_meta(post.id, "footnotes")
                .unwrap_or_default(),
        );
        put("meta", object([("footnotes", footnotes)]));
    }
    for (taxonomy, base) in kind.taxonomies() {
        if has(base) {
            let terms = context.site.post_terms(post.id, taxonomy);
            put(base, list(terms.iter().map(|term| int(term.id)).collect()));
        }
    }
    if has("permalink_template") || has("generated_slug") {
        let (link, slug) = sample_permalink(context, post);
        for (name, value) in [("permalink_template", link), ("generated_slug", slug)] {
            if has(name) {
                put(name, text(value));
            }
        }
    }
    if has("class_list") {
        // The password filter is off again by now, so a protected post
        // shows as one even in the edit context.
        let classes = post_class(reads, Some(&Request::default()), "", post);
        let classes = classes
            .split(|&b| b == b' ')
            .map(|class| text(class.to_vec()));
        put("class_list", list(classes.collect()));
    }
    let shown = params.context();
    let data: Vec<(Key, Value)> = filter_by_context(data, properties, &shown)
        .into_iter()
        .map(
            |(name, value)| match (name.to_php_string().as_slice(), value) {
                (b"title" | b"content" | b"excerpt" | b"guid", Value::Array(parts)) => {
                    (name, Value::Array(filter_by_context(parts, PARTS, &shown)))
                }
                (_, value) => (name, value),
            },
        )
        .collect();
    let mut response = Response::new(Value::Array(data), 200);
    if has("_links") || has("_embedded") {
        links(context, params, kind, post, &mut response);
    }
    Ok(response)
}

/// Whether `the_content` of `content` does what is not ported: core's
/// shortcodes, the duotone filters and, for a user `logged_in`, the
/// comment and login blocks that show them more, or what PHP writes on, an RSS block's feed cache and
/// `WP_Embed::autoembed()`'s oEmbed cache of a URL alone on its line or in
/// its paragraph.
pub(crate) fn writes_or_unported(content: &[u8], logged_in: bool) -> bool {
    const SHORTCODES: [&[u8]; 7] = [
        b"embed",
        b"wp_caption",
        b"caption",
        b"gallery",
        b"playlist",
        b"audio",
        b"video",
    ];
    let word = |b: &u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-');
    let shortcode = content.iter().enumerate().any(|(at, &b)| {
        let rest = content.get(at + 1..).unwrap_or_default();
        b == b'['
            && SHORTCODES.iter().any(|tag| {
                rest.strip_prefix(*tag)
                    .is_some_and(|after| after.first().is_none_or(|b| !word(b)))
            })
    });
    let url = |text: &[u8]| {
        let lower = text.to_ascii_lowercase();
        let rest = lower
            .strip_prefix(b"http://")
            .or_else(|| lower.strip_prefix(b"https://"));
        rest.is_some_and(|rest| {
            !rest.is_empty()
                && !rest
                    .iter()
                    .any(|b| b.is_ascii_whitespace() || matches!(b, b'<' | b'>' | b'"'))
        })
    };
    let alone = content
        .split(|&b| b == b'\n')
        .any(|line| url(php_trim(line)));
    let paragraph = content.windows(2).enumerate().any(|(at, pair)| {
        let rest = content.get(at + 2..).unwrap_or_default();
        let open = rest.iter().position(|&b| b == b'>');
        let attributes = rest.get(..open.unwrap_or(0)).unwrap_or_default();
        let inner = rest
            .get(open.map_or(rest.len(), |open| open + 1)..)
            .unwrap_or_default();
        let close = inner
            .windows(4)
            .position(|w| w.eq_ignore_ascii_case(b"</p>"));
        pair.eq_ignore_ascii_case(b"<p")
            && open.is_some()
            && (attributes.is_empty() || attributes.first() == Some(&b' '))
            && close.is_some_and(|close| url(php_trim(inner.get(..close).unwrap_or_default())))
    });
    let has = |text: &[u8]| content.windows(text.len()).any(|w| w == text);
    let personal = logged_in
        && (has(b"<!-- wp:comment")
            || has(b"<!-- wp:post-comments-form")
            || has(b"<!-- wp:loginout"));
    let rss = has(b"<!-- wp:rss");
    let duotone = has(b"\"duotone\"");
    rss || duotone || personal || shortcode || alone || paragraph
}

/// `get_post_status()` of an attachment: inheriting, its parent's, or
/// published without one.
fn post_status(context: &Context, post: &Post) -> String {
    if post.status != "inherit" {
        return post.status.clone();
    }
    match context.site.post(post.parent) {
        Some(parent) => parent.status,
        None => "publish".into(),
    }
}

/// `prepare_links()` and the actions `get_available_actions()` allows.
fn links(context: &Context, params: &Params, kind: Kind, post: &Post, response: &mut Response) {
    let rest = |path: &str| context.rest_url(path);
    let with_arg = |path: &str, name: &[u8], id: u64| {
        let id = id.to_string();
        let link = add_query_arg(&[(name, Some(id.as_bytes()))], rest(path).as_bytes());
        String::from_utf8_lossy(&link).into_owned()
    };
    let embeddable = || vec![("embeddable", Value::Bool(true))];
    let base = format!("/wp/v2/{}", kind.base());
    let own = format!("{base}/{}", post.id);
    crate::self_link(context, response, &own);
    response.add_link("collection", rest(&base));
    response.add_link("about", rest(&format!("/wp/v2/types/{}", kind.name())));
    if post.author != 0 {
        let author = rest(&format!("/wp/v2/users/{}", post.author));
        response.add_link_with("author", author, embeddable());
    }
    response.add_link_with(
        "replies",
        with_arg("/wp/v2/comments", b"post", post.id),
        embeddable(),
    );
    let (count, latest) = revisions(context, post);
    let history = vec![("count", int(count))];
    response.add_link_with(
        "version-history",
        rest(&format!("{own}/revisions")),
        history,
    );
    if let Some(latest) = latest {
        let id = vec![("id", int(latest))];
        response.add_link_with(
            "predecessor-version",
            rest(&format!("{own}/revisions/{latest}")),
            id,
        );
    }
    if kind == Kind::Page && post.parent != 0 {
        response.add_link_with(
            "up",
            rest(&format!("/wp/v2/pages/{}", post.parent)),
            embeddable(),
        );
    }
    let featured =
        meta(context, post.id, "_thumbnail_id").map_or(0, |id| id.to_int().max(0).unsigned_abs());
    let media = (featured != 0)
        .then(|| context.site.post(featured))
        .flatten();
    let shown = |media: &Post| {
        post_status(context, media) == "publish" || context.can_for("read_post", media.id)
    };
    if let Some(media) = media.filter(shown) {
        let link = rest(&format!("/wp/v2/media/{}", media.id));
        response.add_link_with("https://api.w.org/featuredmedia", link, embeddable());
    }
    response.add_link(
        "https://api.w.org/attachment",
        with_arg("/wp/v2/media", b"parent", post.id),
    );
    for (taxonomy, base) in kind.taxonomies() {
        let mut attributes = vec![("taxonomy", text(*taxonomy))];
        attributes.extend(embeddable());
        let link = with_arg(&format!("/wp/v2/{base}"), b"post", post.id);
        response.add_link_with("https://api.w.org/term", link, attributes);
    }
    if params.context() != b"edit" {
        return;
    }
    let self_link = rest(&own);
    let mut action = |rel: &str| {
        response.add_link(
            &format!("https://api.w.org/action-{rel}"),
            self_link.clone(),
        )
    };
    if context.can(&kind.cap("publish_posts")) {
        action("publish");
    }
    if context.can("unfiltered_html") {
        action("unfiltered-html");
    }
    let others = context.can(&kind.cap("edit_others_posts"));
    if kind == Kind::Post && others && context.can(&kind.cap("publish_posts")) {
        action("sticky");
    }
    if others {
        action("assign-author");
    }
    for (taxonomy, base) in kind.taxonomies() {
        let (create, assign) = match *taxonomy {
            "category" => ("edit_categories", "assign_categories"),
            _ => ("assign_post_tags", "assign_post_tags"),
        };
        if context.can(create) {
            action(&format!("create-{base}"));
        }
        if context.can(assign) {
            action(&format!("assign-{base}"));
        }
    }
}
