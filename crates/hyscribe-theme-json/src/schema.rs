// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Theme_JSON_Schema::migrate()`: theme.json versions 1 and 2 to 3.

use hyscribe_phpser::Value;

use crate::php;
use crate::theme_json::Origin;

const V1_TO_V2_RENAMED_PATHS: [(&[&str], &[&str]); 4] = [
    (&["border", "customRadius"], &["border", "radius"]),
    (&["spacing", "customMargin"], &["spacing", "margin"]),
    (&["spacing", "customPadding"], &["spacing", "padding"]),
    (
        &["typography", "customLineHeight"],
        &["typography", "lineHeight"],
    ),
];

/// An empty theme.json of the latest schema: `{ "version": 3 }`.
pub(crate) fn latest() -> Value {
    Value::Array(vec![(php::key("version"), Value::Int(3))])
}

/// Migrates `theme_json` to the latest schema. Without a version it is
/// replaced by an empty latest-version document, as in PHP.
pub(crate) fn migrate(mut theme_json: Value, origin: Origin) -> Value {
    let version = php::get(&theme_json, &["version"]);
    if !php::isset(version) {
        return latest();
    }
    // `switch` compares loosely: "1", 1.0 and true are all version 1.
    let version = version.and_then(|v| match v {
        Value::Bool(true) => Some(1.0),
        v => php::number(v),
    });
    if version == Some(1.0) {
        if let Some(settings) = php::get_mut(&mut theme_json, &["settings"]) {
            rename_paths(settings);
        }
        php::set(&mut theme_json, &["version"], Value::Int(2));
        return v2_to_v3(theme_json, origin);
    }
    if version == Some(2.0) {
        return v2_to_v3(theme_json, origin);
    }
    theme_json
}

fn rename_paths(settings: &mut Value) {
    rename_settings(settings);
    if let Some(Value::Array(blocks)) = php::get_mut(settings, &["blocks"]) {
        for (_, block) in blocks {
            rename_settings(block);
        }
    }
}

fn rename_settings(settings: &mut Value) {
    for (original, renamed) in V1_TO_V2_RENAMED_PATHS {
        if let Some(value) = php::get(settings, original)
            .filter(|v| **v != Value::Null)
            .cloned()
        {
            php::set(settings, renamed, value);
            php::unset(settings, original);
        }
    }
}

fn v2_to_v3(mut new: Value, origin: Origin) -> Value {
    php::set(&mut new, &["version"], Value::Int(3));
    if origin == Origin::Custom {
        return new;
    }
    let old = new.clone();
    let has = |path: &[&str]| php::isset(php::get(&old, path));
    if has(&["settings", "typography", "fontSizes"]) {
        php::set(
            &mut new,
            &["settings", "typography", "defaultFontSizes"],
            Value::Bool(false),
        );
    }
    let sizes = has(&["settings", "spacing", "spacingSizes"]);
    if sizes || has(&["settings", "spacing", "spacingScale"]) {
        php::set(
            &mut new,
            &["settings", "spacing", "defaultSpacingSizes"],
            Value::Bool(false),
        );
    }
    if sizes {
        php::unset(&mut new, &["settings", "spacing", "spacingScale"]);
    }
    new
}

#[cfg(test)]
pub(crate) mod tests;
