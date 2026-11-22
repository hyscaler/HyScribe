// SPDX-License-Identifier: Apache-2.0 OR MIT

//! A classic theme's templates (ADR 0023): askama's output tied to
//! WordPress's output rules. Values print through [`Esc`], `esc_html()`'s
//! behaviour written for the SDK; markup the host answers prints as it is
//! only as [`Html`]; and a page is bytes, not text: [`Html`]'s bytes are
//! kept aside behind markers and spliced back by [`render`].
//!
//! A port names the escaper in its `askama.toml`:
//!
//! ```toml
//! [[escaper]]
//! path = "::hyscribe_sdk::page::Esc"
//! extensions = ["html"]
//! ```

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use core::any::Any;
use core::cell::RefCell;
use core::fmt::{self, Write};

use askama::filters::Safe;

/// Opens a marker; the index of the bytes kept aside follows, then
/// [`CLOSE`]. With no index between them it stands for itself.
const OPEN: char = '\u{E000}';
/// Closes a marker.
const CLOSE: char = '\u{E001}';
/// The name the kept bytes go by among the render's values.
const KEPT: &str = "hyscribe_kept";

type Kept = RefCell<Vec<Vec<u8>>>;

/// Markup printed as it is, as the host's template tags answer it.
pub type Html = Safe<Raw>;

/// The bytes of an [`Html`]: only the SDK makes one.
pub struct Raw(Vec<u8>);

/// A template tag that prints markup (`wp_head`, `the_content`,
/// `wp_nav_menu`): what the host printed, as [`Html`].
pub fn print(name: &str, args: &[crate::Value]) -> Result<Html, crate::SdkError> {
    crate::template::print(name, args).map(|bytes| Safe(Raw(bytes)))
}

/// Never shown as text: a filter that takes text gets a marker character
/// standing for itself. Markup is only written into a render, kept aside.
impl fmt::Display for Raw {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{OPEN}{CLOSE}")
    }
}

impl askama::FastWritable for Raw {
    fn write_into(
        &self,
        dest: &mut dyn fmt::Write,
        values: &dyn askama::Values,
    ) -> askama::Result<()> {
        let kept = askama::get_value::<Kept>(values, KEPT)?;
        let mut kept = kept.borrow_mut();
        kept.push(self.0.clone());
        write!(dest, "{OPEN}{}{CLOSE}", kept.len() - 1).map_err(askama::Error::from)
    }
}

/// Text the host answered, printed through the escaper: nothing when it
/// is not UTF-8, as `esc_html()` prints nothing for it.
pub struct Text(pub Vec<u8>);

impl fmt::Display for Text {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(core::str::from_utf8(&self.0).unwrap_or_default())
    }
}

/// The escaper of a port's templates: `esc_html()`, and a marker
/// character in a value stands for itself, so no value makes a marker.
#[derive(Clone, Copy, Debug)]
pub struct Esc;

impl askama::filters::Escaper for Esc {
    fn write_escaped_str<W: Write>(&self, mut dest: W, string: &str) -> fmt::Result {
        dest.write_str(&escape(string).replace(OPEN, "\u{E000}\u{E001}"))
    }
}

/// Renders `template` into the bytes of the page.
pub fn render<T: askama::Template + ?Sized>(template: &T) -> Result<Vec<u8>, String> {
    let kept: Kept = RefCell::new(Vec::new());
    let values: [(&str, &dyn Any); 1] = [(KEPT, &kept)];
    finish(template.render_with_values(&values), &kept)
}

/// The rendered page spliced, or why it failed to render.
fn finish(out: askama::Result<String>, kept: &Kept) -> Result<Vec<u8>, String> {
    splice(out.as_deref().map_err(ToString::to_string)?, &kept.borrow())
}

/// The render `out` with each marker replaced by the bytes kept aside.
pub fn splice(out: &str, kept: &[Vec<u8>]) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(out.len());
    let mut chars = out.chars();
    while let Some(c) = chars.next() {
        if c != OPEN {
            bytes.extend_from_slice(c.encode_utf8(&mut [0; 4]).as_bytes());
            continue;
        }
        let mut index: Option<usize> = None;
        loop {
            match chars.next() {
                Some(CLOSE) => break,
                Some(digit @ '0'..='9') => {
                    let value = digit as usize - '0' as usize;
                    let n = index.unwrap_or(0).checked_mul(10);
                    index = Some(
                        n.and_then(|n| n.checked_add(value))
                            .ok_or("marker past the kept bytes")?,
                    );
                }
                _ => return Err("a marker not closed".into()),
            }
        }
        match index {
            None => bytes.extend_from_slice(OPEN.encode_utf8(&mut [0; 4]).as_bytes()),
            Some(at) => bytes.extend_from_slice(kept.get(at).ok_or("marker past the kept bytes")?),
        }
    }
    Ok(bytes)
}

/// `esc_html( $text )`: nothing for text that is not UTF-8.
pub fn esc_html(text: &[u8]) -> Vec<u8> {
    core::str::from_utf8(text).map_or_else(|_| Vec::new(), |text| escape(text).into_bytes())
}

/// What `esc_html()` makes of `text`: `<`, `>`, `"` and `'` as entities,
/// and `&` as `&amp;` unless it starts an entity WordPress allows, which
/// is kept, a numeric one written in its normal form.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(['&', '<', '>', '"', '\'']) {
        let (before, from) = rest.split_at(at);
        out.push_str(before);
        // What was found is one ASCII byte.
        let (found, after) = from.split_at(1);
        rest = match found {
            "&" => {
                let (entity, after) = entity(after);
                out.push_str(&entity);
                after
            }
            "<" => after_pushing(&mut out, "&lt;", after),
            ">" => after_pushing(&mut out, "&gt;", after),
            "\"" => after_pushing(&mut out, "&quot;", after),
            _ => after_pushing(&mut out, "&#039;", after),
        };
    }
    out.push_str(rest);
    out
}

/// `rest` after `entity` is pushed to `out`.
fn after_pushing<'a>(out: &mut String, entity: &str, rest: &'a str) -> &'a str {
    out.push_str(entity);
    rest
}

/// The entity an `&` followed by `after` starts, written as it is
/// printed, and what follows it; `&amp;` and `after` when it starts none.
fn entity(after: &str) -> (String, &str) {
    let ampersand = || (String::from("&amp;"), after);
    let Some((body, rest)) = after.split_once(';') else {
        return ampersand();
    };
    if let Some(number) = body.strip_prefix('#') {
        let (hex, digits) = match number.strip_prefix(['x', 'X']) {
            Some(digits) => (true, digits),
            None => (false, number),
        };
        // Leading zeros, then one to seven decimal or six hex digits, the
        // first of them not a zero.
        let kept = digits.trim_start_matches('0');
        let (radix, longest) = if hex { (16, 6) } else { (10, 7) };
        let shape = kept.chars().all(|c| c.is_digit(radix)) && (1..=longest).contains(&kept.len());
        let Some(code) = u32::from_str_radix(kept, radix).ok().filter(|_| shape) else {
            return ampersand();
        };
        return match (valid_unicode(code), hex) {
            (true, true) => (format!("&#x{kept};"), rest),
            (true, false) => (format!("&#{kept:0>3};"), rest),
            (false, true) => (format!("&amp;#x{digits};"), rest),
            (false, false) => (format!("&amp;#{digits};"), rest),
        };
    }
    // `&apos;` is allowed but is not HTML 4.01's, so it is encoded again.
    if body == "apos" {
        (String::from("&amp;apos;"), rest)
    } else if ENTITY_NAMES.contains(&body) {
        (format!("&{body};"), rest)
    } else {
        ampersand()
    }
}

/// A character XML allows: no surrogate, U+FFFE or U+FFFF.
fn valid_unicode(code: u32) -> bool {
    matches!(code, 0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

/// The named entities of HTML 4.01.
const ENTITY_NAMES: [&str; 252] = [
    "nbsp", "iexcl", "cent", "pound", "curren", "yen", "brvbar", "sect", "uml", "copy", "ordf",
    "laquo", "not", "shy", "reg", "macr", "deg", "plusmn", "acute", "micro", "para", "middot",
    "cedil", "ordm", "raquo", "iquest", "Agrave", "Aacute", "Acirc", "Atilde", "Auml", "Aring",
    "AElig", "Ccedil", "Egrave", "Eacute", "Ecirc", "Euml", "Igrave", "Iacute", "Icirc", "Iuml",
    "ETH", "Ntilde", "Ograve", "Oacute", "Ocirc", "Otilde", "Ouml", "times", "Oslash", "Ugrave",
    "Uacute", "Ucirc", "Uuml", "Yacute", "THORN", "szlig", "agrave", "aacute", "acirc", "atilde",
    "auml", "aring", "aelig", "ccedil", "egrave", "eacute", "ecirc", "euml", "igrave", "iacute",
    "icirc", "iuml", "eth", "ntilde", "ograve", "oacute", "ocirc", "otilde", "ouml", "divide",
    "oslash", "ugrave", "uacute", "ucirc", "uuml", "yacute", "thorn", "yuml", "quot", "amp", "lt",
    "gt", "OElig", "oelig", "Scaron", "scaron", "Yuml", "circ", "tilde", "ensp", "emsp", "thinsp",
    "zwnj", "zwj", "lrm", "rlm", "ndash", "mdash", "lsquo", "rsquo", "sbquo", "ldquo", "rdquo",
    "bdquo", "dagger", "Dagger", "permil", "lsaquo", "rsaquo", "euro", "fnof", "Alpha", "Beta",
    "Gamma", "Delta", "Epsilon", "Zeta", "Eta", "Theta", "Iota", "Kappa", "Lambda", "Mu", "Nu",
    "Xi", "Omicron", "Pi", "Rho", "Sigma", "Tau", "Upsilon", "Phi", "Chi", "Psi", "Omega", "alpha",
    "beta", "gamma", "delta", "epsilon", "zeta", "eta", "theta", "iota", "kappa", "lambda", "mu",
    "nu", "xi", "omicron", "pi", "rho", "sigmaf", "sigma", "tau", "upsilon", "phi", "chi", "psi",
    "omega", "thetasym", "upsih", "piv", "bull", "hellip", "prime", "Prime", "oline", "frasl",
    "weierp", "image", "real", "trade", "alefsym", "larr", "uarr", "rarr", "darr", "harr", "crarr",
    "lArr", "uArr", "rArr", "dArr", "hArr", "forall", "part", "exist", "empty", "nabla", "isin",
    "notin", "ni", "prod", "sum", "minus", "lowast", "radic", "prop", "infin", "ang", "and", "or",
    "cap", "cup", "int", "sim", "cong", "asymp", "ne", "equiv", "le", "ge", "sub", "sup", "nsub",
    "sube", "supe", "oplus", "otimes", "perp", "sdot", "lceil", "rceil", "lfloor", "rfloor",
    "lang", "rang", "loz", "spades", "clubs", "hearts", "diams", "sup1", "sup2", "sup3", "frac14",
    "frac12", "frac34", "there4",
];

/// Checks a port's sources against what would print markup unescaped
/// outside [`Html`]: askama's `safe` filter in `templates/`, a marker
/// character there, and in `src/` askama's `Safe`, `HtmlSafe` and
/// `MaybeSafe` or a template's own `escape`. A port's tests call it.
#[cfg(not(target_arch = "wasm32"))]
pub fn check_port(root: &std::path::Path) -> Result<(), String> {
    let refused: [(&str, &[&str]); 2] = [
        (
            "templates",
            &["|safe", "filtersafe", "\u{E000}", "\u{E001}"],
        ),
        ("src", &["Safe(", "HtmlSafe", "MaybeSafe", "escape="]),
    ];
    for (folder, words) in refused {
        let mut files = Vec::new();
        collect(&root.join(folder), &mut files).map_err(|err| format!("{folder}: {err}"))?;
        for file in files {
            let text = std::fs::read_to_string(&file)
                .map_err(|err| format!("{}: {err}", file.display()))?;
            let squeezed: String = text.chars().filter(|c| !c.is_whitespace()).collect();
            if let Some(word) = words.iter().find(|word| squeezed.contains(*word)) {
                return Err(format!("{}: {word:?} is refused in a port", file.display()));
            }
        }
    }
    Ok(())
}

/// Every file under `dir`.
#[cfg(not(target_arch = "wasm32"))]
fn collect(dir: &std::path::Path, files: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
    // A folder that cannot be listed fails; an entry the listing fails on
    // midway is left out.
    let entries = std::fs::read_dir(dir)?.filter_map(Result::ok);
    let paths: Vec<std::path::PathBuf> = entries.map(|entry| entry.path()).collect();
    let (dirs, plain): (Vec<_>, Vec<_>) = paths.into_iter().partition(|path| path.is_dir());
    files.extend(plain);
    dirs.iter().try_for_each(|dir| collect(dir, files))
}

#[cfg(test)]
mod tests;
