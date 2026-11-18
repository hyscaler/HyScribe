// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Server::get_index()`, the route `/`: the site's settings, the
//! image settings for users who may upload, and its logo, icon and theme.
//! Its `namespaces` and `routes` would list routes HyScribe does not port,
//! so a request whose `_fields` keeps either, or that gives none, is
//! answered 501 (ADR 0013). The namespace indexes, `get_namespace_index()`,
//! describe their routes from the exported descriptions.

use hyscribe_phpser::{Key, Value};
use hyscribe_render::attachment::image_downsize;

use crate::fields::{filter_fields, is_field_included, links_wanted};
use crate::request::{Arg, Params, Sanitize};
use crate::response::{Response, RestError, object, text};
use crate::schema::parse_list;
use crate::{Context, Endpoint, describe, routes, unserialized};

/// The endpoints of the route `regex`, if `/` or a namespace's index.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    if regex == "/" {
        // Its one argument, `context`, describes the routes alone.
        return Some(vec![Endpoint {
            args: Vec::new(),
            permission: |_, _| Ok(()),
            callback: Some(index),
        }]);
    }
    let namespace = regex.trim_start_matches('/');
    routes::namespace_routes(namespace.as_bytes())?;
    let default = |name, value: &str| Arg {
        name,
        schema: object([("default", text(value))]),
        validate: false,
        sanitize: Sanitize::Endpoint,
    };
    Some(vec![Endpoint {
        args: vec![default("namespace", namespace), default("context", "view")],
        permission: |_, _| Ok(()),
        callback: Some(namespace_index),
    }])
}

/// `get_namespace_index()`: the routes of the namespace its argument names,
/// described as `get_data_for_routes()` describes them, with their schemas
/// in the `help` context only.
fn namespace_index(context: &Context, params: &Params) -> Result<Response, RestError> {
    // A namespace that is not a string is an offset PHP fails on.
    let Some(Value::String(namespace)) = params.get("namespace") else {
        return Err(RestError::not_ported());
    };
    let Some(regexes) = routes::namespace_routes(namespace) else {
        let message = "The specified namespace could not be found.";
        return Err(RestError::status("rest_invalid_namespace", message, 404));
    };
    let help = params.get("context") == Some(&text("help"));
    let routes = regexes.into_iter().map(|regex| {
        let mut description = describe(context, regex);
        if let (false, Value::Array(entries)) = (help, &mut description) {
            entries.retain(|(key, _)| *key != Key::String(b"schema".to_vec()));
        }
        (Key::String(regex.as_bytes().to_vec()), description)
    });
    let data = object([
        ("namespace", Value::String(namespace.clone())),
        ("routes", Value::Array(routes.collect())),
    ]);
    let mut response = Response::new(data, 200);
    response.add_link("up", context.rest_url("/"));
    Ok(response)
}

/// `get_option( $name )`: `false` when not stored.
fn option(context: &Context, name: &str) -> Value {
    context
        .option(name)
        .map_or(Value::Bool(false), |stored| unserialized(&stored))
}

/// `is_ssl()`: a site whose URL is https is reached over a TLS proxy, as
/// `hyscribe-http` reads it.
fn is_ssl(context: &Context) -> bool {
    let url = context.option("siteurl").unwrap_or_default();
    url.to_ascii_lowercase().starts_with(b"https:")
}

/// `wp_is_client_side_media_processing_enabled()`: a secure context, or a
/// host of `localhost`.
pub(crate) fn client_side_media(context: &Context) -> bool {
    // `strtok( $host, ':' )` skips the separators before its token.
    let host = context.host.to_ascii_lowercase();
    let host = host.split(|b| *b == b':').find(|part| !part.is_empty());
    let host = host.unwrap_or_default();
    is_ssl(context) || host == b"localhost" || host.ends_with(b".localhost")
}

/// `get_index()`.
fn index(context: &Context, params: &Params) -> Result<Response, RestError> {
    // What PHP gives that is not ported: the routes, and on HTTPS the
    // application passwords `rest_add_application_passwords_to_index()` adds.
    let mut unported = vec![("namespaces", Value::Null), ("routes", Value::Null)];
    if is_ssl(context) {
        unported.push(("authentication", Value::Null));
    }
    if !filter_fields(object_of(unported), params).is_empty() {
        return Err(RestError::not_ported());
    }
    let int = |name| Value::Int(option(context, name).to_int());
    let mut data = vec![
        ("name", option(context, "blogname")),
        ("description", option(context, "blogdescription")),
        ("url", option(context, "siteurl")),
        ("home", text(context.site.reads().home_url())),
        ("gmt_offset", context.gmt_offset()),
        ("timezone_string", option(context, "timezone_string")),
        ("page_for_posts", int("page_for_posts")),
        ("page_on_front", int("page_on_front")),
        ("show_on_front", option(context, "show_on_front")),
        ("authentication", Value::Array(Vec::new())),
    ];
    if client_side_media(context) && context.can("upload_files") {
        let sizes = crate::media::registered_sizes(context)
            .into_iter()
            .map(|size| {
                let crop = size.crop != hyscribe_image::subsizes::Crop::No;
                let entry = object([
                    ("width", Value::Int(size.width.into())),
                    ("height", Value::Int(size.height.into())),
                    ("crop", Value::Bool(crop)),
                ]);
                (Key::String(size.name.into_bytes()), entry)
            });
        data.extend([
            ("image_sizes", Value::Array(sizes.collect())),
            ("image_size_threshold", Value::Int(2560)),
            ("image_strip_meta", Value::Bool(true)),
            ("image_max_bit_depth", Value::Int(16)),
        ]);
    }
    let mut response = Response::new(Value::Null, 200);
    let mut fields: Vec<Vec<u8>> = params
        .get("_fields")
        .map(parse_list)
        .unwrap_or_default()
        .iter()
        .map(|(_, field)| field.to_php_string())
        .collect();
    // Without `_fields` PHP adds `_links`, which is answered 501 above.
    if params.has("_embed") {
        fields.push(b"_embedded".to_vec());
    }
    let stylesheet = context.option("stylesheet").unwrap_or_default();
    let stylesheet = String::from_utf8_lossy(&stylesheet);
    let links = links_wanted(&fields);
    if links {
        response.add_link("help", "https://developer.wordpress.org/rest-api/".into());
        let themes = ["switch_themes", "manage_network_themes"];
        let theme =
            themes.iter().any(|cap| context.can(cap)) || crate::types::can_edit_any(context);
        if theme {
            let route = format!("/wp/v2/themes/{stylesheet}");
            response.add_link("https://api.w.org/active-theme", context.rest_url(&route));
        }
    }
    if links || is_field_included("site_logo", &fields) {
        // `get_theme_mod( 'custom_logo' )`, with the `site_logo` option first
        // (`_override_custom_logo_theme_mod()`).
        let logo = context.option("site_logo").map_or_else(
            || {
                let mods = format!("theme_mods_{stylesheet}");
                let logo = crate::navigation::option_entry(context, &mods, "custom_logo");
                logo.filter(|logo| *logo != Value::Null)
                    .unwrap_or(Value::Int(0))
            },
            |stored| unserialized(&stored),
        );
        image(context, &mut response, &mut data, &logo, "site_logo");
    }
    if links
        || ["site_icon", "site_icon_url"]
            .iter()
            .any(|field| is_field_included(field, &fields))
    {
        let icon = context
            .option("site_icon")
            .map_or(Value::Int(0), |stored| unserialized(&stored));
        image(context, &mut response, &mut data, &icon, "site_icon");
        data.push(("site_icon_url", site_icon_url(context)));
    }
    response.data = object_of(data);
    Ok(response)
}

/// An object of `entries`, in their order.
fn object_of(entries: Vec<(&str, Value)>) -> Value {
    let entries = entries
        .into_iter()
        .map(|(name, value)| (Key::String(name.as_bytes().to_vec()), value));
    Value::Array(entries.collect())
}

/// `add_image_to_index()`: the image's ID, and a link to it if it has one.
fn image(
    context: &Context,
    response: &mut Response,
    data: &mut Vec<(&str, Value)>,
    id: &Value,
    kind: &'static str,
) {
    data.push((kind, Value::Int(id.to_int())));
    if id.is_empty() {
        return;
    }
    // `rest_get_route_for_post()`: none for a post not shown in REST.
    let post = u64::try_from(id.to_int())
        .ok()
        .and_then(|id| context.site.post(id));
    let route = post.and_then(|post| {
        let items = crate::types::items_route(&post.post_type);
        items.map(|items| format!("{items}/{}", post.id))
    });
    let href = context.rest_url(route.as_deref().unwrap_or("/"));
    let attributes = vec![("embeddable", Value::Bool(true)), ("type", text(kind))];
    response.add_link_with("https://api.w.org/featuredmedia", href, attributes);
}

/// `get_site_icon_url()`: the icon's full image, empty without one. Over
/// HTTPS it is https already, as the uploads' URL is the site URL's.
fn site_icon_url(context: &Context) -> Value {
    let id = option(context, "site_icon").to_int();
    let image = u64::try_from(id)
        .ok()
        .filter(|id| *id != 0)
        .and_then(|id| image_downsize(context.site.reads(), id, "full"));
    text(image.map(|(url, _, _)| url).unwrap_or_default())
}
