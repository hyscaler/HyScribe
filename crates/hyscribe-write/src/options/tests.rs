// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{Float, Key};

use super::*;
use crate::tests::{check, fails_at_each_step, rolled_back, text};

fn list(items: &[Value]) -> Value {
    let items = items.iter().enumerate();
    Value::Array(
        items
            .map(|(i, v)| (Key::Int(i as i64), v.clone()))
            .collect(),
    )
}

fn entries(items: &[(&str, Value)]) -> Value {
    let items = items.iter();
    Value::Array(
        items
            .map(|(k, v)| (Key::String(k.as_bytes().to_vec()), v.clone()))
            .collect(),
    )
}

#[tokio::test]
async fn updates_an_option() {
    check(
        "update-option",
        async |_| (),
        async |write, ()| {
            let renamed = update_option(write, "blogname", &text("A renamed site"), None).await;
            assert!(renamed.unwrap());
        },
    )
    .await;
}

#[tokio::test]
async fn adds_options() {
    check(
        "option-add",
        async |_| (),
        async |write, ()| {
            let mut add = async |name, value: Value, autoload| {
                add_option(write, name, &value, autoload).await.unwrap()
            };
            assert!(add("oracle_text", text("A value"), None).await);
            assert!(add("  oracle_trimmed  ", Value::Int(2), None).await);
            let nested = list(&[
                Value::Bool(true),
                Value::Null,
                Value::Float(Float::from_f64(1.5)),
            ]);
            let value = entries(&[("a", Value::Int(1)), ("b", nested)]);
            assert!(add("oracle_list", value, Some(Autoload::Off)).await);
            assert!(add("oracle_yes", text("x"), Some(Autoload::On)).await);
            assert!(add("oracle_no", text("x"), Some(Autoload::Off)).await);
            assert!(add("oracle_true", text("x"), Some(Autoload::On)).await);
            assert!(
                add(
                    "oracle_serialized",
                    text("a:1:{i:0;i:1;}"),
                    Some(Autoload::Auto)
                )
                .await
            );
            assert!(!add("blogname", text("Not added"), None).await);
            assert!(!add("", text("Not added"), None).await);
        },
    )
    .await;
}

#[tokio::test]
async fn updates_options() {
    let setup = async |write: &mut Write<'_>| {
        assert!(
            add_option(write, "oracle_off", &text("one"), None)
                .await
                .unwrap()
        );
        for (name, value) in [
            ("oracle_same", &b"one"[..]),
            ("oracle_auto", b"one"),
            ("oracle_large", b"one"),
            ("oracle_false", b"b:0;"),
        ] {
            write.add_option(name, value, "auto-on").await.unwrap();
        }
        write
            .update_option("siteurl", b"http://127.0.0.1:8081/")
            .await
            .unwrap();
        write.update_option("home", b"").await.unwrap();
    };
    check("option-update", setup, async |write, ()| {
        let mut update = async |name, value: Value, autoload| {
            update_option(write, name, &value, autoload).await.unwrap()
        };
        assert!(!update("oracle_same", text("one"), None).await);
        assert!(update("oracle_auto", list(&[text("x")]), None).await);
        let large = Value::String(vec![b'x'; 150_001]);
        assert!(update("oracle_large", large, None).await);
        assert!(update("oracle_off", text("two"), Some(Autoload::Off)).await);
        assert!(update("oracle_missing", text("new"), None).await);
        assert!(update("oracle_false", text("set"), None).await);
        assert!(!update("siteurl", text("http://127.0.0.1:8081"), None).await);
        assert!(!update("home", text("http://127.0.0.1:8081"), None).await);
    })
    .await;
}

#[tokio::test]
async fn deletes_options() {
    check(
        "option-delete",
        async |_| (),
        async |write, ()| {
            assert!(delete_option(write, "blogdescription").await.unwrap());
            assert!(!delete_option(write, "oracle_none").await.unwrap());
        },
    )
    .await;
}

/// The expiry stored for `name`, less the time now.
async fn lifetime(write: &mut Write<'_>, name: &str) -> i64 {
    let raw = write.option_value(name).await.unwrap().unwrap();
    stored(&raw).to_int() - now()
}

#[tokio::test]
async fn sets_transients() {
    check(
        "transient-set",
        async |_| (),
        async |write, ()| {
            assert!(set_transient(write, "oracle", &text("v"), 0).await.unwrap());
            let value = list(&[Value::Int(1)]);
            assert!(
                set_transient(write, "oracle_expiring", &value, 3600)
                    .await
                    .unwrap()
            );
            assert!(
                set_site_transient(write, "oracle", &text("v"), 0)
                    .await
                    .unwrap()
            );
            assert!(
                set_site_transient(write, "oracle_expiring", &text("v"), 60)
                    .await
                    .unwrap()
            );
            let expiry = lifetime(write, "_transient_timeout_oracle_expiring").await;
            assert!((3599..=3600).contains(&expiry), "{expiry}");
            let expiry = lifetime(write, "_site_transient_timeout_oracle_expiring").await;
            assert!((59..=60).contains(&expiry), "{expiry}");
        },
    )
    .await;
}

#[tokio::test]
async fn updates_transients() {
    let setup = async |write: &mut Write<'_>| {
        set_transient(write, "oracle_a", &text("one"), 0)
            .await
            .unwrap();
        set_transient(write, "oracle_b", &text("one"), 3600)
            .await
            .unwrap();
        set_transient(write, "oracle_c", &text("one"), 0)
            .await
            .unwrap();
        set_site_transient(write, "oracle_s", &text("one"), 60)
            .await
            .unwrap();
        set_site_transient(write, "oracle_t", &text("one"), 0)
            .await
            .unwrap();
    };
    check("transient-update", setup, async |write, ()| {
        assert!(
            set_transient(write, "oracle_a", &text("two"), 0)
                .await
                .unwrap()
        );
        assert!(
            set_transient(write, "oracle_b", &text("two"), 7200)
                .await
                .unwrap()
        );
        assert!(
            set_transient(write, "oracle_c", &text("two"), 60)
                .await
                .unwrap()
        );
        assert!(
            set_site_transient(write, "oracle_s", &text("two"), 120)
                .await
                .unwrap()
        );
        assert!(
            set_site_transient(write, "oracle_t", &text("two"), 0)
                .await
                .unwrap()
        );
        for (name, expected) in [
            ("_transient_timeout_oracle_b", 7200),
            ("_transient_timeout_oracle_c", 60),
            ("_site_transient_timeout_oracle_s", 120),
        ] {
            let expiry = lifetime(write, name).await;
            assert!(
                (expected - 1..=expected).contains(&expiry),
                "{name}: {expiry}"
            );
        }
    })
    .await;
}

#[tokio::test]
async fn deletes_transients() {
    let setup = async |write: &mut Write<'_>| {
        set_transient(write, "oracle_a", &text("one"), 0)
            .await
            .unwrap();
        set_transient(write, "oracle_b", &text("one"), 3600)
            .await
            .unwrap();
        set_site_transient(write, "oracle_s", &text("one"), 60)
            .await
            .unwrap();
    };
    check("transient-delete", setup, async |write, ()| {
        assert!(delete_transient(write, "oracle_a").await.unwrap());
        assert!(delete_transient(write, "oracle_b").await.unwrap());
        assert!(delete_site_transient(write, "oracle_s").await.unwrap());
        assert!(!delete_transient(write, "oracle_none").await.unwrap());
        assert!(!delete_site_transient(write, "oracle_none").await.unwrap());
    })
    .await;
}

#[tokio::test]
async fn the_options_cache_names_are_refused() {
    rolled_back(async |write| {
        for name in ["alloptions", " notoptions "] {
            let value = text("x");
            let refused = [
                add_option(write, name, &value, None).await,
                update_option(write, name, &value, None).await,
                delete_option(write, name).await,
            ];
            for result in refused {
                assert!(matches!(result, Err(Error::Refused("protected option"))));
            }
        }
        assert!(!update_option(write, " 0 ", &text("x"), None).await.unwrap());
        // Only an empty home reads the site URL.
        let empty = get_option(write, "blogdescription").await.unwrap();
        assert_eq!(empty, text(""));
        assert!(!delete_option(write, "").await.unwrap());
    })
    .await;
}

#[tokio::test]
async fn values_are_stored_as_php_stores_them() {
    let object = Value::Object {
        class: b"C".to_vec(),
        properties: Vec::new(),
    };
    assert_eq!(stored_bytes(&object), b"O:1:\"C\":0:{}");
    assert_eq!(stored_bytes(&text("b:1;")), b"s:4:\"b:1;\";");
    assert_eq!(stored_bytes(&text("plain")), b"plain");
    assert_eq!(stored_bytes(&Value::Bool(true)), b"1");
    assert_eq!(stored(b"a:1:{x}"), Value::Bool(false));
    assert_eq!(stored(b"i:5;"), Value::Int(5));
    assert_eq!(autoload_value(&[b'x'; 150_000], None), "auto");
    assert_eq!(
        autoload_value(&[b'x'; 150_001], Some(Autoload::Auto)),
        "auto-off"
    );
}

#[tokio::test]
async fn a_changed_autoload_alone_is_a_change() {
    rolled_back(async |write| {
        write.add_option("oracle_same", b"1", "auto").await.unwrap();
        // The values differ as PHP compares them, the stored bytes do not.
        let updated = update_option(write, "oracle_same", &Value::Int(1), None).await;
        assert!(!updated.unwrap());
        let updated = update_option(write, "oracle_same", &Value::Int(1), Some(Autoload::On));
        assert!(updated.await.unwrap());
        assert_eq!(
            write.option_autoload("oracle_same").await.unwrap().unwrap(),
            "on"
        );
    })
    .await;
}

mod meta;

#[tokio::test]
async fn options_return_each_database_error() {
    let setup = async |write: &mut Write<'_>| {
        write.update_option("home", b"").await.unwrap();
        set_transient(write, "oracle_a", &text("one"), 0)
            .await
            .unwrap();
        set_transient(write, "oracle_b", &text("one"), 60)
            .await
            .unwrap();
        set_site_transient(write, "oracle_s", &text("one"), 60)
            .await
            .unwrap();
    };
    fails_at_each_step(setup, async |write, ()| {
        update_option(write, "home", &text("http://127.0.0.1:8081"), None).await?;
        add_option(write, "oracle_new", &text("one"), None).await?;
        update_option(write, "oracle_new", &text("two"), None).await?;
        update_option(write, "oracle_new", &text("three"), Some(Autoload::On)).await?;
        update_option(write, "oracle_missing", &text("one"), None).await?;
        delete_option(write, "oracle_new").await?;
        set_transient(write, "oracle_n", &text("one"), 60).await?;
        set_transient(write, "oracle_a", &text("two"), 60).await?;
        set_transient(write, "oracle_b", &text("two"), 60).await?;
        set_site_transient(write, "oracle_t", &text("one"), 60).await?;
        set_site_transient(write, "oracle_s", &text("two"), 60).await?;
        delete_transient(write, "oracle_b").await?;
        delete_site_transient(write, "oracle_s").await?;
        Ok(())
    })
    .await;
}

/// The `settings_errors` transient a save keeps.
async fn kept(write: &mut Write<'_>) {
    let saved = entries(&[
        ("setting", text("general")),
        ("code", text("settings_updated")),
        ("message", text("Settings saved.")),
        ("type", text("success")),
    ]);
    set_transient(write, "settings_errors", &list(&[saved]), 30)
        .await
        .unwrap();
}

/// The screen a save sends back deletes the notices it shows, and an
/// expired transient, even an empty one; an empty one still current, or
/// one with no timeout, is left as it is.
#[tokio::test]
async fn shown_settings_errors_are_deleted() {
    check("admin-options-notice-shown", kept, async |write, ()| {
        settings_errors_shown(write, now()).await.unwrap();
    })
    .await;
    let expired = async |write: &mut Write<'_>| {
        kept(write).await;
        let past = Value::Int(now() - 60);
        update_option(write, "_transient_timeout_settings_errors", &past, None)
            .await
            .unwrap();
    };
    check(
        "admin-options-notice-expired",
        expired,
        async |write, ()| {
            settings_errors_shown(write, now()).await.unwrap();
        },
    )
    .await;
    // An empty list, whose timeout is `at`, shown at `now`: whether it is left.
    let left = async |write: &mut Write<'_>, at: Option<i64>, now: i64| {
        set_transient(write, "settings_errors", &list(&[]), 30)
            .await
            .unwrap();
        let timeout = "_transient_timeout_settings_errors";
        match at {
            Some(at) => {
                update_option(write, timeout, &Value::Int(at), None)
                    .await
                    .unwrap();
            }
            None => {
                delete_option(write, timeout).await.unwrap();
            }
        }
        settings_errors_shown(write, now).await.unwrap();
        let left = write
            .option_value("_transient_settings_errors")
            .await
            .unwrap();
        delete_option(write, "_transient_settings_errors")
            .await
            .unwrap();
        left.is_some()
    };
    rolled_back(async |write| {
        settings_errors_shown(write, 100).await.unwrap();
        assert!(left(write, Some(100), 100).await);
        assert!(!left(write, Some(99), 100).await);
        assert!(left(write, None, 100).await);
    })
    .await;
    let shown = async |write: &mut Write<'_>, ()| settings_errors_shown(write, now()).await;
    fails_at_each_step(kept, shown).await;
    fails_at_each_step(expired, shown).await;
}
