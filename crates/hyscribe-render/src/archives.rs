// SPDX-License-Identifier: AGPL-3.0-or-later

//! The archives block, `render_block_core_archives()`, over
//! `wp_get_archives()`.

use hyscribe_kses::{esc_attr, esc_url, strip_tags};
use hyscribe_model::{DateCount, OrderBy, Period, PostQuery};
use hyscribe_phpser::{Key, Value, is_empty_string};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::date::{self, Zone};
use crate::formatting::{CORE_SHORTCODE_TAGS, wptexturize};
use crate::head::json_hex_tag;
use crate::permalinks::{date_archive_link, permalink};
use crate::post::title_filters;
use crate::site::Site;
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

/// The dropdown's script up to its arguments, as the block's PHP template
/// prints it.
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
                b"weekly" => "Select Week",
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
                dropdown_script(
                    self.context.site,
                    SCRIPT,
                    "block_core_archives_build_dropdown_script",
                    &id
                ),
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
        let site = self.context.site;
        let option = |name| site.option(name).unwrap_or_default();
        let period = match kind {
            b"yearly" => Period::Year,
            b"monthly" | b"" => Period::Month,
            b"daily" => Period::Day,
            b"weekly" => Period::Week(Value::String(option("start_of_week")).to_int()),
            b"postbypost" | b"alpha" => {
                let current = self.context.request.post;
                return post_links(site, current, kind == b"alpha", format);
            }
            _ => return Vec::new(),
        };
        let request = self.context.request;
        let var = |name| {
            request
                .vars
                .get_str(name)
                .map_or(0, hyscribe_phpser::absint)
        };
        let (year, month, day, week) = (var("year"), var("monthnum"), var("day"), var("w"));
        let date_format = String::from_utf8_lossy(&option("date_format")).into_owned();
        let zone = self.context.zone();
        let home = site.home_url();
        let mut last_week = None;
        let mut out = Vec::new();
        for count in site.date_counts(period) {
            let DateCount {
                year: y,
                month: m,
                day: d,
                week: w,
                posts,
            } = count;
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
                Period::Week(start) => {
                    // A week number that follows itself is listed once.
                    if last_week.replace(w) == Some(w) {
                        continue;
                    }
                    let (start, end) = week_start_end(y, m, d, start);
                    // date_i18n(): a timestamp's UTC time, as the site's.
                    let local = |unix| {
                        let stored = date::format_unix("Y-m-d H:i:s", unix, Zone::Utc);
                        date::format(&date_format, &stored, zone)
                    };
                    (
                        [&home[..], format!("/?m={y}&w={w}").as_bytes()].concat(),
                        format!("{}&#8211;{}", local(start), local(end)),
                        year == y && week == w,
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
}

/// `get_weekstartend()` of the day `year`-`month`-`day`: the Unix times
/// the week from the `start` day around it begins and ends, in UTC.
fn week_start_end(year: u64, month: u64, day: u64, start: i64) -> (i64, i64) {
    let number = |n: u64| i64::try_from(n).unwrap_or_default();
    let day = date::days_from_civil(number(year), number(month), number(day));
    let weekday = date::weekday(day);
    let weekday = if weekday < start {
        weekday + 7
    } else {
        weekday
    };
    let begins = (day - (weekday - start)) * 86_400;
    (begins, begins + 604_800 - 1)
}

/// The `postbypost` and `alpha` lists: published posts newest first, or
/// by title, the global post's marked current.
fn post_links(site: &dyn Site, current: Option<u64>, alpha: bool, format: Format) -> Vec<u8> {
    let order = if alpha {
        vec![(OrderBy::Title, true)]
    } else {
        vec![(OrderBy::Date, false), (OrderBy::Id, false)]
    };
    let query = PostQuery {
        post_types: vec!["post".into()],
        order,
        limit: u32::MAX,
        ..PostQuery::default()
    };
    let posts = site.query(&query);
    posts
        .iter()
        .filter(|post| post.date != "0000-00-00 00:00:00")
        .flat_map(|post| {
            let text = if is_empty_string(&post.title) {
                post.id.to_string().into_bytes()
            } else {
                strip_tags(&title_filters(&post.title))
            };
            let selected = current == Some(post.id);
            archives_link(&permalink(site, post), &text, format, "", selected)
        })
        .collect()
}

/// A block's inline dropdown script, `script` called with the select's
/// `id` and the home URL, as `wp_get_inline_script_tag()` prints it with
/// the PHP function `source` as its source URL.
pub(crate) fn dropdown_script(site: &dyn Site, script: &str, source: &str, id: &str) -> String {
    let exports = Value::Array(vec![
        (Key::Int(0), Value::String(id.into())),
        (Key::Int(1), Value::String(site.home_url())),
    ]);
    let json = String::from_utf8_lossy(&json_hex_tag(&exports)).into_owned();
    format!("<script>\n{script}{json} );\n//# sourceURL={source}\n</script>\n")
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
