// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the server does with a response's links: the `targetHints` of a
//! `self` link and the `_embed`ded responses of embeddable ones, each from
//! the request `WP_REST_Request::from_url()` makes of a link's `href`.

use hyscribe_phpser::{Key, Value, is_empty_string, lookup, parse_str};

use crate::response::{Response, list, text};
use crate::schema::{is_list, parse_list};
use crate::{Context, NotPorted, dispatch, prepare, routes};

/// A GET request's route and query.
type Asked = (Vec<u8>, Vec<(Key, Value)>);

/// `WP_REST_Request::from_url( $url )`: the route and query of a GET
/// request for `url`, none when it names no route.
fn from_url(context: &Context, url: &[u8]) -> Option<Asked> {
    let before_fragment = url.split(|&b| b == b'#').next().unwrap_or_default();
    let (path, query) = match before_fragment.iter().position(|&b| b == b'?') {
        Some(at) => (
            before_fragment.get(..at).unwrap_or_default(),
            before_fragment.get(at + 1..).unwrap_or_default(),
        ),
        None => (before_fragment, &b""[..]),
    };
    let mut query = parse_str(query).as_array().unwrap_or_default().to_vec();
    let root = context.rest_url("/");
    let pretty = context
        .site
        .option("permalink_structure")
        .is_some_and(|structure| !structure.is_empty());
    let route = if pretty && url.starts_with(root.as_bytes()) {
        let root = root.trim_end_matches('/');
        path.get(root.len()..).unwrap_or_default().to_vec()
    } else {
        let route = lookup(&query, "rest_route")
            .filter(|route| !route.is_empty())?
            .to_php_string();
        query.retain(|(key, _)| key.to_php_string() != b"rest_route");
        route
    };
    (!is_empty_string(&route)).then_some((route, query))
}

/// `get_target_hints_for_link()`: the methods the current user may use on
/// the link's route, as its `Allow` header would list them. Routes link
/// themselves only to ported routes.
pub(crate) fn target_hints(context: &Context, link: &[(Key, Value)]) -> Option<Value> {
    let href = lookup(link, "href").map(Value::to_php_string);
    let asked = href.and_then(|href| from_url(context, &href));
    let prepared = asked.and_then(|(route, query)| {
        let matched = routes::match_request("GET", &route);
        matched.and_then(|matched| prepare(context, matched, &query).ok())
    });
    let allowed = prepared
        .filter(|prepared| prepared.checked.is_ok())
        .map(|prepared| prepared.allowed(context))
        .unwrap_or_default();
    (!allowed.is_empty()).then(|| {
        let methods = allowed.into_iter().map(text).collect();
        Value::Array(vec![(Key::String(b"allow".to_vec()), list(methods))])
    })
}

/// `rest_parse_embed_param()`: every relation, or those listed.
pub(crate) fn embed_param(embed: &Value) -> Option<Vec<Vec<u8>>> {
    let named = matches!(embed, Value::String(text) if matches!(text.as_slice(), b"true" | b"1"));
    let rels: Vec<Vec<u8>> = parse_list(embed)
        .iter()
        .map(|(_, rel)| rel.to_php_string())
        .collect();
    (!embed.is_empty() && !named)
        .then_some(rels)
        .filter(|rels| !rels.is_empty())
}

/// What `_embed` asks for: nothing, every relation, or those listed.
pub(crate) type Embed = Option<Option<Vec<Vec<u8>>>>;

/// `response_to_data()`: the data with its links, and the embeddable
/// links' responses when asked for.
pub(crate) fn response_to_data(
    context: &Context,
    response: Response,
    embed: &Embed,
) -> Result<Value, NotPorted> {
    let data = response.into_data(context);
    let Some(rels) = embed else {
        return Ok(data);
    };
    let mut cache = Vec::new();
    if !is_list(&data) {
        return embed_links(context, data, rels, &mut cache);
    }
    let embedded: Result<Vec<(Key, Value)>, NotPorted> = data
        .as_array()
        .unwrap_or_default()
        .iter()
        .map(|(key, item)| {
            Ok((
                key.clone(),
                embed_links(context, item.clone(), rels, &mut cache)?,
            ))
        })
        .collect();
    embedded.map(Value::Array)
}

/// `embed_links()`: `_embedded` with each embeddable link's response, in
/// context `embed`, by relation.
fn embed_links(
    context: &Context,
    data: Value,
    rels: &Option<Vec<Vec<u8>>>,
    cache: &mut Vec<(Vec<u8>, Value)>,
) -> Result<Value, NotPorted> {
    let links = data.get_str("_links").and_then(Value::as_array);
    let mut embedded = Vec::new();
    for (rel, items) in links.unwrap_or_default() {
        let name = rel.to_php_string();
        if rels.as_ref().is_some_and(|rels| !rels.contains(&name)) {
            continue;
        }
        let mut embeds = Vec::new();
        for (_, item) in items.as_array().unwrap_or_default() {
            let href = item.get_str("href").map(Value::to_php_string);
            let embeddable = item.get_str("embeddable").is_some_and(|e| !e.is_empty());
            let found = match href.filter(|_| embeddable) {
                Some(href) => embedded_response(context, &href, cache)?,
                None => None,
            };
            embeds.push(found.unwrap_or(Value::Array(Vec::new())));
        }
        if embeds.iter().any(|embed| !embed.is_empty()) {
            embedded.push((rel.clone(), list(embeds)));
        }
    }
    if embedded.is_empty() {
        return Ok(data);
    }
    let mut entries = data.as_array().unwrap_or_default().to_vec();
    entries.push((Key::String(b"_embedded".to_vec()), Value::Array(embedded)));
    Ok(Value::Array(entries))
}

/// The response to `href` as a GET request in context `embed`, a page of
/// the most a collection gives; none when `href` names no route.
pub(crate) fn embedded_response(
    context: &Context,
    href: &[u8],
    cache: &mut Vec<(Vec<u8>, Value)>,
) -> Result<Option<Value>, NotPorted> {
    if let Some((_, known)) = cache.iter().find(|(known, _)| known == href) {
        return Ok(Some(known.clone()));
    }
    let Some((route, mut query)) = from_url(context, href) else {
        return Ok(None);
    };
    let set = |query: &[(Key, Value)], name| lookup(query, name).is_some_and(|v| !v.is_empty());
    if !set(&query, "context") {
        query.push((Key::String(b"context".to_vec()), text("embed")));
    }
    if !set(&query, "per_page")
        && let Some(maximum) = routes::match_request("GET", &route)
            .and_then(|matched| crate::per_page_maximum(matched.regex, matched.endpoint))
    {
        query.push((Key::String(b"per_page".to_vec()), Value::Int(maximum)));
    }
    let embedded = Context {
        head: false,
        ..*context
    };
    let response = dispatch(&embedded, "GET", &route, &query)?;
    let data = response.into_data(&embedded);
    cache.push((href.to_vec(), data.clone()));
    Ok(Some(data))
}
