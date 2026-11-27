// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Server::serve_batch_request_v1()`, the route `/batch/v1`: each
//! request's path parsed, matched and checked by its route's arguments
//! first, then each answered in turn, a write through the two steps of
//! ADR 0013's writes, so the site is written one request after the other.
//! Every request is answered once before anything is written, so one HyScribe
//! does not port answers the whole batch 501 then.

use hyscribe_kses::wp_parse_url;
use hyscribe_phpser::{Key, Value, parse_str};

use crate::posts::write::{Finish, Outcome, Writing};
use crate::request::{Params, typed};
use crate::response::{NOT_PORTED, Response, RestError, list, object, text};
use crate::{
    Context, Endpoint, Halt, Pending, Permission, Plan, finish, gives_parameters, hooks, ported,
    responded, routes, validated, widgets, with_route,
};

/// The route's regex.
pub(crate) const ROUTE: &str = "/batch/v1";

pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    let requests = r#"{"required":true,"type":"array","maxItems":25,"items":{"type":"object","properties":{"method":{"type":"string","enum":["POST","PUT","PATCH","DELETE"],"default":"POST"},"path":{"type":"string","required":true},"body":{"type":"object","properties":[],"additionalProperties":true},"headers":{"type":"object","properties":[],"additionalProperties":{"type":["string","array"],"items":{"type":"string"}}}}}}"#;
    let validation =
        r#"{"type":"string","enum":["require-all-validate","normal"],"default":"normal"}"#;
    // Its writes are its requests', each checked by its own route.
    (regex == ROUTE).then(|| {
        vec![Endpoint {
            args: vec![typed("validation", validation), typed("requests", requests)],
            permission: |_, _| Ok(()),
            callback: None,
        }]
    })
}

/// A request of the batch.
#[derive(Clone)]
enum Sub {
    /// `parse_path_failed` or `rest_no_route`, answered without a route.
    Failed(RestError),
    Routed(Box<Routed>),
}

/// A request matched to a ported route, checked by its arguments.
#[derive(Clone)]
pub(crate) struct Routed {
    method: String,
    path: Vec<u8>,
    endpoint: Endpoint,
    permissions: Vec<Permission>,
    methods: &'static [Vec<String>],
    /// It gives parameters a write's arguments would check.
    parameters: bool,
    /// Its parameters, the write it makes, and its arguments' checks.
    params: Params,
    writing: Option<Writing>,
    checked: Result<(), RestError>,
}

/// The batch past the request being written: the outer request's
/// parameters, the answers so far and the requests left, last first.
#[derive(Clone)]
pub(crate) struct Batch {
    outer: Params,
    responses: Vec<Value>,
    left: Vec<Sub>,
}

/// A batch waiting on one of its requests' writes.
pub(crate) type Suspended = (Routed, Batch);

/// `serve_batch_request_v1()` of the checked parameters `params`.
pub(crate) fn serve(context: &Context, params: &Params) -> Result<Response, Halt> {
    let requests = params
        .get("requests")
        .and_then(Value::as_array)
        .unwrap_or_default();
    let subs: Vec<Sub> = requests
        .iter()
        .map(|(_, request)| parsed(context, request))
        .collect::<Result<_, Halt>>()?;
    let all = params.get("validation") == Some(&text("require-all-validate"));
    let failed = |sub: &Sub| match sub {
        Sub::Failed(error) => Some(error.clone()),
        Sub::Routed(routed) => routed.checked.clone().err(),
    };
    if all && subs.iter().any(|sub| failed(sub).is_some()) {
        let responses = subs
            .iter()
            .map(|sub| failed(sub).map_or(Value::Null, |error| envelope(error.into_response())));
        let data = object([
            ("failed", text("validation")),
            ("responses", list(responses.collect())),
        ]);
        return Ok(Response::new(data, 207));
    }
    let batch = Batch {
        outer: params.clone(),
        responses: Vec::new(),
        left: subs.into_iter().rev().collect(),
    };
    // A request not ported answers the batch 501 before anything is written.
    walk(context, batch.clone(), true)?;
    walk(context, batch, false)
}

/// The request `request` gives, parsed, matched and checked; what is not
/// ported answers the batch 501.
fn parsed(context: &Context, request: &Value) -> Result<Sub, Halt> {
    let method = request
        .get_str("method")
        .map_or(b"POST".to_vec(), Value::to_php_string);
    let method = String::from_utf8_lossy(&method).into_owned();
    let path = request
        .get_str("path")
        .map(Value::to_php_string)
        .unwrap_or_default();
    let Some(url) = wp_parse_url(&path) else {
        let message = "Could not parse the path.";
        return Ok(Sub::Failed(RestError::status(
            "parse_path_failed",
            message,
            400,
        )));
    };
    let path = url.path.ok_or(Halt::NotPorted)?;
    let entries = |value: Option<Value>| match value {
        Some(Value::Array(entries)) => entries,
        _ => Vec::new(),
    };
    let query = url
        .query
        .filter(|query| !Value::String(query.clone()).is_empty());
    let get = entries(query.map(|query| parse_str(&query)));
    let post = entries(request.get_str("body").cloned());
    let Some(matched) = routes::match_request(&method, &path) else {
        // An extension's routes are not batched yet.
        if crate::plugins::matches(context, &method, &path) {
            return Err(Halt::NotPorted);
        }
        let message = "No route was found matching the URL and request method.";
        return Ok(Sub::Failed(RestError::status(
            "rest_no_route",
            message,
            404,
        )));
    };
    let (permissions, endpoint) = ported(&matched).ok_or(Halt::NotPorted)?;
    if widgets::would_write(context, matched.regex) {
        return Err(Halt::NotPorted);
    }
    let parameters = gives_parameters(&post) || gives_parameters(&get);
    let given = Params {
        post,
        get,
        ..Params::default()
    };
    let params = with_route(&matched, &endpoint, given);
    let routed = |params, writing, checked| Routed {
        method: method.clone(),
        path: path.clone(),
        endpoint: endpoint.clone(),
        permissions: permissions.clone(),
        methods: matched.methods,
        parameters,
        params,
        writing,
        checked,
    };
    if !allows_batch(matched.regex, matched.endpoint) {
        let message = "The requested route does not support batch requests.";
        let error = RestError::status("rest_batch_not_allowed", message, 400);
        return Ok(Sub::Routed(Box::new(routed(params, None, Err(error)))));
    }
    let sub = context_of(context, parameters);
    let (params, writing, checked) = validated(&sub, &matched, &endpoint, params)?;
    Ok(Sub::Routed(Box::new(routed(params, writing, checked))))
}

/// Whether the route `regex`'s endpoint `index` allows batching, as its
/// exported description says.
fn allows_batch(regex: &str, index: usize) -> bool {
    let index = index.to_string();
    let path = [regex, "endpoints", &index, "allow_batch", "v1"];
    let allowed = crate::descriptions().get_path(&path);
    allowed.is_some_and(|allowed| !allowed.is_empty())
}

/// The context a request of the batch is answered in.
fn context_of<'a>(context: &Context<'a>, parameters: bool) -> Context<'a> {
    Context {
        site: context.site,
        user: context.user,
        head: false,
        uri: context.uri,
        parameters,
        remote_addr: context.remote_addr,
        postpass: context.postpass,
        user_agent: context.user_agent,
        host: context.host,
        now: context.now,
        given: context.given,
    }
}

/// The requests `batch` has left answered in turn, the first write
/// suspending them; `dry`, every one answered on the site as it is, its
/// writes passed over, to find what is not ported.
fn walk(context: &Context, mut batch: Batch, dry: bool) -> Result<Response, Halt> {
    while let Some(sub) = batch.left.pop() {
        let response = match sub {
            Sub::Failed(error) => error.into_response(),
            Sub::Routed(routed) => match answered(context, &routed) {
                Ok(response) => response,
                Err(Halt::Write(_)) if dry => continue,
                Err(Halt::Write(pending)) => {
                    let Pending { plan, finish, .. } = *pending;
                    let outer = batch.outer.clone();
                    return Err(suspend(plan, finish, outer, (*routed, batch)));
                }
                Err(halt) => return Err(halt),
            },
        };
        batch.responses.push(envelope(response));
    }
    let data = object([("responses", list(batch.responses))]);
    Ok(Response::new(data, 207))
}

/// The batch waiting on `plan`, which `finish` answers once written.
fn suspend(plan: Plan, finish: Finish, outer: Params, suspended: Suspended) -> Halt {
    Halt::Write(Box::new(Pending {
        plan,
        params: outer,
        finish,
        batch: Some(Box::new(suspended)),
    }))
}

/// The answer of `routed`: `rest_pre_dispatch`, its permission check and
/// callback, or the write it plans; then the `Allow` header and `_fields`.
fn answered(context: &Context, routed: &Routed) -> Result<Response, Halt> {
    let sub = context_of(context, routed.parameters);
    let request_line: [&dyn std::any::Any; 2] = [&routed.method, &routed.path];
    let early: Option<Response> = hooks::apply_filters("rest_pre_dispatch", None, &request_line);
    early.map_or_else(
        || {
            let endpoint = &routed.endpoint;
            let checked = routed
                .checked
                .clone()
                .and_then(|()| (endpoint.permission)(&sub, &routed.params));
            let writing = routed.writing.clone();
            let (answered, params) =
                responded(&sub, endpoint, (routed.params.clone(), writing, checked))?;
            finished(&sub, routed, answered, &params)
        },
        Ok,
    )
}

/// `answered` with the `Allow` header and `_fields`, or 501 where it is
/// not ported.
fn finished(
    context: &Context,
    routed: &Routed,
    answered: Result<Response, RestError>,
    params: &Params,
) -> Result<Response, Halt> {
    if answered
        .as_ref()
        .is_err_and(|error| error.code == NOT_PORTED)
    {
        return Err(Halt::NotPorted);
    }
    let response = answered.unwrap_or_else(RestError::into_response);
    Ok(finish(
        context,
        response,
        &routed.permissions,
        routed.methods,
        params,
    ))
}

/// The batch once the write of its request `routed` came to `outcome`: that
/// request's answer, then the others'.
pub(crate) fn resumed(
    context: &Context,
    (routed, mut batch): Suspended,
    (plan, finish, outcome): (Plan, Finish, Outcome),
) -> Result<Response, Halt> {
    let sub = context_of(context, routed.parameters);
    let mut params = routed.params.clone();
    // A write's answer is never one not ported: it is written.
    let response =
        finish(&sub, &mut params, plan, outcome).unwrap_or_else(RestError::into_response);
    let response = crate::finish(&sub, response, &routed.permissions, routed.methods, &params);
    batch.responses.push(envelope(response));
    walk(context, batch, false)
}

/// `envelope_response( $response, false )->get_data()`.
fn envelope(response: Response) -> Value {
    let headers: Vec<(Key, Value)> = response
        .headers
        .iter()
        .map(|(name, value)| (Key::String(name.clone().into_bytes()), text(value.clone())))
        .collect();
    let status = Value::Int(i64::from(response.status));
    object([
        ("body", response.into_data()),
        ("status", status),
        ("headers", Value::Array(headers)),
    ])
}
