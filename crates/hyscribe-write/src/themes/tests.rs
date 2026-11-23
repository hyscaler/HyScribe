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
/// The fixture child of Twenty Twenty-Five.
const CHILD: (&str, &str) = ("hyscribe-fixture-child", "HyScribe Fixture Child");

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
    delete_option(write, "theme_mods_hyscribe-fixture-child")
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

/// The `sidebars_widgets` option with these sidebars and their widgets.
async fn sidebars(write: &mut Write<'_>, entries: &[(&str, &[&str])]) {
    let list = |ids: &[&str]| {
        let ids = ids.iter().zip(0..);
        Value::Array(ids.map(|(id, at)| (Key::Int(at), text(id))).collect())
    };
    let mut option: Vec<(Key, Value)> = entries
        .iter()
        .map(|(name, ids)| (Key::String(name.as_bytes().to_vec()), list(ids)))
        .collect();
    option.push((Key::String(b"array_version".to_vec()), Value::Int(3)));
    update_option(write, "sidebars_widgets", &Value::Array(option), None)
        .await
        .unwrap();
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
            async |write, ()| switch_theme(write, to, to.0, FIVE).await.unwrap(),
        )
        .await;
    }
    check_case(
        "admin-themes-activate-active",
        async |_| {},
        async |write, ()| switch_theme(write, FIVE, FIVE.0, FIVE).await.unwrap(),
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn the_next_load_is_php_s() {
    check_case(
        "admin-themes-next-load-twentytwentyfour",
        async |write| switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap(),
        async |write, ()| next_load(write, FOUR).await,
    )
    .await;
    check_case(
        "admin-themes-via-customizer",
        async |write| {
            switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
            let on = Value::Bool(true);
            update_option(write, "theme_switched_via_customizer", &on, None)
                .await
                .unwrap();
        },
        async |write, ()| next_load(write, FOUR).await,
    )
    .await;
}

/// What the oracle's `$themes_base` leaves for the cases of a child theme:
/// the roots PHP finds scanning its themes folder, which a switch involving
/// a child makes it keep and which HyScribe does not (the rows only take
/// the option IDs the golden numbers from).
async fn roots(write: &mut Write<'_>) {
    let found = Value::Array(vec![(Key::String(CHILD.0.into()), text("/themes"))]);
    set_site_transient(write, "theme_roots", &found, 1800)
        .await
        .unwrap();
}

/// A child theme is switched to with its parent as the `template`, and
/// switched from as any theme is.
#[tokio::test(flavor = "multi_thread")]
async fn a_child_theme_is_switched_as_php_switches_it() {
    let to_child = async |write: &mut Write<'_>| {
        roots(write).await;
        switch_theme(write, CHILD, FIVE.0, FIVE).await.unwrap();
    };
    check_case(
        "admin-themes-activate-hyscribe-fixture-child",
        roots,
        async |write, ()| {
            switch_theme(write, CHILD, FIVE.0, FIVE).await.unwrap();
        },
    )
    .await;
    let there = async |write: &mut Write<'_>| {
        to_child(write).await;
        next_load(write, CHILD).await;
    };
    check_case(
        "admin-themes-next-load-hyscribe-fixture-child",
        to_child,
        async |write, ()| next_load(write, CHILD).await,
    )
    .await;
    check_case("admin-themes-child-to-parent", there, async |write, ()| {
        switch_theme(write, FIVE, FIVE.0, CHILD).await.unwrap()
    })
    .await;
    check_case("admin-themes-child-to-other", there, async |write, ()| {
        switch_theme(write, FOUR, FOUR.0, CHILD).await.unwrap()
    })
    .await;
    check_case(
        "admin-themes-next-load-child-to-parent",
        async |write| {
            there(write).await;
            switch_theme(write, FIVE, FIVE.0, CHILD).await.unwrap();
        },
        async |write, ()| next_load(write, FIVE).await,
    )
    .await;
}

/// A site that switched to a theme and back gets its widgets back.
#[tokio::test(flavor = "multi_thread")]
async fn switching_back_is_php_s() {
    let there = async |write: &mut Write<'_>| {
        switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
        next_load(write, FOUR).await;
    };
    check_case("admin-themes-switch-back", there, async |write, ()| {
        switch_theme(write, FIVE, FIVE.0, FOUR).await.unwrap()
    })
    .await;
    check_case(
        "admin-themes-next-load-back",
        async |write| {
            there(write).await;
            switch_theme(write, FIVE, FIVE.0, FOUR).await.unwrap();
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
            switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
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
        switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
        next_load(write, FOUR).await;
    };
    check_case("admin-themes-widgets-revived", kept, async |write, ()| {
        switch_theme(write, FIVE, FIVE.0, FOUR).await.unwrap();
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
        switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap()
    })
    .await;
}

/// A site without a sidebar gets its widgets back to the inactive ones.
#[tokio::test(flavor = "multi_thread")]
async fn a_site_without_sidebars_is_php_s() {
    let none = async |write: &mut Write<'_>| {
        let version = Key::String(b"array_version".to_vec());
        let sidebars = Value::Array(vec![(version, Value::Int(3))]);
        update_option(write, "sidebars_widgets", &sidebars, None)
            .await
            .unwrap();
        switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
        next_load(write, FOUR).await;
        switch_theme(write, FIVE, FIVE.0, FOUR).await.unwrap();
    };
    check_case("admin-themes-no-sidebars", none, async |write, ()| {
        next_load(write, FIVE).await;
    })
    .await;
}

/// A widget the theme had that is not inactive now, and every sidebar
/// emptied, as the oracle's cases of those names.
#[tokio::test(flavor = "multi_thread")]
async fn widgets_the_site_changed_are_php_s() {
    let kept = async |write: &mut Write<'_>| {
        let all: [(&str, &[&str]); 3] = [
            ("wp_inactive_widgets", &["block-6"]),
            ("sidebar-1", &["block-2", "block-3"]),
            ("sidebar-2", &["block-4", "block-5"]),
        ];
        sidebars(write, &all).await;
        switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
        next_load(write, FOUR).await;
    };
    check_case(
        "admin-themes-widgets-lost",
        async |write| {
            kept(write).await;
            let inactive: [(&str, &[&str]); 1] = [(
                "wp_inactive_widgets",
                &["block-3", "block-4", "block-5", "block-6"],
            )];
            sidebars(write, &inactive).await;
        },
        async |write, ()| {
            switch_theme(write, FIVE, FIVE.0, FOUR).await.unwrap();
            next_load(write, FIVE).await;
        },
    )
    .await;
    check_case(
        "admin-themes-widgets-emptied",
        async |write| {
            kept(write).await;
            let none: [(&str, &[&str]); 2] = [("wp_inactive_widgets", &[]), ("sidebar-1", &[])];
            sidebars(write, &none).await;
        },
        async |write, ()| {
            switch_theme(write, FIVE, FIVE.0, FOUR).await.unwrap();
            next_load(write, FIVE).await;
        },
    )
    .await;
}

/// Inactive and orphaned widgets come before the others, whatever the order.
#[tokio::test(flavor = "multi_thread")]
async fn widgets_are_ordered_as_php_orders_them() {
    let mixed = async |write: &mut Write<'_>| {
        let all: [(&str, &[&str]); 4] = [
            ("sidebar-1", &["block-2"]),
            ("wp_inactive_widgets", &["block-3"]),
            ("sidebar-2", &["block-4"]),
            ("orphaned_widgets_2", &["block-5"]),
        ];
        sidebars(write, &all).await;
    };
    check_case("admin-themes-widgets-order", mixed, async |write, ()| {
        switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
        next_load(write, FOUR).await;
    })
    .await;
}

/// A theme without mods, whether the site kept the theme's name or not.
#[tokio::test(flavor = "multi_thread")]
async fn a_theme_without_mods_is_php_s() {
    check_case(
        "admin-themes-no-mods",
        async |write| {
            delete_option(write, "theme_mods_twentytwentyfive")
                .await
                .unwrap();
        },
        async |write, ()| switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap(),
    )
    .await;
    check_case(
        "admin-themes-no-mods-named",
        async |write| {
            delete_option(write, "theme_mods_twentytwentyfive")
                .await
                .unwrap();
            update_option(write, "current_theme", &text(FIVE.1), None)
                .await
                .unwrap();
        },
        async |write, ()| switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap(),
    )
    .await;
}

/// Menu locations, no widgets option, and mods that are not an array.
#[tokio::test(flavor = "multi_thread")]
async fn odd_sites_are_php_s() {
    let mods = |locations: bool| {
        let css = (Key::String(b"custom_css_post_id".to_vec()), Value::Int(-1));
        let primary = Value::Array(vec![(Key::String(b"primary".to_vec()), Value::Int(7))]);
        let menus = (Key::String(b"nav_menu_locations".to_vec()), primary);
        Value::Array(if locations {
            vec![css, menus]
        } else {
            vec![css]
        })
    };
    check_case(
        "admin-themes-menu-locations",
        async |write| {
            update_option(write, "theme_mods_twentytwentyfive", &mods(true), None)
                .await
                .unwrap();
        },
        async |write, ()| {
            switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
            next_load(write, FOUR).await;
        },
    )
    .await;
    check_case(
        "admin-themes-no-widgets-option",
        async |write| {
            delete_option(write, "sidebars_widgets").await.unwrap();
        },
        async |write, ()| {
            switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
            next_load(write, FOUR).await;
        },
    )
    .await;
    check_case(
        "admin-themes-broken-mods",
        async |write| {
            update_option(write, "theme_mods_twentytwentyfive", &text("broken"), None)
                .await
                .unwrap();
        },
        async |write, ()| switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap(),
    )
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

/// Every statement of a switch and of the next load can fail, with widgets
/// to carry over, with them to bring back, and with menu locations.
#[tokio::test(flavor = "multi_thread")]
async fn each_statement_can_fail() {
    let load = async |write: &mut Write<'_>, theme: (&str, &str)| -> Result<(), crate::Error> {
        if after_switch_theme(write, theme.0, theme.1).await? {
            theme_switch_done(write).await?;
        }
        Ok(())
    };
    fails_at_each_step(widgets, async |write, ()| {
        switch_theme(write, FOUR, FOUR.0, FIVE).await?;
        load(write, FOUR).await
    })
    .await;
    fails_at_each_step(
        async |write: &mut Write<'_>| {
            let on = Value::Bool(true);
            update_option(write, "theme_switched_via_customizer", &on, None)
                .await
                .unwrap();
        },
        async |write, ()| {
            switch_theme(write, FOUR, FOUR.0, FIVE).await?;
            load(write, FOUR).await
        },
    )
    .await;
    fails_at_each_step(
        async |write: &mut Write<'_>| {
            let menus = Value::Array(vec![(Key::String(b"primary".to_vec()), Value::Int(7))]);
            let mods = Value::Array(vec![(Key::String(b"nav_menu_locations".to_vec()), menus)]);
            update_option(write, "theme_mods_twentytwentyfive", &mods, None)
                .await
                .unwrap();
        },
        async |write, ()| {
            switch_theme(write, FOUR, FOUR.0, FIVE).await?;
            load(write, FOUR).await?;
            switch_theme(write, FIVE, FIVE.0, FOUR).await?;
            load(write, FIVE).await
        },
    )
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
        let refused = switch_theme(write, FOUR, FOUR.0, FIVE).await;
        assert!(matches!(refused, Err(Error::Unported(_))), "{mods:?}");
    })
    .await;
    rolled_back(async |write| {
        let value = Value::Array(vec![(Key::Int(0), Value::Bool(true))]);
        update_option(write, "mods_Twenty Twenty-Four", &value, None)
            .await
            .unwrap();
        let refused = switch_theme(write, FOUR, FOUR.0, FIVE).await;
        assert!(matches!(refused, Err(Error::Unported(_))));
    })
    .await;
    rolled_back(async |write| {
        update_option(write, "link_manager_enabled", &Value::Int(1), None)
            .await
            .unwrap();
        switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
        let refused = after_switch_theme(write, FOUR.0, FOUR.1).await;
        assert!(matches!(refused, Err(Error::Unported(_))));
    })
    .await;
    rolled_back(async |write| {
        update_option(write, "sidebars_widgets", &Value::Int(1), None)
            .await
            .unwrap();
        let refused = switch_theme(write, FOUR, FOUR.0, FIVE).await;
        assert!(matches!(refused, Err(Error::Unported(_))));
    })
    .await;
    // The sidebars a theme had that are not lists.
    rolled_back(async |write| {
        switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
        let odd = Value::Array(vec![(Key::String(b"sidebar-1".to_vec()), text("x"))]);
        let data = (Key::String(b"data".to_vec()), odd);
        let kept = Value::Array(vec![(
            Key::String(b"sidebars_widgets".to_vec()),
            Value::Array(vec![data]),
        )]);
        update_option(write, "theme_mods_twentytwentyfour", &kept, None)
            .await
            .unwrap();
        let refused = after_switch_theme(write, FOUR.0, FOUR.1).await;
        assert!(matches!(refused, Err(Error::Unported(_))));
    })
    .await;
    // A sidebar that is not a list, as a site that was edited by hand has.
    rolled_back(async |write| {
        let odd = Value::Array(vec![(Key::String(b"sidebar-1".to_vec()), text("x"))]);
        update_option(write, "sidebars_widgets", &odd, None)
            .await
            .unwrap();
        switch_theme(write, FOUR, FOUR.0, FIVE).await.unwrap();
        let refused = after_switch_theme(write, FOUR.0, FOUR.1).await;
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
        ("text-+1", false),
    ] {
        assert_eq!(is_template(id.as_bytes()), template, "{id}");
    }
}
