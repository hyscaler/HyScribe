// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_db::Connection;
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;
use hyscribe_write::options::update_option;

use super::*;
use crate::admin::saves::tests::{SITE, settled, writer};

fn view(settings: bool, rules: bool) -> View {
    View {
        synced: Synced::default(),
        welcome: None,
        draft: false,
        settings,
        rules,
    }
}

/// A Settings screen's view fails at the statement that fails while it
/// refreshes the rules or deletes the notices it showed, and so writes
/// nothing.
#[tokio::test]
async fn a_settings_view_fails_with_its_statement() {
    let writer = writer().await;
    let view = view(true, true);
    let ctx = context(1, SITE, SITE, Default::default());
    let saved = settled(async |budget| {
        let mut write = writer.begin().await;
        if let Ok(write) = &mut write {
            write.fail_after(budget);
        }
        written(write, &ctx, &view, true).await
    });
    assert!(saved.await.is_ok());
}

/// `options` set as the Permalinks screen's save and the Reading save set
/// them.
async fn set(write: &mut Write<'_>, options: &[(&str, &str)]) {
    for (name, value) in options {
        let value = Value::String(value.as_bytes().to_vec());
        update_option(write, name, &value, None).await.unwrap();
    }
}

/// The Permalinks screen's view after `setup`, against the write oracle's
/// `golden`: the rules PHP stores.
async fn viewed_after(golden: &str, setup: impl AsyncFnOnce(&mut Write<'_>)) {
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        let mut write = writer.begin_on(conn).await.unwrap();
        setup(&mut write).await;
        write.commit().await.unwrap();
        Ok::<_, String>(())
    };
    let ctx = context(1, SITE, SITE, Default::default());
    let permalinks = view(false, true);
    let action = async |conn: &mut Connection, ()| {
        let viewed = written(writer.begin_on(conn).await, &ctx, &permalinks, true);
        let (write, _) = viewed.await.map_err(|err| format!("{err:?}"))?;
        write.commit().await.map_err(|err| err.to_string())
    };
    let report = check_writes(&fixture_database(), golden, setup, action).await;
    report.unwrap();
}

/// A view under each structure the screen offers, the rules left as the
/// default install keeps them, and after a structure change.
#[tokio::test]
async fn the_permalinks_view_refreshes_the_rules_as_php() {
    for (golden, structure, category_base, tag_base) in [
        ("day", "/%year%/%monthnum%/%day%/%postname%/", "", ""),
        ("month", "/%year%/%monthnum%/%postname%/", "", ""),
        ("numeric", "/archives/%post_id%", "", ""),
        ("postname", "/%postname%/", "", ""),
        (
            "custom",
            "/%category%/%author%/%postname%/",
            "/topics",
            "/labels",
        ),
    ] {
        let options = [
            ("permalink_structure", structure),
            ("category_base", category_base),
            ("tag_base", tag_base),
        ];
        let golden = format!("admin-permalink-view-{golden}");
        viewed_after(&golden, async |write| set(write, &options).await).await;
    }
    // A static front page's rules, stored by the Reading save, then post names.
    viewed_after("admin-permalink-view-changed", async |write| {
        set(write, &[("permalink_structure", "/archives/%post_id%")]).await;
        set(write, &[("show_on_front", "page"), ("page_on_front", "2")]).await;
        flush_rewrite_rules(write).await.unwrap();
        set(write, &[("permalink_structure", "/%postname%/")]).await;
    })
    .await;
}
