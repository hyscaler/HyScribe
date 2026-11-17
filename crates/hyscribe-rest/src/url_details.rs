// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_URL_Details_Controller`: `/wp-block-editor/v1/url-details`,
//! the title, icon, description and image of a page for the editor's link
//! previews, fetched through the outbound client (ADR 0019) and cached in
//! a site transient for an hour, as PHP caches it. The route answers 501
//! while outbound fetches are off. A URL the client refuses on its first
//! hop is answered as `wp_http_validate_url()` refuses one, for the users
//! the permission check lets through; a visitor learns nothing of a host's
//! addresses, which PHP's check tells anyone.

use hyscribe_fetch::{Args, Error, Kind, validate_url};
use hyscribe_kses::esc_url_raw;
use hyscribe_phpser::{Value, is_empty_string, md5};

use crate::request::{Arg, Params, Sanitize, invalid_param};
use crate::response::{Response, RestError, object, text};
use crate::types::can_edit_any;
use crate::{Context, Endpoint, unserialized};

mod html;

/// The route's regex.
pub(crate) const ROUTE: &str = "/wp-block-editor/v1/url-details";

/// The body read, `limit_response_size`.
const LIMIT: usize = 150 * 1024;

/// How long a fetched page is cached, `HOUR_IN_SECONDS`, which the
/// `rest_url_details_cache_expiration` filter would change.
const EXPIRATION: i64 = 3600;

/// The version `get_bloginfo( 'version' )` reports.
const VERSION: &str = include_str!("../../../wordpress/VERSION");

pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    let url = Arg {
        name: "url",
        schema: hyscribe_phpser::json_decode(
            br#"{"required":true,"type":"string","format":"uri"}"#,
        )
        .unwrap_or_default(),
        validate: false,
        sanitize: Sanitize::Callback(valid_url),
    };
    (regex == ROUTE).then(|| {
        vec![Endpoint {
            args: vec![url],
            permission,
            callback: Some(parse_url_details),
        }]
    })
}

/// `wp_http_validate_url()` as the argument's `validate_callback`, then
/// `sanitize_url()`.
fn valid_url(context: &Context, value: &Value) -> Option<Value> {
    let Value::String(url) = value else {
        return None;
    };
    validate_url(url, &context.site.reads().home_url())?;
    Some(Value::String(esc_url_raw(url)))
}

/// `permissions_check()`: a user who may edit posts of any type the REST
/// API shows.
fn permission(context: &Context, _: &Params) -> Result<(), RestError> {
    if can_edit_any(context) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to process remote URLs.";
    Err(context.forbidden("rest_cannot_view_url_details", message))
}

/// `parse_url_details()`.
fn parse_url_details(context: &Context, params: &Params) -> Result<Response, RestError> {
    let given = params.text("url");
    let end = given.iter().rposition(|b| !b"/\\".contains(b));
    let url = given
        .get(..end.map_or(0, |end| end + 1))
        .unwrap_or_default();
    let key = format!("g_url_details_response_{}", md5(url));
    let body = match cached(context, &key) {
        Some(body) => body,
        None => {
            let body = remote(context, url)?;
            context.site.set_site_transient(&key, &body, EXPIRATION);
            body
        }
    };
    let [title, icon, description, image] = details(&body, url);
    let data = object([
        ("title", text(title)),
        ("icon", text(icon)),
        ("description", text(description)),
        ("image", text(image)),
    ]);
    Ok(Response::new(data, 200))
}

/// The title, icon, description and image `parse_url_details()` reads
/// from `body`, fetched from `url`.
pub fn details(body: &[u8], url: &[u8]) -> [Vec<u8>; 4] {
    let head = html::document_head(body);
    let metas = html::metas(&head);
    [
        html::title(&head),
        html::icon(&head, url),
        html::description(&metas),
        html::image(&metas, url),
    ]
}

/// `get_site_transient( $key )` when not `empty()`: a transient past its
/// timeout is deleted, as PHP deletes it.
pub(crate) fn cached(context: &Context, key: &str) -> Option<Vec<u8>> {
    let timeout = context.option(&format!("_site_transient_timeout_{key}"));
    if timeout.is_some_and(|timeout| Value::String(timeout).to_int() < context.now) {
        context.site.expire_site_transient(key);
        return None;
    }
    let cached = context.option(&format!("_site_transient_{key}"))?;
    let cached = unserialized(&cached);
    (!cached.is_empty()).then(|| cached.to_php_string())
}

/// `get_remote_url()`: the body of a 200 answer, which is not `empty()`.
fn remote(context: &Context, url: &[u8]) -> Result<Vec<u8>, RestError> {
    let home = context.site.reads().home_url();
    let agent = format!(
        "WP-URLDetails/{} (+{})",
        VERSION.trim(),
        String::from_utf8_lossy(&home)
    );
    let args = Args {
        limit_response_size: Some(LIMIT),
        ..Args::new(agent)
    };
    let not_found = || {
        let message = "URL not found. Response returned a non-200 status code for this URL.";
        RestError::status("no_response", message, 404)
    };
    let fetched = match context.site.fetch(url, args) {
        Some(Ok(fetched)) => fetched,
        // What `wp_http_validate_url()` refuses with its DNS lookup.
        Some(Err(Error {
            kind: Kind::Invalid | Kind::Refused | Kind::Resolve,
            hop: 0,
        })) => return Err(invalid_param("url")),
        _ => return Err(not_found()),
    };
    if fetched.status != 200 {
        return Err(not_found());
    }
    if is_empty_string(&fetched.body) {
        let message = "Unable to retrieve body from response at this URL.";
        return Err(RestError::status("no_content", message, 404));
    }
    Ok(fetched.body)
}
