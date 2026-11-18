// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_auth::check_password;
use hyscribe_phpser::Key;

use super::*;
use crate::options::delete_option;
use crate::tests::{
    admin, check, connection, fails_at_each_step, nothing, rolled_back, text, writer,
};

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

fn refused<T>(result: Result<T, Error>, code: &str) -> bool {
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
            // The new address is told to the old one; no password was given.
            let updated = Updated {
                id,
                login: b"oracle-update".to_vec(),
                email: b"oracle-update@example.test".to_vec(),
                password_changed: false,
                new_email: some("new@example.test"),
            };
            assert_eq!(wp_update_user(write, &new).await.unwrap(), updated);
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
        let updated = wp_update_user(write, &new).await.unwrap();
        assert!(updated.password_changed);
        assert_eq!(updated.new_email, None);
        let hash = write.user(id).await.unwrap().unwrap().1.pass;
        assert!(check_password(b"Another pass phrase", hash.as_bytes()));
    })
    .await;
}

#[tokio::test]
async fn an_update_through_insert_stores_the_password_as_given() {
    let old = UserData {
        user_activation_key: some("1:key"),
        ..new_user("oracle-raw")
    };
    check(
        "user-insert-with-id",
        async |write| setup_user(write, old).await,
        async |write, id| {
            let raw = UserData {
                id,
                user_pass: some("stored-as-given"),
                ..new_user("oracle-raw")
            };
            assert_eq!(wp_insert_user(write, &raw).await.unwrap(), id);
        },
    )
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

/// The REST API's create: no role, then each role added, the second time
/// as one the user has.
#[tokio::test]
async fn adds_roles() {
    check("rest-users-create", nothing, async |write, ()| {
        let data = UserData {
            display_name: some("New Bie"),
            first_name: some("New"),
            description: some("<b>Hi</b><script>x()</script>"),
            user_nicename: some("New Bie"),
            user_pass: some("secret-pass"),
            role: some(""),
            ..new_user("newbie")
        };
        let id = insert_ok(write, data).await;
        for role in ["editor", "author", "editor", ""] {
            add_role(write, id, role.as_bytes()).await.unwrap();
        }
    })
    .await;
}

#[tokio::test]
async fn keeps_an_unknown_role_as_a_capability() {
    let setup = async |write: &mut Write<'_>| setup_user(write, new_user("oracle-ghost")).await;
    check("user-unknown-role", setup, async |write, id| {
        set_role(write, id, b"ghost").await.unwrap();
        set_role(write, id, b"ghost").await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn deletes_a_user() {
    let setup = async |write: &mut Write<'_>| setup_user(write, new_user("oracle-delete")).await;
    check("user-delete", setup, async |write, id| {
        assert!(wp_delete_user(write, id, None, &admin()).await.unwrap());
    })
    .await;
}

#[tokio::test]
async fn deletes_a_user_passing_its_posts_on() {
    let setup = async |write: &mut Write<'_>| setup_user(write, new_user("oracle-heir")).await;
    check("user-delete-reassign", setup, async |write, heir| {
        assert!(
            wp_delete_user(write, 1, Some(heir), &admin())
                .await
                .unwrap()
        );
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
        assert!(
            !wp_delete_user(write, 999_999, None, &admin())
                .await
                .unwrap()
        );
    })
    .await;
}

#[tokio::test]
async fn an_update_through_insert_keeps_the_given_hash() {
    rolled_back(async |write| {
        // `empty()` fields take their defaults.
        let zero = UserData {
            rich_editing: some("0"),
            display_name: some(""),
            user_nicename: some(""),
            user_pass: None,
            user_registered: some(""),
            user_url: some(&format!("https://example.test/{}", "u".repeat(79))),
            ..new_user("oracle-insert-update")
        };
        let id = insert_ok(write, zero).await;
        let rich = get_metadata(write, Meta::User, id, "rich_editing").await;
        assert_eq!(rich.unwrap(), [text("true")]);
        let (_, user) = write.user(id).await.unwrap().unwrap();
        assert_eq!(user.display_name, b"oracle-insert-update");
        assert_eq!(user.nicename, b"oracle-insert-update");
        assert_ne!(user.registered, "0000-00-00 00:00:00");
        // No password is the empty one, hashed.
        assert!(check_password(b"", user.pass.as_bytes()));
        let sixty = new_user(&"b".repeat(60));
        assert!(wp_insert_user(write, &sixty).await.is_ok());
        // Fifty characters fit; a second one is cut to make room for `-2`.
        for login in ["oracle-n1", "oracle-n2"] {
            let long = UserData {
                user_nicename: some(&"n".repeat(50)),
                ..new_user(login)
            };
            insert_ok(write, long).await;
        }
        let second = write.user_id_by_login(b"oracle-n2").await.unwrap().unwrap();
        let (_, second) = write.user(second).await.unwrap().unwrap();
        assert_eq!(second.nicename, format!("{}-2", "n".repeat(48)).as_bytes());
        let update = UserData {
            id,
            user_pass: some("$wp$given"),
            first_name: some("Ada"),
            ..new_user("oracle-insert-update")
        };
        assert_eq!(insert_ok(write, update).await, id);
        let (_, user) = write.user(id).await.unwrap().unwrap();
        assert_eq!(user.pass, "$wp$given");
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
async fn links_wait_for_their_port() {
    let _one = crate::tests::ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut conn = connection().await;
    let mut outer = sqlx::Connection::begin(&mut conn).await.unwrap();
    let mut write = writer.begin_on(&mut outer).await.unwrap();
    let author = insert_ok(&mut write, new_user("oracle-author")).await;
    write.commit().await.unwrap();
    sqlx::query("INSERT INTO wp_links (link_url, link_name, link_owner, link_notes) VALUES ('u', 'n', ?, '')")
        .bind(author)
        .execute(&mut *outer)
        .await
        .unwrap();
    let mut write = writer.begin_on(&mut outer).await.unwrap();
    let deleted = wp_delete_user(&mut write, author, None, &admin()).await;
    assert!(matches!(deleted, Err(Error::Unported("wp_delete_link"))));
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
            assert!(wp_delete_user(write, id, None, &admin()).await.unwrap());
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
    assert_eq!(roles::level(&key(b"level_x")), None);
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
        let id = wp_insert_user(write, &new_user("oracle-default")).await?;
        add_role(write, id, b"author").await?;
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
        wp_delete_user(write, id, None, &admin()).await?;
        wp_delete_user(write, 1, Some(999_999), &admin()).await?;
        Ok(())
    })
    .await;
}

#[tokio::test]
async fn an_update_keeps_the_reset_key_and_the_hash() {
    rolled_back(async |write| {
        let old = UserData {
            user_activation_key: some("1:key"),
            role: some("editor"),
            ..new_user("oracle-keep")
        };
        let id = insert_ok(write, old).await;
        let (_, user) = write.user(id).await.unwrap().unwrap();
        // The stored hash given as the password is kept, not hashed again.
        let same = UserData {
            id,
            first_name: some("New"),
            user_pass: Some(user.pass.clone().into_bytes()),
            user_email: some("oracle-keep@example.test"),
            ..UserData::default()
        };
        let updated = wp_update_user(write, &same).await.unwrap();
        assert!(!updated.password_changed);
        assert_eq!(updated.new_email, None);
        let (_, updated) = write.user(id).await.unwrap().unwrap();
        assert_eq!(updated.pass, user.pass);
        assert_eq!(updated.activation_key, "1:key");
        // No role given: the user keeps its own.
        let caps = get_metadata(write, Meta::User, id, "wp_capabilities").await;
        let editor = Value::Array(vec![(Key::String(b"editor".to_vec()), Value::Bool(true))]);
        assert_eq!(caps.unwrap(), [editor]);
        // A quote in the old address is slashed before the two are compared.
        let quoted = UserData {
            id,
            user_email: some("o'keep@example.test"),
            ..UserData::default()
        };
        wp_update_user(write, &quoted).await.unwrap();
        let updated = wp_update_user(write, &quoted).await.unwrap();
        assert_eq!(updated.new_email, quoted.user_email);
        let slashed = UserData {
            user_email: some("o\\'keep@example.test"),
            ..quoted
        };
        assert_eq!(
            wp_update_user(write, &slashed).await.unwrap().new_email,
            None
        );
    })
    .await;
}

/// A password reset key is kept as PHP keeps it, for the action of Edit
/// User's link (the oracle's `admin-send-password-reset`).
#[tokio::test]
async fn keeps_a_password_reset_key() {
    let setup = async |write: &mut Write<'_>| {
        let user = UserData {
            role: some("author"),
            ..new_user("oracle-link")
        };
        setup_user(write, user).await
    };
    check("admin-send-password-reset", setup, async |write, id| {
        let key = get_password_reset_key(write, id).await.unwrap();
        assert!(key.key.len() == 20 && key.key.bytes().all(|b| b.is_ascii_alphanumeric()));
    })
    .await;
}

/// The lost password form keeps a key as PHP keeps it, whose hash the
/// stored key holds, for a user found by their login and one found by
/// their email with spaces around it, and none for a login no user has
/// (the oracle's `login-lostpassword`).
#[tokio::test]
async fn keeps_the_lost_password_form_s_keys() {
    let setup = async |write: &mut Write<'_>| {
        let lost = setup_user(write, new_user("oracle-lost")).await;
        (lost, setup_user(write, new_user("oracle-mail")).await)
    };
    check("login-lostpassword", setup, async |write, (lost, mail)| {
        let found = retrieve_password(write, b"oracle-lost")
            .await
            .unwrap()
            .unwrap();
        let (login, email) = (
            b"oracle-lost".to_vec(),
            b"oracle-lost@example.test".to_vec(),
        );
        assert_eq!((found.id, found.login, found.email), (lost, login, email));
        let (_, user) = write.user(lost).await.unwrap().unwrap();
        let hash = user.activation_key.split_once(':').unwrap().1;
        assert!(hyscribe_auth::wp_verify_fast_hash(
            found.key.as_bytes(),
            hash.as_bytes()
        ));
        let found = retrieve_password(write, b" oracle-mail@example.test ").await;
        assert_eq!(found.unwrap().unwrap().id, mail);
        let refused = retrieve_password(write, b"nobody").await.unwrap();
        assert_eq!(refused.unwrap_err().code, "invalidcombo");
    })
    .await;
}

/// The lost password form's errors: an empty login, an address no user
/// has, and a login starting with `@`, which is looked up as a login.
#[tokio::test]
async fn refuses_what_the_lost_password_form_refuses() {
    rolled_back(async |write| {
        let at = UserData {
            user_email: some("at@example.test"),
            ..new_user("@at")
        };
        let at = insert_ok(write, at).await;
        let mail = UserData {
            user_email: some("other@example.test"),
            ..new_user("mail@login.test")
        };
        let mail = insert_ok(write, mail).await;
        for (login, code) in [
            (&b""[..], "empty_username"),
            (b" 0 ", "empty_username"),
            (b"nobody@example.test", "invalid_email"),
            (b"@at@example.test", "invalidcombo"),
        ] {
            let refused = retrieve_password(write, login).await.unwrap();
            assert_eq!(refused.unwrap_err().code, code, "{login:?}");
        }
        // A login that is an address no user has as their email.
        let found = retrieve_password(write, b"mail@login.test").await.unwrap();
        assert_eq!(found.unwrap().id, mail);
        let found = retrieve_password(write, b"@at").await.unwrap();
        assert_eq!(found.unwrap().id, at);
    })
    .await;
}

/// A reset stores the slashed password's hash, clears the reset key and
/// empties the default password nag, added where the user had none (the
/// oracle's `login-resetpass`).
#[tokio::test]
async fn resets_a_password() {
    let setup = async |write: &mut Write<'_>| {
        let nagged = UserData {
            user_activation_key: some("1:key"),
            ..new_user("oracle-nagged")
        };
        let nagged = setup_user(write, nagged).await;
        let nag = (Value::Bool(true), Value::Null);
        let meta = update_metadata(
            write,
            Meta::User,
            nagged,
            "default_password_nag",
            &nag.0,
            &nag.1,
        );
        meta.await.unwrap();
        (nagged, setup_user(write, new_user("oracle-plain")).await)
    };
    check("login-resetpass", setup, async |write, (nagged, plain)| {
        for (id, login) in [(nagged, "oracle-nagged"), (plain, "oracle-plain")] {
            let user = reset_password(write, id, b" It\\'s new ").await.unwrap();
            let email = format!("{login}@example.test").into_bytes();
            assert_eq!(user, (login.as_bytes().to_vec(), email));
            let (_, user) = write.user(id).await.unwrap().unwrap();
            assert!(check_password(b"It\\'s new", user.pass.as_bytes()));
        }
    })
    .await;
}

#[tokio::test]
async fn lost_passwords_return_each_database_error() {
    let setup = async |write: &mut Write<'_>| insert_ok(write, new_user("oracle-lost")).await;
    fails_at_each_step(setup, async |write, id| {
        retrieve_password(write, b"oracle-lost@example.test")
            .await?
            .unwrap();
        retrieve_password(write, b"oracle-lost@other.test")
            .await?
            .unwrap_err();
        retrieve_password(write, b"oracle-lost").await?.unwrap();
        reset_password(write, id, b"new").await?;
        Ok(())
    })
    .await;
}

/// An email change waits under a random hash, and none waits when another
/// user has the address; every failed statement fails the action.
#[tokio::test]
async fn an_email_change_waits_for_its_confirmation() {
    rolled_back(async |write| {
        let new = b"new\\'s@example.test";
        let hash = request_email_change(write, 1, new).await.unwrap();
        let waiting = get_metadata(write, Meta::User, 1, "_new_email").await;
        let waiting = waiting.unwrap();
        let [Value::Array(change)] = &waiting[..] else {
            panic!("{waiting:?}");
        };
        assert_eq!(change[0].1.as_bytes(), hash.as_deref());
        let hash = change[0].1.as_bytes().unwrap();
        assert!(hash.len() == 32 && hash.iter().all(u8::is_ascii_hexdigit));
        assert_eq!(change[1].1.as_bytes(), Some(&b"new's@example.test"[..]));
        let own = b" admin@default-install.test";
        assert_eq!(request_email_change(write, 1, own).await.unwrap(), None);
        let gone = get_metadata(write, Meta::User, 1, "_new_email").await;
        assert!(gone.unwrap().is_empty());
    })
    .await;
    let setup = async |_: &mut Write<'_>| {};
    fails_at_each_step(setup, async |write, ()| {
        request_email_change(write, 1, b"new@example.test")
            .await
            .map(|_| ())
    })
    .await;
}

/// `wp_set_password()` stores the password's hash, which it answers, and
/// clears the reset key.
#[tokio::test]
async fn a_set_password_stores_its_hash() {
    rolled_back(async |write| {
        let hash = wp_set_password(write, b"new", 1).await.unwrap();
        let (_, user) = write.user(1).await.unwrap().unwrap();
        assert_eq!(user.pass, hash);
        assert!(check_password(b"new", hash.as_bytes()));
        assert!(user.activation_key.is_empty());
    })
    .await;
}

const LOGIN_URL: &str = "http://127.0.0.1:8081/wp-login.php";

fn codes(refused: &[Rejected]) -> Vec<&str> {
    refused.iter().map(|error| error.code).collect()
}

/// The registration form's users and refusals (the oracle's
/// `login-register`): a new user with the default role, a reset key and the
/// password nag, and nothing written for a login with a space before it, a
/// taken login or address, a login too long or an empty form.
#[tokio::test]
async fn registers_users_as_php_does() {
    let setup = async |write: &mut Write<'_>| {
        insert_ok(write, new_user("oracle-taken")).await;
    };
    check("login-register", setup, async |write, ()| {
        let cases: [(&[u8], &[u8], Option<&str>); 6] = [
            (b"oracle-new", b"oracle-new@example.test", None),
            (b" 10", b"ten@example.test", Some("invalid_username")),
            (
                b"oracle-taken",
                b"free@example.test",
                Some("username_exists"),
            ),
            (
                b"oracle-fresh",
                b"oracle-taken@example.test",
                Some("email_exists"),
            ),
            (&[b'a'; 61], b"long@example.test", Some("registerfail")),
            (b"", b"", Some("empty_username")),
        ];
        for (login, email, code) in cases {
            let registered = register_new_user(write, (login, email), LOGIN_URL).await;
            match (registered.unwrap(), code) {
                (Ok(key), None) => {
                    assert_eq!((&key.login[..], &key.email[..]), (login, email));
                    assert!(key.key.len() == 20 && key.id > 1);
                }
                (Err(refused), Some(code)) => assert_eq!(refused[0].code, code),
                (other, code) => panic!("{other:?} {code:?}"),
            }
        }
    })
    .await;
}

fn rejected_as(code: &'static str, message: &str) -> Rejected {
    Rejected {
        code,
        message: message.into(),
    }
}

/// Each error with its message, in the order the form adds them: the
/// username's, then the address's.
#[tokio::test]
async fn registration_errors_read_as_php_s() {
    rolled_back(async |write| {
        insert_ok(write, new_user("taken")).await;
        let admin = write.user(1).await.unwrap().unwrap().1.email;
        let cases: [(&[u8], &[u8], &[&str]); 9] = [
            (b"", b"", &["empty_username", "empty_email"]),
            (b"0", b"x", &["invalid_username", "invalid_email"]),
            (b"<b>", b"a@b", &["empty_username", "invalid_email"]),
            (b"a/b", b"a@b.test", &["invalid_username"]),
            (b" taken", b"a@b.test", &["invalid_username"]),
            (b"taken", b"a@b.test", &["username_exists"]),
            (b"Taken", b"a@b.test", &["username_exists"]),
            (b"fine", b"taken@example.test", &["email_exists"]),
            (b"fine", b"a@b.test", &[]),
        ];
        for (login, email, expected) in cases {
            let registered = register_new_user(write, (login, email), LOGIN_URL).await;
            match registered.unwrap() {
                Ok(_) => assert!(expected.is_empty(), "{login:?}"),
                Err(refused) => assert_eq!(codes(&refused), expected, "{login:?}"),
            }
        }
        let refused = register_new_user(write, (b"taken", b""), LOGIN_URL).await;
        let used = "<strong>Error:</strong> This username is already registered. Please choose another one.";
        let empty = "<strong>Error:</strong> Please type your email address.";
        assert_eq!(
            refused.unwrap().unwrap_err(),
            [
                rejected_as("username_exists", used),
                rejected_as("empty_email", empty)
            ]
        );
        let refused = register_new_user(write, (b"fine2", b"taken@example.test"), LOGIN_URL)
            .await
            .unwrap()
            .unwrap_err();
        let link = "<strong>Error:</strong> This email address is already registered. <a href=\"http://127.0.0.1:8081/wp-login.php\">Log in</a> with this address or choose another one.";
        assert_eq!(refused, [rejected_as("email_exists", link)]);
        let refused = register_new_user(write, (&[b'a'; 61], b"long@example.test"), LOGIN_URL)
            .await
            .unwrap()
            .unwrap_err();
        let admin = String::from_utf8(admin).unwrap();
        let fail = format!(
            "<strong>Error:</strong> Could not register you&hellip; please contact the <a href=\"mailto:{admin}\">site admin</a>!"
        );
        assert_eq!(refused, [rejected_as("registerfail", &fail)]);
        let invalid = register_new_user(write, (b"a/b", b"new@example.test"), LOGIN_URL).await;
        let message = "<strong>Error:</strong> This username is invalid because it uses illegal characters. Please enter a valid username.";
        assert_eq!(
            invalid.unwrap().unwrap_err(),
            [rejected_as("invalid_username", message)]
        );
        let wrong = register_new_user(write, (b"fine3", b"a@b"), LOGIN_URL).await;
        let message = "<strong>Error:</strong> The email address is not correct.";
        assert_eq!(
            wrong.unwrap().unwrap_err(),
            [rejected_as("invalid_email", message)]
        );
    })
    .await;
}

/// Every failed statement of a registration, passing or refused, fails it.
#[tokio::test]
async fn registrations_return_each_database_error() {
    let setup = async |write: &mut Write<'_>| insert_ok(write, new_user("taken")).await;
    fails_at_each_step(setup, async |write, _| {
        register_new_user(write, (b"fresh", b"fresh@example.test"), LOGIN_URL)
            .await?
            .unwrap();
        register_new_user(write, (b"taken", b"other@example.test"), LOGIN_URL)
            .await?
            .unwrap_err();
        register_new_user(write, (b"other", b"taken@example.test"), LOGIN_URL)
            .await?
            .unwrap_err();
        register_new_user(write, (&[b'a'; 61], b"long@example.test"), LOGIN_URL)
            .await?
            .unwrap_err();
        Ok(())
    })
    .await;
}
