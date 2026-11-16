// SPDX-License-Identifier: AGPL-3.0-or-later

//! Autosaves past what the REST goldens send (`reference/rest/autosaves.list`):
//! every form of a post lock, and an autosave that copies its post's
//! footnotes.

use hyscribe_phpser::Value;

use super::*;
use crate::posts::{Context, wp_insert_post};
use crate::tests::{admin, rolled_back};

fn text(text: &str) -> Value {
    Value::String(text.as_bytes().to_vec())
}

/// The time the lock tests run at, fixed so no second passes between.
const NOW: i64 = 1_800_000_000;

/// A context for user 2, who does not exist, so the fixture's
/// administrator can hold a lock against them.
fn other() -> Context {
    Context {
        user: 2,
        now: NOW,
        ..admin()
    }
}

/// What user 2's autosave of their own draft writes with the `_edit_lock`
/// `lock` and the `_edit_last` `last`: whether the draft itself.
async fn in_place(lock: Option<String>, last: Option<&str>) -> Result<bool> {
    let mut found = Err(Error::Refused("not run"));
    rolled_back(async |w| {
        found = async {
            let ctx = other();
            let draft = PostArgs {
                title: b"Draft".to_vec(),
                status: "draft".into(),
                author: Some(2),
                ..PostArgs::default()
            };
            let id = wp_insert_post(w, &ctx, draft).await?;
            for (key, value) in [("_edit_lock", lock.as_deref()), ("_edit_last", last)] {
                if let Some(value) = value {
                    add_metadata(w, Meta::Post, id, key, &text(value), false).await?;
                }
            }
            let fields = Revisioned {
                title: Some(b"Changed".to_vec()),
                ..Revisioned::default()
            };
            let change = |args: &mut PostArgs| fields.set(args);
            let written = wp_create_post_autosave(w, &ctx, id, &fields, &change).await?;
            Ok(written == id)
        }
        .await;
    })
    .await;
    found
}

/// `wp_check_post_lock()`: only another existing user's lock taken within
/// 150 seconds holds, the user from `_edit_last` when the lock names none;
/// a lock time that is not a number is not ported.
#[tokio::test]
async fn locks_are_checked_as_php_checks_them() {
    let now = NOW;
    let lock = |time: i64, user: &str| Some(format!("{time}:{user}"));
    assert!(in_place(None, None).await.unwrap());
    assert!(in_place(Some(String::new()), None).await.unwrap());
    assert!(!in_place(lock(now, "1"), None).await.unwrap());
    assert!(!in_place(lock(now - 149, "1"), None).await.unwrap());
    assert!(in_place(lock(now - 150, "1"), None).await.unwrap());
    assert!(in_place(lock(now, "2"), None).await.unwrap());
    assert!(in_place(lock(now, "3"), None).await.unwrap());
    assert!(in_place(lock(now, "x"), None).await.unwrap());
    assert!(in_place(lock(now, "0"), None).await.unwrap());
    assert!(in_place(Some("0:1".into()), None).await.unwrap());
    assert!(!in_place(Some(now.to_string()), Some("1")).await.unwrap());
    assert!(in_place(Some(now.to_string()), None).await.unwrap());
    let unported = in_place(Some("soon:1".into()), None).await;
    assert!(matches!(unported, Err(Error::Unported(_))));
}

/// Another user's autosave of a published post: a new revision with the
/// post's footnotes, left as it is while nothing changes, then updated.
#[tokio::test]
async fn autosaves_are_one_per_user() {
    rolled_back(async |w| {
        let ctx = other();
        let post = PostArgs {
            title: b"Published".to_vec(),
            content: b"Words".to_vec(),
            status: "publish".into(),
            author: Some(1),
            ..PostArgs::default()
        };
        let id = wp_insert_post(w, &admin(), post).await.unwrap();
        let notes = text(r#"[{"content":"A note","id":"1"}]"#);
        add_metadata(w, Meta::Post, id, "footnotes", &notes, false)
            .await
            .unwrap();
        let mut save = async |title: &str, content: Option<&str>| {
            let fields = Revisioned {
                title: Some(title.as_bytes().to_vec()),
                content: content.map(|content| content.as_bytes().to_vec()),
                excerpt: None,
            };
            let change = |args: &mut PostArgs| fields.set(args);
            wp_create_post_autosave(w, &ctx, id, &fields, &change)
                .await
                .unwrap()
        };
        let autosave = save("Published", Some("Words")).await;
        assert_ne!(autosave, id);
        assert_eq!(save("Published", Some("  Words ")).await, autosave);
        assert_eq!(save("Published", None).await, autosave);
        assert_eq!(save("Renamed", None).await, autosave);
        let saved = w.post(autosave).await.unwrap().unwrap();
        assert_eq!(
            (saved.title, saved.content, saved.name, saved.author),
            (
                b"Renamed".to_vec(),
                b"Words".to_vec(),
                format!("{id}-autosave-v1").into_bytes(),
                2
            )
        );
        let copied = get_metadata(w, Meta::Post, autosave, "footnotes").await;
        assert_eq!(copied.unwrap(), vec![notes]);
    })
    .await;
}
