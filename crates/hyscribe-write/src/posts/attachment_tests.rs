// SPDX-License-Identifier: AGPL-3.0-or-later

//! The attachments' updates and deletes (`REST` media writes) against the
//! write oracle's goldens (`reference/writes/actions-media-rest.php`), and
//! what `wp_delete_attachment()` names as its files or does not port.

use super::trash::attachment_files;
use super::*;
use crate::options::update_option;
use crate::posts::tests::{act, check, shown};
use crate::tests::{ONE_AT_A_TIME, admin, fails_at_each_step, rolled_back, text, writer};
use hyscribe_db::{CommentFields, Connection};
use hyscribe_phpser::unserialize;

/// What an oracle attachment is made with, besides its own rows.
#[derive(Clone, Copy, PartialEq)]
enum Extras {
    None,
    /// A comment, and the image as post 1's featured image.
    Used,
    /// [`Extras::Used`], with the trash meta (and the status, set raw).
    UsedTrashed,
    /// The status of the trash, set raw.
    Trashed,
}

/// `serialize()` of the oracle's attachment's metadata.
fn metadata() -> Value {
    let size = |w: u32, h: u32, bytes: u32| {
        format!(
            r#"a:5:{{s:4:"file";s:18:"oracle-{w}x{h}.jpg";s:5:"width";i:{w};s:6:"height";i:{h};s:9:"mime-type";s:10:"image/jpeg";s:8:"filesize";i:{bytes};}}"#
        )
    };
    let image_meta = r#"a:12:{s:8:"aperture";s:1:"0";s:6:"credit";s:0:"";s:6:"camera";s:0:"";s:7:"caption";s:0:"";s:17:"created_timestamp";s:1:"0";s:9:"copyright";s:0:"";s:12:"focal_length";s:1:"0";s:3:"iso";s:1:"0";s:13:"shutter_speed";s:1:"0";s:5:"title";s:0:"";s:11:"orientation";s:1:"0";s:8:"keywords";a:0:{}}"#;
    let sizes = format!(
        r#"a:2:{{s:6:"medium";{}s:9:"thumbnail";{}}}"#,
        size(300, 200, 12345),
        size(150, 150, 5432)
    );
    let text = format!(
        r#"a:6:{{s:5:"width";i:1200;s:6:"height";i:800;s:4:"file";s:18:"2026/11/oracle.jpg";s:8:"filesize";i:345678;s:5:"sizes";{sizes}s:10:"image_meta";{image_meta}}}"#
    );
    unserialize(text.as_bytes()).unwrap()
}

/// The oracle's `$media_attachment( $args, $parent )` as attachment `id`,
/// and with `extras` what `$media_used` and the trashed cases add.
async fn oracle_attachment(
    write: &mut Write<'_>,
    ctx: &Context,
    (id, parent, name): (u64, u64, Option<&str>),
    extras: Extras,
) -> Result<()> {
    let args = PostArgs {
        import_id: id,
        title: b"A write oracle picture".to_vec(),
        content: b"Its description".to_vec(),
        excerpt: b"Its caption".to_vec(),
        mime_type: "image/jpeg".into(),
        author: Some(1),
        parent,
        name: name.map(|name| name.as_bytes().to_vec()),
        ..PostArgs::default()
    };
    let id = wp_insert_attachment(write, ctx, args, "2026/11/oracle.jpg").await?;
    let any = Value::String(Vec::new());
    let backups = br#"a:1:{s:9:"full-orig";a:3:{s:5:"width";i:1200;s:6:"height";i:800;s:4:"file";s:15:"oracle-orig.jpg";}}"#;
    for (key, value) in [
        ("_wp_attachment_metadata", metadata()),
        ("_wp_attachment_image_alt", text("Old alt")),
        ("_wp_attachment_backup_sizes", unserialize(backups).unwrap()),
    ] {
        update_metadata(write, Meta::Post, id, key, &value, &any).await?;
    }
    if matches!(extras, Extras::Used | Extras::UsedTrashed) {
        let comment = CommentFields {
            post_id: id,
            author: b"A reader".to_vec(),
            date: "2026-01-01 00:00:00".into(),
            date_gmt: "2026-01-01 00:00:00".into(),
            content: b"A comment.".to_vec(),
            approved: "1".into(),
            comment_type: "comment".into(),
            ..CommentFields::default()
        };
        write.insert_comment(&comment).await?;
        let image = Value::Int(id.cast_signed());
        add_metadata(write, Meta::Post, 1, "_thumbnail_id", &image, false).await?;
    }
    if extras == Extras::UsedTrashed {
        let inherit = text("inherit");
        add_metadata(
            write,
            Meta::Post,
            id,
            "_wp_trash_meta_status",
            &inherit,
            false,
        )
        .await?;
        let time = Value::Int(ctx.now);
        add_metadata(write, Meta::Post, id, "_wp_trash_meta_time", &time, false).await?;
    }
    Ok(())
}

/// The setup of an oracle case: its attachments written and committed,
/// and for a trashed one its status set as the oracle does, raw.
async fn setup(
    conn: &mut Connection,
    ctx: &Context,
    named: Option<&str>,
    (parent, extras): (u64, Extras),
) -> std::result::Result<u64, String> {
    let writer = writer().await;
    let id = 9301;
    let mut write = writer.begin_on(conn).await.map_err(shown)?;
    if let Some(name) = named {
        let taken = (id - 1, parent, Some(name));
        oracle_attachment(&mut write, ctx, taken, Extras::None)
            .await
            .map_err(shown)?;
    }
    let oracle = (id, parent, None);
    oracle_attachment(&mut write, ctx, oracle, extras)
        .await
        .map_err(shown)?;
    write.commit().await.map_err(shown)?;
    if matches!(extras, Extras::Trashed | Extras::UsedTrashed) {
        let sql = "UPDATE wp_posts SET post_status = 'trash' WHERE ID = ?";
        let trashed = sqlx::query(sql).bind(id).execute(&mut *conn).await;
        trashed.map_err(shown)?;
    }
    Ok(id)
}

/// `golden` of the write oracle: the oracle's attachment set up as `named`
/// and `kind` say, then `action` on it.
async fn oracle(
    golden: &str,
    named: Option<&str>,
    kind: (u64, Extras),
    action: impl AsyncFn(&mut Write<'_>, &Context, u64) -> Result<()>,
) {
    let writer = &writer().await;
    let ctx = admin();
    let setup = async |conn: &mut Connection| setup(conn, &ctx, named, kind).await;
    let action = async |conn: &mut Connection, id: u64| {
        let result = act(writer, conn, &ctx, async |w, ctx| action(w, ctx, id).await);
        result.await
    };
    check(golden, setup, action).await;
}

/// What the controller writes after the post: the template meta and the
/// alt text (`saved()` of `hyscribe-http`).
async fn after(w: &mut Write<'_>, id: u64, alt: Option<&str>) -> Result<()> {
    let any = Value::String(Vec::new());
    let empty = text("");
    update_metadata(w, Meta::Post, id, "_wp_page_template", &empty, &any).await?;
    if let Some(alt) = alt {
        let alt = text(alt);
        update_metadata(w, Meta::Post, id, "_wp_attachment_image_alt", &alt, &any).await?;
    }
    Ok(())
}

/// An update by `wp_update_post()` of the attachment `id`, as `saved()`
/// makes it.
async fn update(
    w: &mut Write<'_>,
    ctx: &Context,
    id: u64,
    change: &dyn Fn(&mut PostArgs),
) -> Result<()> {
    wp_update_post(w, ctx, id, change).await.map(drop)
}

#[tokio::test]
async fn media_updates_match_php() {
    let _one = ONE_AT_A_TIME.lock().await;
    let plain = (1, Extras::None);
    oracle("rest-media-update", None, plain, async |w, ctx, id| {
        let change = |args: &mut PostArgs| {
            args.title = b"<b>New</b> <script>x()</script>title".to_vec();
            args.excerpt =
                b"A <em>caption</em><iframe src=\"https://example.test/\"></iframe>".to_vec();
            args.content = b"<p onclick=\"y()\">Described</p>".to_vec();
            args.parent = 2;
            args.name = Some(b"renamed picture".to_vec());
            args.comment_status = "closed".into();
            args.ping_status = "closed".into();
        };
        update(w, ctx, id, &change).await?;
        after(w, id, Some("A new alt")).await
    })
    .await;
    oracle(
        "rest-media-update-private",
        None,
        plain,
        async |w, ctx, id| {
            let change = |args: &mut PostArgs| {
                args.status = "private".into();
                args.parent = 0;
                args.date = "2026-01-02 03:04:05".into();
                args.date_gmt = "2026-01-02 03:04:05".into();
                args.edit_date = true;
            };
            update(w, ctx, id, &change).await
        },
    )
    .await;
    oracle("rest-media-update-alt", None, plain, async |w, _, id| {
        let any = Value::String(Vec::new());
        let alt = text("");
        update_metadata(w, Meta::Post, id, "_wp_attachment_image_alt", &alt, &any).await?;
        Ok(())
    })
    .await;
    let slug =
        |name: &'static str| move |args: &mut PostArgs| args.name = Some(name.as_bytes().to_vec());
    oracle(
        "rest-media-update-slug",
        Some("taken-slug"),
        plain,
        async |w, ctx, id| update(w, ctx, id, &slug("taken-slug")).await,
    )
    .await;
    oracle(
        "rest-media-update-draft",
        None,
        plain,
        async |w, ctx, id| {
            let change = |args: &mut PostArgs| {
                args.status = "draft".into();
                args.name = Some(b"a-draft-slug".to_vec());
                args.author = Some(1);
            };
            update(w, ctx, id, &change).await
        },
    )
    .await;
    let trashed = (1, Extras::Trashed);
    oracle(
        "rest-media-update-trashed",
        None,
        trashed,
        async |w, ctx, id| {
            let change = |args: &mut PostArgs| args.title = b"Renamed in the trash".to_vec();
            update(w, ctx, id, &change).await
        },
    )
    .await;
}

/// The files of the oracle's attachment, in the order they are deleted.
fn oracle_files() -> Vec<AttachmentFile> {
    let file = |path: &str, within: &str| AttachmentFile {
        path: path.into(),
        within: within.into(),
    };
    vec![
        file("2026/11/oracle-300x200.jpg", "2026/11"),
        file("2026/11/oracle-150x150.jpg", "2026/11"),
        file("2026/11/oracle-orig.jpg", "2026/11"),
        file("2026/11/oracle.jpg", ""),
    ]
}

#[tokio::test]
async fn media_deletes_match_php() {
    let _one = ONE_AT_A_TIME.lock().await;
    let deleted = async |w: &mut Write<'_>, ctx: &Context, id: u64| {
        let files = wp_delete_attachment(w, ctx, id).await?;
        assert_eq!(files, Some(oracle_files()));
        assert!(w.post(id).await?.is_none());
        Ok(())
    };
    oracle("rest-media-delete", None, (1, Extras::Used), deleted).await;
    oracle(
        "rest-media-delete-trashed",
        None,
        (1, Extras::UsedTrashed),
        deleted,
    )
    .await;
    oracle(
        "rest-media-delete-unattached",
        None,
        (0, Extras::None),
        deleted,
    )
    .await;
}

/// Metadata and backups as `serialize()` gives them.
fn stored(text: &str) -> Value {
    unserialize(text.as_bytes()).unwrap()
}

#[test]
fn files_are_those_php_deletes_in_its_order() {
    let none = Value::Bool(false);
    let paths = |files: Vec<AttachmentFile>| -> Vec<(String, String)> {
        let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
        files
            .into_iter()
            .map(|file| (text(file.path), text(file.within)))
            .collect()
    };
    let pair = |path: &str, within: &str| (path.to_string(), within.to_string());
    // Sizes, the original image, the backups, then the file; a size without
    // a file name has the folder for its path, which is no file within it.
    let meta = stored(
        r#"a:3:{s:4:"file";s:13:"2026/11/a.jpg";s:5:"sizes";a:2:{s:5:"small";a:1:{s:4:"file";s:9:"a-100.jpg";}s:3:"bad";a:0:{}}s:14:"original_image";s:10:"a-orig.jpg";}"#,
    );
    let backups =
        stored(r#"a:2:{s:9:"full-orig";a:1:{s:4:"file";s:10:"a-orig.jpg";}s:5:"empty";a:0:{}}"#);
    let files = attachment_files(b"2026/11/a.jpg", &meta, &backups).unwrap();
    assert_eq!(
        paths(files),
        [
            pair("2026/11/a-100.jpg", "2026/11"),
            pair("2026/11/", "2026/11"),
            pair("2026/11/a-orig.jpg", "2026/11"),
            pair("2026/11/a-orig.jpg", "2026/11"),
            pair("2026/11/", "2026/11"),
            pair("2026/11/a.jpg", ""),
        ]
    );
    // A file without a folder, and what is not an array or not set.
    let bare = attachment_files(b"a.jpg", &none, &none).unwrap();
    assert_eq!(paths(bare), [pair("a.jpg", "")]);
    let odd = stored(r#"a:2:{s:5:"sizes";s:1:"x";s:14:"original_image";s:0:"";}"#);
    let files = attachment_files(b"a.jpg", &odd, &stored("s:1:\"x\";")).unwrap();
    assert_eq!(paths(files), [pair("a.jpg", "")]);
    // A bare file whose size has no name is the empty path, which PHP skips.
    let nameless = stored(r#"a:1:{s:5:"sizes";a:1:{s:1:"x";a:0:{}}}"#);
    let files = attachment_files(b"a.jpg", &nameless, &none).unwrap();
    assert_eq!(paths(files), [pair("a.jpg", "")]);
    // No file stored: nothing to delete.
    assert!(attachment_files(b"", &meta, &backups).unwrap().is_empty());
    // An absolute path, or the thumb of WordPress 2.6, is not ported.
    let thumb = stored(r#"a:1:{s:5:"thumb";s:5:"t.jpg";}"#);
    for (file, meta) in [(&b"/var/a.jpg"[..], &none), (b"a.jpg", &thumb)] {
        assert!(matches!(
            attachment_files(file, meta, &none),
            Err(Error::Unported(_))
        ));
    }
}

#[tokio::test]
async fn what_an_attachment_delete_does_not_port_is_refused() {
    let ctx = admin();
    let set = async |w: &mut Write<'_>, name: &str, value: &Value| {
        update_option(w, name, value, None).await.unwrap();
    };
    let id = 9301;
    rolled_back(async |w| {
        // Not an attachment, and missing.
        assert_eq!(wp_delete_attachment(w, &ctx, 1).await.unwrap(), None);
        assert_eq!(wp_delete_attachment(w, &ctx, id).await.unwrap(), None);
        let kind = (1, Extras::None);
        oracle_attachment(w, &ctx, (id, kind.0, None), kind.1)
            .await
            .unwrap();
        let unported = |result: Result<Option<Vec<AttachmentFile>>>| {
            assert!(matches!(result, Err(Error::Unported(_))), "{result:?}");
        };
        // A classic menu's item.
        let mid = Value::String(id.to_string().into_bytes());
        add_metadata(w, Meta::Post, 1, "_menu_item_object_id", &mid, false)
            .await
            .unwrap();
        unported(wp_delete_attachment(w, &ctx, id).await);
        delete_metadata(w, Meta::Post, 1, "_menu_item_object_id", &mid, false)
            .await
            .unwrap();
        // What the theme or the site uses: its logo, icon or images.
        let logo = stored(&format!("a:1:{{s:11:\"custom_logo\";i:{id};}}"));
        let header = stored(r#"a:1:{s:12:"header_image";s:5:"x.jpg";}"#);
        let background = stored(r#"a:1:{s:16:"background_image";s:5:"x.jpg";}"#);
        for mods in [logo, header, background] {
            set(w, "theme_mods_twentytwentyfive", &mods).await;
            unported(wp_delete_attachment(w, &ctx, id).await);
        }
        set(w, "theme_mods_twentytwentyfive", &Value::Bool(false)).await;
        for option in ["site_logo", "site_icon"] {
            set(w, option, &Value::Int(id.cast_signed())).await;
            unported(wp_delete_attachment(w, &ctx, id).await);
            set(w, option, &Value::Int(0)).await;
        }
        // A file that is not a relative path.
        let any = Value::String(Vec::new());
        let absolute = text("/var/www/a.jpg");
        update_metadata(w, Meta::Post, id, "_wp_attached_file", &absolute, &any)
            .await
            .unwrap();
        unported(wp_delete_attachment(w, &ctx, id).await);
        // Other functions leave an attachment to `wp_delete_attachment()`.
        unported(wp_delete_post(w, &ctx, id, true).await.map(|_| None));
        unported(wp_publish_post(w, &ctx, id).await.map(|()| None));
        unported(
            wp_insert_post(
                w,
                &ctx,
                PostArgs {
                    post_type: "attachment".into(),
                    title: b"Loose".to_vec(),
                    ..PostArgs::default()
                },
            )
            .await
            .map(|_| None),
        );
    })
    .await;
}

#[tokio::test]
async fn database_errors_end_an_attachment_delete() {
    let ctx = admin();
    fails_at_each_step(
        async |w| {
            oracle_attachment(w, &ctx, (9301, 1, None), Extras::Used)
                .await
                .unwrap();
        },
        async |w, ()| wp_delete_attachment(w, &ctx, 9301).await.map(drop),
    )
    .await;
}
