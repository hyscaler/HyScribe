// SPDX-License-Identifier: AGPL-3.0-or-later

//! What `reference/attachment-edit/content.sql` adds to the default
//! install: its users, posts and post meta.

use hyscribe_model::{Post, User};

/// A user the content adds, or the fixture's administrator.
pub(crate) fn user(id: u64) -> Option<User> {
    let (login, display_name): (&[u8], &[u8]) = match id {
        1 => (b"admin", b"admin"),
        2 => (b"locker", b"Lock &amp; Holder"),
        3 => (b"sub", b""),
        _ => return None,
    };
    let email = if id == 1 {
        b"admin@default-install.test".to_vec()
    } else {
        [login, b"@example.test"].concat()
    };
    Some(User {
        id,
        login: login.to_vec(),
        nicename: login.to_vec(),
        email,
        display_name: display_name.to_vec(),
        ..User::default()
    })
}

/// The posts the content adds.
pub(crate) fn posts() -> Vec<Post> {
    let post = |id, author, date: &str, (title, excerpt, content): (&[u8], &[u8], &[u8])| Post {
        id,
        author,
        date: date.into(),
        date_gmt: date.into(),
        modified: date.into(),
        modified_gmt: date.into(),
        title: title.to_vec(),
        excerpt: excerpt.to_vec(),
        content: content.to_vec(),
        status: "inherit".into(),
        comment_status: "closed".into(),
        ping_status: "closed".into(),
        post_type: "attachment".into(),
        ..Post::default()
    };
    let texts = (
        &b"Sunset \"dusk\" &amp; sea"[..],
        &b"A caption with <em>markup</em> &amp; more"[..],
        &b"Description &amp; <b>bold</b> \"quoted\""[..],
    );
    let sunset = Post {
        name: b"sunset-dusk-sea".to_vec(),
        comment_status: "open".into(),
        ping_status: "open".into(),
        parent: 1,
        mime_type: "image/jpeg".into(),
        comment_count: 1,
        ..post(801, 2, "2026-03-04 05:06:07", texts)
    };
    let report = Post {
        name: b"report".to_vec(),
        parent: 1,
        mime_type: "application/pdf".into(),
        ..post(802, 1, "2025-12-31 23:59:00", (b"Report", b"", b""))
    };
    let notes = Post {
        name: b"notes".to_vec(),
        comment_status: "open".into(),
        parent: 804,
        mime_type: "text/plain".into(),
        ..post(803, 3, "2025-12-01 08:00:00", (b"Notes", b"", b""))
    };
    let secret = Post {
        name: b"secret-plans".to_vec(),
        status: "private".into(),
        comment_status: "open".into(),
        ping_status: "open".into(),
        post_type: "post".into(),
        ..post(
            804,
            2,
            "2025-12-01 07:00:00",
            (b"Secret &amp; plans", b"", b"Hidden"),
        )
    };
    let dot = Post {
        parent: 806,
        mime_type: "image/png".into(),
        ..post(805, 99, "2024-01-02 03:04:05", (b"", b"", b""))
    };
    let draft = Post {
        status: "draft".into(),
        post_type: "post".into(),
        ..post(806, 1, "2024-01-01 00:00:00", (b"", b"", b""))
    };
    let logo = Post {
        name: b"logo".to_vec(),
        comment_status: "open".into(),
        mime_type: "image/svg+xml".into(),
        ..post(807, 1, "2024-02-03 04:05:06", (b"Logo", b"", b""))
    };
    let data = Post {
        comment_status: "open".into(),
        ping_status: "open".into(),
        name: b"data".to_vec(),
        parent: 809,
        mime_type: "text/csv".into(),
        ..post(808, 1, "2024-03-04 05:06:07", (b"Data", b"", b""))
    };
    let waiting = Post {
        status: "pending".into(),
        post_type: "post".into(),
        ..post(809, 1, "2024-03-04 05:00:00", (b"Waiting", b"", b""))
    };
    vec![
        sunset, report, notes, secret, dot, draft, logo, data, waiting,
    ]
}

/// Attachments the screen does not port, which no golden records: audio,
/// video, trashed, scaled, sizeless, and one on a post in the Trash.
pub(crate) fn unported() -> Vec<Post> {
    let mut posts: Vec<Post> = [
        (901, "audio/mpeg", "inherit"),
        (902, "video/mp4", "inherit"),
        (903, "image/png", "trash"),
        (904, "image/png", "inherit"),
        (905, "application/pdf", "inherit"),
        (906, "image/png", "inherit"),
    ]
    .into_iter()
    .map(|(id, mime_type, status)| Post {
        id,
        author: 1,
        status: status.into(),
        parent: if id == 906 { 907 } else { 0 },
        post_type: "attachment".into(),
        mime_type: mime_type.into(),
        ..Post::default()
    })
    .collect();
    posts.push(Post {
        id: 907,
        status: "trash".into(),
        post_type: "post".into(),
        ..Post::default()
    });
    posts
}

/// The post meta the content adds, and the unported attachments'.
pub(crate) fn meta(id: u64, key: &str) -> Option<Vec<u8>> {
    let value: &[u8] = match (id, key) {
        (901..=903 | 906, "_wp_attachment_metadata") => b"a:1:{s:8:\"filesize\";i:1;}",
        (904, "_wp_attachment_metadata") => {
            b"a:2:{s:8:\"filesize\";i:1;s:14:\"original_image\";s:5:\"a.png\";}"
        }
        (801, "_wp_attached_file") => b"2026/03/sunset.jpg",
        (801, "_wp_attachment_metadata") => b"a:6:{s:5:\"width\";i:1200;s:6:\"height\";i:600;s:4:\"file\";s:18:\"2026/03/sunset.jpg\";s:8:\"filesize\";i:345678;s:5:\"sizes\";a:4:{s:6:\"medium\";a:5:{s:4:\"file\";s:18:\"sunset-300x150.jpg\";s:5:\"width\";i:300;s:6:\"height\";i:150;s:9:\"mime-type\";s:10:\"image/jpeg\";s:8:\"filesize\";i:12345;}s:5:\"large\";a:5:{s:4:\"file\";s:19:\"sunset-1024x512.jpg\";s:5:\"width\";i:1024;s:6:\"height\";i:512;s:9:\"mime-type\";s:10:\"image/jpeg\";s:8:\"filesize\";i:123456;}s:9:\"thumbnail\";a:5:{s:4:\"file\";s:18:\"sunset-150x150.jpg\";s:5:\"width\";i:150;s:6:\"height\";i:150;s:9:\"mime-type\";s:10:\"image/jpeg\";s:8:\"filesize\";i:5432;}s:12:\"medium_large\";a:5:{s:4:\"file\";s:18:\"sunset-768x384.jpg\";s:5:\"width\";i:768;s:6:\"height\";i:384;s:9:\"mime-type\";s:10:\"image/jpeg\";s:8:\"filesize\";i:65432;}}s:10:\"image_meta\";a:0:{}}",
        (801, "_wp_attachment_image_alt") => b"Alt \"text\" &amp; more",
        (802, "_wp_attached_file") => b"2025/12/report.pdf",
        (802, "_wp_attachment_metadata") => {
            b"a:2:{s:8:\"filesize\";i:1536000;s:9:\"mime_type\";s:17:\"application/x-pdf\";}"
        }
        (803, "_wp_attached_file") => b"2025/12/notes",
        (803, "_wp_attachment_metadata") => b"a:1:{s:8:\"filesize\";i:512;}",
        (805, "_wp_attached_file") => b"2024/01/dot.png",
        (805, "_wp_attachment_metadata") => b"a:6:{s:5:\"width\";i:40;s:6:\"height\";i:30;s:4:\"file\";s:15:\"2024/01/dot.png\";s:8:\"filesize\";i:2048;s:5:\"sizes\";a:0:{}s:10:\"image_meta\";a:0:{}}",
        (807, "_wp_attached_file") => b"2024/02/logo.svg",
        (807, "_wp_attachment_metadata") => b"a:1:{s:8:\"filesize\";i:3072;}",
        (808, "_wp_attached_file") => b"2024/03/data.csv",
        (808, "_wp_attachment_metadata") => b"a:1:{s:8:\"filesize\";i:100;}",
        _ => return None,
    };
    Some(value.to_vec())
}
