// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Privacy_Requests_Table` and its export and erasure lists: the views,
//! the query of `prepare_items()` and the rows.

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::{Post, PostQuery, StatusFilter};
use hyscribe_phpser::{Value, add_query_arg};
use hyscribe_render::date::{Zone, format, format_unix, human_time_diff};
use hyscribe_render::formatting::sanitize_text_field;
use hyscribe_render::{number_format, order_by, search_terms};

use super::{Requests, ToolsSite};
use crate::admin_bar::BarSite;
use crate::list_table::{Column, View, pagenum, row_actions, view_list};
use crate::page::query_strings;
use crate::text;

/// `get_post_stati()`, which `WP_Query` takes a status filter from.
const STATI: [&str; 12] = [
    "publish",
    "future",
    "draft",
    "pending",
    "private",
    "trash",
    "auto-draft",
    "inherit",
    "request-pending",
    "request-confirmed",
    "request-failed",
    "request-completed",
];

/// `_wp_privacy_statuses()`, with their labels.
const PRIVACY_STATI: [(&str, &str); 4] = [
    ("request-pending", "Pending"),
    ("request-confirmed", "Confirmed"),
    ("request-failed", "Failed"),
    ("request-completed", "Completed"),
];

/// The screens' arguments: `filter-status`, `orderby`, `order`, `s` and
/// `paged`, as given.
#[derive(Default)]
pub struct RequestsArgs {
    pub(super) filter: Option<String>,
    pub(super) orderby: Option<String>,
    pub(super) order: Option<String>,
    pub(super) search: Option<String>,
    pub(super) paged: Option<String>,
}

impl RequestsArgs {
    /// The arguments of the query string `query`, `None` with one that is
    /// not ported, such as an action's.
    pub fn parse(query: &str) -> Option<RequestsArgs> {
        let mut args = RequestsArgs::default();
        for (key, value) in query_strings(query)? {
            let slot = match key.as_str() {
                "filter-status" => &mut args.filter,
                "orderby" => &mut args.orderby,
                "order" => &mut args.order,
                "s" => &mut args.search,
                "paged" => &mut args.paged,
                _ => return None,
            };
            *slot = Some(value);
        }
        Some(args)
    }

    /// `sanitize_text_field()` of an argument, empty when not given.
    pub(super) fn sanitized(value: Option<&String>) -> String {
        text(&sanitize_text_field(
            value.map_or(&b""[..], |v| v.as_bytes()),
        ))
    }
}

/// What a list shows: its requests, how many match, the counts of each
/// status and the sanitized `filter-status`.
pub(super) struct Listing {
    pub(super) items: Vec<Post>,
    pub(super) total: u64,
    pub(super) counts: Vec<(String, u64)>,
    pub(super) filter: String,
}

/// Whether a pending request is older than a day, which
/// `_wp_personal_data_cleanup_requests()` marks failed with a write.
pub(super) fn stale(site: &dyn ToolsSite) -> bool {
    let query = PostQuery {
        post_types: vec!["user_request".into()],
        statuses: vec![StatusFilter {
            status: "request-pending".into(),
            author: None,
            post_type: None,
        }],
        limit: u32::MAX,
        ..PostQuery::default()
    };
    let before = format_unix("Y-m-d H:i:s", site.clock().1 - 86_400, Zone::Utc);
    let (pending, _) = site.list_posts(&query);
    pending.iter().any(|post| post.modified_gmt < before)
}

/// The statuses of `present` `WP_Query` lists for `filter-status`: the named
/// registered ones, or all but `any` and none leave out the Trash and drafts.
pub(super) fn statuses(filter: &str, present: &[(String, u64)]) -> Vec<String> {
    let named: Vec<&str> = filter.split(',').collect();
    let registered: Vec<String> = STATI
        .iter()
        .filter(|status| named.contains(status))
        .map(|status| (*status).to_string())
        .collect();
    let any = Value::String(filter.into()).is_empty() || named.contains(&"any");
    if !any && !registered.is_empty() {
        return registered;
    }
    present
        .iter()
        .map(|(status, _)| status.clone())
        .filter(|status| !any || !["trash", "auto-draft"].contains(&status.as_str()))
        .collect()
}

/// `prepare_items()` of the requests of `action`, `per_page` a page.
pub(super) fn listing(
    site: &dyn ToolsSite,
    action: &str,
    args: &RequestsArgs,
    per_page: u64,
) -> Listing {
    let counts = site.named_status_counts("user_request", action);
    let filter = RequestsArgs::sanitized(args.filter.as_ref());
    let offset = (pagenum(args.paged.as_deref()) - 1).saturating_mul(per_page);
    let mut query = PostQuery {
        post_types: vec!["user_request".into()],
        names: vec![action.into()],
        statuses: statuses(&filter, &counts)
            .into_iter()
            .map(|status| StatusFilter {
                status,
                author: None,
                post_type: None,
            })
            .collect(),
        limit: u32::try_from(per_page).unwrap_or(u32::MAX),
        offset: u32::try_from(offset).unwrap_or(u32::MAX),
        ..PostQuery::default()
    };
    let search = RequestsArgs::sanitized(args.search.as_ref());
    // `parse_query()` empties a search over 1600 bytes. Addresses hold no
    // spaces, so every request found ranks alike and relevance orders none.
    if (1..=1600).contains(&search.len()) {
        query.search = search_terms(&search);
        query.logged_in = true;
    }
    let orderby = match args.orderby.as_deref() {
        Some("requester") => "post_title",
        _ => "post_date",
    };
    let ascending = args
        .order
        .as_deref()
        .is_some_and(|order| order.eq_ignore_ascii_case("asc"));
    query.order = order_by(&Value::String(orderby.into()), ascending, &query);
    let (items, found) = site.list_posts(&query);
    // `WP_Query` counts nothing when the page is past the last.
    let total = if items.is_empty() { 0 } else { found };
    Listing {
        items,
        total,
        counts,
        filter,
    }
}

/// `get_views()`: every request and each status that has some.
pub(super) fn views(site: &dyn ToolsSite, file: &str, listing: &Listing) -> String {
    let filter = listing.filter.as_str();
    let url = BarSite::admin_url(site, file);
    let total: u64 = listing.counts.iter().map(|(_, count)| count).sum();
    let mut views = vec![View {
        class: "all".into(),
        url: url.clone(),
        label: format!(
            "All <span class=\"count\">({})</span>",
            number_format(total)
        ),
        current: filter.is_empty(),
    }];
    for (status, label) in PRIVACY_STATI {
        let count = listing
            .counts
            .iter()
            .find(|(counted, _)| counted == status)
            .map_or(0, |(_, count)| *count);
        if count == 0 {
            continue;
        }
        let link = add_query_arg(
            &[(b"filter-status", Some(status.as_bytes()))],
            url.as_bytes(),
        );
        views.push(View {
            class: status.into(),
            url: text(&link),
            label: format!(
                "{label} <span class=\"count\">({})</span>",
                number_format(count)
            ),
            current: status == filter,
        });
    }
    view_list(&views)
}

/// `get_timestamp_as_date()`: how long ago, within a day, or the date and
/// time; empty for none.
fn timestamp_as_date(timestamp: i64, now: i64) -> String {
    if timestamp == 0 {
        return String::new();
    }
    if (0..86_400).contains(&(now - timestamp)) {
        return format!("{} ago", human_time_diff(timestamp, now));
    }
    format!(
        "{} at {}",
        format_unix("Y/m/d", timestamp, Zone::Utc),
        format_unix("g:i a", timestamp, Zone::Utc)
    )
}

/// The link of a bulk `action` on request `id` of `file`.
fn bulk_link(site: &dyn ToolsSite, file: &str, action: &str, id: u64) -> String {
    let url = BarSite::admin_url(
        site,
        &format!(
            "{file}?action={action}&request_id%5B0%5D={id}&_wpnonce={}",
            site.nonce("bulk-privacy_requests")
        ),
    );
    text(&esc_url(url.as_bytes()))
}

/// The markup that starts an export or an erasure of request `id` in the
/// browser, `count` the exporters or erasers.
fn handle(site: &dyn ToolsSite, requests: Requests, id: u64, class: &str) -> String {
    let (kind, counted, count) = match requests {
        Requests::Export => ("export", "exporters", 3),
        Requests::Erase => ("erase", "erasers", 1),
    };
    format!(
        "{class} data-{counted}-count=\"{count}\" data-request-id=\"{id}\" data-nonce=\"{}\">",
        site.nonce(&format!("wp-privacy-{kind}-personal-data-{id}"))
    )
}

/// `column_email()`: the address, and the export or erasure and completion
/// row actions.
fn column_email(site: &dyn ToolsSite, requests: Requests, file: &str, item: &Post) -> String {
    let (id, email) = (item.id, text(&item.title));
    let mut actions = Vec::new();
    match requests {
        Requests::Export => actions.push((
            "download-data",
            format!(
                "{}<span class=\"export-personal-data-idle\"><button type=\"button\" class=\"button-link export-personal-data-handle\">Download personal data</button></span><span class=\"export-personal-data-processing hidden\">Downloading data... <span class=\"export-progress\"></span></span><span class=\"export-personal-data-success hidden\"><button type=\"button\" class=\"button-link export-personal-data-handle\">Download personal data again</button></span><span class=\"export-personal-data-failed hidden\">Download failed. <button type=\"button\" class=\"button-link export-personal-data-handle\">Retry</button></span></span>",
                handle(site, requests, id, "<span class=\"export-personal-data\"")
            ),
        )),
        Requests::Erase if item.status != "request-confirmed" => actions.push((
            "remove-data",
            format!(
                "{}<span class=\"remove-personal-data-idle\"><button type=\"button\" class=\"button-link remove-personal-data-handle\">Force erase personal data</button></span><span class=\"remove-personal-data-processing hidden\">Erasing data... <span class=\"erasure-progress\"></span></span><span class=\"remove-personal-data-success hidden\">Erasure completed.</span><span class=\"remove-personal-data-failed hidden\">Force erasure has failed. <button type=\"button\" class=\"button-link remove-personal-data-handle\">Retry</button></span></span>",
                handle(
                    site,
                    requests,
                    id,
                    "<span class=\"remove-personal-data force-remove-personal-data\""
                )
            ),
        )),
        Requests::Erase => {}
    }
    if item.status != "request-completed" {
        let label = esc_attr(
            format!("Mark export request for &#8220;{email}&#8221; as completed.").as_bytes(),
        );
        actions.push((
            "complete-request",
            format!(
                "<span><a href=\"{}\" class=\"complete-request\" aria-label=\"{label}\">Complete request</a></span>",
                bulk_link(site, file, "complete", id)
            ),
        ));
    }
    let mailto = esc_url(format!("mailto:{email}").as_bytes());
    format!(
        "<a href=\"{}\">{email}</a> {}<button type=\"button\" class=\"toggle-row\"><span class=\"screen-reader-text\">Show more details</span></button>",
        text(&mailto),
        row_actions(&actions, false)
    )
}

/// `column_next_steps()`.
fn column_next_steps(site: &dyn ToolsSite, requests: Requests, file: &str, item: &Post) -> String {
    let id = item.id;
    match (item.status.as_str(), requests) {
        ("request-pending", _) => "Waiting for confirmation".into(),
        ("request-confirmed", Requests::Export) => format!(
            "{}\t\t\t\t<span class=\"export-personal-data-idle\"><button type=\"button\" class=\"button-link export-personal-data-handle\">Send export link</button></span>\n\t\t\t\t<span class=\"export-personal-data-processing hidden\">Sending email... <span class=\"export-progress\"></span></span>\n\t\t\t\t<span class=\"export-personal-data-success success-message hidden\">Email sent.</span>\n\t\t\t\t<span class=\"export-personal-data-failed hidden\">Email could not be sent. <button type=\"button\" class=\"button-link export-personal-data-handle\">Retry</button></span>\n\t\t\t\t</div>",
            handle(
                site,
                requests,
                id,
                "<div class=\"export-personal-data\" data-send-as-email=\"1\""
            )
        ),
        ("request-confirmed", Requests::Erase) => format!(
            "{}\t\t\t\t<span class=\"remove-personal-data-idle\"><button type=\"button\" class=\"button-link remove-personal-data-handle\">Erase personal data</button></span>\n\t\t\t\t<span class=\"remove-personal-data-processing hidden\">Erasing data... <span class=\"erasure-progress\"></span></span>\n\t\t\t\t<span class=\"remove-personal-data-success success-message hidden\" >Erasure completed.</span>\n\t\t\t\t<span class=\"remove-personal-data-failed hidden\">Data erasure has failed. <button type=\"button\" class=\"button-link remove-personal-data-handle\">Retry</button></span>\n\t\t\t\t</div>",
            handle(
                site,
                requests,
                id,
                "<div class=\"remove-personal-data\" data-force-erase=\"1\""
            )
        ),
        ("request-failed", _) => format!(
            "<button type=\"submit\" class=\"button-link\" name=\"privacy_action_email_retry[{id}]\" id=\"privacy_action_email_retry[{id}]\">Retry</button>"
        ),
        _ => format!(
            "<a href=\"{}\">Remove request</a>",
            bulk_link(site, file, "delete", id)
        ),
    }
}

/// `column_status()`: the status, with when it was confirmed or completed.
fn column_status(site: &dyn ToolsSite, item: &Post, label: &str, now: i64) -> String {
    let key = match item.status.as_str() {
        "request-confirmed" => "_wp_user_request_confirmed_timestamp",
        "request-completed" => "_wp_user_request_completed_timestamp",
        _ => "",
    };
    let stored = site.meta(item, key).unwrap_or_default();
    let date = timestamp_as_date(Value::String(stored).to_int(), now);
    let date = if date.is_empty() {
        date
    } else {
        format!("<span class=\"status-date\">{date}</span>")
    };
    format!(
        "<span class=\"status-label status-{}\">{label}{date}</span>",
        esc_attr(item.status.as_bytes())
    )
}

/// The rows of `items`, `None` with a request of a status not a request's,
/// which is not ported.
pub(super) fn rows(
    site: &dyn ToolsSite,
    requests: Requests,
    file: &str,
    (columns, hidden): (&[Column], &[&str]),
    items: &[Post],
) -> Option<String> {
    let now = site.clock().1;
    let mut out = String::new();
    for item in items {
        let id = item.id;
        let (_, label) = PRIVACY_STATI
            .iter()
            .find(|(status, _)| *status == item.status)?;
        let email = text(&item.title);
        out.push_str(&format!(
            "<tr id=\"request-{id}\" class=\"status-{}\">",
            esc_attr(item.status.as_bytes())
        ));
        for column in columns {
            let key = column.key;
            if key == "cb" {
                out.push_str(&format!(
                    "<td class=\"check-column\"><input type=\"checkbox\" name=\"request_id[]\" id=\"requester_{id}\" value=\"{id}\" /><label for=\"requester_{id}\"><span class=\"screen-reader-text\">Select {email}</span></label><span class=\"spinner\"></span></td>"
                ));
                continue;
            }
            let hidden = if hidden.contains(&key) { " hidden" } else { "" };
            let label_text = esc_attr(column.label.as_bytes());
            let cell = match key {
                "email" => column_email(site, requests, file, item),
                "status" => column_status(site, item, label, now),
                "created_timestamp" => {
                    let created = format("U", &item.date_gmt, Zone::Utc);
                    let created = Value::String(created.into_bytes()).to_int();
                    timestamp_as_date(created, now)
                }
                _ => column_next_steps(site, requests, file, item),
            };
            if column.primary {
                out.push_str(&format!(
                    "<th class='{key} column-{key} has-row-actions column-primary{hidden}' data-colname=\"{label_text}\" scope=\"row\" aria-label=\"{}\">{cell}</th>",
                    esc_attr(email.as_bytes())
                ));
            } else {
                out.push_str(&format!(
                    "<td class='{key} column-{key}{hidden}' data-colname=\"{label_text}\">{cell}</td>"
                ));
            }
        }
        out.push_str("</tr>");
    }
    Some(out)
}
