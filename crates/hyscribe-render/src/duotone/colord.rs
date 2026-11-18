// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Duotone::colord_parse()`: a port of colord's parse for hex, `rgb()`
//! and `hsl()` strings, to the red, green and blue (0 to 255) and alpha (0
//! to 1) a duotone filter table holds.

/// A color's channels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rgba {
    pub(super) r: f64,
    pub(super) g: f64,
    pub(super) b: f64,
    pub(super) a: f64,
}

/// `WP_Duotone::colord_parse( $input )`.
pub(super) fn parse(input: &[u8]) -> Option<Rgba> {
    hex(input)
        .or_else(|| rgba_string(input))
        .or_else(|| hsla_string(input))
}

/// `colord_clamp()`; no number here is NaN, which `f64::clamp()` keeps.
fn clamp(number: f64, min: f64, max: f64) -> f64 {
    number.clamp(min, max)
}

/// One hexadecimal pair or digit pair as `base_convert()` reads it.
fn pair(text: &[u8]) -> f64 {
    text.iter().fold(0.0, |value, digit| {
        value * 16.0 + f64::from(char::from(*digit).to_digit(16).unwrap_or(0))
    })
}

/// `colord_parse_hex()`: `#` and 3 to 8 hexadecimal digits, of which only
/// 3, 4, 6 and 8 make a color.
fn hex(input: &[u8]) -> Option<Rgba> {
    // `$` also matches before a final newline.
    let digits = input.strip_prefix(b"#")?;
    let digits = digits.strip_suffix(b"\n").unwrap_or(digits);
    if !digits.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    // `round( $alpha / 255, 2 )`; no quotient is a tie, so rounding is exact.
    let alpha = |value: f64| (value / 255.0 * 100.0).round() / 100.0;
    let digit = |at: usize| digits.get(at).copied().unwrap_or(b'0');
    let double = |at: usize| pair(&[digit(at), digit(at)]);
    let part = |from: usize| pair(digits.get(from..from + 2).unwrap_or_default());
    match digits.len() {
        3 | 4 => Some(Rgba {
            r: double(0),
            g: double(1),
            b: double(2),
            a: if digits.len() == 4 {
                alpha(double(3))
            } else {
                1.0
            },
        }),
        6 | 8 => Some(Rgba {
            r: part(0),
            g: part(2),
            b: part(4),
            a: if digits.len() == 8 {
                alpha(part(6))
            } else {
                1.0
            },
        }),
        _ => None,
    }
}

/// A scanner over a string, for the patterns colord matches.
struct Scan<'a> {
    text: &'a [u8],
    at: usize,
}

impl<'a> Scan<'a> {
    /// PCRE's `\s`.
    fn spaces(&mut self) -> usize {
        let start = self.at;
        while self
            .text
            .get(self.at)
            .is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r'))
        {
            self.at += 1;
        }
        self.at - start
    }

    /// The literal `byte`, as written in lower case, of either case.
    fn byte(&mut self, byte: u8) -> bool {
        let found = self
            .text
            .get(self.at)
            .is_some_and(|b| b.eq_ignore_ascii_case(&byte));
        self.at += usize::from(found);
        found
    }

    /// The literal `word`, of either case.
    fn word(&mut self, word: &[u8]) -> bool {
        let found = self
            .text
            .get(self.at..self.at + word.len())
            .is_some_and(|w| w.eq_ignore_ascii_case(word));
        self.at += word.len() * usize::from(found);
        found
    }

    /// `[+-]?\d*\.?\d+`, as a number.
    fn number(&mut self) -> Option<f64> {
        let start = self.at;
        if matches!(self.text.get(self.at), Some(b'+' | b'-')) {
            self.at += 1;
        }
        let digits = |scan: &mut Scan<'_>| {
            let from = scan.at;
            while scan.text.get(scan.at).is_some_and(u8::is_ascii_digit) {
                scan.at += 1;
            }
            scan.at - from
        };
        let whole = digits(self);
        let dot = self.text.get(self.at) == Some(&b'.')
            && self.text.get(self.at + 1).is_some_and(u8::is_ascii_digit);
        if dot {
            self.at += 1;
            digits(self);
        } else if whole == 0 {
            self.at = start;
            return None;
        }
        // Only ASCII digits, a sign and a dot were taken: they always parse.
        let taken = self.text.get(start..self.at).unwrap_or_default();
        Some(String::from_utf8_lossy(taken).parse().unwrap_or_default())
    }

    /// The end of the pattern: `$`, which allows one newline last.
    fn end(&self) -> bool {
        self.at == self.text.len()
            || (self.at + 1 == self.text.len() && self.text.get(self.at) == Some(&b'\n'))
    }
}

/// The two syntaxes of a color function: commas, or spaces and a slash
/// before the alpha.
#[derive(Clone, Copy, PartialEq)]
enum Syntax {
    Commas,
    Spaces,
}

/// The separator that goes between values, `\s*,\s*` or `\s+`.
fn separator(scan: &mut Scan<'_>, syntax: Syntax) -> bool {
    match syntax {
        Syntax::Commas => {
            scan.spaces();
            let comma = scan.byte(b',');
            scan.spaces();
            comma
        }
        Syntax::Spaces => scan.spaces() > 0,
    }
}

/// The alpha group's opening, `,\s*` or `\/\s*`, with the leading space
/// already taken.
fn alpha_start(scan: &mut Scan<'_>, syntax: Syntax) -> bool {
    let found = match syntax {
        Syntax::Commas => scan.byte(b','),
        Syntax::Spaces => scan.byte(b'/'),
    };
    scan.spaces();
    found
}

/// The values a color function matched: its numbers, with whether each
/// had a `%`, and the unit the first had.
struct Values {
    numbers: [f64; 3],
    percents: [bool; 3],
    unit: Vec<u8>,
    /// The alpha, with its `%`.
    alpha: Option<(f64, bool)>,
}

/// A separator, then a number and its `%`, which a hue's saturation and
/// lightness must have.
fn next_value(scan: &mut Scan<'_>, syntax: Syntax, percent_needed: bool) -> Option<(f64, bool)> {
    if !separator(scan, syntax) {
        return None;
    }
    let number = scan.number()?;
    let percent = scan.byte(b'%');
    (percent || !percent_needed).then_some((number, percent))
}

/// `^name a?\( ... \)$` under `syntax`; `hue` reads the first value's unit
/// and the other two as percentages, else all three may have a `%`.
fn function(input: &[u8], name: &[u8], syntax: Syntax, hue: bool) -> Option<Values> {
    let mut scan = Scan { text: input, at: 0 };
    if !scan.word(name) {
        return None;
    }
    scan.byte(b'a');
    if !scan.byte(b'(') {
        return None;
    }
    scan.spaces();
    let first = scan.number()?;
    let mut unit = Vec::new();
    let mut first_percent = false;
    if hue {
        // `(deg|rad|grad|turn)?`: the first alternative that fits.
        for candidate in [&b"deg"[..], b"rad", b"grad", b"turn"] {
            let start = scan.at;
            if scan.word(candidate) {
                unit = input.get(start..scan.at).unwrap_or_default().to_vec();
                break;
            }
        }
    } else {
        first_percent = scan.byte(b'%');
    }
    let (second, second_percent) = next_value(&mut scan, syntax, hue)?;
    let (third, third_percent) = next_value(&mut scan, syntax, hue)?;
    scan.spaces();
    let mut alpha = None;
    let before = scan.at;
    if alpha_start(&mut scan, syntax) {
        let value = scan.number()?;
        let percent = scan.byte(b'%');
        scan.spaces();
        alpha = Some((value, percent));
    } else {
        scan.at = before;
    }
    (scan.byte(b')') && scan.end()).then_some(Values {
        numbers: [first, second, third],
        percents: [first_percent, second_percent, third_percent],
        unit,
        alpha,
    })
}

/// `colord_parse_rgba_string()`.
fn rgba_string(input: &[u8]) -> Option<Rgba> {
    let values = [Syntax::Commas, Syntax::Spaces]
        .into_iter()
        .find_map(|syntax| function(input, b"rgb", syntax, false))?;
    let [first, second, third] = values.percents;
    if first != second || second != third {
        return None;
    }
    let channel = |number: f64, percent: bool| {
        let divisor = if percent { 100.0 / 255.0 } else { 1.0 };
        clamp(number / divisor, 0.0, 255.0)
    };
    let [r, g, b] = values.numbers;
    Some(Rgba {
        r: channel(r, first),
        g: channel(g, second),
        b: channel(b, third),
        a: clamp(alpha(&values), 0.0, 1.0),
    })
}

/// `'' === $match[7] ? 1 : (float) $match[7] / ( $match[8] ? 100 : 1 )`.
fn alpha(values: &Values) -> f64 {
    values.alpha.map_or(1.0, |(value, percent)| {
        value / if percent { 100.0 } else { 1.0 }
    })
}

/// `colord_parse_hsla_string()`.
fn hsla_string(input: &[u8]) -> Option<Rgba> {
    let values = [Syntax::Commas, Syntax::Spaces]
        .into_iter()
        .find_map(|syntax| function(input, b"hsl", syntax, true))?;
    // `colord_parse_hue()`: the unit's factor, looked up as written.
    let factor = match values.unit.as_slice() {
        b"grad" => 360.0 / 400.0,
        b"turn" => 360.0,
        b"rad" => 360.0 / (std::f64::consts::PI * 2.0),
        _ => 1.0,
    };
    // `colord_clamp_hue()`: `%` truncates to an integer.
    let [hue_number, saturation_number, lightness_number] = values.numbers;
    let degrees = hue_number * factor;
    let degrees = if degrees.is_finite() {
        (degrees as i64 % 360) as f64
    } else {
        0.0
    };
    let hue = if degrees > 0.0 {
        degrees
    } else {
        degrees + 360.0
    };
    let saturation = clamp(saturation_number, 0.0, 100.0);
    let lightness = clamp(lightness_number, 0.0, 100.0);
    let alpha = clamp(alpha(&values), 0.0, 1.0);
    // `colord_hsla_to_hsva()` and `colord_hsva_to_rgba()`.
    let scaled = saturation
        * (if lightness < 50.0 {
            lightness
        } else {
            100.0 - lightness
        })
        / 100.0;
    let s = if scaled > 0.0 {
        ((2.0 * scaled) / (lightness + scaled)) * 100.0
    } else {
        0.0
    };
    let v = lightness + scaled;
    let h = (hue / 360.0) * 6.0;
    let (s, v) = (s / 100.0, v / 100.0);
    let floor = h.floor();
    let b = v * (1.0 - s);
    let c = v * (1.0 - (h - floor) * s);
    let d = v * (1.0 - (1.0 - h + floor) * s);
    let module = (floor as i64 % 6) as usize;
    let pick = |table: [f64; 6]| table.get(module).copied().unwrap_or_default() * 255.0;
    Some(Rgba {
        r: pick([v, c, b, b, d, v]),
        g: pick([d, v, v, c, b, b]),
        b: pick([b, b, d, v, v, c]),
        a: alpha,
    })
}
