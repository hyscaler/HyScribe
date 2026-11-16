// SPDX-License-Identifier: AGPL-3.0-or-later

//! The block editor's view writes against the write oracle
//! (`reference/writes/actions-editor.php`).

use super::*;
use hyscribe_db::Connection;
use hyscribe_testkit::{fixture_database, writes::check_writes};

use crate::tests::{ONE_AT_A_TIME, admin, writer};

const THEME: (&str, &str) = ("twentytwentyfive", "Twenty Twenty-Five");

async fn view(
    conn: &mut Connection,
    ctx: &Context,
    edited: Edited<'_>,
) -> std::result::Result<u64, String> {
    let writer = writer().await;
    let mut write = writer.begin_on(conn).await.map_err(|e| format!("{e:?}"))?;
    let id = block_editor_view(&mut write, ctx, edited, THEME)
        .await
        .map_err(|e| format!("{e:?}"))?;
    write.commit().await.map_err(|e| format!("{e:?}"))?;
    Ok(id)
}

/// `$editor_viewed` of actions-editor.php: a site whose editor was viewed.
async fn viewed(conn: &mut Connection) -> std::result::Result<(), String> {
    let writer = writer().await;
    let mut write = writer.begin_on(conn).await.map_err(|e| format!("{e:?}"))?;
    let ctx = admin();
    get_user_global_styles_post_id(&mut write, &ctx, THEME.0)
        .await
        .map_err(|e| format!("{e:?}"))?;
    custom_css_post_lookup(&mut write, THEME.0, THEME.1)
        .await
        .map_err(|e| format!("{e:?}"))?;
    write.commit().await.map_err(|e| format!("{e:?}"))
}

async fn check<T>(
    golden: &str,
    setup: impl AsyncFnOnce(&mut Connection) -> std::result::Result<T, String>,
    action: impl AsyncFnOnce(&mut Connection, T) -> std::result::Result<(), String>,
) {
    if let Err(differences) = check_writes(&fixture_database(), golden, setup, action).await {
        panic!("{differences}");
    }
}

#[tokio::test]
async fn editor_views_match_php() {
    let _one = ONE_AT_A_TIME.lock().await;
    let nothing = async |_: &mut Connection| Ok(());
    let new =
        async |conn: &mut Connection, ()| view(conn, &admin(), Edited::New("post")).await.map(drop);
    check("admin-editor-new", nothing, new).await;
    let unfiltered = Context {
        unfiltered_html: true,
        ..admin()
    };
    let new = async |conn: &mut Connection, ()| {
        view(conn, &unfiltered, Edited::New("post")).await.map(drop)
    };
    check("admin-editor-new-unfiltered-html", nothing, new).await;
    let page =
        async |conn: &mut Connection, ()| view(conn, &admin(), Edited::New("page")).await.map(drop);
    check("admin-editor-new-page", viewed, page).await;
    let edit =
        async |conn: &mut Connection, ()| view(conn, &admin(), Edited::Existing(1)).await.map(drop);
    check("admin-editor-edit", viewed, edit).await;
}

/// Runs `f` in a transaction that is rolled back.
async fn rolled_back<T>(f: impl AsyncFnOnce(&mut Write<'_>) -> Result<T>) -> Result<T> {
    let writer = writer().await;
    let mut conn = crate::tests::connection().await;
    let mut outer = sqlx::Connection::begin(&mut conn).await.unwrap();
    let mut write = writer.begin_on(&mut outer).await.unwrap();
    f(&mut write).await
}

async fn set(write: &mut Write<'_>, name: &str, value: &[u8]) {
    write.add_option(name, value, "auto").await.unwrap();
}

/// The theme mod's value after the lookup, or the lookup's error.
async fn looked_up(setup: impl AsyncFnOnce(&mut Write<'_>)) -> Result<Value> {
    rolled_back(async |w| {
        setup(w).await;
        custom_css_post_lookup(w, THEME.0, THEME.1).await?;
        Ok(crate::options::get_option(w, "theme_mods_twentytwentyfive").await?)
    })
    .await
}

#[tokio::test]
async fn the_custom_css_lookup_is_kept_as_php_keeps_it() {
    let mods =
        |value: &str| format!("a:2:{{s:4:\"seen\";b:1;s:18:\"custom_css_post_id\";{value}}}");
    let parsed = |value: &str| hyscribe_phpser::unserialize(mods(value).as_bytes()).unwrap();
    // A lookup kept, and one of a post that is still there, stay.
    for kept in ["i:-1;", "i:1;"] {
        let found =
            looked_up(async |w| set(w, "theme_mods_twentytwentyfive", mods(kept).as_bytes()).await)
                .await;
        assert_eq!(found.unwrap(), parsed(kept), "{kept}");
    }
    // One of a missing post, or not an ID, is looked up again.
    for again in ["i:999;", "i:0;", "s:2:\"-1\";"] {
        let found = looked_up(async |w| {
            set(w, "theme_mods_twentytwentyfive", mods(again).as_bytes()).await
        })
        .await;
        assert_eq!(found.unwrap(), parsed("i:-1;"), "{again}");
    }
    let unported = |result: Result<Value>| matches!(result, Err(Error::Unported(_)));
    assert!(unported(
        looked_up(async |w| set(w, "theme_mods_twentytwentyfive", b"s:1:\"x\";").await).await
    ));
    assert!(unported(
        looked_up(async |w| set(w, "mods_Twenty Twenty-Five", b"a:0:{}").await).await
    ));
    let named = async |w: &mut Write<'_>| {
        set(w, "current_theme", b"Renamed").await;
        set(w, "mods_Renamed", b"a:0:{}").await;
    };
    assert!(unported(looked_up(named).await));
    // Mods kept under a name the theme no longer has stay where they are.
    let renamed = async |w: &mut Write<'_>| {
        set(w, "current_theme", b"Renamed").await;
        set(w, "mods_Twenty Twenty-Five", b"a:0:{}").await;
    };
    let fresh = hyscribe_phpser::unserialize(b"a:1:{s:18:\"custom_css_post_id\";i:-1;}").unwrap();
    assert_eq!(looked_up(renamed).await.unwrap(), fresh);
    // A custom CSS post is not looked up.
    let css = async |w: &mut Write<'_>| {
        let mut post = w.post(1).await.unwrap().unwrap();
        post.post_type = "custom_css".into();
        post.name = b"twentytwentyfive".to_vec();
        w.insert_post(&post).await.unwrap();
    };
    assert!(unported(looked_up(css).await));
}

/// The newest published global styles post of the theme is its own, the
/// first of a tie.
#[tokio::test]
async fn the_newest_global_styles_post_is_the_theme_s() {
    let found = rolled_back(async |w| {
        let ctx = admin();
        let first = get_user_global_styles_post_id(w, &ctx, THEME.0).await?;
        let mut ids = vec![first];
        for (date, status) in [
            ("2001-01-01 00:00:00", "publish"),
            ("2098-01-01 00:00:00", "publish"),
            ("2098-01-01 00:00:00", "publish"),
            ("2099-01-01 00:00:00", "draft"),
        ] {
            let mut post = w.post(first).await?.unwrap();
            (post.date, post.status) = (date.into(), status.into());
            let id = w.insert_post(&post).await?;
            let theme = [TermRef::Text(THEME.0.as_bytes())];
            wp_set_post_terms(w, id, TermList::List(&theme), Taxonomy::WpTheme, true).await?;
            ids.push(id);
        }
        let again = get_user_global_styles_post_id(w, &ctx, THEME.0).await?;
        Ok((ids, again))
    })
    .await
    .unwrap();
    assert_eq!(found.1, found.0[2]);
    // With only a draft, one is created.
    let created = rolled_back(async |w| {
        let ctx = admin();
        let first = get_user_global_styles_post_id(w, &ctx, THEME.0).await?;
        let mut draft = w.post(first).await?.unwrap();
        draft.status = "draft".into();
        w.update_post(first, &draft).await?;
        let again = get_user_global_styles_post_id(w, &ctx, THEME.0).await?;
        Ok((first, again))
    })
    .await
    .unwrap();
    assert_ne!(created.0, created.1);
}

/// A page has no post format, which a default one is not ported for.
#[tokio::test]
async fn a_page_takes_no_default_format() {
    let drafted = rolled_back(async |w| {
        w.add_option("default_post_format", b"aside", "auto")
            .await?;
        let page = get_default_post_to_edit(w, &admin(), "page").await;
        let post = get_default_post_to_edit(w, &admin(), "post").await;
        Ok((page.is_ok(), matches!(post, Err(Error::Unported(_)))))
    })
    .await
    .unwrap();
    assert_eq!(drafted, (true, true));
}

/// A database error at any statement of an editor's first view, or of a
/// later one with the theme's posts and mods in place, ends it.
#[tokio::test]
async fn database_errors_end_editor_views() {
    let new = async |w: &mut Write<'_>, ()| {
        block_editor_view(w, &admin(), Edited::New("post"), THEME)
            .await
            .map(drop)
    };
    crate::tests::fails_at_each_step(async |_: &mut Write<'_>| (), new).await;
    let seen = async |w: &mut Write<'_>| {
        get_user_global_styles_post_id(w, &admin(), THEME.0)
            .await
            .unwrap();
        let mods = b"a:1:{s:18:\"custom_css_post_id\";i:1;}";
        w.add_option("theme_mods_twentytwentyfive", mods, "auto")
            .await
            .unwrap();
    };
    let edit = async |w: &mut Write<'_>, ()| {
        block_editor_view(w, &admin(), Edited::Existing(1), THEME)
            .await
            .map(drop)
    };
    crate::tests::fails_at_each_step(seen, edit).await;
}
