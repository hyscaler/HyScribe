// SPDX-License-Identifier: AGPL-3.0-or-later

//! The archives block, `render_block_core_archives()`, over
//! `wp_get_archives()`'s yearly, monthly and daily lists.

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::{DateCount, Period};
use hyscribe_phpser::{Key, Value, json_encode};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::date;
use crate::formatting::{CORE_SHORTCODE_TAGS, wptexturize};
use crate::permalinks::date_archive_link;
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

/// The dropdown's script between `( ( [ dropdownId, homeUrl ] ) => {` and
/// its arguments, as the block's PHP template prints it.
const SCRIPT: &str = "( ( [ dropdownId, homeUrl ] ) => {
\t\tconst dropdown = document.getElementById( dropdownId );
\t\tfunction onSelectChange() {
\t\t\tsetTimeout( () => {
\t\t\t\tif ( 'escape' === dropdown.dataset.lastkey ) {
\t\t\t\t\treturn;
\t\t\t\t}
\t\t\t\tif ( dropdown.value ) {
\t\t\t\t\tlocation.href = dropdown.value;
\t\t\t\t}
\t\t\t}, 250 );
\t\t}
\t\tfunction onKeyUp( event ) {
\t\t\tif ( 'Escape' === event.key ) {
\t\t\t\tdropdown.dataset.lastkey = 'escape';
\t\t\t} else {
\t\t\t\tdelete dropdown.dataset.lastkey;
\t\t\t}
\t\t}
\t\tfunction onClick() {
\t\t\tdelete dropdown.dataset.lastkey;
\t\t}
\t\tdropdown.addEventListener( 'keyup', onKeyUp );
\t\tdropdown.addEventListener( 'click', onClick );
\t\tdropdown.addEventListener( 'change', onSelectChange );
\t} )( ";

impl Renderer<'_> {
    /// `render_block_core_archives()`.
    pub(crate) fn archives(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/archives", parsed);
        let flag = |name| attributes.get_str(name).is_some_and(|v| !v.is_empty());
        let kind = attributes
            .get_str("type")
            .map(Value::to_php_string)
            .unwrap_or_default();
        let dropdown = flag("displayAsDropdown");
        let format = if dropdown {
            Format::Option
        } else {
            Format::Html
        };
        let archives = self.archive_links(&kind, format, flag("showPostCounts"));
        let class = if dropdown {
            "wp-block-archives-dropdown"
        } else {
            "wp-block-archives-list"
        };
        let extra = [("class", Value::String(class.into()))];
        let theme = &self.context.theme.global;
        let wrapper = get_block_wrapper_attributes("core/archives", parsed, &extra, theme);
        if dropdown {
            let id = self.unique_id("wp-block-archives-");
            let label = match kind.as_slice() {
                b"yearly" => "Select Year",
                b"monthly" => "Select Month",
                b"daily" => "Select Day",
                _ => "Select Post",
            };
            let hidden = if flag("showLabel") {
                ""
            } else {
                " screen-reader-text"
            };
            let id = String::from_utf8_lossy(&id).into_owned();
            let content = format!(
                "<label for=\"{id}\" class=\"wp-block-archives__label{hidden}\">Archives</label>\n\t\t<select id=\"{}\" name=\"archive-dropdown\">\n\t\t<option value=\"\">{label}</option>{}</select>{}",
                esc_attr(id.as_bytes()),
                String::from_utf8_lossy(&archives),
                self.dropdown_script(&id),
            );
            return format!("<div {wrapper}>{content}</div>").into_bytes();
        }
        if archives.is_empty() {
            return format!("<div {wrapper}>No archives to show.</div>").into_bytes();
        }
        [format!("<ul {wrapper}>").as_bytes(), &archives, b"</ul>"].concat()
    }

    /// `wp_get_archives()` of `kind` in `format`, `''` as monthly.
    fn archive_links(&self, kind: &[u8], format: Format, show_post_count: bool) -> Vec<u8> {
        let period = match kind {
            b"yearly" => Period::Year,
            b"monthly" | b"" => Period::Month,
            b"daily" => Period::Day,
            _ => return Vec::new(),
        };
        let site = self.context.site;
        let request = self.context.request;
        let var = |name| {
            request
                .vars
                .get_str(name)
                .map_or(0, hyscribe_phpser::absint)
        };
        let (year, month, day) = (var("year"), var("monthnum"), var("day"));
        let date_format =
            String::from_utf8_lossy(&site.option("date_format").unwrap_or_default()).into_owned();
        let zone = self.context.zone();
        let mut out = Vec::new();
        for DateCount {
            year: y,
            month: m,
            day: d,
            posts,
        } in site.date_counts(period)
        {
            let (link, text, selected) = match period {
                Period::Year => (date_archive_link(site, y, None), y.to_string(), year == y),
                Period::Month => {
                    let first = format!("{y:04}-{m:02}-01 00:00:00");
                    let name = date::format("F", &first, zone);
                    (
                        date_archive_link(site, y, Some((m, None))),
                        format!("{name} {y}"),
                        year == y && month == m,
                    )
                }
                Period::Day => {
                    let stored = format!("{y}-{m:02}-{d:02} 00:00:00");
                    (
                        date_archive_link(site, y, Some((m, Some(d)))),
                        date::format(&date_format, &stored, zone),
                        year == y && month == m && day == d,
                    )
                }
            };
            let after = if show_post_count {
                format!("&nbsp;({posts})")
            } else {
                String::new()
            };
            let selected = request.archive.is_some() && selected;
            out.extend(archives_link(
                &link,
                text.as_bytes(),
                format,
                &after,
                selected,
            ));
        }
        out
    }

    /// `block_core_archives_build_dropdown_script()`, inline.
    fn dropdown_script(&self, id: &str) -> String {
        let exports = Value::Array(vec![
            (Key::Int(0), Value::String(id.into())),
            (Key::Int(1), Value::String(self.context.site.home_url())),
        ]);
        // JSON_HEX_TAG | JSON_UNESCAPED_SLASHES.
        let json = String::from_utf8_lossy(&json_encode(&exports).unwrap_or_default())
            .replace("\\/", "/")
            .replace('<', "\\u003C")
            .replace('>', "\\u003E");
        format!(
            "<script>\n{SCRIPT}{json} );\n//# sourceURL=block_core_archives_build_dropdown_script\n</script>\n"
        )
    }
}

/// The formats of [`archives_link`].
#[derive(Clone, Copy)]
enum Format {
    Html,
    Option,
}

/// `get_archives_link()`.
fn archives_link(url: &[u8], text: &[u8], format: Format, after: &str, selected: bool) -> Vec<u8> {
    let text = String::from_utf8_lossy(&wptexturize(text, &CORE_SHORTCODE_TAGS)).into_owned();
    let url = String::from_utf8_lossy(&esc_url(url)).into_owned();
    match format {
        Format::Option => {
            let selected = if selected { " selected='selected'" } else { "" };
            format!("\t<option value='{url}'{selected}> {text} {after}</option>\n")
        }
        Format::Html => {
            let current = if selected {
                " aria-current=\"page\""
            } else {
                ""
            };
            format!("\t<li><a href='{url}'{current}>{text}</a>{after}</li>\n")
        }
    }
    .into_bytes()
}

#[cfg(test)]
mod tests;
