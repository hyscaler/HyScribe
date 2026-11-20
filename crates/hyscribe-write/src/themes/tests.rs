// SPDX-License-Identifier: AGPL-3.0-or-later

//! Switching themes against the write oracle (`reference/writes/actions-themes.php`).

use hyscribe_db::Write;
use hyscribe_phpser::{Key, Value};

use super::*;
use crate::options::update_option;
use crate::options::{delete_option, set_site_transient};
use crate::tests::{check, fails_at_each_step, rolled_back, text};

const FIVE: (&str, &str) = ("twentytwentyfive", "Twenty Twenty-Five");
const FOUR: (&str, &str) = ("twentytwentyfour", "Twenty Twenty-Four");
const THREE: (&str, &str) = ("twentytwentythree", "Twenty Twenty-Three");

/// The site every oracle case starts from (`$themes_base`): no pattern
/// cache, and the mods PHP's first page view writes.
async fn base(write: &mut Write<'_>) {
    for prefix in [
        "_site_transient_wp_theme_files_patterns-",
        "_site_transient_timeout_wp_theme_files_patterns-",
    ] {
        for (name, _) in write.options_starting(prefix).await.unwrap() {
            delete_option(write, &name).await.unwrap();
        }
    }
    delete_option(write, "theme_mods_twentytwentyfive")
        .await
        .unwrap();
    let mods = Value::Array(vec![(
        Key::String(b"custom_css_post_id".to_vec()),
        Value::Int(-1),
    )]);
    update_option(write, "theme_mods_twentytwentyfive", &mods, None)
        .await
        .unwrap();
}

/// [`check`] of an oracle case, which starts from [`base`].
async fn check_case<T>(
    name: &str,
    setup: impl AsyncFnOnce(&mut Write<'_>) -> T,
    action: impl AsyncFnOnce(&mut Write<'_>, T),
) {
    let setup = async |write: &mut Write<'_>| {
        base(write).await;
        setup(write).await
    };
    check(name, setup, action).await;
}

/// The next load of a site whose active theme is `theme`.
async fn next_load(write: &mut Write<'_>, theme: (&str, &str)) {
    if after_switch_theme(write, theme.0, theme.1).await.unwrap() {
        theme_switch_done(write).await.unwrap();
    }
}

/// The widgets in the states a site leaves them in
/// (`$themes_widgets` of the oracle).
async fn widgets(write: &mut Write<'_>) {
    let instance = |title: &str, content: &str| {
        Value::Array(vec![
            (Key::String(b"title".to_vec()), text(title)),
            (Key::String(b"text".to_vec()), text(content)),
        ])
    };
    let texts = Value::Array(vec![
        (Key::Int(2), instance("T", "x")),
        (Key::Int(3), instance("U", "y")),
        (Key::String(b"_multiwidget".to_vec()), Value::Int(1)),
    ]);
    update_option(write, "widget_text", &texts, None)
        .await
        .unwrap();
    let list = |ids: &[&str]| {
        let ids = ids.iter().zip(0..);
        Value::Array(ids.map(|(id, at)| (Key::Int(at), text(id))).collect())
    };
    let sidebars = Value::Array(vec![
        (
            Key::String(b"wp_inactive_widgets".to_vec()),
            list(&["text-2"]),
        ),
        (
            Key::String(b"orphaned_widgets_1".to_vec()),
            list(&["text-3"]),
        ),
        (
            Key::String(b"sidebar-1".to_vec()),
            list(&["block-2", "gone-2", "block-3"]),
        ),
        (Key::String(b"sidebar-2".to_vec()), list(&[])),
        (Key::String(b"array_version".to_vec()), Value::Int(3)),
    ]);
    update_option(write, "sidebars_widgets", &sidebars, None)
        .await
        .unwrap();
}

/// The switch of a request, and what the next load adds, are PHP's.
#[tokio::test(flavor = "multi_thread")]
async fn a_switch_is_php_s() {
    for (name, to) in [
        ("admin-themes-activate-twentytwentyfour", FOUR),
        ("admin-themes-activate-twentytwentythree", THREE),
    ] {
        check_case(
            name,
            async |_| {},
            async |write, ()| switch_theme(write, to, FIVE).await.unwrap(),
        )
        .await;
    }
    check_case(
        "admin-themes-activate-active",
        async |_| {},
        async |write, ()| switch_theme(write, FIVE, FIVE).await.unwrap(),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_next_load_is_php_s() {
    check_case(
        "admin-themes-next-load-twentytwentyfour",
        async |write| switch_theme(write, FOUR, FIVE).await.unwrap(),
        async |write, ()| next_load(write, FOUR).await,
    )
    .await;
    check_case(
        "admin-themes-via-customizer",
        async |write| {
            switch_theme(write, FOUR, FIVE).await.unwrap();
            let on = Value::Bool(true);
            update_option(write, "theme_switched_via_customizer", &on, None)
                .await
                .unwrap();
        },
        async |write, ()| next_load(write, FOUR).await,
    )
    .await;
}

/// A site that switched to a theme and back gets its widgets back.
#[tokio::test(flavor = "multi_thread")]
async fn switching_back_is_php_s() {
    let there = async |write: &mut Write<'_>| {
        switch_theme(write, FOUR, FIVE).await.unwrap();
        next_load(write, FOUR).await;
    };
    check_case("admin-themes-switch-back", there, async |write, ()| {
        switch_theme(write, FIVE, FOUR).await.unwrap()
    })
    .await;
    check_case(
        "admin-themes-next-load-back",
        async |write| {
            there(write).await;
            switch_theme(write, FIVE, FOUR).await.unwrap();
        },
        async |write, ()| next_load(write, FIVE).await,
    )
    .await;
}

/// Widgets that are orphaned, inactive, gone and empty are carried over as
/// PHP carries them, and come back to the sidebars the theme had but those
/// still inactive.
#[tokio::test(flavor = "multi_thread")]
async fn widgets_are_carried_as_php_carries_them() {
    check_case(
        "admin-themes-widgets-carried",
        widgets,
        async |write, ()| {
            switch_theme(write, FOUR, FIVE).await.unwrap();
            next_load(write, FOUR).await;
        },
    )
    .await;
    let kept = async |write: &mut Write<'_>| {
        let list = |ids: &[&str]| {
            let ids = ids.iter().zip(0..);
            Value::Array(ids.map(|(id, at)| (Key::Int(at), text(id))).collect())
        };
        let sidebars = Value::Array(vec![
            (
                Key::String(b"wp_inactive_widgets".to_vec()),
                list(&["block-6"]),
            ),
            (
                Key::String(b"sidebar-1".to_vec()),
                list(&["block-2", "block-3"]),
            ),
            (
                Key::String(b"sidebar-2".to_vec()),
                list(&["block-4", "block-5"]),
            ),
            (Key::String(b"array_version".to_vec()), Value::Int(3)),
        ]);
        update_option(write, "sidebars_widgets", &sidebars, None)
            .await
            .unwrap();
        switch_theme(write, FOUR, FIVE).await.unwrap();
        next_load(write, FOUR).await;
    };
    check_case("admin-themes-widgets-revived", kept, async |write, ()| {
        switch_theme(write, FIVE, FOUR).await.unwrap();
        next_load(write, FIVE).await;
    })
    .await;
}

/// The pattern caches of the two themes are deleted, named after PHP's
/// folder for them, and another theme's is left.
#[tokio::test(flavor = "multi_thread")]
async fn pattern_caches_are_deleted_as_php_deletes_them() {
    let caches = async |write: &mut Write<'_>| {
        for stylesheet in [FIVE.0, FOUR.0, "other"] {
            let pattern = Value::Array(vec![(
                Key::String(b"slug".to_vec()),
                text(&format!("{stylesheet}/one")),
            )]);
            let patterns = Value::Array(vec![(Key::String(b"one.php".to_vec()), pattern)]);
            let cache = Value::Array(vec![
                (Key::String(b"version".to_vec()), text("1")),
                (Key::String(b"patterns".to_vec()), patterns),
            ]);
            let folder = format!("/var/www/html/wp-content/themes/{stylesheet}");
            let name = format!(
                "wp_theme_files_patterns-{}",
                hyscribe_phpser::md5(folder.as_bytes())
            );
            set_site_transient(write, &name, &cache, 86_400)
                .await
                .unwrap();
        }
    };
    check_case("admin-themes-pattern-caches", caches, async |write, ()| {
        switch_theme(write, FOUR, FIVE).await.unwrap()
    })
    .await;
}

/// Without a switch nothing is done on the next load.
#[tokio::test(flavor = "multi_thread")]
async fn no_switch_leaves_the_next_load_alone() {
    rolled_back(async |write| {
        assert!(!after_switch_theme(write, FIVE.0, FIVE.1).await.unwrap());
    })
    .await;
}

/// Every statement of a switch and of the next load can fail.
#[tokio::test(flavor = "multi_thread")]
async fn each_statement_can_fail() {
    fails_at_each_step(widgets, async |write, ()| {
        switch_theme(write, FOUR, FIVE).await?;
        if after_switch_theme(write, FOUR.0, FOUR.1).await? {
            theme_switch_done(write).await?;
        }
        Ok(())
    })
    .await;
}

/// What a site keeps the old way, or the sidebars of a Links widget, are
/// not ported.
#[tokio::test(flavor = "multi_thread")]
async fn what_is_not_ported_is_refused() {
    rolled_back(async |write| {
        let keep = |name: &'static str| {
            let value = Value::Array(vec![(Key::Int(0), Value::Bool(true))]);
            (name, value)
        };
        // Mods kept under the theme's name, for the old and the new theme.
        let (name, value) = keep("mods_Twenty Twenty-Five");
        let mods = Value::Bool(false);
        crate::options::delete_option(write, "theme_mods_twentytwentyfive")
            .await
            .unwrap();
        update_option(write, name, &value, None).await.unwrap();
        let refused = switch_theme(write, FOUR, FIVE).await;
        assert!(matches!(refused, Err(Error::Unported(_))), "{mods:?}");
    })
    .await;
    rolled_back(async |write| {
        let value = Value::Array(vec![(Key::Int(0), Value::Bool(true))]);
        update_option(write, "mods_Twenty Twenty-Four", &value, None)
            .await
            .unwrap();
        let refused = switch_theme(write, FOUR, FIVE).await;
        assert!(matches!(refused, Err(Error::Unported(_))));
    })
    .await;
    rolled_back(async |write| {
        update_option(write, "link_manager_enabled", &Value::Int(1), None)
            .await
            .unwrap();
        switch_theme(write, FOUR, FIVE).await.unwrap();
        let refused = after_switch_theme(write, FOUR.0, FOUR.1).await;
        assert!(matches!(refused, Err(Error::Unported(_))));
    })
    .await;
    rolled_back(async |write| {
        update_option(write, "sidebars_widgets", &Value::Int(1), None)
            .await
            .unwrap();
        let refused = switch_theme(write, FOUR, FIVE).await;
        assert!(matches!(refused, Err(Error::Unported(_))));
    })
    .await;
}

/// A widget's template ID, which a lost widget is not counted for.
#[test]
fn templates_are_widgets_without_instances() {
    for (id, template) in [
        ("text-1", true),
        ("text-0", true),
        ("text-2", false),
        ("text-10", false),
        ("text-", false),
        ("-1", false),
        ("text", false),
        ("text-x", false),
    ] {
        assert_eq!(is_template(id.as_bytes()), template, "{id}");
    }
}
