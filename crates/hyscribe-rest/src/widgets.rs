// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_REST_Sidebars_Controller`, `WP_REST_Widgets_Controller` and
//! `WP_REST_Widget_Types_Controller`: the sidebars, widgets and widget
//! types as a block theme answers them, which registers no sidebars, so
//! every widget is inactive. Their writes are not ported. Reading the
//! sidebars or widgets runs `retrieve_widgets()`, which stores the widgets
//! of a classic theme's sidebars as inactive; a site whose widgets it would
//! store, or whose widget settings `WP_Widget::get_settings()` would, is
//! answered 501 on those routes. Only the block widget's form and instance
//! are ported. A HEAD request is answered as a GET, whose body is not sent.

use std::sync::OnceLock;

use hyscribe_kses::esc_textarea;
use hyscribe_phpser::{Key, Value, add_query_arg, base64_encode, encode, json_decode};

use crate::fields::{
    Properties, fields_for_response, filter_by_context, is_field_included, links_wanted,
};
use crate::request::{Params, typed};
use crate::response::{Response, RestError, list, object, text};
use crate::types::context_arg;
use crate::{Context, Endpoint, self_link, unserialized};

const SIDEBARS: &str = "/wp/v2/sidebars";
const SIDEBAR: &str = "/wp/v2/sidebars/(?P<id>[\\w-]+)";
const WIDGETS: &str = "/wp/v2/widgets";
const WIDGET: &str = "/wp/v2/widgets/(?P<id>[\\w\\-]+)";
const TYPES: &str = "/wp/v2/widget-types";
const TYPE: &str = "/wp/v2/widget-types/(?P<id>[a-zA-Z0-9_-]+)";

/// The inactive sidebar, the only one `wp_get_sidebar()` finds under a
/// block theme.
const INACTIVE: &str = "wp_inactive_widgets";

const SIDEBAR_PROPERTIES: &Properties = &[
    ("id", &["embed", "view", "edit"]),
    ("name", &["embed", "view", "edit"]),
    ("description", &["embed", "view", "edit"]),
    ("class", &["embed", "view", "edit"]),
    ("before_widget", &["embed", "view", "edit"]),
    ("after_widget", &["embed", "view", "edit"]),
    ("before_title", &["embed", "view", "edit"]),
    ("after_title", &["embed", "view", "edit"]),
    ("status", &["embed", "view", "edit"]),
    ("widgets", &["embed", "view", "edit"]),
];

const WIDGET_PROPERTIES: &Properties = &[
    ("id", &["view", "edit", "embed"]),
    ("id_base", &["view", "edit", "embed"]),
    ("sidebar", &["view", "edit", "embed"]),
    ("rendered", &["view", "edit", "embed"]),
    ("rendered_form", &["edit"]),
    ("instance", &["edit"]),
    ("form_data", &[]),
];

const TYPE_PROPERTIES: &Properties = &[
    ("id", &["embed", "view", "edit"]),
    ("name", &["embed", "view", "edit"]),
    ("description", &["view", "edit", "embed"]),
    ("is_multi", &["view", "edit", "embed"]),
    ("classname", &["embed", "view", "edit"]),
];

pub(crate) fn endpoints(regex: &str) -> Option<Vec<Endpoint>> {
    let read = |args, callback| Endpoint {
        args,
        permission: can_manage,
        callback: Some(callback),
    };
    // Every write is checked as PHP checks it, then not ported.
    let write = || Endpoint {
        args: Vec::new(),
        permission: can_manage,
        callback: None,
    };
    let id = || typed("id", r#"{"type":"string"}"#);
    match regex {
        SIDEBARS => Some(vec![read(vec![context_arg()], sidebars)]),
        SIDEBAR => Some(vec![read(vec![id(), context_arg()], sidebar), write()]),
        WIDGETS => {
            let sidebar = typed("sidebar", r#"{"type":"string"}"#);
            Some(vec![read(vec![context_arg(), sidebar], widgets), write()])
        }
        WIDGET => Some(vec![read(vec![context_arg()], widget), write(), write()]),
        TYPES => Some(vec![read(vec![context_arg()], widget_types)]),
        TYPE => Some(vec![Endpoint {
            args: vec![id(), context_arg()],
            permission: type_permission,
            callback: Some(widget_type),
        }]),
        _ => None,
    }
}

/// `do_permissions_check()` and `permissions_check()`: no sidebar a block
/// theme has is shown in REST, so every route asks for this.
fn can_manage(context: &Context, _: &Params) -> Result<(), RestError> {
    if context.can("edit_theme_options") {
        return Ok(());
    }
    let message = "Sorry, you are not allowed to manage widgets on this site.";
    Err(context.forbidden("rest_cannot_manage_widgets", message))
}

/// Whether a request to the route `regex` would store the widgets, which
/// is not ported: the sidebars' and widgets' permission checks all run
/// `retrieve_widgets()`, for the `Allow` header too.
pub(crate) fn would_write(context: &Context, regex: &str) -> bool {
    [SIDEBARS, SIDEBAR, WIDGETS, WIDGET].contains(&regex) && retrieved(context).is_none()
}

/// A registered widget: its ID, `id_base` and instance number.
struct Registered {
    id: Vec<u8>,
    base: &'static str,
    number: Vec<u8>,
    /// The stored instance, none for a widget registered without one.
    instance: Option<Value>,
}

/// `$wp_registered_widgets` as `WP_Widget::_register()` registers the core
/// widgets: each stored instance, else instance 1; `None` where
/// `get_settings()` would store the settings, missing or in the old form.
fn registered(context: &Context) -> Option<Vec<Registered>> {
    let mut widgets = Vec::new();
    for base in hyscribe_render::core_widgets() {
        let settings = settings(context, base)?;
        let numbered = settings
            .into_iter()
            .map(|(key, instance)| (key.to_php_string(), Some(instance)))
            .filter(|(number, _)| Value::String(number.clone()).is_numeric());
        let mut numbered: Vec<(Vec<u8>, Option<Value>)> = numbered.collect();
        if numbered.is_empty() {
            numbered.push((b"1".to_vec(), None));
        }
        widgets.extend(numbered.into_iter().map(|(number, instance)| Registered {
            id: [base.as_bytes(), b"-", &number].concat(),
            base,
            number,
            instance,
        }));
    }
    Some(widgets)
}

/// `get_settings()` of the widget `base`, without `_multiwidget` and
/// `__i__`; `None` where it would store them.
fn settings(context: &Context, base: &str) -> Option<Vec<(Key, Value)>> {
    let stored = context.option(&format!("widget_{base}"))?;
    let settings = match unserialized(&stored) {
        Value::Bool(false) => return None,
        Value::Array(settings) => settings,
        _ => Vec::new(),
    };
    let named = |name: &[u8]| {
        let name = Key::String(name.to_vec());
        move |(key, _): &(Key, Value)| *key == name
    };
    if !settings.is_empty() && !settings.iter().any(named(b"_multiwidget")) {
        return None;
    }
    let kept = settings
        .into_iter()
        .filter(|entry| !named(b"_multiwidget")(entry) && !named(b"__i__")(entry));
    Some(kept.collect())
}

/// The sidebars' widgets `retrieve_widgets()` leaves under a block theme,
/// as `wp_get_sidebars_widgets()` then reads them: `None` where it would
/// store them changed, or restore the theme's widgets from before.
fn retrieved(context: &Context) -> Option<Vec<(Key, Value)>> {
    let registered = registered(context)?;
    let stylesheet = context.option("stylesheet").unwrap_or_default();
    let mods = format!("theme_mods_{}", String::from_utf8_lossy(&stylesheet));
    let old = crate::navigation::option_entry(context, &mods, "sidebars_widgets");
    if old.is_some_and(|old| matches!(old, Value::Array(_))) {
        return None;
    }
    let stored = context
        .option("sidebars_widgets")
        .map_or(Value::Array(Vec::new()), |stored| unserialized(&stored));
    let Value::Array(mut sidebars) = stored.clone() else {
        return stored.is_empty().then(Vec::new);
    };
    sidebars.retain(|(key, _)| *key != Key::String(b"array_version".to_vec()));
    if sidebars.is_empty() {
        return Some(sidebars);
    }
    let is_registered = |id: &Value| {
        registered
            .iter()
            .any(|widget| widget.id == id.to_php_string())
    };
    let ids = |widgets: &Value| -> Option<Vec<Value>> {
        let Value::Array(entries) = widgets else {
            return Some(
                vec![widgets.clone()]
                    .into_iter()
                    .filter(|id| *id != Value::Null)
                    .collect(),
            );
        };
        // A list keyed by name would keep its keys in `array_merge()`.
        if entries.iter().any(|(key, _)| matches!(key, Key::String(_))) {
            return None;
        }
        Some(
            entries
                .iter()
                .map(|(_, id)| id.clone())
                .filter(&is_registered)
                .collect(),
        )
    };
    // `wp_map_sidebars_widgets()` with no sidebar registered: the inactive
    // and orphaned widgets, then those of every other sidebar.
    let mut inactive = Vec::new();
    let mut others = Vec::new();
    for (sidebar, widgets) in &sidebars {
        let name = sidebar.to_php_string();
        if name == INACTIVE.as_bytes() || name.starts_with(b"orphaned_widgets") {
            inactive.extend(ids(widgets)?);
        } else if let Value::Array(_) = widgets {
            others.extend(ids(widgets)?);
        }
    }
    inactive.extend(others);
    let shown = |id: &[u8]| inactive.iter().any(|known| known.to_php_string() == id);
    // The lost widgets: registered, in no sidebar and numbered from 2.
    let lost = registered
        .iter()
        .filter(|widget| !shown(&widget.id) && Value::String(widget.number.clone()).to_int() >= 2);
    let mut widgets: Vec<Value> = lost
        .map(|widget| Value::String(widget.id.clone()))
        .collect();
    widgets.extend(inactive);
    let retrieved = vec![(Key::String(INACTIVE.as_bytes().to_vec()), list(widgets))];
    let mut written = retrieved.clone();
    written.push((Key::String(b"array_version".to_vec()), Value::Int(3)));
    (encode(&Value::Array(written)) == encode(&stored)).then_some(retrieved)
}

/// The widgets the retrieved sidebars list: [`would_write`] answered 501
/// before any callback where there are none.
fn sidebars_widgets(context: &Context) -> Vec<(Key, Value)> {
    retrieved(context).unwrap_or_default()
}

/// `get_items()` of the sidebars.
fn sidebars(context: &Context, params: &Params) -> Result<Response, RestError> {
    let sidebars = sidebars_widgets(context);
    let data = sidebars
        .iter()
        .filter(|(id, _)| id.to_php_string() == INACTIVE.as_bytes())
        .map(|(_, widgets)| prepare_sidebar(context, params, widgets).into_data());
    Ok(Response::new(list(data.collect()), 200))
}

/// `get_item()` of a sidebar: only the inactive one is there.
fn sidebar(context: &Context, params: &Params) -> Result<Response, RestError> {
    if params.text("id") != INACTIVE.as_bytes() {
        let message = "No sidebar exists with that id.";
        return Err(RestError::status("rest_sidebar_not_found", message, 404));
    }
    let sidebars = sidebars_widgets(context);
    let found = sidebars
        .iter()
        .find(|(id, _)| id.to_php_string() == INACTIVE.as_bytes());
    let widgets = found.map_or(Value::Array(Vec::new()), |(_, widgets)| widgets.clone());
    Ok(prepare_sidebar(context, params, &widgets))
}

/// `prepare_item_for_response()` of the inactive sidebar listing `widgets`.
fn prepare_sidebar(context: &Context, params: &Params, widgets: &Value) -> Response {
    let fields = fields_for_response(params, SIDEBAR_PROPERTIES);
    // The retrieved widgets are all registered; where they are not asked
    // for, `_fields` leaves them out.
    let widgets = widgets.clone();
    let data = object([
        ("id", text(INACTIVE)),
        ("name", text("Inactive widgets")),
        ("description", text("")),
        ("class", text("")),
        ("before_widget", text("")),
        ("after_widget", text("")),
        ("before_title", text("")),
        ("after_title", text("")),
        ("status", text("inactive")),
        ("widgets", widgets),
    ]);
    let mut response = Response::new(data, 200);
    if links_wanted(&fields) {
        response.add_link("collection", context.rest_url(SIDEBARS));
        self_link(context, &mut response, &format!("{SIDEBARS}/{INACTIVE}"));
        let widgets = context.rest_url(WIDGETS);
        let href = add_query_arg(
            &[(b"sidebar", Some(INACTIVE.as_bytes()))],
            widgets.as_bytes(),
        );
        let href = String::from_utf8_lossy(&href).into_owned();
        let embeddable = vec![("embeddable", Value::Bool(true))];
        response.add_link_with("https://api.w.org/widget", href, embeddable);
    }
    response
}

/// `get_items()` of the widgets: those of the inactive sidebar, if asked
/// for.
fn widgets(context: &Context, params: &Params) -> Result<Response, RestError> {
    let sidebars = sidebars_widgets(context);
    let registered = registered(context).unwrap_or_default();
    let mut data = Vec::new();
    for (sidebar, ids) in &sidebars {
        let sidebar = sidebar.to_php_string();
        if params
            .get("sidebar")
            .is_some_and(|asked| asked.to_php_string() != sidebar)
        {
            continue;
        }
        let ids = ids.as_array().unwrap_or_default().iter();
        let widgets = ids.filter_map(|(_, id)| {
            registered
                .iter()
                .find(|widget| widget.id == id.to_php_string())
        });
        for widget in widgets {
            data.push(prepare_widget(context, params, widget, &sidebar)?.into_data());
        }
    }
    Ok(Response::new(list(data), 200))
}

/// `get_item()` of a widget: one in a sidebar, all of which are inactive.
fn widget(context: &Context, params: &Params) -> Result<Response, RestError> {
    let id = params.text("id");
    let sidebars = sidebars_widgets(context);
    // `wp_find_widgets_sidebar()`.
    let listed = sidebars.iter().find_map(|(sidebar, ids)| {
        let ids = ids.as_array().unwrap_or_default();
        ids.iter()
            .any(|(_, known)| known.to_php_string() == id)
            .then(|| sidebar.to_php_string())
    });
    let registered = registered(context).unwrap_or_default();
    let found = registered.iter().find(|widget| widget.id == id);
    // The retrieved sidebars list registered widgets alone.
    match (listed, found) {
        (Some(sidebar), Some(widget)) => prepare_widget(context, params, widget, &sidebar),
        _ => {
            let message = "No widget was found with that id.";
            Err(RestError::status("rest_widget_not_found", message, 404))
        }
    }
}

/// `prepare_item_for_response()` of `widget` in `sidebar`.
fn prepare_widget(
    context: &Context,
    params: &Params,
    widget: &Registered,
    sidebar: &[u8],
) -> Result<Response, RestError> {
    let fields = fields_for_response(params, WIDGET_PROPERTIES);
    // An inactive widget renders nothing.
    let mut data = vec![
        ("id", Value::String(widget.id.clone())),
        ("id_base", text(widget.base)),
        ("sidebar", Value::String(sidebar.to_vec())),
        ("rendered", text("")),
        ("rendered_form", Value::Null),
        ("instance", Value::Null),
    ];
    let wanted = |field| is_field_included(field, &fields);
    if wanted("rendered_form") || wanted("instance") {
        let (form, instance) = block_widget(context, widget).ok_or_else(RestError::not_ported)?;
        for (name, value) in &mut data {
            match *name {
                "rendered_form" if wanted(name) => *value = form.clone(),
                "instance" if wanted(name) => *value = instance.clone(),
                _ => {}
            }
        }
    }
    let data = data
        .into_iter()
        .map(|(name, value)| (Key::String(name.as_bytes().to_vec()), value))
        .collect();
    let data = filter_by_context(data, WIDGET_PROPERTIES, &params.context());
    let mut response = Response::new(Value::Array(data), 200);
    if links_wanted(&fields) {
        let id = String::from_utf8_lossy(&widget.id);
        self_link(context, &mut response, &format!("{WIDGETS}/{id}"));
        response.add_link("collection", context.rest_url(WIDGETS));
        let about = context.rest_url(&format!("{TYPES}/{}", widget.base));
        response.add_link_with("about", about, vec![("embeddable", Value::Bool(true))]);
        let sidebar = String::from_utf8_lossy(sidebar);
        let rel = "https://api.w.org/sidebar";
        response.add_link(rel, context.rest_url(&format!("{SIDEBARS}/{sidebar}/")));
    }
    Ok(response)
}

/// A block widget's form, as `WP_Widget_Block::form()` prints it, and its
/// instance encoded, hashed and raw; `None` for any other widget, or an
/// instance that is not settings with content.
fn block_widget(context: &Context, widget: &Registered) -> Option<(Value, Value)> {
    let instance = widget.instance.clone().filter(|_| widget.base == "block")?;
    let Some(Value::String(content)) = instance.get_str("content") else {
        return None;
    };
    let number = String::from_utf8_lossy(&widget.number);
    let form = [
        format!("<p>\n\t\t\t<label for=\"widget-block-{number}-content\">\n\t\t\t\tBlock HTML:\t\t\t</label>\n\t\t\t<textarea id=\"widget-block-{number}-content\" name=\"widget-block[{number}][content]\" rows=\"6\" cols=\"50\" class=\"widefat code\">").as_bytes(),
        &esc_textarea(content),
        b"</textarea>\n\t\t</p>",
    ]
    .concat();
    let serialized = encode(&instance);
    let hash = context.site.wp_hash(&serialized)?;
    let encoded = object([
        ("encoded", text(base64_encode(&serialized))),
        ("hash", text(hash)),
        ("raw", instance),
    ]);
    Some((Value::String(form), encoded))
}

/// The widget types, exported by `reference/wordpress/export.sh` in the
/// widget types route's order.
fn types() -> &'static [(Key, Value)] {
    static TYPES: OnceLock<Value> = OnceLock::new();
    const DATA: &[u8] = include_bytes!("../../../wordpress/wp-includes/rest-widget-types.json");
    let data = TYPES.get_or_init(|| json_decode(DATA).unwrap_or_default());
    data.as_array().unwrap_or_default()
}

/// `get_items()` of the widget types.
fn widget_types(context: &Context, params: &Params) -> Result<Response, RestError> {
    let data = types()
        .iter()
        .map(|(_, widget)| prepare_type(context, params, widget).into_data());
    Ok(Response::new(list(data.collect()), 200))
}

/// The widget type `params` asks for.
fn requested_type(params: &Params) -> Option<&'static Value> {
    let id = text(params.text("id"));
    let found = types()
        .iter()
        .find(|(_, widget)| widget.get_str("id") == Some(&id));
    found.map(|(_, widget)| widget)
}

/// `get_item_permissions_check()` of a widget type: one there.
fn type_permission(context: &Context, params: &Params) -> Result<(), RestError> {
    can_manage(context, params)?;
    requested_type(params)
        .map(|_| ())
        .ok_or_else(|| RestError::status("rest_widget_type_invalid", "Invalid widget type.", 404))
}

/// `get_item()` of a widget type, which its permission check found.
fn widget_type(context: &Context, params: &Params) -> Result<Response, RestError> {
    let widget = requested_type(params).cloned().unwrap_or_default();
    Ok(prepare_type(context, params, &widget))
}

/// `prepare_item_for_response()` of a widget type.
fn prepare_type(context: &Context, params: &Params, widget: &Value) -> Response {
    let fields = fields_for_response(params, TYPE_PROPERTIES);
    let entries = widget
        .as_array()
        .unwrap_or_default()
        .iter()
        .filter(|(key, _)| {
            let key = String::from_utf8_lossy(&key.to_php_string()).into_owned();
            key == "id" || is_field_included(&key, &fields)
        });
    let data = filter_by_context(
        entries.cloned().collect(),
        TYPE_PROPERTIES,
        &params.context(),
    );
    let mut response = Response::new(Value::Array(data), 200);
    if links_wanted(&fields) {
        let id = widget
            .get_str("id")
            .map(Value::to_php_string)
            .unwrap_or_default();
        response.add_link("collection", context.rest_url(TYPES));
        self_link(
            context,
            &mut response,
            &format!("{TYPES}/{}", String::from_utf8_lossy(&id)),
        );
    }
    response
}
