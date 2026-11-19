// SPDX-License-Identifier: AGPL-3.0-or-later

//! An upload's type and name as WordPress makes them (MVP 6.2):
//! `wp_check_filetype()`, `wp_check_filetype_and_ext()`,
//! `sanitize_file_name()` and `wp_unique_filename()`. A type check PHP
//! hands to `fileinfo` is [`crate::fileinfo`]'s, [`Unported`] where it
//! cannot say for certain what libmagic answers.

use std::sync::OnceLock;

use hyscribe_phpser::json_decode;
use hyscribe_render::formatting::{remove_accents, sanitize_title_for_query};

use crate::fileinfo::finfo_candidates;
use crate::header::image_mime;
use crate::meta::Unported;

/// `wp_get_mime_types()`: each extension pattern (`jpg|jpeg|jpe`) and its
/// type, in WordPress's order.
fn mime_types() -> &'static [(String, String)] {
    static TYPES: OnceLock<Vec<(String, String)>> = OnceLock::new();
    TYPES.get_or_init(|| {
        let json = include_bytes!("../../../wordpress/wp-includes/wp-mime-types.json");
        let text = |v: &[u8]| String::from_utf8_lossy(v).into_owned();
        let pairs = json_decode(json)
            .and_then(|value| value.as_array().map(<[_]>::to_vec))
            .unwrap_or_default();
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    text(&k.to_php_string()),
                    text(v.as_bytes().unwrap_or_default()),
                )
            })
            .collect()
    })
}

/// `get_allowed_mime_types()` for a user who may or may not post
/// unfiltered HTML: no `swf` or `exe`, and `htm|html` and `js` only with it.
fn allowed(unfiltered_html: bool) -> impl Iterator<Item = &'static (String, String)> {
    mime_types()
        .iter()
        .filter(move |(pattern, _)| match pattern.as_str() {
            "swf" | "exe" => false,
            "htm|html" | "js" => unfiltered_html,
            _ => true,
        })
}

/// The first of `types` whose pattern ends `filename` after a dot, any
/// case: the extension as written, and the type.
fn check<'a>(
    filename: &str,
    mut types: impl Iterator<Item = &'a (String, String)>,
) -> Option<(String, &'a str)> {
    types.find_map(|(pattern, mime)| {
        pattern.split('|').find_map(|ext| {
            let at = filename.len().checked_sub(ext.len() + 1)?;
            let suffix = filename.get(at..)?;
            let (dot, written) = suffix.split_at_checked(1)?;
            (dot == "." && written.eq_ignore_ascii_case(ext))
                .then(|| (written.to_string(), mime.as_str()))
        })
    })
}

/// `wp_check_filetype( $filename )`: the extension and type, if allowed.
pub fn wp_check_filetype(filename: &str, unfiltered_html: bool) -> Option<(String, &'static str)> {
    check(filename, allowed(unfiltered_html))
}

/// What `wp_check_filetype_and_ext()` gives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checked {
    /// `ext` and `type`, `None` for `false`.
    pub ext_type: Option<(String, &'static str)>,
    /// `proper_filename`: the name with the extension of the type the
    /// file's header gives, where it differs.
    pub proper_filename: Option<String>,
}

/// Whether `kind`, the type a file's extension gives, stands once
/// `fileinfo` says the file is `real`: the vetting at the end of
/// `wp_check_filetype_and_ext()`.
fn stands(kind: &str, real: &str) -> bool {
    let major = |mime: &str| mime.split('/').next().unwrap_or(mime).to_string();
    let nonspecific = [
        "application/octet-stream",
        "application/encrypted",
        "application/CDFV2-encrypted",
        "application/zip",
    ];
    let one_of = |kinds: &[&str]| kinds.contains(&kind);
    if nonspecific.contains(&real) {
        ["application", "video", "audio"].contains(&major(kind).as_str())
    } else if real.starts_with("video/") || real.starts_with("audio/") {
        major(real) == major(kind)
    } else {
        match real {
            "text/plain" => one_of(&[
                "text/plain",
                "text/csv",
                "application/csv",
                "text/richtext",
                "text/tsv",
                "text/vtt",
            ]),
            "application/csv" => one_of(&["text/csv", "text/plain", "application/csv"]),
            "text/rtf" => one_of(&["text/rtf", "text/plain", "application/rtf"]),
            _ => kind == real,
        }
    }
}

/// `wp_check_filetype_and_ext( $file, $filename )` of the upload `bytes`,
/// `fileinfo` read by [`finfo_candidates`]: [`Unported`] where the types
/// libmagic may give do not agree on the verdict.
pub fn wp_check_filetype_and_ext(
    bytes: &[u8],
    filename: &str,
    unfiltered_html: bool,
) -> Result<Checked, Unported> {
    let mut checked = Checked {
        ext_type: wp_check_filetype(filename, unfiltered_html),
        proper_filename: None,
    };
    let Some((ext, kind)) = checked.ext_type.clone() else {
        return Ok(checked);
    };
    let real = image_mime(bytes).filter(|_| kind.starts_with("image/"));
    if let Some(real) = real {
        if real == kind && !["heif", "heics", "heifs"].contains(&ext.as_str()) {
            return Ok(checked);
        }
        let ext = match real {
            "image/jpeg" => "jpg",
            "image/png" => "png",
            "image/gif" => "gif",
            "image/bmp" => "bmp",
            "image/tiff" => "tif",
            "image/webp" => "webp",
            "image/avif" => "avif",
            _ => "heic",
        };
        let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
        let renamed = format!("{stem}.{ext}");
        checked.ext_type = wp_check_filetype(&renamed, unfiltered_html);
        checked.proper_filename = (renamed != filename).then_some(renamed);
        return Ok(checked);
    }
    let verdicts: Vec<bool> = finfo_candidates(bytes)?
        .into_iter()
        .map(|real| stands(kind, real))
        .collect();
    if verdicts.iter().all(|stands| *stands) {
        Ok(checked)
    } else if verdicts.iter().all(|stands| !stands) {
        checked.ext_type = None;
        Ok(checked)
    } else {
        Err(Unported)
    }
}

/// The characters `sanitize_file_name()` removes.
const SPECIAL: [char; 33] = [
    '?', '[', ']', '/', '\\', '=', '<', '>', ':', ';', ',', '\'', '"', '&', '$', '#', '*', '(',
    ')', '|', '~', '`', '!', '{', '}', '%', '+', '\u{2019}', '\u{ab}', '\u{bb}', '\u{201d}',
    '\u{201c}', '\0',
];

/// Unicode's space separators (`\p{Zs}`).
fn is_space_separator(c: char) -> bool {
    matches!(
        c,
        ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}' | '\u{202f}' | '\u{205f}' | '\u{3000}'
    )
}

/// `sanitize_file_name( $filename )`.
pub fn sanitize_file_name(filename: &[u8], unfiltered_html: bool) -> String {
    let name = remove_accents(filename);
    let name = match std::str::from_utf8(&name) {
        Ok(name) => name.to_string(),
        // Not UTF-8: the name part as a slug, the extension as it is.
        Err(_) => {
            let at = name.iter().rposition(|&b| b == b'.');
            let (stem, ext) = at.map_or((&name[..], &b""[..]), |at| name.split_at(at));
            let ext = ext.strip_prefix(b".").unwrap_or_default();
            let slug = sanitize_title_for_query(stem);
            String::from_utf8_lossy(&[slug.as_slice(), b".", ext].concat()).into_owned()
        }
    };
    let mut out = String::new();
    for c in name
        .chars()
        .map(|c| if is_space_separator(c) { ' ' } else { c })
        .filter(|c| !SPECIAL.contains(c))
    {
        // Dots run together are one; a run of whitespace and dashes is a
        // dash (every dash in `out` ends such a run).
        let separator = matches!(c, '\r' | '\n' | '\t' | ' ' | '-');
        match (c, out.chars().last()) {
            ('.', Some('.')) => {}
            (_, Some('-')) if separator => {}
            _ if separator => out.push('-'),
            _ => out.push(c),
        }
    }
    let mut name = out.trim_matches(['.', '-', '_']).to_string();
    if !name.contains('.')
        && let Some((ext, _)) =
            check(&format!("test.{name}"), mime_types().iter()).filter(|(ext, _)| *ext == name)
    {
        name = format!("unnamed-file.{ext}");
    }
    let parts: Vec<&str> = name.split('.').collect();
    // Two parts come back as they are, with no middle part to mark.
    let [first, middle @ .., last] = parts.as_slice() else {
        return name;
    };
    let mut out = (*first).to_string();
    for part in middle {
        out.push('.');
        out.push_str(part);
        // A part shaped like an extension that is not an allowed one gets
        // an underscore, so the server never reads it as the type.
        let letters = part.bytes().take_while(u8::is_ascii_alphabetic).count();
        let digits = part.len() - letters;
        let shaped = (2..=5).contains(&letters)
            && (digits == 0 || (digits == 1 && part.ends_with(|c: char| c.is_ascii_digit())));
        let known = check(&format!(".{part}"), allowed(unfiltered_html)).is_some();
        if shaped && !known {
            out.push('_');
        }
    }
    out.push('.');
    out.push_str(last);
    out
}

/// PHP's `pathinfo()` of a base name: the file name and the extension,
/// `None` without a dot.
fn pathinfo(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, ext)) => (stem, Some(ext)),
        None => (name, None),
    }
}

/// PHP's `str_replace()` of each of `searches` in turn with `to`.
fn str_replace(subject: &str, searches: [&str; 2], to: &str) -> String {
    searches
        .iter()
        .filter(|s| !s.is_empty())
        .fold(subject.to_string(), |out, search| out.replace(search, to))
}

/// `_wp_check_existing_file_names()`: whether `files` holds a size,
/// `-scaled` or `-rotated` copy named after `filename`, in any case.
fn existing_copy(filename: &str, files: &[String]) -> bool {
    let (stem, ext) = pathinfo(filename);
    if stem.is_empty() || stem == "0" {
        return false;
    }
    let ext = ext.map_or(String::new(), |ext| format!(".{ext}"));
    files.iter().any(|file| {
        let lower = file.to_lowercase();
        let Some(rest) = lower.strip_prefix(&format!("{}-", stem.to_lowercase())) else {
            return false;
        };
        let Some(middle) = rest.strip_suffix(&ext.to_lowercase()) else {
            return false;
        };
        copy_suffix(middle)
    })
}

/// `wp_get_default_extension_for_mime_type()`.
fn default_extension(mime: &str) -> Option<&'static str> {
    let (pattern, _) = mime_types().iter().find(|(_, known)| known == mime)?;
    pattern.split('|').next()
}

/// `wp_unique_filename( $dir, $filename )` for a folder of the uploads
/// that holds `files`: the sanitized name, numbered past any file of that
/// name (or its lower case extension's) and past the copies WordPress
/// makes of an image, a HEIC's JPEG included.
pub fn wp_unique_filename(filename: &[u8], files: &[String], unfiltered_html: bool) -> String {
    let mut filename = sanitize_file_name(filename, unfiltered_html);
    let mut number: Option<u64> = None;
    let (stem, ext) = pathinfo(&filename);
    let (stem, ext) = (
        stem.to_string(),
        ext.map_or(String::new(), |ext| format!(".{ext}")),
    );
    let ending = |number: Option<u64>| number.map_or(String::new(), |n| n.to_string());
    if !stem.is_empty() && stem != "0" && size_suffixed(&stem) {
        number = Some(1);
        filename = filename.replace(&format!("{stem}{ext}"), &format!("{stem}-1{ext}"));
    }
    let mime = wp_check_filetype(&filename, unfiltered_html).map(|(_, mime)| mime);
    let lc_ext = ext.to_lowercase();
    let mut lc_filename = (lc_ext != ext).then(|| {
        format!(
            "{}{lc_ext}",
            filename.strip_suffix(&ext).unwrap_or(&filename)
        )
    });
    // An empty name is the folder itself, which exists.
    let exists = |name: &str| name.is_empty() || files.iter().any(|file| file == name);
    while exists(&filename) || lc_filename.as_deref().is_some_and(exists) {
        let next = number.unwrap_or(0) + 1;
        let n = ending(number);
        lc_filename = lc_filename.map(|lc| {
            str_replace(
                &lc,
                [&format!("-{n}{lc_ext}"), &format!("{n}{lc_ext}")],
                &format!("-{next}{lc_ext}"),
            )
        });
        filename = if n.is_empty() && ext.is_empty() {
            format!("{filename}-{next}")
        } else {
            str_replace(
                &filename,
                [&format!("-{n}{ext}"), &format!("{n}{ext}")],
                &format!("-{next}{ext}"),
            )
        };
        number = Some(next);
    }
    if let Some(lc) = lc_filename {
        filename = lc;
    }
    let bump = |filename: &str, number: Option<u64>, ext: &str| {
        let n = ending(number);
        let next = number.unwrap_or(0) + 1;
        str_replace(
            filename,
            [&format!("-{n}{ext}"), &format!("{n}{ext}")],
            &format!("-{next}{ext}"),
        )
    };
    // Each number the loops below try frees one file at most, so they end
    // within PHP's bound of the folder's file count.
    // A sanitized name never starts with a dot: it is never only `$ext`.
    if !ext.is_empty() {
        while existing_copy(&filename, files) {
            filename = bump(&filename, number, &lc_ext);
            number = Some(number.unwrap_or(0) + 1);
        }
    }
    // A HEIC's JPEG, and a JPEG's HEIC kin, may not take a name in use.
    let heics = [
        "image/heic",
        "image/heif",
        "image/heic-sequence",
        "image/heif-sequence",
    ];
    let alt_types: Vec<&str> = match mime {
        Some(mime) if heics.contains(&mime) => heics
            .iter()
            .copied()
            .chain(["image/jpeg"])
            .filter(|t| *t != mime)
            .collect(),
        Some("image/jpeg") => heics.to_vec(),
        _ => Vec::new(),
    };
    let mut alts: Vec<(String, String)> = alt_types
        .iter()
        .filter_map(|t| default_extension(t))
        .map(|alt| {
            let alt = format!(".{alt}");
            let name = format!(
                "{}{alt}",
                filename.strip_suffix(&lc_ext).unwrap_or(&filename)
            );
            (alt, name)
        })
        .collect();
    if !alts.is_empty() {
        alts.push((lc_ext.clone(), filename.clone()));
        let taken = |alts: &[(String, String)]| {
            alts.iter()
                .any(|(_, name)| exists(name) || existing_copy(name, files))
        };
        while taken(&alts) {
            for (alt, name) in &mut alts {
                *name = bump(name, number, alt);
            }
            filename = bump(&filename, number, &lc_ext);
            number = Some(number.unwrap_or(0) + 1);
        }
    }
    filename
}

/// Whether a file name ends as a copy's does: `-WxH`, `-scaled` or
/// `-rotated`.
fn size_suffixed(stem: &str) -> bool {
    stem.rsplit_once('-')
        .is_some_and(|(_, suffix)| copy_suffix(suffix))
}

/// `\d+x\d+|scaled|rotated`.
fn copy_suffix(suffix: &str) -> bool {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let size = suffix
        .split_once('x')
        .is_some_and(|(w, h)| digits(w) && digits(h));
    size || suffix == "scaled" || suffix == "rotated"
}

#[cfg(test)]
mod tests;
