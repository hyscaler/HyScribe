// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the block editor learns of the registered blocks:
//! `get_block_categories()`, the block bindings sources,
//! `get_block_editor_server_block_settings()` and the attributes each block
//! lets bindings set. The block types are those `reference/rest/registered.php`
//! exports, as the block types route gives them.

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Value, json_decode};
use hyscribe_render::theme::Theme;

use crate::page::{object, text};

const REGISTERED: &[u8] = include_bytes!("../../../../wordpress/wp-includes/rest-registered.json");

/// The registered block types, in registration order.
fn block_types() -> &'static [(Key, Value)] {
    static TYPES: OnceLock<Value> = OnceLock::new();
    let all = TYPES.get_or_init(|| json_decode(REGISTERED).unwrap_or_default());
    let types = all.get_str("block_types").and_then(Value::as_array);
    types.unwrap_or_default()
}

/// `get_block_categories( $post )`: `get_default_block_categories()`, which
/// a default install does not filter.
pub fn block_categories() -> Value {
    let categories = [
        ("text", "Text"),
        ("media", "Media"),
        ("design", "Design"),
        ("widgets", "Widgets"),
        ("theme", "Theme"),
        ("embed", "Embeds"),
        ("reusable", "Patterns"),
    ];
    let list = categories.iter().zip(0..).map(|((slug, title), i)| {
        let category = object(vec![
            ("slug", text(slug)),
            ("title", text(title)),
            ("icon", Value::Null),
        ]);
        (Key::Int(i), category)
    });
    Value::Array(list.collect())
}

/// The block bindings sources core registers, by name, then the bundled
/// theme's: their label and the context they use (`null` for none), as
/// `edit-form-blocks.php` gives them to the editor.
pub fn bindings_sources(theme: &Theme) -> Value {
    let core: [(&str, &str, &[&str]); 4] = [
        (
            "core/pattern-overrides",
            "Pattern Overrides",
            &["pattern/overrides"],
        ),
        ("core/post-data", "Post Data", &["postId", "postType"]),
        ("core/post-meta", "Post Meta", &["postId", "postType"]),
        ("core/term-data", "Term Data", &["termId", "taxonomy"]),
    ];
    let source = |name: &str, label: &str, context: Value| {
        let context = uses_context(context);
        object(vec![
            ("name", text(name)),
            ("label", text(label)),
            ("usesContext", context),
        ])
    };
    let own = theme.registered("binding_sources").iter().map(|(_, own)| {
        let field = |name: &str| {
            String::from_utf8_lossy(
                &own.get_str(name)
                    .map(Value::to_php_string)
                    .unwrap_or_default(),
            )
            .into_owned()
        };
        let context = own.get_str("uses_context").cloned().unwrap_or(Value::Null);
        source(&field("name"), &field("label"), context)
    });
    let core = core.iter().map(|(name, label, context)| {
        let listed = context.iter().zip(0..).map(|(c, j)| (Key::Int(j), text(c)));
        source(name, label, Value::Array(listed.collect()))
    });
    let list = core
        .chain(own)
        .zip(0..)
        .map(|(source, i)| (Key::Int(i), source));
    Value::Array(list.collect())
}

/// A source's `usesContext`: `null` for none.
fn uses_context(context: Value) -> Value {
    match context.as_array() {
        Some(listed) if !listed.is_empty() => context,
        _ => Value::Null,
    }
}

#[cfg(test)]
#[test]
fn a_source_without_context_uses_null() {
    assert_eq!(uses_context(Value::Array(Vec::new())), Value::Null);
    assert_eq!(uses_context(Value::Null), Value::Null);
    let one = Value::Array(vec![(Key::Int(0), text("postId"))]);
    assert_eq!(uses_context(one.clone()), one);
}

/// `get_block_editor_server_block_settings()`: each block type's fields
/// that are set, under the editor's names, with hs-admin's descriptions.
pub fn server_block_settings(theme: &Theme) -> Value {
    const FIELDS: [(&str, &str); 19] = [
        ("api_version", "apiVersion"),
        ("title", "title"),
        ("description", "description"),
        ("icon", "icon"),
        ("attributes", "attributes"),
        ("provides_context", "providesContext"),
        ("uses_context", "usesContext"),
        ("block_hooks", "blockHooks"),
        ("selectors", "selectors"),
        ("supports", "supports"),
        ("category", "category"),
        ("styles", "styles"),
        ("textdomain", "textdomain"),
        ("parent", "parent"),
        ("ancestor", "ancestor"),
        ("keywords", "keywords"),
        ("example", "example"),
        ("variations", "variations"),
        ("allowed_blocks", "allowedBlocks"),
    ];
    let blocks = block_types().iter().map(|(_, block)| {
        // A property left `null` is not set, which the route gives as `null`,
        // or as an empty list for the context a block provides.
        let set = FIELDS.iter().filter_map(|(field, key)| {
            let unset = |v: &Value| match *field {
                "provides_context" => v.as_array().is_some_and(<[_]>::is_empty),
                _ => *v == Value::Null,
            };
            let value = block.get_str(field).filter(|v| !unset(v))?;
            let value = match *field {
                "variations" => instance_descriptions(&themed_variations(theme, block, value)),
                "description" => crate::look::block_description(&value.to_php_string())
                    .map_or_else(|| value.clone(), text),
                _ => value.clone(),
            };
            Some((Key::String(key.as_bytes().to_vec()), value))
        });
        (name(block), Value::Array(set.collect()))
    });
    Value::Array(blocks.collect())
}

/// The variations of `block`, those the theme changes (its template parts,
/// its post format link) as it registers them.
fn themed_variations(theme: &Theme, block: &Value, core: &Value) -> Value {
    let name = block.get_str("name").map(Value::to_php_string);
    let mut themed = theme.registered("block_variations").iter();
    let own = themed.find(|(key, _)| Some(key.to_php_string()) == name);
    own.map_or_else(|| core.clone(), |(_, variations)| variations.clone())
}

/// `variations` as the block type has them: a template part instance's
/// description is PHP's `$template->description || $area_description`, a
/// boolean the route gives as the string `"1"` or `""`.
fn instance_descriptions(variations: &Value) -> Value {
    let list = variations.as_array().unwrap_or_default().iter();
    let list = list.map(|(i, variation)| {
        let name = variation.get_str("name").map(Value::to_php_string);
        let instance = name.is_some_and(|name| name.starts_with(b"instance_"));
        let fields = variation.as_array().unwrap_or_default().iter();
        let fields = fields.map(|(key, value)| {
            let boolean = instance && key.to_php_string() == b"description";
            let value = if boolean {
                Value::Bool(!value.is_empty())
            } else {
                value.clone()
            };
            (key.clone(), value)
        });
        (i.clone(), Value::Array(fields.collect()))
    });
    Value::Array(list.collect())
}

fn name(block: &Value) -> Key {
    Key::String(
        block
            .get_str("name")
            .map(Value::to_php_string)
            .unwrap_or_default(),
    )
}

/// `__experimentalBlockBindingsSupportedAttributes`: the attributes each
/// registered block lets bindings set (`get_block_bindings_supported_attributes()`).
pub fn bindings_supported_attributes() -> Value {
    let supported: [(&[u8], &[&str]); 8] = [
        (b"core/paragraph", &["content"]),
        (b"core/heading", &["content"]),
        (b"core/list-item", &["content"]),
        (b"core/image", &["id", "url", "title", "alt", "caption"]),
        (b"core/button", &["url", "text", "linkTarget", "rel"]),
        (b"core/post-date", &["datetime"]),
        (b"core/navigation-link", &["url"]),
        (b"core/navigation-submenu", &["url"]),
    ];
    let blocks = block_types().iter().filter_map(|(_, block)| {
        let key = name(block);
        let (_, attributes) = supported
            .iter()
            .find(|(name, _)| Key::String(name.to_vec()) == key)?;
        let list = attributes
            .iter()
            .zip(0..)
            .map(|(a, i)| (Key::Int(i), text(a)));
        Some((key, Value::Array(list.collect())))
    });
    Value::Array(blocks.collect())
}
