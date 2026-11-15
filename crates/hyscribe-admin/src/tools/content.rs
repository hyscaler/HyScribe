// SPDX-License-Identifier: AGPL-3.0-or-later

//! What `reference/privacy-requests/content.sql` adds to the default
//! install, at the test site's clock: its requests and their meta, and the
//! requests a query selects.

use hyscribe_model::{OrderBy, Post, PostQuery};

/// The test site's clock, as the dates made at recording time stand.
const NOW: &str = "2026-11-12 16:13:20";

/// The requests the content adds.
pub(crate) fn posts() -> Vec<Post> {
    [
        (
            861,
            "pending@example.test",
            "request-pending",
            "2026-01-02 03:04:05",
        ),
        (
            862,
            "admin@default-install.test",
            "request-confirmed",
            "2026-01-03 13:14:15",
        ),
        (
            863,
            "done@example.test",
            "request-completed",
            "2025-12-24 23:00:00",
        ),
        (
            864,
            "failed@example.test",
            "request-failed",
            "2025-11-30 08:00:00",
        ),
        (
            865,
            "Another.Pending@example.test",
            "request-pending",
            "2026-11-12 15:13:20",
        ),
        (
            871,
            "erase-pending@example.test",
            "request-pending",
            "2026-01-05 10:00:00",
        ),
        (
            872,
            "erase-confirmed@example.test",
            "request-confirmed",
            "2026-01-06 10:00:00",
        ),
        (
            873,
            "erase-done@example.test",
            "request-completed",
            "2026-01-07 10:00:00",
        ),
        (
            874,
            "erase-failed@example.test",
            "request-failed",
            "2026-01-08 10:00:00",
        ),
    ]
    .into_iter()
    .map(|(id, email, status, date)| Post {
        id,
        title: email.as_bytes().to_vec(),
        status: status.into(),
        name: if id < 870 {
            b"export_personal_data".to_vec()
        } else {
            b"remove_personal_data".to_vec()
        },
        date: date.into(),
        date_gmt: date.into(),
        modified_gmt: NOW.into(),
        post_type: "user_request".into(),
        ..Post::default()
    })
    .collect()
}

/// The confirmation and completion times the content keeps.
pub(crate) fn meta(id: u64, key: &str) -> Option<Vec<u8>> {
    let value = match (id, key) {
        (862, "_wp_user_request_confirmed_timestamp") => "1794492800",
        (863, "_wp_user_request_confirmed_timestamp") => "1766620800",
        (863, "_wp_user_request_completed_timestamp") => "1766707200",
        (872, "_wp_user_request_confirmed_timestamp") => "1767744000",
        (873, "_wp_user_request_confirmed_timestamp") => "1767830400",
        (873, "_wp_user_request_completed_timestamp") => "1767873600",
        _ => return None,
    };
    Some(value.as_bytes().to_vec())
}

/// The requests of `posts` that `query` selects, in its order, and how
/// many it matches; a search matches the address in any case.
pub(crate) fn list(posts: Vec<Post>, query: &PostQuery) -> (Vec<Post>, u64) {
    let lower = |post: &Post| post.title.to_ascii_lowercase();
    let mut found: Vec<Post> = posts
        .into_iter()
        .filter(|post| {
            query.post_types.contains(&post.post_type)
                && query.names.iter().all(|name| name.as_bytes() == post.name)
                && query
                    .statuses
                    .iter()
                    .any(|filter| filter.status == post.status)
                && query.search.iter().all(|term| {
                    let term = term.to_ascii_lowercase();
                    lower(post)
                        .windows(term.len())
                        .any(|w| w == term.as_bytes())
                })
        })
        .collect();
    // Every request a search finds matches its words alike.
    found.sort_by(|a, b| {
        query
            .order
            .iter()
            .filter(|(column, _)| *column != OrderBy::Relevance)
            .map(|&(column, ascending)| {
                let order = match column {
                    OrderBy::Title => lower(a).cmp(&lower(b)),
                    _ => a.date.cmp(&b.date),
                };
                if ascending { order } else { order.reverse() }
            })
            .find(|order| order.is_ne())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let total = found.len() as u64;
    let page = found
        .into_iter()
        .skip(query.offset as usize)
        .take(query.limit as usize)
        .collect();
    (page, total)
}
