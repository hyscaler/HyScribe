// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Post_Statuses_Controller`: `/wp/v2/statuses` and
//! `/wp/v2/statuses/<status>`, over the statuses core registers.

use std::sync::LazyLock;

use hyscribe_phpser::{Key, Value};

use crate::fields::{Properties, fields_for_response, filter_by_context};
use crate::request::{Arg, Params, Sanitize};
use crate::response::{Response, RestError, object, text};
use crate::types::{can_edit_any, context_arg};
use crate::{Context, Endpoint};

/// The schema's properties and their contexts.
const PROPERTIES: &Properties = &[
    ("name", &["embed", "view", "edit"]),
    ("private", &["edit"]),
    ("protected", &["edit"]),
    ("public", &["view", "edit"]),
    ("queryable", &["view", "edit"]),
    ("show_in_list", &["edit"]),
    ("slug", &["embed", "view", "edit"]),
    ("date_floating", &["view", "edit"]),
];

/// A registered post status.
#[derive(Clone, Default)]
pub(crate) struct Status {
    pub(crate) name: &'static str,
    label: &'static str,
    pub(crate) public: bool,
    private: bool,
    protected: bool,
    pub(crate) internal: bool,
    queryable: bool,
    date_floating: bool,
}

fn status(name: &'static str, label: &'static str) -> Status {
    Status {
        name,
        label,
        public: false,
        private: false,
        protected: false,
        internal: true,
        queryable: false,
        date_floating: false,
    }
}

/// `get_post_stati()`, in the order `create_initial_post_types()`
/// registers them.
pub(crate) static STATUSES: LazyLock<[Status; 12]> = LazyLock::new(|| {
    [
        Status {
            public: true,
            internal: false,
            queryable: true,
            ..status("publish", "Published")
        },
        Status {
            protected: true,
            internal: false,
            ..status("future", "Scheduled")
        },
        Status {
            protected: true,
            internal: false,
            date_floating: true,
            ..status("draft", "Draft")
        },
        Status {
            protected: true,
            internal: false,
            date_floating: true,
            ..status("pending", "Pending")
        },
        Status {
            private: true,
            internal: false,
            ..status("private", "Private")
        },
        status("trash", "Trash"),
        status("auto-draft", "auto-draft"),
        status("inherit", "inherit"),
        status("request-pending", "Pending"),
        status("request-confirmed", "Confirmed"),
        status("request-failed", "Failed"),
        status("request-completed", "Completed"),
    ]
});

/// `get_post_status_object( $name )`.
pub(crate) fn find(name: &[u8]) -> Option<&'static Status> {
    STATUSES
        .iter()
        .find(|status| status.name.as_bytes() == name)
}

/// The endpoints of the route `regex`, if one of these.
pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    match regex {
        "/wp/v2/statuses" => Some(vec![Endpoint {
            args: vec![context_arg()],
            permission: items_permission,
            callback: Some(items),
        }]),
        "/wp/v2/statuses/(?P<status>[\\w-]+)" => Some(vec![Endpoint {
            args: vec![
                Arg {
                    name: "status",
                    schema: object([("type", text("string"))]),
                    validate: false,
                    sanitize: Sanitize::Schema,
                },
                context_arg(),
            ],
            permission: item_permission,
            callback: Some(item),
        }]),
        _ => None,
    }
}

/// `check_read_permission()`.
fn can_read(context: &Context, status: &Status) -> bool {
    status.public || (!status.internal || status.name == "trash") && can_edit_any(context)
}

/// `get_items_permissions_check()`.
fn items_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    if params.context() != b"edit" || can_edit_any(context) {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to manage post statuses.";
    Err(context.forbidden("rest_cannot_view", message))
}

/// `get_items()`: the statuses not internal, and trash.
fn items(context: &Context, params: &Params) -> Result<Response, RestError> {
    let data = STATUSES
        .iter()
        .filter(|status| !status.internal || status.name == "trash")
        .filter(|status| can_read(context, status))
        .map(|status| {
            let key = Key::String(status.name.as_bytes().to_vec());
            (key, prepare(context, params, status).into_data())
        })
        .collect();
    Ok(Response::new(Value::Array(data), 200))
}

fn requested(params: &Params) -> Option<&'static Status> {
    let name = params.get("status").map(Value::to_php_string);
    find(&name.unwrap_or_default())
}

/// `get_item_permissions_check()`.
fn invalid_status() -> RestError {
    RestError::status("rest_status_invalid", "Invalid status.", 404)
}

fn item_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    let status = requested(params).ok_or_else(invalid_status)?;
    if can_read(context, status) {
        return Ok(());
    }
    Err(context.forbidden("rest_cannot_read_status", "Cannot view status."))
}

/// `get_item()`, after its permission check found the status.
fn item(context: &Context, params: &Params) -> Result<Response, RestError> {
    // The permission check found the status.
    let status = requested(params).cloned().unwrap_or_default();
    Ok(prepare(context, params, &status))
}

/// `prepare_item_for_response()`.
fn prepare(context: &Context, params: &Params, status: &Status) -> Response {
    let wanted = fields_for_response(params, PROPERTIES);
    let fields = [
        ("name", text(status.label)),
        ("private", Value::Bool(status.private)),
        ("protected", Value::Bool(status.protected)),
        ("public", Value::Bool(status.public)),
        ("queryable", Value::Bool(status.queryable)),
        ("show_in_list", Value::Bool(!status.internal)),
        ("slug", text(status.name)),
        ("date_floating", Value::Bool(status.date_floating)),
    ];
    let data = fields
        .into_iter()
        .filter(|(name, _)| wanted.iter().any(|field| field == name.as_bytes()))
        .map(|(name, value)| (Key::String(name.as_bytes().to_vec()), value))
        .collect();
    let mut response = Response::new(
        Value::Array(filter_by_context(data, PROPERTIES, &params.context())),
        200,
    );
    let posts = context.rest_url("/wp/v2/posts");
    let archives = if status.name == "publish" {
        posts
    } else {
        let link = hyscribe_phpser::add_query_arg(
            &[(b"status", Some(status.name.as_bytes()))],
            posts.as_bytes(),
        );
        String::from_utf8_lossy(&link).into_owned()
    };
    response.add_link("archives", archives);
    response
}
