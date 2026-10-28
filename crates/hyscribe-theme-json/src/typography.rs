// SPDX-License-Identifier: AGPL-3.0-or-later

//! Fluid typography: `wp_get_typography_font_size_value()` and the helpers
//! it uses, which turn a font size into a `clamp()` between a minimum and a
//! maximum that scale with the viewport.

use hyscribe_phpser::Value;

use hyscribe_phpser::float_to_string;

use crate::php::{self, round, string};

/// `wp_get_typography_value_and_unit( $raw, array( 'coerce_to' => ... ) )`.
fn value_and_unit(raw: &Value, coerce_to: &str) -> Option<(f64, String)> {
    if !matches!(raw, Value::String(_) | Value::Int(_) | Value::Float(_)) || php::empty(Some(raw)) {
        return None;
    }
    let mut text = php::php_string(raw);
    if raw.is_numeric() {
        text.push_str("px");
    }
    // `$` also matches before a final newline.
    let text = text.strip_suffix('\n').unwrap_or(&text);
    // A `%` unit would parse but is never acceptable, so letters suffice.
    let number = text.trim_end_matches(|c: char| c.is_ascii_alphabetic());
    let unit = text.get(number.len()..).unwrap_or_default();
    let (int, frac) = number.rsplit_once('.').unwrap_or(("", number));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if unit.is_empty() || frac.is_empty() || !digits(int) || !digits(frac) {
        return None;
    }
    if !["rem", "px", "em"].contains(&unit) {
        return None;
    }
    let mut value = hyscribe_phpser::string_to_float(number.as_bytes());
    let mut unit = unit.to_owned();
    let relative = |u: &str| u == "em" || u == "rem";
    if coerce_to == "px" && relative(&unit) {
        value *= 16.0;
        unit = coerce_to.to_owned();
    }
    if unit == "px" && relative(coerce_to) {
        value /= 16.0;
        unit = coerce_to.to_owned();
    }
    if relative(coerce_to) && relative(&unit) {
        unit = coerce_to.to_owned();
    }
    Some((round(value, 3), unit))
}

/// `wp_get_computed_fluid_typography_value()`.
fn computed_fluid(
    min_viewport: &Value,
    max_viewport: &Value,
    min_size: &Value,
    max_size: &Value,
) -> Option<String> {
    let minimum = value_and_unit(min_size, "");
    let unit = minimum
        .as_ref()
        .map_or("rem", |(_, u)| u.as_str())
        .to_owned();
    let maximum = value_and_unit(max_size, &unit)?;
    let minimum = minimum?;
    // A valid minimum is valid in rem too.
    let minimum_rem = value_and_unit(min_size, "rem").unwrap_or_default();
    let max_viewport = value_and_unit(max_viewport, &unit);
    let min_viewport = value_and_unit(min_viewport, &unit)?;
    let max_viewport = max_viewport?;
    let denominator = max_viewport.0 - min_viewport.0;
    if denominator == 0.0 {
        return None;
    }
    let offset = format!(
        "{}{unit}",
        float_to_string(round(min_viewport.0 / 100.0, 3))
    );
    let linear = 100.0 * ((maximum.0 - minimum.0) / denominator);
    let scaled = round(linear, 3);
    let scaled = if scaled == 0.0 {
        "1".to_owned()
    } else {
        float_to_string(scaled)
    };
    Some(format!(
        "clamp({}, {}{} + ((1vw - {offset}) * {scaled}), {})",
        php::php_string(min_size),
        float_to_string(minimum_rem.0),
        minimum_rem.1,
        php::php_string(max_size),
    ))
}

/// `wp_get_typography_font_size_value( $preset, $settings )`, where
/// `global` stands for `wp_get_global_settings()`.
pub(crate) fn font_size_value(preset: &Value, settings: &Value, global: &Value) -> Option<Value> {
    let size = php::get(preset, &["size"]).filter(|v| php::isset(Some(v)))?;
    let fluid_size = php::get(preset, &["fluid"]).unwrap_or(&Value::Null);
    if *fluid_size == Value::Bool(false) || php::empty(Some(size)) {
        return Some(size.clone());
    }
    // wp_parse_args( $settings, $global ): the node's top-level keys win.
    let lookup = |path: &[&str]| {
        let top = path.first().copied().unwrap_or_default();
        let source = if php::get(settings, &[top]).is_some() {
            settings
        } else {
            global
        };
        php::get(source, path)
    };
    if php::empty(lookup(&["typography", "fluid"])) && php::empty(Some(fluid_size)) {
        return Some(size.clone());
    }
    let fluid = |key: &str| lookup(&["typography", "fluid", key]).filter(|v| php::isset(Some(v)));
    let default_max = string("1600px");
    let min_viewport = fluid("minViewportWidth")
        .cloned()
        .unwrap_or_else(|| string("320px"));
    let wide = lookup(&["layout", "wideSize"])
        .filter(|v| php::isset(Some(v)) && value_and_unit(v, "").is_some());
    let max_viewport = fluid("maxViewportWidth")
        .or(wide)
        .unwrap_or(&default_max)
        .clone();
    let limit = fluid("minFontSize")
        .filter(|v| value_and_unit(v, "").is_some())
        .cloned()
        .unwrap_or_else(|| string("14px"));
    let explicit = |key: &str| {
        php::get(fluid_size, &[key])
            .filter(|v| !php::empty(Some(v)))
            .cloned()
    };
    let (mut min_size, mut max_size) = (explicit("min"), explicit("max"));
    let Some((value, unit)) = value_and_unit(size, "") else {
        return Some(size.clone());
    };
    let limit = value_and_unit(&limit, &unit);
    if let (Some((limit_value, _)), None, None) = (&limit, &min_size, &max_size)
        && value <= *limit_value
    {
        return Some(size.clone());
    }
    let max_size =
        max_size.get_or_insert_with(|| string(&format!("{}{unit}", float_to_string(value))));
    let min_size = min_size.get_or_insert_with(|| {
        let px = if unit == "px" { value } else { value * 16.0 };
        let factor = (1.0 - 0.075 * px.log2()).clamp(0.25, 0.75);
        let calculated = round(value * factor, 3);
        match &limit {
            Some((limit_value, limit_unit)) if calculated <= *limit_value => {
                string(&format!("{}{limit_unit}", float_to_string(*limit_value)))
            }
            _ => string(&format!("{}{unit}", float_to_string(calculated))),
        }
    });
    Some(
        match computed_fluid(&min_viewport, &max_viewport, min_size, max_size) {
            Some(fluid) => string(&fluid),
            None => size.clone(),
        },
    )
}

#[cfg(test)]
mod tests;
