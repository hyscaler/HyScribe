// SPDX-License-Identifier: AGPL-3.0-or-later

//! What the block editor learns of the registered blocks:
//! `get_block_categories()`, the block bindings sources,
//! `get_block_editor_server_block_settings()` and the attributes each block
//! lets bindings set. The block types are those `reference/rest/registered.php`
//! exports, as the block types route gives them.

use std::sync::OnceLock;

use hyscribe_phpser::{Key, Value, json_decode};

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
pub fn bindings_sources() -> Value {
    let sources: [(&str, &str, &[&str]); 5] = [
        (
            "core/pattern-overrides",
            "Pattern Overrides",
            &["pattern/overrides"],
        ),
        ("core/post-data", "Post Data", &["postId", "postType"]),
        ("core/post-meta", "Post Meta", &["postId", "postType"]),
        ("core/term-data", "Term Data", &["termId", "taxonomy"]),
        ("twentytwentyfive/format", "Post format name", &[]),
    ];
    let list = sources.iter().zip(0..).map(|((name, label, context), i)| {
        let listed = context.iter().zip(0..).map(|(c, j)| (Key::Int(j), text(c)));
        let context = match context.is_empty() {
            true => Value::Null,
            false => Value::Array(listed.collect()),
        };
        let source = object(vec![
            ("name", text(name)),
            ("label", text(label)),
            ("usesContext", context),
        ]);
        (Key::Int(i), source)
    });
    Value::Array(list.collect())
}

/// `get_block_editor_server_block_settings()`: each block type's fields
/// that are set, under the editor's names.
pub fn server_block_settings() -> Value {
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
                "variations" => instance_descriptions(value),
                _ => value.clone(),
            };
            Some((Key::String(key.as_bytes().to_vec()), value))
        });
        (name(block), Value::Array(set.collect()))
    });
    Value::Array(blocks.collect())
}

/// `variations` as the block type has them: a template part instance's
/// description is PHP's `$template->description || $area_description`, a
/// boolean the route gives as the string `"1"` or `""`.
fn instance_descriptions(variations: &Value) -> Value {
    let mut variations = variations.clone();
    if let Value::Array(list) = &mut variations {
        for (_, variation) in list.iter_mut() {
            let name = variation.get_str("name").map(Value::to_php_string);
            let instance = name.is_some_and(|name| name.starts_with(b"instance_"));
            if let (true, Value::Array(fields)) = (instance, variation) {
                for (key, value) in fields.iter_mut() {
                    if key.to_php_string() == b"description" {
                        *value = Value::Bool(!value.is_empty());
                    }
                }
            }
        }
    }
    variations
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
