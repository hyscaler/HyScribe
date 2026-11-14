// SPDX-License-Identifier: AGPL-3.0-or-later

//! The fields a response gives: `_fields` as `rest_filter_response_fields()`
//! and `get_fields_for_response()` read it, and the context's as
//! `rest_filter_response_by_context()` keeps them.

use hyscribe_phpser::{Key, Value};

use crate::request::Params;
use crate::schema::{is_list, parse_list};

/// A schema's properties and the contexts each is shown in.
pub(crate) type Properties = [(&'static str, &'static [&'static str])];

/// `trim()`.
fn trim(text: &[u8]) -> Vec<u8> {
    let space = |b: &u8| matches!(b, b' ' | b'\t' | b'\n' | b'\r' | 0 | 0x0b);
    let start = text.iter().position(|b| !space(b)).unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|b| !space(b))
        .map_or(start, |end| end + 1);
    text.get(start..end).unwrap_or_default().to_vec()
}

/// `_fields` as a list, trimmed: none when not set or empty.
fn requested(params: &Params) -> Vec<Vec<u8>> {
    params.get("_fields").map_or_else(Vec::new, |fields| {
        parse_list(fields)
            .iter()
            .map(|(_, field)| trim(&field.to_php_string()))
            .collect()
    })
}

/// The fields asked for, nested: a name with nothing under it is wanted
/// whole.
#[derive(Debug, Default)]
struct Tree(Vec<(Vec<u8>, Option<Tree>)>);

impl Tree {
    /// Adds `name` and the parts under it; false under a field wanted whole.
    fn add(&mut self, name: &[u8], rest: &[&[u8]]) -> bool {
        for (known, child) in &mut self.0 {
            if known == name {
                return fill(child, rest);
            }
        }
        let mut child = Some(Tree::default());
        let added = fill(&mut child, rest);
        self.0.push((name.to_vec(), child));
        added
    }
}

/// Adds `parts` under `node`; with none left, the node is wanted whole.
fn fill(node: &mut Option<Tree>, parts: &[&[u8]]) -> bool {
    match (parts.split_first(), node) {
        (None, node) => {
            *node = None;
            true
        }
        (Some(_), None) => false,
        (Some((name, rest)), Some(tree)) => tree.add(name, rest),
    }
}

/// The tree of `fields`. A field under one already wanted whole ends the
/// list, as the `break 2` of PHP leaves the loop.
fn tree(fields: &[Vec<u8>]) -> Tree {
    let mut root = Some(Tree::default());
    for field in fields {
        let parts: Vec<&[u8]> = field.split(|&b| b == b'.').collect();
        if !fill(&mut root, &parts) {
            break;
        }
    }
    root.unwrap_or_default()
}

/// `_rest_array_intersect_key_recursive()`.
fn intersect(value: &Value, tree: &Tree) -> Value {
    let Value::Array(entries) = value else {
        return value.clone();
    };
    let kept = entries.iter().filter_map(|(key, item)| {
        let (_, wanted) = tree
            .0
            .iter()
            .find(|(name, _)| *name == key.to_php_string())?;
        let item = match wanted {
            Some(tree) => intersect(item, tree),
            None => item.clone(),
        };
        Some((key.clone(), item))
    });
    Value::Array(kept.collect())
}

/// `rest_filter_response_fields()` of a response's data.
pub(crate) fn filter_fields(data: Value, params: &Params) -> Value {
    let fields = requested(params);
    if fields.is_empty() {
        return data;
    }
    let tree = tree(&fields);
    match data {
        Value::Array(items) if is_list(&data) => Value::Array(
            items
                .into_iter()
                .map(|(key, item)| (key, intersect(&item, &tree)))
                .collect(),
        ),
        data => intersect(&data, &tree),
    }
}

/// `rest_is_field_included()`.
pub(crate) fn is_field_included(field: &str, fields: &[Vec<u8>]) -> bool {
    let field = field.as_bytes();
    let under = |a: &[u8], b: &[u8]| a.strip_prefix(b).is_some_and(|rest| rest.starts_with(b"."));
    fields
        .iter()
        .any(|accepted| accepted == field || under(accepted, field) || under(field, accepted))
}

/// Whether a property is shown in `context`.
fn shown(contexts: &[&str], context: &[u8]) -> bool {
    contexts.iter().any(|c| c.as_bytes() == context)
}

/// `get_fields_for_response()`: the properties shown in the request's
/// context and `_links`, narrowed to those `_fields` asks for.
pub(crate) fn fields_for_response(params: &Params, properties: &Properties) -> Vec<Vec<u8>> {
    let context = params.context();
    let mut fields: Vec<Vec<u8>> = properties
        .iter()
        .filter(|(_, contexts)| shown(contexts, &context))
        .map(|(name, _)| name.as_bytes().to_vec())
        .collect();
    fields.push(b"_links".to_vec());
    if params.has("_embed") {
        fields.push(b"_embedded".to_vec());
    }
    let mut requested = requested(params);
    if requested.is_empty() {
        return fields;
    }
    // `id` is always kept, for `add_additional_fields_to_object()`; a
    // schema without it drops it below.
    requested.push(b"id".to_vec());
    requested
        .into_iter()
        .filter(|field| {
            let first = field.split(|&b| b == b'.').next().unwrap_or_default();
            fields.iter().any(|known| known == field || known == first)
        })
        .collect()
}

/// `rest_filter_response_by_context()` of an object whose properties are
/// all named: those not shown in `context` go.
pub(crate) fn filter_by_context(
    data: Vec<(Key, Value)>,
    properties: &Properties,
    context: &[u8],
) -> Vec<(Key, Value)> {
    data.into_iter()
        .filter(|(key, _)| {
            properties
                .iter()
                .find(|(name, _)| name.as_bytes() == key.to_php_string())
                .is_none_or(|(_, contexts)| shown(contexts, context))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use hyscribe_phpser::{decode, json_decode, json_encode};
    use hyscribe_testkit::unhex;

    use super::*;

    /// Every case of `reference/rest/fields-golden.php`.
    #[test]
    fn fields_are_kept_as_php_keeps_them() {
        let golden = include_str!("../tests/golden/fields.tsv");
        for line in golden.lines() {
            let mut fields = line.split('\t').map(unhex);
            let mut field = || fields.next().unwrap();
            let (data, wanted, kept) = (field(), field(), field());
            let params = Params {
                get: vec![(Key::String(b"_fields".to_vec()), decode(&wanted).unwrap())],
                ..Params::default()
            };
            let ours = filter_fields(json_decode(&data).unwrap(), &params);
            let case = String::from_utf8_lossy(&wanted).into_owned();
            assert_eq!(json_encode(&ours).unwrap(), kept, "{case}");
        }
        assert_eq!(golden.lines().count(), 17);
    }
}
