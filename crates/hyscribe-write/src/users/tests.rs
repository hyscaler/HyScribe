// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_auth::check_password;
use hyscribe_db::Connection;
use hyscribe_phpser::Key;

use super::*;
use crate::options::delete_option;
use crate::tests::{check, fails_at_each_step, rolled_back, text, writer};

fn some(text: &str) -> Option<Vec<u8>> {
    Some(text.as_bytes().to_vec())
}

/// The oracle's `$new_user`: a login, a pass phrase and an email.
fn new_user(login: &str) -> UserData {
    UserData {
        user_login: some(login),
        user_pass: some("A pass phrase"),
        user_email: some(&format!("{login}@example.test")),
        ..UserData::default()
    }
}

async fn insert_ok(write: &mut Write<'_>, data: UserData) -> u64 {
    wp_insert_user(write, &data).await.unwrap()
}

fn refused(result: Result<u64, Error>, code: &str) -> bool {
    matches!(result, Err(Error::Refused(refused)) if refused == code)
}

#[tokio::test]
async fn inserts_users_of_each_role() {
    check(
        "user-insert-roles",
        async |_| (),
        async |write, ()| {
            insert_ok(write, new_user("oracle-subscriber")).await;
            let editor = UserData {
                role: some("editor"),
                first_name: some("Ada"),
                last_name: some("Lovelace"),
                ..new_user("oracle-editor")
            };
            insert_ok(write, editor).await;
            let author = UserData {
                role: some("author"),
                first_name: some("Only First"),
                ..new_user("oracle-author")
            };
            insert_ok(write, author).await;
            let contributor = UserData {
                role: some("contributor"),
                last_name: some("Only Last"),
                ..new_user("oracle-contributor")
            };
            insert_ok(write, contributor).await;
            let admin = UserData {
                role: some("administrator"),
                nickname: some("Boss"),
                display_name: some("The Boss"),
                ..new_user("oracle-admin")
            };
            insert_ok(write, admin).await;
            let jane = UserData {
                user_email: some("jane@example.test"),
                ..new_user("Jane Doe")
            };
            insert_ok(write, jane).await;
            let jane = UserData {
                user_email: some("jane.doe@example.test"),
                role: some(""),
                ..new_user("jane.doe")
            };
            insert_ok(write, jane).await;
            insert_ok(write, new_user("jane-doe")).await;
            let taken = UserData {
                user_email: some("other@example.test"),
                ..new_user("oracle-editor")
            };
            let taken = wp_insert_user(write, &taken).await;
            assert!(refused(taken, "existing_user_login"));
            let taken = UserData {
                user_email: some("oracle-editor@example.test"),
                ..new_user("oracle-other")
            };
            let taken = wp_insert_user(write, &taken).await;
            assert!(refused(taken, "existing_user_email"));
            let empty = wp_insert_user(write, &new_user("<b></b>")).await;
            assert!(refused(empty, "empty_user_login"));
        },
    )
    .await;
}

#[tokio::test]
async fn inserts_a_user_with_every_field() {
    check(
        "user-insert-every-field",
        async |_| (),
        async |write, ()| {
            let full = UserData {
                user_email: some(" full@example.test "),
                user_nicename: some("Full Name"),
                user_url: some("https://example.test/<b>me</b>?a=1&b=2"),
                user_registered: some("2020-01-02 03:04:05"),
                user_activation_key: some("123:abc"),
                display_name: some("Full <em>Display</em>"),
                nickname: some("O'Neil & <b>co</b>"),
                first_name: some("Back\\slash"),
                last_name: some("Tab\tand  space"),
                description: some(
                    "<a href=\"https://example.test\" rel=\"me\" target=\"_blank\" \
                 onclick=\"x()\">Me</a> & <b>bold</b> <script>x</script>",
                ),
                rich_editing: some("false"),
                syntax_highlighting: some("false"),
                infinite_scrolling: some("false"),
                comment_shortcuts: some("yes"),
                admin_color: some("fresh!<x>"),
                use_ssl: some("1"),
                show_admin_bar_front: some("false"),
                locale: some("fr_FR"),
                role: some("author"),
                meta_input: vec![
                    ("oracle_custom".into(), text("one")),
                    (
                        "oracle_list".into(),
                        Value::Array(vec![
                            (Key::Int(0), Value::Int(1)),
                            (Key::Int(1), text("two")),
                        ]),
                    ),
                ],
                ..new_user("  Oracle\\Full!  ")
            };
            insert_ok(write, full).await;
        },
    )
    .await;
}

async fn setup_user(write: &mut Write<'_>, data: UserData) -> u64 {
    insert_ok(write, data).await
}

#[tokio::test]
async fn updates_a_user() {
    let old = UserData {
        first_name: some("Old"),
        description: some("Old \\ bio"),
        user_activation_key: some("1:key"),
        ..new_user("oracle-update")
    };
    check(
        "user-update",
        async |write| setup_user(write, old).await,
        async |write, id| {
            let new = UserData {
                id,
                first_name: some("New"),
                user_email: some("new@example.test"),
                admin_color: some("light"),
                role: some("editor"),
                ..UserData::default()
            };
            assert_eq!(wp_update_user(write, &new).await.unwrap(), id);
        },
    )
    .await;
}

#[tokio::test]
async fn updates_a_password() {
    let old = UserData {
        user_activation_key: some("1:key"),
        ..new_user("oracle-password")
    };
    let setup = async |write: &mut Write<'_>| setup_user(write, old).await;
    check("user-update-password", setup, async |write, id| {
        let new = UserData {
            id,
            user_pass: some("Another pass phrase"),
            ..UserData::default()
        };
        wp_update_user(write, &new).await.unwrap();
        let hash = write.user(id).await.unwrap().unwrap().1.pass;
        assert!(check_password(b"Another pass phrase", hash.as_bytes()));
        // The same hash again is kept, not hashed again.
        let same = UserData {
            id,
            user_pass: Some(hash.clone().into_bytes()),
            ..UserData::default()
        };
        wp_update_user(write, &same).await.unwrap();
        assert_eq!(write.user(id).await.unwrap().unwrap().1.pass, hash);
    })
    .await;
}

#[tokio::test]
async fn sets_a_role() {
    let setup = async |write: &mut Write<'_>| setup_user(write, new_user("oracle-role")).await;
    check("user-set-role", setup, async |write, id| {
        set_role(write, id, b"subscriber").await.unwrap();
        set_role(write, id, b"author").await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn adds_and_removes_roles() {
    let setup = async |write: &mut Write<'_>| setup_user(write, new_user("oracle-roles")).await;
    check("user-add-remove-role", setup, async |write, id| {
        add_role(write, id, b"editor").await.unwrap();
        add_role(write, id, b"editor").await.unwrap();
        remove_role(write, id, b"subscriber").await.unwrap();
        remove_role(write, id, b"author").await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn keeps_an_unknown_role_as_a_capability() {
    let setup = async |write: &mut Write<'_>| setup_user(write, new_user("oracle-ghost")).await;
    check("user-unknown-role", setup, async |write, id| {
        add_role(write, id, b"").await.unwrap();
        set_role(write, id, b"ghost").await.unwrap();
        set_role(write, id, b"ghost").await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn deletes_a_user() {
    let setup = async |write: &mut Write<'_>| setup_user(write, new_user("oracle-delete")).await;
    check("user-delete", setup, async |write, id| {
        assert!(wp_delete_user(write, id, None).await.unwrap());
    })
    .await;
}

#[tokio::test]
async fn deletes_a_user_passing_its_posts_on() {
    let setup = async |write: &mut Write<'_>| setup_user(write, new_user("oracle-heir")).await;
    check("user-delete-reassign", setup, async |write, heir| {
        assert!(wp_delete_user(write, 1, Some(heir)).await.unwrap());
    })
    .await;
}

#[tokio::test]
async fn refuses_what_php_refuses() {
    rolled_back(async |write| {
        let unknown = UserData {
            id: 999_999,
            ..new_user("oracle-unknown")
        };
        assert!(refused(
            wp_insert_user(write, &unknown).await,
            "invalid_user_id"
        ));
        assert!(refused(
            wp_update_user(write, &unknown).await,
            "invalid_user_id"
        ));
        let long = new_user(&"a".repeat(61));
        assert!(refused(
            wp_insert_user(write, &long).await,
            "user_login_too_long"
        ));
        let nicename = UserData {
            user_nicename: some("%%"),
            ..new_user("oracle-nicename")
        };
        let empty = wp_insert_user(write, &nicename).await;
        assert!(refused(empty, "empty_user_nicename"));
        let nicename = UserData {
            user_nicename: some(&"n".repeat(51)),
            ..new_user("oracle-nicename")
        };
        let long = wp_insert_user(write, &nicename).await;
        assert!(refused(long, "user_nicename_too_long"));
        let url = UserData {
            user_url: some(&format!("https://example.test/{}", "u".repeat(80))),
            ..new_user("oracle-url")
        };
        assert!(refused(
            wp_insert_user(write, &url).await,
            "user_url_too_long"
        ));
        assert!(!wp_delete_user(write, 999_999, None).await.unwrap());
    })
    .await;
}

#[tokio::test]
async fn an_update_through_insert_hashes_the_password() {
    rolled_back(async |write| {
        let id = insert_ok(write, new_user("oracle-insert-update")).await;
        let update = UserData {
            id,
            user_pass: some("Changed"),
            ..new_user("oracle-insert-update")
        };
        assert_eq!(insert_ok(write, update).await, id);
        let (_, user) = write.user(id).await.unwrap().unwrap();
        assert!(check_password(b"Changed", user.pass.as_bytes()));
        // Updated, not inserted: the display name falls back to the login.
        assert_eq!(user.display_name, b"oracle-insert-update");
        let kept = UserData {
            id,
            user_pass: None,
            ..new_user("oracle-insert-update")
        };
        insert_ok(write, kept).await;
        assert_eq!(write.user(id).await.unwrap().unwrap().1.pass, user.pass);
    })
    .await;
}

#[tokio::test]
async fn posts_and_links_wait_for_their_ports() {
    let _one = crate::tests::ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut conn = <Connection as sqlx::Connection>::connect_with(&options())
        .await
        .unwrap();
    let mut outer = sqlx::Connection::begin(&mut conn).await.unwrap();
    let mut write = writer.begin_on(&mut outer).await.unwrap();
    let author = insert_ok(&mut write, new_user("oracle-author")).await;
    write.reassign_posts(1, author).await.unwrap();
    let deleted = wp_delete_user(&mut write, author, None).await;
    assert!(matches!(deleted, Err(Error::Unported("wp_delete_post"))));
    write.reassign_posts(author, 1).await.unwrap();
    write.commit().await.unwrap();
    sqlx::query("INSERT INTO wp_links (link_url, link_name, link_owner, link_notes) VALUES ('u', 'n', ?, '')")
        .bind(author)
        .execute(&mut *outer)
        .await
        .unwrap();
    let mut write = writer.begin_on(&mut outer).await.unwrap();
    let deleted = wp_delete_user(&mut write, author, None).await;
    assert!(matches!(deleted, Err(Error::Unported("wp_delete_link"))));
}

fn options() -> sqlx::mysql::MySqlConnectOptions {
    let db = hyscribe_testkit::fixture_database();
    sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose())
}

#[tokio::test]
async fn the_user_count_stops_at_ten_thousand() {
    rolled_back(async |write| {
        for (known, expected) in [("10000", "2"), ("10001", "10001")] {
            write
                .update_option("user_count", known.as_bytes())
                .await
                .unwrap();
            let id = insert_ok(write, new_user(&format!("oracle-count-{known}"))).await;
            let count = write.option_value("user_count").await.unwrap().unwrap();
            assert_eq!(count, expected.as_bytes(), "{known}");
            assert!(wp_delete_user(write, id, None).await.unwrap());
        }
        delete_option(write, "user_count").await.unwrap();
        insert_ok(write, new_user("oracle-uncounted")).await;
        let count = write.option_value("user_count").await.unwrap().unwrap();
        assert_eq!(count, b"2");
    })
    .await;
}

#[tokio::test]
async fn levels_come_from_level_keys() {
    let key = |name: &[u8]| Key::String(name.to_vec());
    assert_eq!(roles::level(&key(b"level_10")), Some(10));
    assert_eq!(roles::level(&key(b"LEVEL_7\n")), Some(7));
    assert_eq!(roles::level(&key(b"level_11")), None);
    assert_eq!(roles::level(&key(b"level_")), None);
    assert_eq!(roles::level(&key(b"levels_1")), None);
    assert_eq!(roles::level(&Key::Int(5)), None);
}

#[tokio::test]
async fn new_users_return_each_database_error() {
    let setup = async |write: &mut Write<'_>| insert_ok(write, new_user("Jane Doe")).await;
    fails_at_each_step(setup, async |write, _| {
        let jane = UserData {
            role: some("editor"),
            // A core key takes the new value in its place, as `array_merge()` gives it.
            meta_input: vec![("nickname".into(), text("Janey"))],
            ..new_user("jane.doe")
        };
        wp_insert_user(write, &jane).await?;
        wp_insert_user(write, &new_user("oracle-default")).await?;
        Ok(())
    })
    .await;
}

#[tokio::test]
async fn changes_to_users_return_each_database_error() {
    let setup = async |write: &mut Write<'_>| insert_ok(write, new_user("oracle-change")).await;
    fails_at_each_step(setup, async |write, id| {
        let new = UserData {
            id,
            first_name: some("New"),
            ..UserData::default()
        };
        wp_update_user(write, &new).await?;
        add_role(write, id, b"editor").await?;
        remove_role(write, id, b"editor").await?;
        wp_delete_user(write, id, None).await?;
        wp_delete_user(write, 1, Some(999_999)).await?;
        Ok(())
    })
    .await;
}
