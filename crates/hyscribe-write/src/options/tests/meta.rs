// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[tokio::test]
async fn adds_meta() {
    check(
        "meta-add",
        async |_| (),
        async |write, ()| {
            let mut add = async |meta, object, key, value: Value, unique| {
                add_metadata(write, meta, object, key, &value, unique)
                    .await
                    .unwrap()
            };
            assert!(
                add(Meta::Post, 1, "oracle_key", text("one"), false)
                    .await
                    .is_some()
            );
            assert!(
                add(Meta::Post, 1, "oracle_key", text("two"), false)
                    .await
                    .is_some()
            );
            assert!(
                add(Meta::Post, 1, "oracle_unique", text("u"), true)
                    .await
                    .is_some()
            );
            assert!(
                add(Meta::Post, 1, "oracle_unique", text("u2"), true)
                    .await
                    .is_none()
            );
            let value = entries(&[("a", text("b\\c"))]);
            assert!(
                add(Meta::Comment, 1, "oracle_list", value, false)
                    .await
                    .is_some()
            );
            assert!(
                add(Meta::Term, 1, "oracle_slash", text("a\\b"), false)
                    .await
                    .is_some()
            );
            assert!(
                add(Meta::User, 1, "oracle_int", Value::Int(5), false)
                    .await
                    .is_some()
            );
            let key = "oracle_\\key";
            assert!(
                add(Meta::User, 1, key, Value::Bool(false), false)
                    .await
                    .is_some()
            );
            assert!(
                add(Meta::Post, 0, "oracle_key", text("none"), false)
                    .await
                    .is_none()
            );
            assert!(add(Meta::Post, 1, "", text("none"), false).await.is_none());
            assert!(add(Meta::Post, 1, "0", text("none"), false).await.is_none());
        },
    )
    .await;
}

#[tokio::test]
async fn updates_meta() {
    let setup = async |write: &mut Write<'_>| {
        for (key, value) in [
            ("oracle_one", text("one")),
            ("oracle_two", text("a")),
            ("oracle_two", text("b")),
            ("oracle_both", text("a")),
            ("oracle_both", text("b")),
            ("oracle_list", list(&[Value::Int(1)])),
        ] {
            add_metadata(write, Meta::Post, 1, key, &value, false)
                .await
                .unwrap();
        }
        write
            .add_meta(Meta::Post, 1, "oracle_str", b"s:3:\"abc\";")
            .await
            .unwrap();
        write
            .add_meta(Meta::Post, 1, "oracle_num", b"5")
            .await
            .unwrap();
    };
    check("meta-update", setup, async |write, ()| {
        let mut update = async |meta, object, key, value: Value, previous: Value| {
            update_metadata(write, meta, object, key, &value, &previous)
                .await
                .unwrap()
        };
        let none = Value::Null;
        assert!(!update(Meta::Post, 1, "oracle_one", text("one"), none.clone()).await);
        let value = list(&[Value::Int(1)]);
        assert!(!update(Meta::Post, 1, "oracle_list", value, none.clone()).await);
        assert!(!update(Meta::Post, 1, "oracle_str", text("abc"), none.clone()).await);
        // Not the same value to PHP, the same bytes to the database.
        assert!(!update(Meta::Post, 1, "oracle_num", Value::Int(5), none.clone()).await);
        assert!(update(Meta::Post, 1, "oracle_two", text("c"), text("b")).await);
        assert!(!update(Meta::Post, 1, "oracle_two", text("d"), text("none")).await);
        assert!(update(Meta::Post, 1, "oracle_both", text("z"), none.clone()).await);
        assert!(update(Meta::Post, 1, "oracle_new", text("n"), none.clone()).await);
        let value = list(&[Value::Int(1), Value::Int(2)]);
        assert!(update(Meta::User, 1, "oracle_list", value, none.clone()).await);
        assert!(update(Meta::Term, 1, "oracle_new", text("n"), text("ignored")).await);
        assert!(!update(Meta::Post, 0, "oracle_one", text("none"), none.clone()).await);
        assert!(!update(Meta::Post, 1, "", text("none"), none).await);
    })
    .await;
}

#[tokio::test]
async fn deletes_meta() {
    let setup = async |write: &mut Write<'_>| {
        for (object, key, value) in [
            (1, "oracle_del", text("a")),
            (1, "oracle_del", text("b")),
            (1, "oracle_del", text("c")),
            (1, "oracle_all", text("x")),
            (2, "oracle_all", text("x")),
            (3, "oracle_all", text("y")),
            (1, "oracle_every", text("z")),
            (2, "oracle_every", text("z")),
            (1, "oracle_list", list(&[Value::Int(1)])),
            (2, "oracle_zero", text("0")),
        ] {
            add_metadata(write, Meta::Post, object, key, &value, false)
                .await
                .unwrap();
        }
    };
    check("meta-delete", setup, async |write, ()| {
        let mut delete = async |object, key, value: Value, all| {
            delete_metadata(write, Meta::Post, object, key, &value, all)
                .await
                .unwrap()
        };
        assert!(delete(1, "oracle_del", text("a"), false).await);
        assert!(delete(0, "oracle_all", text("x"), true).await);
        assert!(delete(0, "oracle_every", text(""), true).await);
        assert!(delete(1, "oracle_list", list(&[Value::Int(1)]), false).await);
        assert!(delete(2, "oracle_zero", Value::Int(0), false).await);
        assert!(!delete(1, "oracle_del", text("none"), false).await);
        assert!(delete(1, "oracle_del", Value::Null, false).await);
        assert!(!delete(0, "oracle_del", Value::Null, false).await);
        assert!(!delete(1, "", Value::Null, false).await);
    })
    .await;
}

#[tokio::test]
async fn meta_values_are_unslashed_as_php_unslashes_them() {
    let private = (Key::String(b"\0C\0secret".to_vec()), text("a\\'b"));
    let object = |name| Value::Object {
        class: b"C".to_vec(),
        properties: vec![(Key::String(b"name".to_vec()), name), private.clone()],
    };
    let unslashed = crate::options::meta::unslash(&object(text("a\\'b")));
    assert_eq!(unslashed, object(text("a'b")));
    assert_eq!(crate::options::meta::unslash(&Value::Int(3)), Value::Int(3));
}

#[tokio::test]
async fn meta_functions_return_each_database_error() {
    let setup = async |write: &mut Write<'_>| {
        add_metadata(write, Meta::Post, 1, "oracle_one", &text("one"), false)
            .await
            .unwrap();
    };
    fails_at_each_step(setup, async |write, ()| {
        let none = Value::Null;
        add_metadata(write, Meta::Post, 1, "oracle_one", &text("two"), true).await?;
        update_metadata(write, Meta::Post, 1, "oracle_one", &text("one"), &none).await?;
        update_metadata(write, Meta::Post, 1, "oracle_new", &text("n"), &none).await?;
        update_metadata(write, Meta::Post, 1, "oracle_new", &text("m"), &none).await?;
        update_metadata(write, Meta::Post, 1, "oracle_new", &text("o"), &text("m")).await?;
        delete_metadata(write, Meta::Post, 1, "oracle_one", &text("one"), false).await?;
        delete_metadata(write, Meta::Post, 0, "oracle_new", &text("o"), true).await?;
        delete_metadata(write, Meta::Post, 0, "oracle_new", &none, true).await?;
        delete_metadata(write, Meta::Post, 1, "oracle_new", &none, false).await?;
        Ok(())
    })
    .await;
}

#[tokio::test]
async fn no_key_or_no_object_is_never_changed() {
    rolled_back(async |write| {
        write.add_meta(Meta::Post, 1, "", b"x").await.unwrap();
        write
            .add_meta(Meta::Post, 0, "oracle_zero", b"x")
            .await
            .unwrap();
        let none = Value::Null;
        let updated = update_metadata(write, Meta::Post, 1, "", &text("y"), &none).await;
        assert!(!updated.unwrap());
        let y = text("y");
        let updated = update_metadata(write, Meta::Post, 0, "oracle_zero", &y, &none).await;
        assert!(!updated.unwrap());
        assert_eq!(
            get_metadata(write, Meta::Post, 1, "").await.unwrap(),
            [text("x")]
        );
        let zero = get_metadata(write, Meta::Post, 0, "oracle_zero")
            .await
            .unwrap();
        assert_eq!(zero, [text("x")]);
        let deleted = delete_metadata(write, Meta::Post, 1, "", &none, false).await;
        assert!(!deleted.unwrap());
        assert_eq!(
            get_metadata(write, Meta::Post, 1, "").await.unwrap(),
            [text("x")]
        );
    })
    .await;
}
