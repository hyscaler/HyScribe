// SPDX-License-Identifier: AGPL-3.0-or-later

//! The screen state's writes against the write oracle
//! (`reference/writes/actions-dashboard.php`), each statement budget short
//! of success failing first, and `wp_user_settings()`'s choices.

use hyscribe_db::{Connection, Writer};
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;

use super::super::tests::{fields, writer};
use super::super::{Handler, act, handler};
use super::*;

/// The request of `query`, posted by user 1 to the posts list.
pub(in crate::admin::saves) fn asked(post: &[(Key, Value)]) -> Asked<'_> {
    Asked {
        get: &[],
        post,
        referer: None,
        uri: b"/hs-admin/edit?paged=2",
        home: "http://127.0.0.1:8081",
        user: 1,
    }
}

/// `handler` over the POST `query` as one action on `conn`, once every
/// statement budget short of it has failed, each failure rolled back.
pub(in crate::admin::saves) async fn answered(
    writer: &Writer,
    conn: &mut Connection,
    handler: Handler,
    query: &str,
) -> Result<Answer, Error> {
    let post = fields(query);
    let mut budget = 0;
    loop {
        let mut write = writer.begin_on(conn).await;
        if let Ok(write) = &mut write {
            write.fail_after(budget);
        }
        match act(write, handler, &asked(&post)).await {
            Err(Error::Db(_)) => budget += 1,
            answer => return answer,
        }
    }
}

/// `f` as one action on `conn` once every shorter statement budget failed.
pub(in crate::admin::saves) async fn cut<T>(
    writer: &Writer,
    conn: &mut Connection,
    f: impl AsyncFn(&mut Write<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    let mut budget = 0;
    loop {
        let mut write = writer.begin_on(conn).await?;
        write.fail_after(budget);
        match f(&mut write).await {
            Err(Error::Db(_)) => budget += 1,
            done => return Ok((done?, write.commit().await?).0),
        }
    }
}

/// The oracle's case `golden` of `query` posted to the action `name`.
pub(in crate::admin::saves) async fn ajax(name: &str, golden: &str, query: &str, answer: &str) {
    let writer = writer().await;
    let action = async |conn: &mut Connection, ()| {
        let handler = handler("admin-ajax", name.as_bytes()).unwrap();
        let saved = answered(&writer, conn, handler, query).await;
        assert!(
            matches!(saved, Ok(Answer::Page(body)) if body == answer),
            "{name}"
        );
        Ok(())
    };
    let report = check_writes(&fixture_database(), golden, async |_| Ok(()), action).await;
    report.unwrap();
}

#[tokio::test]
async fn the_scripts_saves_are_php_s() {
    let closed = "closed=dashboard_activity,,0&hidden=dashboard_primary,submitdiv&page=dashboard";
    ajax(
        "closed-postboxes",
        "admin-ajax-closed-postboxes",
        closed,
        "1",
    )
    .await;
    let hidden = "hidden=author,tags&page=edit-post";
    ajax("hidden-columns", "admin-ajax-hidden-columns", hidden, "1").await;
    let order = "order[normal]=dashboard_quick_press,dashboard_site_health\
        &order[side]=dashboard_right_now&page_columns=2&page=dashboard";
    let success = "{\"success\":true}";
    ajax(
        "meta-box-order",
        "admin-ajax-meta-box-order",
        order,
        success,
    )
    .await;
}

#[tokio::test]
async fn the_screen_options_form_is_php_s() {
    let writer = writer().await;
    let action = async |conn: &mut Connection, ()| {
        let form = "wp_screen_options[option]=edit_post_per_page&wp_screen_options[value]=5";
        let saved = answered(&writer, conn, options, form).await;
        assert!(matches!(saved, Ok(Answer::Redirect(to)) if to == "/hs-admin/edit"));
        Ok(())
    };
    let db = fixture_database();
    let report = check_writes(&db, "admin-screen-options", async |_| Ok(()), action).await;
    report.unwrap();
}

/// The settings `found` with `mode`, saved as user 1.
async fn settings(writer: &Writer, conn: &mut Connection, found: &Found<'_>, mode: Option<&[u8]>) {
    let synced = sync(found, mode, crate::now());
    let saved = cut(writer, conn, async |w| {
        Ok(save_settings(w, 1, &synced, (crate::now(), true)).await?)
    });
    saved.await.unwrap();
}

#[tokio::test]
async fn the_settings_a_view_saves_are_php_s() {
    let writer = writer().await;
    let found = |cookie| Found {
        cookie: Some(cookie),
        time: Some(b"1"),
        stored: None,
        saved: None,
    };
    let newer = async |conn: &mut Connection, ()| {
        settings(&writer, conn, &found(b"mfold=f&editor=html"), None).await;
        Ok(())
    };
    let db = fixture_database();
    let report = check_writes(&db, "admin-user-settings-newer", async |_| Ok(()), newer).await;
    report.unwrap();
    let mode = async |conn: &mut Connection, ()| {
        settings(&writer, conn, &found(b"mfold=f"), Some(b"excerpt")).await;
        Ok(())
    };
    let report = check_writes(&db, "admin-user-settings-mode", async |_| Ok(()), mode).await;
    report.unwrap();
}

fn text_value(text: &str) -> Value {
    Value::String(text.as_bytes().to_vec())
}

/// `wp_user_settings()`'s choices and `set_user_setting()`'s settings.
#[test]
fn settings_sync_as_php_s() {
    let found =
        |cookie: Option<&'static [u8]>, time: Option<&'static [u8]>, stored: Option<&str>| Found {
            cookie,
            time,
            stored: stored.map(text_value),
            saved: Some(Value::Int(100)),
        };
    let set = |settings: &str| Synced {
        cookie: Some(settings.as_bytes().to_vec()),
        ..Synced::default()
    };
    // No cookie: the cookies are set from the option.
    assert_eq!(sync(&found(None, None, None), None, 200), set(""));
    assert_eq!(sync(&found(None, None, Some("a=1")), None, 200), set("a=1"));
    // The same settings (once cleaned): nothing.
    let same = found(Some(b"a=1!"), None, Some("a=1"));
    assert_eq!(sync(&same, None, 200), Synced::default());
    // A newer cookie is saved, an older one replaced.
    let newer = Synced {
        newer: Some(b"b=2".to_vec()),
        ..Synced::default()
    };
    assert_eq!(
        sync(&found(Some(b"b=2"), Some(b"1x01"), Some("a=1")), None, 200),
        newer
    );
    assert_eq!(
        sync(&found(Some(b"b=2"), Some(b"100"), Some("a=1")), None, 200),
        set("a=1")
    );
    // A list's mode joins the cookie's settings, cleaned, or the option's.
    let mode =
        |found: &Found, mode: &[u8]| sync(found, Some(mode), 200).mode.map(String::from_utf8);
    let cookie = found(
        Some(b"a=1&0=x&b[]=y&c=d.e&posts_list_mode=list"),
        None,
        Some("z=9"),
    );
    let joined = "a=1&b=y&c=de&posts_list_mode=excerpt";
    let array = found(None, None, Some("b[]=y"));
    assert_eq!(
        mode(&array, b"list"),
        Some(Ok("b=Array&posts_list_mode=list".into()))
    );
    assert_eq!(mode(&cookie, b"excerpt"), Some(Ok(joined.into())));
    assert_eq!(
        mode(&found(Some(b"=a"), None, Some("z=9")), b"x"),
        Some(Ok("posts_list_mode=list".into()))
    );
    assert_eq!(
        mode(&found(None, None, Some("z=9")), b"list"),
        Some(Ok("z=9&posts_list_mode=list".into()))
    );
    assert_eq!(
        mode(&found(None, None, Some("0")), b"list"),
        Some(Ok("posts_list_mode=list".into()))
    );
    // Once the mode is saved, the cookie is older than the settings.
    let saved = sync(&found(Some(b"z=9"), Some(b"150"), None), Some(b"list"), 200);
    assert_eq!(saved.cookie, saved.mode);
    let later = sync(&found(Some(b"z=9"), Some(b"300"), None), Some(b"list"), 200);
    assert_eq!(later.newer, Some(b"z=9".to_vec()));
}

/// `handler` over the POST `query` in `write`, which is rolled back with it.
pub(in crate::admin::saves) async fn run(
    write: &mut Write<'_>,
    handler: Handler,
    query: &str,
) -> Result<Answer, super::super::Stop> {
    let post = fields(query);
    handler(write, &asked(&post)).await
}

/// User 1's meta `key`, as stored.
pub(in crate::admin::saves) async fn meta(write: &mut Write<'_>, key: &str) -> Option<Vec<u8>> {
    let rows = write.object_meta(Meta::User, 1).await.unwrap();
    rows.into_iter().find(|(name, _)| name == key.as_bytes())?.1
}

/// What PHP does not save, or cannot: lists given as arrays, screen options
/// a default install does not save, a mode given as an array.
#[tokio::test]
async fn what_is_not_saved() {
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let unported = |saved| matches!(saved, Err(super::super::Stop::Failed(Error::Unported(_))));
    assert!(unported(
        run(&mut write, closed_postboxes, "closed[]=a&page=x").await
    ));
    assert!(unported(
        run(&mut write, hidden_columns, "hidden[]=a&page=x").await
    ));
    for form in [
        "wp_screen_options[value]=5",
        "wp_screen_options[option]=Edit_post_per_page&wp_screen_options[value]=5",
        "wp_screen_options[option]=edit_post_per_page&wp_screen_options[value]=0",
        "wp_screen_options[option]=edit_post_per_page&wp_screen_options[value]=1000",
        "wp_screen_options[option]=layout_columns&wp_screen_options[value]=2",
        "wp_screen_options[option]=edit_post_per_page&wp_screen_options[value]=5&mode[]=x",
    ] {
        assert!(unported(run(&mut write, options, form).await), "{form}");
    }
    let referer = "&_wp_http_referer=%2Fhs-admin%2Fedit-tags%3Fpaged%3D3%26taxonomy%3Dcategory";
    let terms = "wp_screen_options[option]=edit_category_per_page&wp_screen_options[value]=7";
    let to = run(&mut write, options, &format!("{terms}{referer}&mode=a'b")).await;
    let back = "/hs-admin/edit-tags?taxonomy=category&mode=a\\'b";
    assert!(matches!(to, Ok(Answer::Redirect(to)) if to == back));
    let users = "wp_screen_options[option]=users-per-page&wp_screen_options[value]=7";
    let to = run(&mut write, options, users).await;
    assert!(matches!(to, Ok(Answer::Redirect(to)) if to == "/hs-admin/edit"));
    let seven = Some(b"7".to_vec());
    assert_eq!(meta(&mut write, "edit_category_per_page").await, seven);
    assert_eq!(meta(&mut write, "users_per_page").await, seven);
    // A lone order, columns given as an array, an empty order kept.
    for query in [
        "order=a&page_columns[]=x&page=p",
        "order[]=&page_columns=0&page=q",
    ] {
        assert!(run(&mut write, meta_box_order, query).await.is_ok());
    }
    let order = |text: &str| Some(text.as_bytes().to_vec());
    assert_eq!(
        meta(&mut write, "meta-box-order_p").await,
        order("a:1:{i:0;s:1:\"a\";}")
    );
    assert_eq!(meta(&mut write, "screen_layout_p").await, order("1"));
    assert_eq!(
        meta(&mut write, "meta-box-order_q").await,
        order("a:1:{i:0;s:0:\"\";}")
    );
    assert_eq!(meta(&mut write, "screen_layout_q").await, None);
    // No order, the columns left to the screen; a backslash kept, as PHP
    // slashes the request.
    assert!(run(&mut write, meta_box_order, "page=r").await.is_ok());
    assert_eq!(meta(&mut write, "meta-box-order_r").await, None);
    assert_eq!(meta(&mut write, "screen_layout_r").await, order("auto"));
    assert!(
        run(&mut write, hidden_columns, "hidden=0&page=h")
            .await
            .is_ok()
    );
    assert_eq!(
        meta(&mut write, "managehcolumnshidden").await,
        order("a:0:{}")
    );
    assert!(
        run(&mut write, closed_postboxes, "closed=a%5Cb&page=c")
            .await
            .is_ok()
    );
    let closed = order("a:1:{i:0;s:3:\"a\\b\";}");
    assert_eq!(meta(&mut write, "closedpostboxes_c").await, closed);
}
