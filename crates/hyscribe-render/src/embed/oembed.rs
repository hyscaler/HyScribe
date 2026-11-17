// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_oEmbed`: the provider of a URL, from WordPress's list (exported from
//! the oracle) or discovered in the page's `<link>` tags; its answer in
//! JSON or XML turned into HTML; and core's `oembed_dataparse` filters,
//! which give an iframe its title and sandbox untrusted providers' HTML.

use std::sync::LazyLock;

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_phpser::{
    Pattern, Value, add_query_arg, is_empty_string, json_decode, php_trim, urlencode,
};

use crate::kses::{AllowedHtml, wp_kses, wp_kses_hair};
use crate::outbound::{Get, Response, Size};
use crate::site::Site;
use crate::template::replace;
use crate::xml::{Event, Parser};

/// A provider: its URL pattern and endpoint.
struct Provider {
    pattern: Pattern,
    endpoint: String,
}

/// `WP_oEmbed::$providers`, in their order.
static PROVIDERS: LazyLock<Vec<Provider>> = LazyLock::new(|| {
    let json = include_bytes!("../../../../wordpress/wp-includes/oembed-providers.json");
    let entries = json_decode(json).unwrap_or_default();
    let provider = |entry: &Value| {
        let regex = entry.get_str("0")?.as_bytes()?;
        let regex = std::str::from_utf8(regex).ok()?;
        // `#...#` or `#...#i`.
        let (body, flags) = regex.strip_prefix('#')?.rsplit_once('#')?;
        let pattern = match flags {
            "i" => Pattern::caseless(body),
            _ => Pattern::new(body),
        }?;
        let endpoint = entry.get_str("1")?.as_bytes()?;
        Some(Provider {
            pattern,
            endpoint: String::from_utf8_lossy(endpoint).into_owned(),
        })
    };
    entries
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter_map(|(_, entry)| provider(entry))
        .collect()
});

/// The tags and attributes `wp_filter_oembed_result()` keeps, before and
/// after the secret is added.
static SANDBOXED: LazyLock<[AllowedHtml; 2]> = LazyLock::new(|| {
    let frame = r#""src":true,"width":true,"height":true,"frameborder":true,"marginwidth":true,"marginheight":true,"scrolling":true,"title":true"#;
    let list = |secret: &str| {
        let json = format!(
            r#"{{"a":{{"href":true}},"blockquote":{{{}}},"iframe":{{{frame}{}}}}}"#,
            secret.trim_start_matches(','),
            secret
        );
        AllowedHtml::from_json(json.as_bytes())
    };
    [list(""), list(r#","data-secret":true"#)]
});

/// What `wp_oembed_get()` gave.
pub(super) enum Got {
    /// The embed's HTML.
    Html(Vec<u8>),
    /// `false`: no provider, or its answer was not an embed.
    Failed,
    /// Fetches are off, so nothing was asked.
    Off,
}

/// The decoded answer of a provider: an object's properties.
type Data = Vec<(Vec<u8>, Value)>;

/// A property of `data`, as `$data->$name`.
fn property<'a>(data: &'a Data, name: &[u8]) -> Option<&'a Value> {
    data.iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value)
}

/// `! empty( $data->$name ) && is_string( $data->$name )`.
fn text<'a>(data: &'a Data, name: &[u8]) -> Option<&'a [u8]> {
    match property(data, name)? {
        Value::String(text) if !is_empty_string(text) => Some(text),
        _ => None,
    }
}

/// `wp_oembed_get( $url, $args )` with `$args` the embed's size and
/// `discover` on.
pub(super) fn wp_oembed_get(site: &dyn Site, url: &[u8], args: Size) -> Got {
    let provider = match get_provider(site, url) {
        Some(provider) => provider,
        None => match discover(site, url) {
            Some(Some(provider)) => provider,
            Some(None) => return Got::Failed,
            None => return Got::Off,
        },
    };
    let data = match fetch(site, &provider, url, args) {
        Some(Some(data)) => data,
        Some(None) => return Got::Failed,
        None => return Got::Off,
    };
    let html = data2html(site, &data, url);
    match site.hooks().oembed_result(html, url, args) {
        Some(html) if !is_empty_string(&html) => Got::Html(html),
        _ => Got::Failed,
    }
}

/// `get_provider( $url, array( 'discover' => false ) )`: the endpoint of
/// the first provider whose pattern `url` matches, in JSON.
fn get_provider(site: &dyn Site, url: &[u8]) -> Option<Vec<u8>> {
    let provider = PROVIDERS.iter().find(|provider| {
        (0..url.len()).any(|at| {
            let rest = url.get(at..).unwrap_or_default();
            provider.pattern.captures(rest).is_some()
        })
    })?;
    let home = String::from_utf8_lossy(&urlencode(&site.home_url())).into_owned();
    let endpoint = provider.endpoint.replace("%home%", &home);
    Some(endpoint.replace("{format}", "json").into_bytes())
}

/// `wp_safe_remote_get()` with WordPress's defaults and `limit`: `None`
/// while fetches are off, the body (empty after an error) otherwise.
fn remote_get(site: &dyn Site, url: &[u8], limit: Option<usize>) -> Option<Response> {
    let get = Get {
        limit,
        ..Get::new(site)
    };
    Some(site.remote_get(url, &get)?.unwrap_or_default())
}

/// `WP_oEmbed::discover()`: the page's oEmbed endpoint, JSON first.
fn discover(site: &dyn Site, url: &[u8]) -> Option<Option<Vec<u8>>> {
    let html = remote_get(site, url, Some(153_600))?.body;
    let lower = html.to_ascii_lowercase();
    // `stripos()` found at 0 counts as not found.
    let head = match find(&lower, b"</head>") {
        Some(end) if end > 0 => html.get(..end).unwrap_or_default(),
        _ => &html,
    };
    let lower = head.to_ascii_lowercase();
    let types: [(&[u8], &str); 3] = [
        (b"application/json+oembed", "json"),
        (b"text/xml+oembed", "xml"),
        (b"application/xml+oembed", "xml"),
    ];
    let tagged = types
        .iter()
        .any(|(kind, _)| find(&lower, kind).is_some_and(|at| at > 0));
    let mut json = None;
    let mut xml = None;
    for link in links(head).into_iter().filter(|_| tagged) {
        let atts = attributes(link);
        let get = |name: &[u8]| atts.iter().find(|(key, _)| key == name).map(|(_, v)| v);
        let format = get(b"type").and_then(|kind| types.iter().find(|(t, _)| t == kind));
        let href = get(b"href").filter(|href| !is_empty_string(href));
        if let (Some((_, format)), Some(href)) = (format, href) {
            let href = Some(htmlspecialchars_decode(href));
            if *format == "json" {
                json = href;
                break;
            }
            xml = href;
        }
    }
    Some(json.or(xml))
}

/// The first position of `needle` in `text`.
fn find(text: &[u8], needle: &[u8]) -> Option<usize> {
    text.windows(needle.len()).position(|w| w == needle)
}

/// `preg_match_all( '#<link([^<>]+)/?>#iU', ... )`: each tag's attributes.
fn links(html: &[u8]) -> Vec<&[u8]> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(start) = html.get(at..).and_then(|rest| {
        rest.windows(5)
            .position(|w| w.eq_ignore_ascii_case(b"<link"))
    }) {
        let start = at + start + 5;
        let rest = html.get(start..).unwrap_or_default();
        let end = rest.iter().position(|&b| b == b'<' || b == b'>');
        at = start;
        let Some(end) = end.filter(|&end| rest.get(end) == Some(&b'>')) else {
            continue;
        };
        // The lazy run stops before a `/` that the closing `>` follows.
        let inner = rest.get(..end).unwrap_or_default();
        let inner = inner
            .strip_suffix(b"/")
            .filter(|i| !i.is_empty())
            .unwrap_or(inner);
        if !inner.is_empty() {
            found.push(inner);
            at = start + end + 1;
        }
    }
    found
}

/// `shortcode_parse_atts()`'s named attributes, names in lower case.
fn attributes(text: &[u8]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let text = replace(
        &replace(text, "\u{a0}".as_bytes(), b" "),
        "\u{200b}".as_bytes(),
        b" ",
    );
    let word = |b: &u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-');
    let space = |b: &u8| super::is_space(*b);
    let mut found = Vec::new();
    let mut at = 0;
    while at < text.len() {
        let rest = text.get(at..).unwrap_or_default();
        let name = rest.iter().take_while(|b| word(b)).count();
        let after = rest.get(name..).unwrap_or_default();
        let gap = after.iter().take_while(|b| space(b)).count();
        let value = (name > 0 && after.get(gap) == Some(&b'='))
            .then(|| {
                let start = gap + 1;
                let start = start + after.get(start..)?.iter().take_while(|b| space(b)).count();
                let quoted = after.get(start).filter(|q| matches!(q, b'"' | b'\''));
                let (value, end) = match quoted {
                    Some(&quote) => {
                        let close = after.get(start + 1..)?.iter().position(|&b| b == quote)?;
                        (after.get(start + 1..start + 1 + close)?, start + close + 2)
                    }
                    None => {
                        let length = after
                            .get(start..)?
                            .iter()
                            .take_while(|b| !space(b) && !matches!(b, b'"' | b'\''))
                            .count();
                        (after.get(start..start + length)?, start + length)
                    }
                };
                // Each alternative ends at white space or the end.
                (after.get(end).is_none_or(space) && (quoted.is_some() || end > start))
                    .then_some((value, end))
            })
            .flatten();
        match value {
            Some((value, end)) => {
                let key = rest.get(..name).unwrap_or_default().to_ascii_lowercase();
                found.push((key, value.to_vec()));
                at += name + end + 1;
            }
            None => {
                // A positional value: up to the next white space.
                let skip = rest.iter().take_while(|b| !space(b)).count();
                at += skip.max(1);
            }
        }
    }
    found
}

/// `htmlspecialchars_decode()` with PHP's default `ENT_QUOTES`.
fn htmlspecialchars_decode(text: &[u8]) -> Vec<u8> {
    let entities: [(&[u8], &[u8]); 6] = [
        (b"&amp;", b"&"),
        (b"&quot;", b"\""),
        (b"&#039;", b"'"),
        (b"&#39;", b"'"),
        (b"&lt;", b"<"),
        (b"&gt;", b">"),
    ];
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        let rest = text.get(at..).unwrap_or_default();
        match entities.iter().find(|(entity, _)| rest.starts_with(entity)) {
            Some((entity, plain)) => {
                out.extend_from_slice(plain);
                at += entity.len();
            }
            None => {
                out.extend(rest.first());
                at += 1;
            }
        }
    }
    out
}

/// `WP_oEmbed::fetch()`: the provider's answer, JSON first and XML when it
/// answers 501 to JSON.
fn fetch(site: &dyn Site, provider: &[u8], url: &[u8], args: Size) -> Option<Option<Data>> {
    let width = args.width.to_string();
    let height = args.height.to_string();
    let encoded = urlencode(url);
    let mut provider = provider.to_vec();
    for (name, value) in [
        (&b"maxwidth"[..], width.as_bytes()),
        (b"maxheight", height.as_bytes()),
        (b"url", &encoded),
        (b"dnt", b"1"),
    ] {
        provider = add_query_arg(&[(name, Some(value))], &provider);
    }
    let provider = site.hooks().oembed_fetch_url(provider, url, args);
    for format in ["json", "xml"] {
        let with = add_query_arg(&[(b"format", Some(format.as_bytes()))], &provider);
        let response = remote_get(site, &with, None)?;
        if response.status == 501 {
            continue;
        }
        if response.body.is_empty() {
            return Some(None);
        }
        return Some(match format {
            "json" => parse_json(&response.body),
            _ => parse_xml(&response.body),
        });
    }
    Some(None)
}

/// `_parse_json()`: an object's properties.
fn parse_json(body: &[u8]) -> Option<Data> {
    let body = php_trim(body);
    if body.first() != Some(&b'{') {
        return None;
    }
    let decoded = json_decode(body)?;
    let entries = decoded.as_array()?;
    let data: Data = entries
        .iter()
        .map(|(key, value)| (key.to_php_string(), value.clone()))
        .collect();
    // An empty object casts to false.
    (!data.is_empty()).then_some(data)
}

/// `_parse_xml_body()`: the root's child elements, each with its own text,
/// for a document without a doctype.
fn parse_xml(body: &[u8]) -> Option<Data> {
    let mut parser = Parser::new(body, false);
    let mut data: Data = Vec::new();
    let mut depth = 0;
    let mut current = 0;
    loop {
        match parser.next().ok()? {
            Event::Doctype => return None,
            Event::Start { name, .. } => {
                depth += 1;
                if depth == 2 {
                    // A repeated name keeps its place and takes the last text.
                    current = match data.iter().position(|(key, _)| *key == name.local) {
                        Some(at) => at,
                        None => {
                            data.push((name.local, Value::Null));
                            data.len() - 1
                        }
                    };
                    if let Some((_, value)) = data.get_mut(current) {
                        *value = Value::String(Vec::new());
                    }
                }
            }
            Event::Text(chunk) if depth == 2 => {
                if let Some((_, Value::String(text))) = data.get_mut(current) {
                    text.extend(chunk);
                }
            }
            Event::Text(_) => {}
            Event::End => depth -= 1,
            Event::Eof => return Some(data),
        }
    }
}

/// `WP_oEmbed::data2html()`, with core's `oembed_dataparse` filters.
fn data2html(site: &dyn Site, data: &Data, url: &[u8]) -> Option<Vec<u8>> {
    let kind = text(data, b"type")?.to_vec();
    let html = match kind.as_slice() {
        b"photo" => {
            let numeric = |name: &[u8]| {
                property(data, name).filter(|value| !value.is_empty() && value.is_numeric())
            };
            let (src, width, height) =
                (text(data, b"url")?, numeric(b"width")?, numeric(b"height")?);
            let title = text(data, b"title").unwrap_or_default();
            format!(
                "<a href=\"{}\"><img src=\"{}\" alt=\"{}\" width=\"{}\" height=\"{}\" /></a>",
                String::from_utf8_lossy(&esc_url(url)),
                String::from_utf8_lossy(&esc_url(src)),
                esc_attr(title),
                esc_attr(&width.to_php_string()),
                esc_attr(&height.to_php_string()),
            )
            .into_bytes()
        }
        b"video" | b"rich" => text(data, b"html")?.to_vec(),
        b"link" => format!(
            "<a href=\"{}\">{}</a>",
            String::from_utf8_lossy(&esc_url(url)),
            esc_html(text(data, b"title")?)
        )
        .into_bytes(),
        _ => return None,
    };
    if !matches!(kind.as_slice(), b"rich" | b"video") {
        return Some(html);
    }
    let titled = iframe_title(html, data);
    filter_oembed_result(site, titled, url)
}

/// `wp_filter_oembed_iframe_title_attribute()`: the first iframe's title,
/// else the provider's, on every iframe.
fn iframe_title(html: Vec<u8>, data: &Data) -> Vec<u8> {
    let lower = html.to_ascii_lowercase();
    let tag = find(&lower, b"<iframe").and_then(|start| {
        let end = start + html.get(start..)?.iter().position(|&b| b == b'>')?;
        Some((start, end))
    });
    let attrs = tag.map(|(start, end)| {
        let inner = html.get(start + 7..end).unwrap_or_default();
        (
            html.get(start..=end).unwrap_or_default().to_vec(),
            wp_kses_hair(inner),
        )
    });
    let own = attrs.as_ref().and_then(|(_, attrs)| {
        attrs
            .iter()
            .find(|attr| attr.name == b"title" && !is_empty_string(&attr.value))
    });
    let title = match own {
        Some(attr) => attr.value.clone(),
        None => property(data, b"title")
            .filter(|title| !title.is_empty())
            .map(Value::to_php_string)
            .unwrap_or_default(),
    };
    if title.is_empty() {
        return html;
    }
    let mut html = html;
    if let Some((whole, attrs)) = attrs.filter(|(_, a)| a.iter().any(|a| a.name == b"title")) {
        let kept: Vec<&[u8]> = attrs
            .iter()
            .filter(|attr| attr.name != b"title")
            .map(|attr| attr.whole.as_slice())
            .collect();
        let rebuilt = [&b"<iframe "[..], php_trim(&kept.join(&b' ')), b">"].concat();
        html = replace(&html, &whole, &rebuilt);
    }
    let titled = format!("<iframe title=\"{}\" ", esc_attr(&title));
    ireplace(&html, b"<iframe ", titled.as_bytes())
}

/// `str_ireplace()` of an ASCII `needle`.
fn ireplace(text: &[u8], needle: &[u8], with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while at < text.len() {
        let rest = text.get(at..).unwrap_or_default();
        if rest
            .get(..needle.len())
            .is_some_and(|w| w.eq_ignore_ascii_case(needle))
        {
            out.extend_from_slice(with);
            at += needle.len();
        } else {
            out.extend(rest.first());
            at += 1;
        }
    }
    out
}

/// `wp_filter_oembed_result()`: an untrusted provider's HTML cut to a
/// blockquote and an iframe, sandboxed and tied to a secret.
fn filter_oembed_result(site: &dyn Site, html: Vec<u8>, url: &[u8]) -> Option<Vec<u8>> {
    if get_provider(site, url).is_some() {
        return Some(html);
    }
    let html = wp_kses(&html, &SANDBOXED[0], b"");
    // `|(<blockquote>.*?</blockquote>)?.*(<iframe.*?></iframe>)|ms`
    // The greedy `.*` takes the last `<iframe` a `></iframe>` follows.
    let last_close = html.windows(10).rposition(|w| w == b"></iframe>")?;
    let start = html
        .windows(7)
        .take(last_close.checked_sub(6)?)
        .rposition(|w| w == b"<iframe")?;
    let close = start + 7 + find(html.get(start + 7..)?, b"></iframe>")?;
    let frame = (start, close + 10);
    let quote = html
        .starts_with(b"<blockquote>")
        .then(|| find(&html, b"</blockquote>").map(|end| end + 13))
        .flatten()
        .filter(|&end| end <= frame.0);
    let quote = quote.map(|end| html.get(..end).unwrap_or_default().to_vec());
    let mut html = [
        quote.as_deref().unwrap_or_default(),
        html.get(frame.0..frame.1).unwrap_or_default(),
    ]
    .concat();
    if let Some((whole, q, src)) = source(&html) {
        let secret = secret();
        let url = esc_url(&[&src[..], b"#?secret=", secret.as_bytes()].concat());
        let q = char::from(q);
        let with = format!(
            " src={q}{}{q} data-secret={q}{secret}{q}",
            String::from_utf8_lossy(&url)
        );
        html = replace(&html, &whole, with.as_bytes());
        let quote = format!("<blockquote data-secret=\"{secret}\"");
        html = replace(&html, b"<blockquote", quote.as_bytes());
    }
    let mut html = wp_kses(&html, &SANDBOXED[1], b"");
    if quote.is_some_and(|quote| !quote.is_empty()) {
        html = replace(
            &html,
            b"<iframe",
            b"<iframe style=\"position: absolute; visibility: hidden;\"",
        );
        html = replace(
            &html,
            b"<blockquote",
            b"<blockquote class=\"wp-embedded-content\"",
        );
    }
    Some(ireplace(
        &html,
        b"<iframe",
        b"<iframe class=\"wp-embedded-content\" sandbox=\"allow-scripts\" security=\"restricted\"",
    ))
}

/// `/ src=([\'"])(.*?)\1/`: the whole match, the quote and the URL.
fn source(html: &[u8]) -> Option<(Vec<u8>, u8, Vec<u8>)> {
    (0..html.len()).find_map(|at| {
        let rest = html.get(at..)?;
        let quote = *rest
            .strip_prefix(b" src=")?
            .first()
            .filter(|q| matches!(q, b'"' | b'\''))?;
        let value = rest.get(6..)?;
        let end = value.iter().position(|&b| b == quote || b == b'\n')?;
        (value.get(end) == Some(&quote)).then(|| {
            (
                rest.get(..end + 7).unwrap_or_default().to_vec(),
                quote,
                value.get(..end).unwrap_or_default().to_vec(),
            )
        })
    })
}

/// `wp_generate_password( 10, false )`: ten letters and digits, drawn from
/// the standard library's randomly keyed hasher, as they only tie an
/// iframe to its page.
fn secret() -> String {
    use std::hash::{BuildHasher, RandomState};
    const CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut draw = RandomState::new().hash_one(0_u8);
    (0..10)
        .map(|_| {
            let at = usize::try_from(draw % 62).unwrap_or_default();
            draw /= 62;
            char::from(*CHARS.get(at).unwrap_or(&b'a'))
        })
        .collect()
}

#[cfg(test)]
mod tests;
