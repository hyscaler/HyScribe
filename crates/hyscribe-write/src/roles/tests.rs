// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::tests::{check, fails_at_each_step, rolled_back, text};

fn caps(names: &[(&str, bool)]) -> Caps {
    let caps = names
        .iter()
        .map(|(cap, grant)| ((*cap).to_string(), *grant));
    caps.collect()
}

#[tokio::test]
async fn adds_roles() {
    check(
        "roles-add",
        async |_| (),
        async |write, ()| {
            let granted = caps(&[
                ("read", true),
                ("oracle_cap", true),
                ("edit_posts", false),
                ("read", false),
            ]);
            let added = add_role(write, "oracle_manager", "Oracle Manager", &granted);
            assert!(added.await.unwrap());
            assert!(!add_role(write, "editor", "Again", &[]).await.unwrap());
            assert!(!add_role(write, "0", "Empty name", &[]).await.unwrap());
            assert!(add_role(write, "oracle_empty", "Empty", &[]).await.unwrap());
            let read = get_role(write, "oracle_manager").await.unwrap();
            let expected = caps(&[("read", false), ("oracle_cap", true), ("edit_posts", false)]);
            assert_eq!(read, Some(expected));
        },
    )
    .await;
}

#[tokio::test]
async fn gives_and_takes_capabilities() {
    check(
        "roles-caps",
        async |_| (),
        async |write, ()| {
            let mut set = async |role, cap, grant| set_cap(write, role, cap, grant).await.unwrap();
            assert!(set("administrator", "oracle_cap", Some(true)).await);
            assert!(set("editor", "oracle_cap", Some(false)).await);
            assert!(set("author", "upload_files", None).await);
            assert!(set("author", "oracle_none", None).await);
            assert!(set("contributor", "read", Some(true)).await);
            assert!(!set("oracle_missing", "oracle_cap", Some(true)).await);
        },
    )
    .await;
}

#[tokio::test]
async fn removes_roles() {
    let setup = async |write: &mut Write<'_>| {
        let granted = caps(&[("read", true)]);
        add_role(write, "oracle_gone", "Gone", &granted)
            .await
            .unwrap();
        let role = text("oracle_gone");
        update_option(write, "default_role", &role, None)
            .await
            .unwrap();
    };
    check("roles-remove", setup, async |write, ()| {
        assert!(remove_role(write, "oracle_gone").await.unwrap());
        assert!(!remove_role(write, "oracle_none").await.unwrap());
        assert!(remove_role(write, "contributor").await.unwrap());
    })
    .await;
}

/// Roles stored in shapes WordPress never writes: no capabilities, or not
/// an array at all, and no roles option.
#[tokio::test]
async fn odd_roles() {
    rolled_back(async |write| {
        let roles = Value::Array(vec![
            (key("bare"), Value::Array(vec![(key("name"), text("Bare"))])),
            (key("flat"), text("flat")),
            (key("nulled"), Value::Null),
            (
                key("odd"),
                Value::Array(vec![
                    (key("extra"), Value::Array(Vec::new())),
                    (key("capabilities"), Value::Array(Vec::new())),
                ]),
            ),
        ]);
        update_option(write, "wp_user_roles", &roles, None)
            .await
            .unwrap();
        assert_eq!(get_role(write, "bare").await.unwrap(), Some(Vec::new()));
        assert_eq!(get_role(write, "nulled").await.unwrap(), None);
        assert!(set_cap(write, "bare", "read", Some(true)).await.unwrap());
        assert_eq!(
            get_role(write, "bare").await.unwrap(),
            Some(caps(&[("read", true)]))
        );
        assert!(!set_cap(write, "flat", "read", Some(true)).await.unwrap());
        assert!(set_cap(write, "odd", "read", Some(true)).await.unwrap());
        assert_eq!(
            get_role(write, "odd").await.unwrap(),
            Some(caps(&[("read", true)]))
        );
        // A removed role the default role does not name leaves it as it is.
        update_option(write, "default_role", &text("odd"), None)
            .await
            .unwrap();
        assert!(remove_role(write, "bare").await.unwrap());
        assert_eq!(
            get_option(write, "default_role").await.unwrap(),
            text("odd")
        );
        crate::options::delete_option(write, "wp_user_roles")
            .await
            .unwrap();
        assert_eq!(get_role(write, "bare").await.unwrap(), None);
        assert!(add_role(write, "first", "First", &[]).await.unwrap());
        assert_eq!(get_role(write, "first").await.unwrap(), Some(Vec::new()));
    })
    .await;
}

#[tokio::test]
async fn roles_return_each_database_error() {
    let setup = async |write: &mut Write<'_>| {
        let role = text("contributor");
        update_option(write, "default_role", &role, None)
            .await
            .unwrap();
    };
    fails_at_each_step(setup, async |write, ()| {
        get_role(write, "editor").await?;
        add_role(write, "oracle_new", "New", &[]).await?;
        set_cap(write, "oracle_new", "read", Some(true)).await?;
        remove_role(write, "contributor").await?;
        Ok(())
    })
    .await;
}
