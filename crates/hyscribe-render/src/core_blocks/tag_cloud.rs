// SPDX-License-Identifier: AGPL-3.0-or-later

//! The tag cloud block, `render_block_core_tag_cloud()`, over
//! `wp_tag_cloud()` and `wp_generate_tag_cloud()`.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_phpser::{Value, absint, float_to_string, string_to_float, strnatcasecmp};

use super::{Block, Context, number_format};
use crate::categories::keep_used;
use crate::permalinks::term_link;
use crate::post::post_format_string;

/// `render_block_core_tag_cloud()`: nothing without tags.
pub(super) fn tag_cloud(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let text = |name| {
        block
            .attribute(name)
            .map(Value::to_php_string)
            .unwrap_or_default()
    };
    let taxonomy = String::from_utf8_lossy(&text("taxonomy")).into_owned();
    // get_term_link() fails for a taxonomy that is not registered, and
    // wp_tag_cloud() then prints nothing.
    if !matches!(taxonomy.as_str(), "post_tag" | "category" | "post_format") {
        return Vec::new();
    }
    let smallest_text = text("smallestFontSize");
    let unit = font_unit(&smallest_text);
    let smallest = string_to_float(&smallest_text);
    let largest = string_to_float(&text("largestFontSize"));
    // The attribute's schema keeps it from 1 to 100.
    let number = block.attribute("numberOfTags").map_or(45, absint);
    let show_count = block
        .attribute("showTagCounts")
        .is_some_and(|v| !v.is_empty());
    let mut tags = if taxonomy == "category" {
        // A tree's query is not limited: empty terms with a used one below
        // stay, then the most used are kept.
        let all = context.site.taxonomy_terms(&taxonomy);
        let mut used = all.clone();
        keep_used(&all, &mut used);
        used.sort_by_key(|term| std::cmp::Reverse(term.count));
        used.truncate(usize::try_from(number).unwrap_or(usize::MAX));
        used
    } else {
        context.site.cloud_terms(&taxonomy, number)
    };
    if tags.is_empty() {
        return Vec::new();
    }
    // _post_format_get_terms(): a format's name is its label.
    if taxonomy == "post_format" {
        for term in &mut tags {
            term.name = post_format_string(&term.slug).as_bytes().to_vec();
        }
    }
    // A stable sort by name, as uasort() keeps ties in order.
    tags.sort_by(|a, b| strnatcasecmp(&a.name, &b.name));
    let scaled: Vec<i64> = tags.iter().map(|term| scale(term.count)).collect();
    let min = scaled.iter().copied().min().unwrap_or_default();
    let max = scaled.iter().copied().max().unwrap_or_default();
    let spread = (max - min).max(1);
    let font_spread = largest - smallest;
    let font_spread = if font_spread < 0.0 { 1.0 } else { font_spread };
    let step = font_spread / spread as f64;
    let links: Vec<Vec<u8>> = tags
        .iter()
        .zip(&scaled)
        .enumerate()
        .map(|(position, (term, &scaled))| {
            let count = term.count;
            let label = if count == 1 { "item" } else { "items" };
            let formatted = format!("{} {label}", number_format(count));
            let size = smallest + (scaled - min) as f64 * step;
            let size = format!("{}{unit}", float_to_string(size));
            let shown = if show_count {
                format!("<span class=\"tag-link-count\"> ({count})</span>")
            } else {
                String::new()
            };
            // The aria-label always shows: wp_generate_tag_cloud() compares
            // the float font spread with the integer 0.
            [
                &b"<a href=\""[..],
                &esc_url(&term_link(context.site, &taxonomy, term)),
                format!(
                    "\" class=\"tag-cloud-link tag-link-{} tag-link-position-{}\" style=\"font-size: {};\" aria-label=\"{} ({})\">",
                    term.id,
                    position + 1,
                    esc_attr(size.as_bytes()),
                    esc_attr(&term.name),
                    esc_attr(formatted.as_bytes()),
                )
                .as_bytes(),
                esc_html(&term.name).as_bytes(),
                shown.as_bytes(),
                b"</a>",
            ]
            .concat()
        })
        .collect();
    let wrapper = block.wrapper(context, b"");
    [
        format!("<p {wrapper}>").as_bytes(),
        &links.join(&b'\n'),
        b"</p>",
    ]
    .concat()
}

/// The unit of the smallest font size, as `/^[0-9.]+(?P<unit>[a-z%]+)$/i`
/// finds it (`$` also before a final newline), else `pt`.
fn font_unit(size: &[u8]) -> String {
    let size = size.strip_suffix(b"\n").unwrap_or(size);
    let number = size
        .iter()
        .take_while(|b| b.is_ascii_digit() || **b == b'.')
        .count();
    let unit = size.get(number..).unwrap_or_default();
    let valid = number > 0
        && !unit.is_empty()
        && unit.iter().all(|b| b.is_ascii_alphabetic() || *b == b'%');
    if valid {
        String::from_utf8_lossy(unit).into_owned()
    } else {
        "pt".into()
    }
}

/// `default_topic_count_scale()`: `round( log10( $count + 1 ) * 100 )`.
fn scale(count: u64) -> i64 {
    ((count as f64 + 1.0).log10() * 100.0).round() as i64
}

#[cfg(test)]
mod tests;
