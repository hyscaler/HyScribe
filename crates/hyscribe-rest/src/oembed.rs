// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_oEmbed_Controller`: `/oembed/1.0/embed`, this site's posts as oEmbed
//! in JSON or, through `_oembed_rest_pre_serve_request()`, XML; and
//! `/oembed/1.0/proxy` for this site's own posts, which fetches nothing.
//! What the proxy would fetch waits on `WP_oEmbed` and is answered 501, as
//! is a URL `url_to_postid()` would match against the rewrite rules, and
//! an attachment, whose thumbnail is not ported.

use hyscribe_kses::{esc_html, parse_url};
use hyscribe_phpser::{Key, Value, absint, encode, md5, php_trim};
use hyscribe_render::head::post_embed_html;
use hyscribe_render::post::the_title;

use crate::request::{Arg, Params, Sanitize, arg, typed};
use crate::response::{Response, RestError, object, text};
use crate::schema::parse_list;
use crate::{Context, Endpoint, hooks, types, unserialized};

/// The route `_oembed_rest_pre_serve_request()` serves as XML.
pub(crate) const EMBED: &str = "/oembed/1.0/embed";

const PROXY: &str = "/oembed/1.0/proxy";

/// The URL both routes take.
fn url_arg() -> Arg {
    typed("url", r#"{"required":true,"type":"string","format":"uri"}"#)
}

pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    // `oembed_default_width`'s default.
    let width = |schema| arg("maxwidth", schema, false, Sanitize::Absint);
    match regex {
        EMBED => Some(vec![Endpoint {
            // `wp_oembed_ensure_format()` gives `xml` for `xml` alone, which
            // is all the XML is served for, so the format is kept as given.
            args: vec![
                url_arg(),
                arg("format", r#"{"default":"json"}"#, false, Sanitize::Endpoint),
                width(r#"{"default":600}"#),
            ],
            permission: |_, _| Ok(()),
            callback: Some(embed),
        }]),
        PROXY => Some(vec![Endpoint {
            args: vec![
                url_arg(),
                typed(
                    "format",
                    r#"{"type":"string","default":"json","enum":["json","xml"]}"#,
                ),
                width(r#"{"type":"integer","default":600}"#),
                arg(
                    "maxheight",
                    r#"{"type":"integer"}"#,
                    false,
                    Sanitize::Absint,
                ),
                typed("discover", r#"{"type":"boolean","default":true}"#),
            ],
            permission: proxy_permission,
            callback: Some(proxy),
        }]),
        _ => None,
    }
}

/// `oembed_invalid_url`.
fn not_found() -> RestError {
    RestError::status("oembed_invalid_url", "Not Found", 404)
}

/// `get_item()`.
fn embed(context: &Context, params: &Params) -> Result<Response, RestError> {
    let data = post_data(context, params)?.ok_or_else(not_found)?;
    Ok(Response::new(data, 200))
}

/// `get_proxy_item_permissions_check()`.
fn proxy_permission(context: &Context, _: &Params) -> Result<(), RestError> {
    if context.can("edit_posts") {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to make proxied oEmbed requests.";
    Err(context.forbidden("rest_forbidden", message))
}

/// `get_proxy_item()` for this site's own posts: one PHP cached, or a URL
/// it would fetch, is not ported.
fn proxy(context: &Context, params: &Params) -> Result<Response, RestError> {
    // `rest_filter_response_fields()` fails on the object PHP answers.
    if params
        .get("_fields")
        .is_some_and(|fields| !parse_list(fields).is_empty())
    {
        return Err(RestError::not_ported());
    }
    let key = format!(
        "_transient_oembed_{}",
        md5(&encode(&cache_args(context, params)))
    );
    if context.option(&key).is_some() {
        return Err(RestError::not_ported());
    }
    let data = post_data(context, params)?.ok_or_else(RestError::not_ported)?;
    Ok(Response::new(data, 200))
}

/// `$request->get_params()` without `_wpnonce`, which the proxy's cache key
/// is made of: the defaults, then the URL's, the query's and the body's
/// parameters, each kept where it first appears; without pretty permalinks,
/// without `rest_route`.
fn cache_args(context: &Context, params: &Params) -> Value {
    let mut args: Vec<(Key, Value)> = Vec::new();
    let sources = [
        &params.defaults,
        &params.url,
        &params.get,
        &params.post,
        &params.json,
    ];
    for (key, value) in sources.into_iter().flatten() {
        match args.iter_mut().find(|(known, _)| known == key) {
            Some((_, held)) => *held = value.clone(),
            None => args.push((key.clone(), value.clone())),
        }
    }
    let plain = context
        .option("permalink_structure")
        .is_none_or(|structure| Value::String(structure).is_empty());
    args.retain(|(key, _)| {
        let name = key.to_php_string();
        name != b"_wpnonce" && !(plain && name == b"rest_route")
    });
    Value::Array(args)
}

/// `get_oembed_response_data()` of the post the `url` parameter names, at
/// the `maxwidth` parameter's width.
fn post_data(context: &Context, params: &Params) -> Result<Option<Value>, RestError> {
    let url = params.text("url");
    let id = url_to_postid(context, &url)?;
    let id: u64 = hooks::apply_filters("oembed_request_post_id", id, &[&url]);
    let width = params.get("maxwidth").map_or(0, Value::to_int);
    response_data(context, id, width)
}

/// `url_to_postid()`: 0 for a URL of another host, or of none of this
/// site's posts under plain permalinks.
fn url_to_postid(context: &Context, url: &[u8]) -> Result<u64, RestError> {
    let host = |url: &[u8]| {
        let host = parse_url(url)
            .and_then(|parts| parts.host)
            .unwrap_or_default();
        replace(&host, b"www.", b"")
    };
    let home = context.site.reads().home_url();
    let url_host = host(url);
    if !Value::String(url_host.clone()).is_empty() && url_host != host(&home) {
        return Ok(0);
    }
    if let Some(id) = id_argument(url).filter(|id| *id != 0) {
        return Ok(id);
    }
    let url = normalized(url, &home);
    let start = url.iter().position(|b| *b != b'/').unwrap_or(url.len());
    let end = url
        .iter()
        .rposition(|b| *b != b'/')
        .map_or(start, |end| end + 1);
    let trimmed = url.get(start..end).unwrap_or_default();
    let front = context.option("show_on_front").unwrap_or_default() == b"page";
    if trimmed == home.as_slice() && front {
        let page = context
            .option("page_on_front")
            .map_or(0, |page| absint(&unserialized(&page)));
        if context.site.post(page).is_some() {
            return Ok(page);
        }
    }
    let plain = context
        .option("permalink_structure")
        .is_none_or(|structure| Value::String(structure).is_empty());
    if plain {
        return Ok(0);
    }
    // The rewrite rules are matched by `hyscribe-http` alone.
    Err(RestError::not_ported())
}

/// `url_to_postid()`'s `url` without its fragment and query, in the scheme
/// of `home` and with `www.` where `home` has it.
pub(crate) fn normalized(url: &[u8], home: &[u8]) -> Vec<u8> {
    let url = url.split(|b| *b == b'#').next().unwrap_or_default();
    let url = url.split(|b| *b == b'?').next().unwrap_or_default();
    // `set_url_scheme()` to the home URL's scheme.
    let scheme = parse_url(home)
        .and_then(|parts| parts.scheme)
        .unwrap_or_default();
    let url = php_trim(url);
    let url = match url
        .iter()
        .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_'))
    {
        _ if url.starts_with(b"//") => [&scheme[..], b":", url].concat(),
        Some(end) if end > 0 && url.get(end..).is_some_and(|rest| rest.starts_with(b"://")) => {
            [&scheme[..], url.get(end..).unwrap_or_default()].concat()
        }
        _ => url.to_vec(),
    };
    let www = |url: &[u8]| contains(url, b"://www.");
    match (www(home), www(&url)) {
        (true, false) => replace(&url, b"://", b"://www."),
        (false, _) => replace(&url, b"://www.", b"://"),
        (true, true) => url,
    }
}

/// `preg_match( '#[?&](p|page_id|attachment_id)=(\d+)#', $url )`'s ID,
/// through `absint()`.
fn id_argument(url: &[u8]) -> Option<u64> {
    (0..url.len()).find_map(|at| {
        let rest = url.get(at..).unwrap_or_default();
        let rest = rest
            .strip_prefix(b"?")
            .or_else(|| rest.strip_prefix(b"&"))?;
        let value = [&b"p="[..], b"page_id=", b"attachment_id="]
            .iter()
            .find_map(|name| rest.strip_prefix(*name))?;
        let digits = value.iter().take_while(|b| b.is_ascii_digit()).count();
        let digits = value.get(..digits).filter(|digits| !digits.is_empty())?;
        Some(absint(&Value::String(digits.to_vec())))
    })
}

/// Whether `needle` is in `haystack`.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// `str_replace( $from, $to, $subject )`.
fn replace(subject: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(subject.len());
    let mut rest = subject;
    while !rest.is_empty() {
        if rest.starts_with(from) {
            out.extend_from_slice(to);
            rest = rest.get(from.len()..).unwrap_or_default();
        } else {
            out.extend(rest.first());
            rest = rest.get(1..).unwrap_or_default();
        }
    }
    out
}

/// `get_oembed_response_data( $post, $width )` with
/// `get_oembed_response_data_rich()`: `None` for a post that is not
/// published or not of a type the public sees.
fn response_data(context: &Context, id: u64, width: i64) -> Result<Option<Value>, RestError> {
    let Some(post) = (id != 0).then(|| context.site.post(id)).flatten() else {
        return Ok(None);
    };
    // Its thumbnail, an image's own or a video's, is not ported.
    if post.post_type == "attachment" {
        return Err(RestError::not_ported());
    }
    // `is_post_publicly_viewable()` and `is_post_embeddable()`: of the
    // types WordPress registers, posts and pages are both when published.
    if !types::viewable(&post.post_type) || post.status != "publish" {
        return Ok(None);
    }
    let thumbnail = context.site.post_meta(post.id, "_thumbnail_id");
    if thumbnail.is_some_and(|id| absint(&Value::String(id)) != 0) {
        return Err(RestError::not_ported());
    }
    let width = width.unsigned_abs().clamp(200, 600);
    let height = (width * 9).div_ceil(16).max(200);
    let name = context
        .option("blogname")
        .map_or(Value::Bool(false), |name| unserialized(&name))
        .to_php_string();
    let home = context.site.reads().home_url();
    let author = context.site.user(post.author);
    let (author_name, author_url) = author.map_or((name.clone(), home.clone()), |author| {
        let link = context.site.author_link(&author).into_bytes();
        (author.display_name, link)
    });
    let secret = context.site.embed_secret();
    let html = post_embed_html(context.site.reads(), &post, (width, height), &secret);
    let html = html.ok_or_else(RestError::not_ported)?;
    let int = |n: u64| Value::Int(i64::try_from(n).unwrap_or_default());
    Ok(Some(object([
        ("version", text("1.0")),
        ("provider_name", text(name)),
        ("provider_url", text(home)),
        ("author_name", text(author_name)),
        ("author_url", text(author_url)),
        ("title", text(the_title(&post))),
        ("type", text("rich")),
        ("width", int(width)),
        ("height", int(height)),
        ("html", text(html)),
    ])))
}

/// `_oembed_create_xml()` of `data` as SimpleXML writes it: `None` where
/// it gives no XML, or text SimpleXML would refuse or cut.
pub(crate) fn create_xml(data: &Value) -> Option<Vec<u8>> {
    let entries = data.as_array().filter(|entries| !entries.is_empty())?;
    let mut out = b"<?xml version=\"1.0\"?>\n<oembed>".to_vec();
    children(entries, &mut out)?;
    out.extend_from_slice(b"</oembed>\n");
    Some(out)
}

/// Each entry as a child element: a list's items named `oembed`.
fn children(entries: &[(Key, Value)], out: &mut Vec<u8>) -> Option<()> {
    for (key, value) in entries {
        let name = match key {
            Key::String(name) if !Value::String(name.clone()).is_numeric() => name.clone(),
            _ => b"oembed".to_vec(),
        };
        let inner = match value {
            Value::Array(entries) => {
                let mut inner = Vec::new();
                children(entries, &mut inner)?;
                inner
            }
            value => xml_text(esc_html(&value.to_php_string()).as_bytes())?,
        };
        out.push(b'<');
        out.extend_from_slice(&name);
        if inner.is_empty() {
            out.extend_from_slice(b"/>");
            continue;
        }
        out.push(b'>');
        out.extend(inner);
        out.extend_from_slice(b"</");
        out.extend_from_slice(&name);
        out.push(b'>');
    }
    Some(())
}

/// Escaped HTML as `addChild()` reads it and `asXML()` writes it back: the
/// entities XML defines and character references read, each other entity
/// kept, then `&`, `<`, `>`, carriage returns and all but ASCII as
/// references. `None` for what libxml refuses: a control character, a
/// bad reference or a lone `&`.
pub(crate) fn xml_text(escaped: &[u8]) -> Option<Vec<u8>> {
    let text = std::str::from_utf8(escaped).ok()?;
    let mut out = Vec::new();
    let mut parts = text.split('&');
    xml_chars(parts.next().unwrap_or_default(), &mut out)?;
    for part in parts {
        let (entity, after) = part.split_once(';')?;
        // `&amp;`, `&lt;` and `&gt;` are written back as they are read.
        let named = match entity {
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => None,
        };
        let numeric = || {
            let number = entity.strip_prefix('#')?;
            let code = match number.strip_prefix(['x', 'X']) {
                Some(hex) => u32::from_str_radix(hex, 16).ok(),
                None => number.parse().ok(),
            };
            code.and_then(char::from_u32)
        };
        match named.or_else(numeric) {
            Some(c) => xml_chars(c.encode_utf8(&mut [0; 4]), &mut out)?,
            None if !entity.is_empty() && entity.bytes().all(|b| b.is_ascii_alphanumeric()) => {
                out.extend_from_slice(format!("&{entity};").as_bytes());
            }
            None => return None,
        }
        xml_chars(after, &mut out)?;
    }
    Some(out)
}

/// `text`'s characters as `asXML()` writes them; `None` for a control.
fn xml_chars(text: &str, out: &mut Vec<u8>) -> Option<()> {
    for c in text.chars() {
        match c {
            '&' => out.extend_from_slice(b"&amp;"),
            '<' => out.extend_from_slice(b"&lt;"),
            '>' => out.extend_from_slice(b"&gt;"),
            '\t' | '\n' => out.push(c as u8),
            c if u32::from(c) < 0x20 && c != '\r' => return None,
            c if c.is_ascii() && c != '\r' => out.push(c as u8),
            c => out.extend_from_slice(format!("&#x{:X};", u32::from(c)).as_bytes()),
        }
    }
    Some(())
}
