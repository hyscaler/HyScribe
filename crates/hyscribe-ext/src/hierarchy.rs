// SPDX-License-Identifier: AGPL-3.0-or-later

//! The classic template hierarchy (ADR 0023): the template file a classic
//! theme renders for a request, as PHP's template loader picks it. The
//! loader asks each true conditional tag's `get_*_template()` in turn for
//! the first of its names `locate_template()` finds among the theme's
//! files, then `index.php`: the first of the request's candidates, in the
//! order the block hierarchy lists them, that the theme has as a PHP file.

use crate::TemplateFile;

/// The first of `candidates` (template names without `.php`, a page
/// template's path among them) that `files` has; none when it has none.
pub fn locate<'a>(candidates: &[String], files: &'a [TemplateFile]) -> Option<&'a str> {
    candidates.iter().find_map(|candidate| {
        let found = files.iter().find(|file| {
            let stem = file.file.strip_suffix(".php");
            stem == Some(candidate.as_str())
        });
        found.map(|file| file.file.as_str())
    })
}

#[cfg(test)]
mod tests;
