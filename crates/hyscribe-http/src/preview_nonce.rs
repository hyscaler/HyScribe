// SPDX-License-Identifier: AGPL-3.0-or-later

//! `_show_post_preview()`, which WordPress runs on `init`: a request that
//! carries `preview_id` and `preview_nonce` without a valid nonce for the
//! current user dies with a 403, before any query runs. Every entry point
//! that loads WordPress runs it; `wp_die()` answers in the form its handler
//! picks.

use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use hyscribe_admin::die::page;
use hyscribe_phpser::{Value, isset, parse_str};
use tokio::runtime::Handle;

use crate::login::{LoginReads, Names, cookie, cookies, is_ssl, joined, nocache, response};
use crate::{Site, now};

const MESSAGE: &str = "Sorry, you are not allowed to preview drafts.";

/// The kind of entry point a request reaches: what `wp_die()` and the
/// cookies `init` sees depend on it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum Entry {
    /// Every other entry point.
    Page,
    /// `admin-ajax.php`, which defines `DOING_AJAX`.
    Ajax,
    /// `xmlrpc.php`, which defines `XMLRPC_REQUEST` and discards the cookies.
    XmlRpc,
}

/// The refusal of a request whose preview nonce does not verify, else none.
pub(crate) async fn refuse(
    site: &Arc<Site>,
    headers: &HeaderMap,
    query: Option<&str>,
    post: bool,
    entry: Entry,
) -> Option<Response> {
    let get = parse_str(query.unwrap_or_default().as_bytes());
    let (id, nonce) = (get.get_str("preview_id"), get.get_str("preview_nonce"));
    if !isset(id) || !isset(nonce) {
        return None;
    }
    let action = format!("post_preview_{}", id.map_or(0, Value::to_int));
    let nonce = nonce.map(Value::to_php_string).unwrap_or_default();
    let values = headers.get_all(header::COOKIE).iter();
    let jar = match entry {
        Entry::XmlRpc => Vec::new(),
        _ => cookies(values.map(HeaderValue::as_bytes)),
    };
    let grace = grace(post, entry);
    let handler = Handler::pick(headers, entry);
    let site = Arc::clone(site);
    let (now, handle) = (now(), Handle::current());
    let checked = tokio::task::spawn_blocking(move || {
        let proof = (action.as_str(), nonce.as_slice());
        (!verifies(&site, (handle, now), &jar, proof, grace)).then(|| denied(&site, handler))
    })
    .await;
    settled(checked)
}

/// `wp_validate_auth_cookie()` gives a POST or Ajax request an hour's grace
/// past an expired cookie.
fn grace(post: bool, entry: Entry) -> bool {
    post || entry == Entry::Ajax
}

/// Whether `nonce` verifies for the user `init` finds: the auth cookie's,
/// else the logged in cookie's, else a visitor.
fn verifies(
    site: &Site,
    (handle, now): (Handle, i64),
    jar: &[(Vec<u8>, Vec<u8>)],
    (action, nonce): (&str, &[u8]),
    grace: bool,
) -> bool {
    let mut reads = LoginReads::new(site, handle, now);
    let names = Names::new(site);
    let https = is_ssl(site);
    let user = reads
        .auth_user(cookie(jar, &names.auth(https)), grace, https)
        .or_else(|| reads.logged_in_user(cookie(jar, &names.logged_in()), grace));
    if let Some((user, token)) = user {
        reads.act_as(user, token);
    }
    reads.verify_nonce(nonce, action)
}

/// The check's answer, or the 500 of a check that panicked.
fn settled(checked: Result<Option<Response>, tokio::task::JoinError>) -> Option<Response> {
    checked.unwrap_or_else(|failed| Some(joined(Err(failed))))
}

/// The `wp_die()` handler a request gets.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Handler {
    Ajax,
    Json,
    XmlRpc,
    Xml,
    Default,
}

impl Handler {
    /// `wp_die()`'s choice: Ajax, JSON, XML-RPC, XML, else the page. (JSONP
    /// needs a REST request, which `init` precedes.)
    fn pick(headers: &HeaderMap, entry: Entry) -> Handler {
        let (accept, kind) = (
            joined_values(headers, header::ACCEPT),
            content_type(headers),
        );
        if entry == Entry::Ajax {
            Handler::Ajax
        } else if is_json_media_type(&accept) || is_json_media_type(&kind) {
            Handler::Json
        } else if entry == Entry::XmlRpc {
            Handler::XmlRpc
        } else if is_xml_request(&accept, &kind) {
            Handler::Xml
        } else {
            Handler::Default
        }
    }
}

/// `$_SERVER['HTTP_ACCEPT']`: the header's values, comma joined.
fn joined_values(headers: &HeaderMap, name: header::HeaderName) -> Vec<u8> {
    let values = headers.get_all(name).iter().map(HeaderValue::as_bytes);
    values.collect::<Vec<_>>().join(&b", "[..])
}

/// `$_SERVER['CONTENT_TYPE']`.
fn content_type(headers: &HeaderMap) -> Vec<u8> {
    let kind = headers.get(header::CONTENT_TYPE);
    kind.map_or(Vec::new(), |kind| kind.as_bytes().to_vec())
}

/// `wp_is_json_media_type()`: `/(^|\s|,)application\/([\w!#\$&-\^\.\+]+\+)?json(\+oembed)?($|\s|;|,)/i`.
fn is_json_media_type(media: &[u8]) -> bool {
    let media = media.to_ascii_lowercase();
    (0..media.len()).any(|start| {
        let (head, tail) = media.split_at(start);
        let boundary = head.last().is_none_or(|b| *b == b',' || space(*b));
        boundary && tail.strip_prefix(b"application/").is_some_and(json_tail)
    })
}

/// PCRE's `\s`.
fn space(byte: u8) -> bool {
    byte.is_ascii_whitespace() || byte == 0x0b
}

/// What follows `application/`: an optional `<token>+` prefix, `json`, an
/// optional `+oembed`, and the end, a space, `;` or `,`.
fn json_tail(rest: &[u8]) -> bool {
    let token =
        |b: &u8| b.is_ascii_alphanumeric() || b"_!#$.+".contains(b) || (0x26..=0x5e).contains(b);
    let plain = |s: &[u8]| {
        let s = s.strip_prefix(b"json")?;
        let s = s.strip_prefix(b"+oembed").unwrap_or(s);
        let end = s.first().is_none_or(|b| b";,".contains(b) || space(*b));
        end.then_some(())
    };
    let prefixed = rest
        .iter()
        .enumerate()
        .take_while(|(_, b)| token(b))
        .any(|(i, b)| i > 0 && *b == b'+' && plain(rest.split_at(i + 1).1).is_some());
    plain(rest).is_some() || prefixed
}

/// `wp_is_xml_request()`.
fn is_xml_request(accept: &[u8], kind: &[u8]) -> bool {
    const TYPES: [&[u8]; 6] = [
        b"text/xml",
        b"application/rss+xml",
        b"application/atom+xml",
        b"application/rdf+xml",
        b"text/xml+oembed",
        b"application/xml+oembed",
    ];
    TYPES
        .iter()
        .any(|t| accept.windows(t.len()).any(|w| w == *t))
        || TYPES.contains(&kind)
}

/// `wp_die( 'Sorry, you are not allowed to preview drafts.', 403 )` as the
/// request's handler answers it.
fn denied(site: &Site, handler: Handler) -> Response {
    let json = format!(
        r#"{{"code":"wp_die","message":"{MESSAGE}","data":{{"status":403}},"additional_errors":[]}}"#
    );
    let xml = format!(
        "<error>\n    <code>wp_die</code>\n    <title><![CDATA[WordPress &amp;rsaquo; Error]]></title>\n    <message><![CDATA[{MESSAGE}]]></message>\n    <data>\n        <status>403</status>\n    </data>\n</error>\n"
    );
    let (status, kind, body) = match handler {
        Handler::Ajax => (403, "text/html", MESSAGE.as_bytes().to_vec()),
        Handler::Json => (403, "application/json", json.into_bytes()),
        // There is no XML-RPC server yet on `init`: nothing to answer with.
        Handler::XmlRpc => (200, "text/html", Vec::new()),
        Handler::Xml => (403, "text/xml", xml.into_bytes()),
        Handler::Default => {
            let title = "WordPress &rsaquo; Error";
            (403, "text/html", page(site.public(), title, MESSAGE))
        }
    };
    let mut headers = nocache();
    headers.push((header::CONTENT_TYPE, format!("{kind}; charset=UTF-8")));
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN);
    response(status, headers, body)
}

#[cfg(test)]
mod tests;
