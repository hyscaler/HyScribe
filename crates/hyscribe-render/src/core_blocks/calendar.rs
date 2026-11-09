// SPDX-License-Identifier: AGPL-3.0-or-later

//! The calendar block, `render_block_core_calendar()`, over
//! `get_calendar()`: the month the request asks for, or this month, with
//! its days with posts linked and the months with posts around it.

use hyscribe_kses::esc_attr;
use hyscribe_phpser::{Key, Value, is_empty_string};

use super::{Block, Context};
use crate::date::{self, Zone, days_from_civil, weekday};
use crate::permalinks::date_archive_link;
use crate::style::get_styles;

/// `render_block_core_calendar()`: nothing on a site with no published
/// posts, which tells a visitor nothing either.
pub(super) fn calendar(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let site = context.site;
    // block_core_calendar_has_published_posts(), which the option caches;
    // without it, read here and not stored.
    let published = match site.option("wp_calendar_block_has_published_posts") {
        Some(value) => !is_empty_string(&value),
        None => site.has_published_posts(),
    };
    if !published {
        return Vec::new();
    }
    let (year, month) = shown_month(context, block);
    let table = get_calendar(context, year, month);
    let (styles, classes) = color_styles(block);
    let table = table
        .replacen("<table", &format!("<table{styles}"), 1)
        .replacen(
            "class=\"wp-calendar-table",
            &format!("class=\"wp-calendar-table{classes}"),
            1,
        );
    format!("<div {}>{table}</div>", block.wrapper(context, b"")).into_bytes()
}

/// The month `get_calendar()` shows: the request's `monthnum` and
/// `year`, which the block's own attributes replace under permalinks with
/// both; else the month of week `w` of `m`'s year; else `m`'s; else now's.
fn shown_month(context: &Context<'_>, block: &Block<'_>) -> (i64, i64) {
    let request = context.request;
    let var = |name| request.vars.get_str(name).map_or(0, |v| v.to_int());
    let (mut month, mut year) = (var("monthnum").abs(), var("year").abs());
    let structure = context
        .site
        .option("permalink_structure")
        .unwrap_or_default();
    let has = |tag: &[u8]| structure.windows(tag.len()).any(|w| w == tag);
    if let (Some(m), Some(y)) = (block.attribute("month"), block.attribute("year"))
        && has(b"%monthnum%")
        && has(b"%year%")
    {
        (month, year) = (m.to_int(), y.to_int());
    }
    if month != 0 && year != 0 {
        return (year, month);
    }
    // WP_Query keeps only `m`'s digits.
    let m: Vec<u8> = request
        .vars
        .get_str("m")
        .map(Value::to_php_string)
        .unwrap_or_default()
        .into_iter()
        .filter(u8::is_ascii_digit)
        .collect();
    let digits = |range: std::ops::Range<usize>| {
        Value::String(m.get(range).unwrap_or_default().to_vec()).to_int()
    };
    let w = request.get.get_str("w").map_or(0, Value::to_int);
    if w != 0 {
        // MySQL's DATE_ADD() from 1 January of `m`'s year.
        let year = digits(0..4.min(m.len()));
        let day = days_from_civil(year, 1, 1) + (w - 1) * 7 + 6;
        let stored = date::format_unix("Y-m-d H:i:s", day * 86_400, Zone::Utc);
        return (
            year,
            date::format("n", &stored, Zone::Utc)
                .parse()
                .unwrap_or_default(),
        );
    }
    if !m.is_empty() {
        let month = if m.len() < 6 { 1 } else { digits(4..6) };
        return (digits(0..4.min(m.len())), month);
    }
    let now = |format| date::format_unix(format, request.now, context.zone());
    (
        now("Y").parse().unwrap_or_default(),
        now("n").parse().unwrap_or_default(),
    )
}

/// `get_calendar( true, false )` for `month` of `year`.
fn get_calendar(context: &Context<'_>, year: i64, month: i64) -> String {
    let site = context.site;
    let week_begins = Value::String(site.option("start_of_week").unwrap_or_default()).to_int();
    // mktime() of the 1st carries a month past 12 into the next year.
    let (first_year, first_month) = (
        year + (month - 1).div_euclid(12),
        (month - 1).rem_euclid(12) + 1,
    );
    let first = days_from_civil(first_year, first_month, 1);
    let next_first = days_from_civil(first_year + first_month / 12, first_month % 12 + 1, 1);
    let days_in_month = next_first - first;
    let data = site.calendar_month(year, month.unsigned_abs(), days_in_month.unsigned_abs());
    let mut out = format!(
        "<table id=\"wp-calendar\" class=\"wp-calendar-table\">\n\t<caption>{} {:04}</caption>\n\t<thead>\n\t<tr>",
        month_name(month),
        first_year
    );
    for count in 0..7 {
        let name = weekday_name((count + week_begins) % 7);
        let initial = name.get(..1).unwrap_or_default();
        out.push_str(&format!(
            "\n\t\t<th scope=\"col\" aria-label=\"{}\">{initial}</th>",
            esc_attr(name.as_bytes())
        ));
    }
    out.push_str("\n\t</tr>\n\t</thead>\n\t<tbody>\n\t<tr>");
    let pad = (weekday(first) - week_begins).rem_euclid(7);
    if pad > 0 {
        out.push_str(&format!(
            "\n\t\t<td colspan=\"{pad}\" class=\"pad\">&nbsp;</td>"
        ));
    }
    let today = |format| date::format_unix(format, context.request.now, context.zone());
    let (today_day, today_month, today_year) = (today("j"), today("n"), today("Y"));
    let mut new_row = false;
    for day in 1..=days_in_month {
        if new_row {
            out.push_str("\n\t</tr>\n\t<tr>\n\t\t");
        }
        let is_today = today_day == day.to_string()
            && today_month == month.to_string()
            && today_year == year.to_string();
        out.push_str(if is_today {
            "<td id=\"today\">"
        } else {
            "<td>"
        });
        if data.days.contains(&day.unsigned_abs()) {
            let stored = format!("{first_year:04}-{first_month:02}-{day:02} 00:00:00");
            let label = format!(
                "Posts published on {}",
                date::format("F j, Y", &stored, Zone::Utc)
            );
            let link = date_archive_link(
                site,
                year.unsigned_abs(),
                Some((month.unsigned_abs(), Some(day.unsigned_abs()))),
            );
            out.push_str(&format!(
                "<a href=\"{}\" aria-label=\"{}\">{day}</a>",
                String::from_utf8_lossy(&link),
                esc_attr(label.as_bytes())
            ));
        } else {
            out.push_str(&day.to_string());
        }
        out.push_str("</td>");
        new_row = (weekday(first + day - 1) - week_begins).rem_euclid(7) == 6;
    }
    let pad = 7 - (weekday(next_first) - week_begins).rem_euclid(7);
    if pad < 7 {
        out.push_str(&format!(
            "\n\t\t<td class=\"pad\" colspan=\"{pad}\">&nbsp;</td>"
        ));
    }
    out.push_str("\n\t</tr>\n\t</tbody>\n\t</table><nav aria-label=\"Previous and next months\" class=\"wp-calendar-nav\">");
    let month_link = |(year, month): (u64, u64)| {
        String::from_utf8_lossy(&date_archive_link(site, year, Some((month, None)))).into_owned()
    };
    let abbrev = |month: u64| {
        month_name(month.cast_signed())
            .get(..3)
            .unwrap_or_default()
            .to_string()
    };
    match data.previous {
        Some(previous) => out.push_str(&format!(
            "\n\t\t<span class=\"wp-calendar-nav-prev\"><a href=\"{}\">&laquo; {}</a></span>",
            month_link(previous),
            abbrev(previous.1)
        )),
        None => out.push_str("\n\t\t<span class=\"wp-calendar-nav-prev\">&nbsp;</span>"),
    }
    out.push_str("\n\t\t<span class=\"pad\">&nbsp;</span>");
    match data.next {
        Some(next) => out.push_str(&format!(
            "\n\t\t<span class=\"wp-calendar-nav-next\"><a href=\"{}\">{} &raquo;</a></span>",
            month_link(next),
            abbrev(next.1)
        )),
        None => out.push_str("\n\t\t<span class=\"wp-calendar-nav-next\">&nbsp;</span>"),
    }
    out.push_str("\n\t</nav>");
    out
}

/// `$wp_locale->get_month( $month )`: its name, empty past 12.
fn month_name(month: i64) -> String {
    if !(1..=12).contains(&month) {
        return String::new();
    }
    date::format("F", &format!("2000-{month:02}-01 00:00:00"), Zone::Utc)
}

/// `$wp_locale->get_weekday( $day )`, 0 for Sunday: empty for a day of a
/// negative `start_of_week`.
fn weekday_name(day: i64) -> String {
    if !(0..7).contains(&day) {
        return String::new();
    }
    // 4 January 1970 was a Sunday.
    date::format_unix("l", (3 + day) * 86_400, Zone::Utc)
}

/// The color styles and classes the block puts on its table: a named or
/// custom text and background color, as `wp_style_engine_get_styles()`
/// gives them, and the link color class.
fn color_styles(block: &Block<'_>) -> (String, String) {
    let color = |named: &str, custom: &str| match block.attribute(named) {
        Some(slug) => Value::String([b"var:preset|color|", &slug.to_php_string()[..]].concat()),
        None => block
            .attributes
            .get_path(&["style", "color", custom])
            .cloned()
            .unwrap_or(Value::Null),
    };
    let colors = Value::Array(vec![(
        Key::String(b"color".to_vec()),
        Value::Array(vec![
            (Key::String(b"text".to_vec()), color("textColor", "text")),
            (
                Key::String(b"background".to_vec()),
                color("backgroundColor", "background"),
            ),
        ]),
    )]);
    let styles = get_styles(&colors, true);
    let css = if styles.css.is_empty() {
        String::new()
    } else {
        format!(" style=\"{}\"", esc_attr(&styles.css))
    };
    let mut classes = if styles.classnames.is_empty() {
        String::new()
    } else {
        format!(" {}", esc_attr(styles.classnames.as_bytes()))
    };
    if block.has_link_color() {
        classes.push_str(" has-link-color");
    }
    (css, classes)
}

#[cfg(test)]
mod tests;
