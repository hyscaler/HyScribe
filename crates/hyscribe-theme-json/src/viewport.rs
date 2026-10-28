// SPDX-License-Identifier: AGPL-3.0-or-later

//! `settings.viewport`: the breakpoints behind the `@mobile` and `@tablet`
//! style states, sanitized as `sanitize_viewport_settings()` does.

use hyscribe_phpser::Value;

use crate::php::{self, as_str, string};

const DEFAULTS: [(&str, &str); 2] = [("mobile", "480px"), ("tablet", "782px")];

/// A valid breakpoint (`\d+` or `\d*\.\d+` then `px`, `em` or `rem`), with
/// its size in pixels for ordering.
fn breakpoint(value: &Value) -> Option<(&str, f64)> {
    let value = php::trim(as_str(value)?);
    let (number, unit) = ["rem", "px", "em"]
        .iter()
        .find_map(|unit| value.strip_suffix(unit).map(|n| (n, *unit)))?;
    let (int, frac) = number.split_once('.').unwrap_or((number, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let valid = digits(int)
        && digits(frac)
        && if number.contains('.') {
            !frac.is_empty()
        } else {
            !int.is_empty()
        };
    let px = php::to_float(number) * if unit == "px" { 1.0 } else { 16.0 };
    valid.then_some((value, px))
}

/// The sanitized breakpoints, `(name, value)`.
fn breakpoints(settings: Option<&Value>) -> Vec<(&'static str, String)> {
    let defaults = || DEFAULTS.iter().map(|&(k, v)| (k, v.to_owned())).collect();
    let Some(settings @ Value::Array(_)) = settings else {
        return defaults();
    };
    let valid: Vec<(&'static str, &str, f64)> = DEFAULTS
        .iter()
        .filter_map(|&(name, _)| {
            let (value, px) = breakpoint(php::get(settings, &[name])?)?;
            Some((name, value, px))
        })
        .collect();
    match valid.as_slice() {
        [] => defaults(),
        [(mobile, value, mobile_px), rest @ ..] => {
            let mut out = vec![(*mobile, (*value).to_owned())];
            out.extend(
                rest.iter()
                    .filter(|(_, _, px)| mobile_px < px)
                    .map(|(name, value, _)| (*name, (*value).to_owned())),
            );
            out
        }
    }
}

/// `sanitize_viewport_settings( $settings )` as a PHP array.
pub(crate) fn sanitize(settings: &Value) -> Value {
    Value::Array(
        breakpoints(Some(settings))
            .into_iter()
            .map(|(k, v)| (php::key(k), string(&v)))
            .collect(),
    )
}

/// `get_viewport_media_queries( $settings )`: `(state, media query)`.
pub(crate) fn media_queries(settings: Option<&Value>) -> Vec<(String, String)> {
    let points = breakpoints(settings);
    let mobile = points.iter().find(|(k, _)| *k == "mobile").map(|(_, v)| v);
    points
        .iter()
        .map(|(name, value)| {
            let query = match (*name, mobile) {
                ("tablet", Some(mobile)) => format!("@media ({mobile} < width <= {value})"),
                _ => format!("@media (width <= {value})"),
            };
            (format!("@{name}"), query)
        })
        .collect()
}

#[cfg(test)]
mod tests;
