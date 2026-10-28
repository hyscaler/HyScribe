// SPDX-License-Identifier: AGPL-3.0-or-later

//! Spacing size presets generated from `settings.spacing.spacingScale`
//! (`compute_spacing_sizes()`) and merged with explicit ones
//! (`merge_spacing_sizes()`).

use hyscribe_phpser::{Key, Value};

use hyscribe_phpser::{float_to_string, string_to_float};

use crate::php::{self, round, string};

/// More steps than any scale needs; PHP has no bound, which would let a
/// hostile theme.json loop for ever.
const MAX_STEPS: usize = 100;

fn preset(name: &str, slug: i64, size: String) -> Value {
    Value::Array(vec![
        (php::key("name"), string(name)),
        (php::key("slug"), string(&slug.to_string())),
        (php::key("size"), string(&size)),
    ])
}

/// `sanitize_title()` for a CSS unit: lower case ASCII letters, digits,
/// `_` and `-`, with dots and spaces as dashes.
fn sanitize_unit(unit: &str) -> String {
    let mut out = String::new();
    for c in unit.to_ascii_lowercase().chars() {
        match c {
            'a'..='z' | '0'..='9' | '_' => out.push(c),
            '-' | '.' | ' ' if !out.ends_with('-') => out.push('-'),
            _ => {}
        }
    }
    out.trim_matches('-').to_owned()
}

/// `compute_spacing_sizes( $spacing_scale )`.
pub(crate) fn compute_sizes(scale: &Value) -> Vec<Value> {
    let field = |key: &str| php::get(scale, &[key]).filter(|v| php::isset(Some(v)));
    let numeric = |key: &str| field(key).and_then(php::number);
    let operator = field("operator")
        .and_then(php::as_str)
        .filter(|op| *op == "+" || *op == "*");
    let (Some(steps), Some(medium), Some(unit), Some(operator), Some(increment)) = (
        numeric("steps"),
        numeric("mediumStep"),
        field("unit"),
        operator,
        numeric("increment"),
    ) else {
        return Vec::new();
    };
    if field("steps") == Some(&Value::Int(0)) {
        return Vec::new();
    }
    let unit = match php::as_str(unit) {
        Some("%") => "%".to_owned(),
        _ => sanitize_unit(&php::php_string(unit)),
    };
    let add = operator == "+";
    let mid = round(steps / 2.0, 0);

    let mut below = Vec::new();
    let mut current = medium;
    let mut x_small: Option<i64> = None;
    let mut remainder = 0.0;
    let mut count = mid - 1.0;
    // A positive count means at least three steps.
    for slug in [40, 30, 20, 10] {
        if count <= 0.0 {
            break;
        }
        // Down by the increment: divided when above 1, else multiplied.
        current = if add {
            current - increment
        } else {
            current * increment.min(1.0) / increment.max(1.0)
        };
        if current <= 0.0 {
            remainder = count;
            break;
        }
        let name = if count == mid - 1.0 {
            "Small".to_owned()
        } else {
            format!(
                "{}X-Small",
                x_small.map(|n| n.to_string()).unwrap_or_default()
            )
        };
        below.push(preset(
            &name,
            slug,
            format!("{}{unit}", float_to_string(round(current, 2))),
        ));
        if count == mid - 2.0 {
            x_small = Some(2);
        }
        if count < mid - 2.0 {
            x_small = Some(x_small.map_or(1, |n| n + 1));
        }
        count -= 1.0;
    }
    below.reverse();
    let medium_text = php::php_string(field("mediumStep").unwrap_or(&Value::Null));
    below.push(preset("Medium", 50, format!("{medium_text}{unit}")));

    let mut current = medium;
    let mut x_large: Option<i64> = None;
    let mut slug = 60;
    let above = (steps - mid) + remainder;
    for count in (0..MAX_STEPS).take_while(|&c| (c as f64) < above) {
        current = if add {
            current + increment
        } else {
            current * increment.max(1.0) / increment.min(1.0)
        };
        let name = if count == 0 {
            "Large".to_owned()
        } else {
            format!(
                "{}X-Large",
                x_large.map(|n| n.to_string()).unwrap_or_default()
            )
        };
        below.push(preset(
            &name,
            slug,
            format!("{}{unit}", float_to_string(round(current, 2))),
        ));
        if count == 1 {
            x_large = Some(2);
        }
        if count > 1 {
            x_large = Some(x_large.map_or(1, |n| n + 1));
        }
        slug += 10;
    }
    below
}

/// The array key PHP makes of a preset's slug.
fn slug_key(item: &Value) -> Key {
    match php::get(item, &["slug"]) {
        Some(Value::Int(n)) => Key::Int(*n),
        Some(Value::Bool(b)) => Key::Int(i64::from(*b)),
        Some(Value::Float(f)) => Key::Int(f.to_f64() as i64),
        Some(slug) => php::key(&php::php_string(slug)),
        None => php::key(""),
    }
}

/// `merge_spacing_sizes( $base, $incoming )`: one preset per slug, sorted
/// by slug as numbers.
pub(crate) fn merge_sizes(base: Vec<Value>, incoming: &Value) -> Value {
    if base.is_empty() {
        return incoming.clone();
    }
    let mut merged = php::Ordered::new();
    let items = base
        .into_iter()
        .chain(php::entries(incoming).iter().map(|(_, v)| v.clone()));
    for item in items {
        merged.put(slug_key(&item), item);
    }
    let mut merged = merged.into_vec();
    let number = |k: &Key| match k {
        Key::Int(n) => *n as f64,
        Key::String(_) => string_to_float(php::key_str(k).as_bytes()),
    };
    merged.sort_by(|(a, _), (b, _)| number(a).total_cmp(&number(b)));
    Value::Array(
        merged
            .into_iter()
            .enumerate()
            .map(|(i, (_, v))| (Key::Int(i as i64), v))
            .collect(),
    )
}

#[cfg(test)]
mod tests;
