// SPDX-License-Identifier: AGPL-3.0-or-later

//! `_embed`: `WP_REST_Server::response_to_data()` and `embed_links()`, each
//! embeddable link dispatched as an internal GET request in the `embed`
//! context. A link to a route not ported leaves the whole request so.

use hyscribe_phpser::{Key, Value, parse_str};

use crate::request::Params;
use crate::response::Response;
use crate::{Context, NotPorted, dispatch, ported, routes, schema};

/// `rest_parse_embed_param()`: every relation, or only those listed.
pub(crate) fn relations(embed: &Value) -> Option<Vec<(Key, Value)>> {
    if embed.is_empty() || matches!(embed.as_bytes(), Some(b"true" | b"1")) {
        return None;
    }
    Some(schema::parse_list(embed)).filter(|rels| !rels.is_empty())
}

/// `response_to_data( $response, $embed )`: the data with its links, and
/// with those embedded `embed` asks for, if any.
pub(crate) fn to_data(
    context: &Context,
    response: Response,
    embed: Option<&Option<Vec<(Key, Value)>>>,
) -> Result<Value, NotPorted> {
    let data = response.into_data();
    let Some(rels) = embed else {
        return Ok(data);
    };
    let inner = Context {
        site: context.site,
        user: context.user,
        head: false,
        uri: context.uri,
        parameters: false,
        remote_addr: context.remote_addr,
        user_agent: context.user_agent,
    };
    let mut cache = Vec::new();
    let mut embed = |item| embed_links(&inner, item, rels.as_deref(), &mut cache);
    if !schema::is_list(&data) {
        return embed(data);
    }
    let items = data.as_array().unwrap_or_default().iter().cloned();
    let embedded: Result<Vec<(Key, Value)>, NotPorted> =
        items.map(|(key, item)| Ok((key, embed(item)?))).collect();
    Ok(Value::Array(embedded?))
}

/// `embed_links()`: each embeddable link of the relations wanted served,
/// once for each URL, under `_embedded` where any is not empty.
fn embed_links(
    context: &Context,
    data: Value,
    rels: Option<&[(Key, Value)]>,
    cache: &mut Vec<(Vec<u8>, Value)>,
) -> Result<Value, NotPorted> {
    let links = data.get_str("_links").cloned().unwrap_or_default();
    let mut embedded = Vec::new();
    for (rel, links) in links.as_array().unwrap_or_default() {
        let wanted = rels.is_none_or(|rels| {
            let rel = Value::String(rel.to_php_string());
            rels.iter().any(|(_, r)| *r == rel)
        });
        if !wanted {
            continue;
        }
        let mut embeds = Vec::new();
        for (_, link) in links.as_array().unwrap_or_default() {
            let href = link.get_str("href").map(Value::to_php_string);
            let embeddable = link.get_str("embeddable").is_some_and(|e| !e.is_empty());
            let found = match href.filter(|_| embeddable) {
                None => Value::Array(Vec::new()),
                Some(href) => match cache.iter().find(|(known, _)| *known == href) {
                    Some((_, data)) => data.clone(),
                    None => {
                        let data = serve(context, &href)?;
                        cache.push((href, data.clone()));
                        data
                    }
                },
            };
            embeds.push(found);
        }
        if embeds.iter().any(|embed| !embed.is_empty()) {
            embedded.push((rel.clone(), crate::response::list(embeds)));
        }
    }
    match data {
        Value::Array(mut entries) if !embedded.is_empty() => {
            entries.push((Key::String(b"_embedded".to_vec()), Value::Array(embedded)));
            Ok(Value::Array(entries))
        }
        data => Ok(data),
    }
}

/// The data of `href` served as `WP_REST_Request::from_url()` reads it:
/// in the `embed` context unless it asks for one, and a collection's
/// largest page; nothing for a URL that is not the API's.
pub(crate) fn serve(context: &Context, href: &[u8]) -> Result<Value, NotPorted> {
    let mut parts = href.splitn(2, |&b| b == b'?');
    let url = parts.next().unwrap_or_default();
    let query = parts.next().unwrap_or_default();
    let mut get = parse_str(query).as_array().unwrap_or_default().to_vec();
    let root = context.rest_url("/");
    let pretty = context
        .option("permalink_structure")
        .is_some_and(|structure| !Value::String(structure).is_empty());
    let route = if pretty && href.starts_with(root.as_bytes()) {
        let base = root.trim_end_matches(['/', '\\']).len();
        Some(Value::String(url.get(base..).unwrap_or_default().to_vec()))
    } else {
        let at = get
            .iter()
            .position(|(key, _)| key.to_php_string() == b"rest_route");
        at.map(|at| get.remove(at).1)
    };
    let route = route.filter(|route| !route.is_empty());
    let Some(route) = route.as_ref().and_then(Value::as_bytes) else {
        return Ok(Value::Array(Vec::new()));
    };
    if hyscribe_phpser::lookup(&get, "context").is_none_or(Value::is_empty) {
        set(&mut get, "context", Value::String(b"embed".to_vec()));
    }
    if hyscribe_phpser::lookup(&get, "per_page").is_none_or(Value::is_empty) {
        let maximum = routes::match_request("GET", route)
            .and_then(|matched| ported(&matched))
            .and_then(|(_, endpoint)| {
                let arg = endpoint.args.iter().find(|arg| arg.name == "per_page")?;
                arg.schema.get_str("maximum").map(Value::to_int)
            });
        if let Some(maximum) = maximum {
            set(&mut get, "per_page", Value::Int(maximum));
        }
    }
    let params = Params {
        get,
        ..Params::default()
    };
    let response = dispatch(context, ("GET", route), (params, true), None)?;
    Ok(response.into_data())
}

/// `$request[ $key ] = $value`: in place if set, else last.
fn set(get: &mut Vec<(Key, Value)>, key: &str, value: Value) {
    match get
        .iter_mut()
        .find(|(name, _)| name.to_php_string() == key.as_bytes())
    {
        Some((_, known)) => *known = value,
        None => get.push((Key::String(key.as_bytes().to_vec()), value)),
    }
}
