// SPDX-License-Identifier: AGPL-3.0-or-later

//! `revision.php`'s restore against the write oracle
//! (`admin-revision-restore` of `reference/writes/actions-posts-admin.php`).

use hyscribe_write::options::add_metadata;

use super::*;

/// The setup's footnotes of `id`.
async fn footnotes(write: &mut Write<'_>, id: u64, notes: &[u8]) -> Result<(), Error> {
    let notes = Value::String(notes.to_vec());
    add_metadata(write, Meta::Post, id, "footnotes", &notes, false).await?;
    Ok(())
}

/// The restore of post 1's first revision writes PHP's rows: the post as
/// the revision has it, a revision of that, `_edit_last` and the footnotes.
#[tokio::test]
async fn restores_write_as_php() {
    let _turn = turn().await;
    let writer = &writer().await;
    let revised = async |conn: &mut Connection| {
        setup(writer, conn, async |w, ctx| {
            wp_update_post(w, ctx, 1, &|a| a.content = b"First edit.".to_vec()).await?;
            let first = w.revision_ids(1).await?[0];
            footnotes(w, first, br#"[{"id":"n1","content":"A note."}]"#).await?;
            wp_update_post(w, ctx, 1, &|a| {
                a.title = b"Hello again".to_vec();
                a.content = b"Second edit.".to_vec();
                a.excerpt = b"Its excerpt.".to_vec();
            })
            .await?;
            footnotes(w, 1, b"[]").await?;
            Ok(first)
        })
        .await
    };
    check("admin-revision-restore", revised, async |conn, first| {
        let query = format!("revision={first}&action=restore");
        let answer = answered(
            writer,
            conn,
            (1, "revision", Constants::default()),
            (&query, ""),
        );
        let to = format!("{SITE}/hs-admin/post?post=1&action=edit&message=5&revision={first}");
        assert_eq!(answer.await?, Answer::Redirect(to));
        Ok(())
    })
    .await;
}

/// A revision gone since its checks, or one of a type whose revisions may
/// be turned off, is not ported.
#[tokio::test]
async fn restores_of_what_is_not_ported() {
    let _turn = turn().await;
    let restore = async |sql: &[String], id: u64| {
        let query = format!("revision={id}&action=restore");
        tried(1, sql, ("revision", &query, ""), "SELECT ''").await.0
    };
    assert!(unported(&restore(&[], 9999).await));
    let sql = [
        post_row(950, (1, "publish", "wp_block"), ("Block", "Text."), 0),
        post_row(951, (1, "inherit", "revision"), ("Block", "Text."), 950),
    ];
    assert!(unported(&restore(&sql, 951).await));
}
