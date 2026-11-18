// SPDX-License-Identifier: AGPL-3.0-or-later

//! The categories and tags handlers through [`act`], against the write
//! oracle, their answers against `reference/admin-saves/golden.sh`'s.

use hyscribe_admin::terms::Taxonomy as Listed;
use hyscribe_db::{Connection, Writer};
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;
use hyscribe_write::terms::TermIds;

use super::super::tests::unread::Unread;
use super::super::tests::{acted, asked, fields, php_answer, writer};
use super::super::{Handler, ajax_handler, handler};
use super::*;

/// The term writes of these tests, which would wait on each other's locks.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// A request to `uri` by user 1, with `get` and `post` as query strings.
struct Sent<'a> {
    get: &'a str,
    post: &'a str,
    uri: &'a str,
}

const POSTED: Sent = Sent {
    get: "",
    post: "",
    uri: "/hs-admin/edit-tags",
};

/// The answer of `handler` to `sent`, as [`acted`].
async fn settled<T: super::super::Mailed>(
    writer: &Writer,
    conn: &mut Connection,
    handler: Handler<T>,
    sent: &Sent<'_>,
) -> T {
    let (get, post) = (fields(sent.get), fields(sent.post));
    let answer = acted(
        writer,
        conn,
        (handler, &Unread::default()),
        &asked(sent.uri, &get, &post),
    )
    .await;
    answer.map_err(|err| format!("{err:?}")).unwrap()
}

/// The location `answer` redirects to.
fn location(answer: Answer) -> String {
    let Answer::Redirect(to) = answer else {
        panic!("no redirect");
    };
    to
}

/// Terms added in one committed action on `conn`: each name in its
/// taxonomy, under the parent given (an index of an earlier one), with
/// `description` (slashed).
async fn added(
    writer: &Writer,
    conn: &mut Connection,
    terms: &[(&str, Taxonomy, Option<usize>)],
    description: &[u8],
) -> Vec<u64> {
    let mut write = writer.begin_on(conn).await.unwrap();
    let mut ids: Vec<u64> = Vec::new();
    for &(name, taxonomy, parent) in terms {
        let args = TermArgs {
            parent: parent.map(|at| ids[at] as i64),
            description: Some(description),
            ..TermArgs::default()
        };
        let TermIds { term_id, .. } = wp_insert_term(&mut write, taxonomy, name.as_bytes(), &args)
            .await
            .unwrap();
        ids.push(term_id);
    }
    write.commit().await.unwrap();
    ids
}

/// The term `answer` shows, if a row or an addition.
fn term_of(answer: &Ajax) -> &Term {
    match answer {
        Ajax::TermRow(_, term) | Ajax::TermAdded(_, Added { term, .. }) => term,
        _ => panic!("{answer:?}"),
    }
}

/// edit-tags.php's add form: the category's rows and the list's notice.
#[tokio::test]
async fn adding_is_php_s() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let action = async |conn: &mut Connection, ()| {
        let post = "action=add-tag&taxonomy=category&tag-name=Oracle%27s+added&slug=&parent=-1&description=Some+%3Cb%3Etext%3C%2Fb%3E+%5C+here&_wp_http_referer=%2Fhs-admin%2Fedit-tags%3Ftaxonomy%3Dcategory";
        let sent = Sent { post, ..POSTED };
        let to = location(
            settled(
                &writer,
                conn,
                handler("edit-tags", b"add-tag").unwrap(),
                &sent,
            )
            .await,
        );
        assert_eq!(to, php_answer("terms-add").1);
        Ok(())
    };
    let ok = async |_: &mut Connection| Ok(());
    check_writes(&fixture_database(), "admin-terms-add", ok, action)
        .await
        .unwrap();
}

/// The add form through admin-ajax.php: a child category, and Quick Edit
/// refused its parent's slug; then a tag.
#[tokio::test]
async fn adding_by_ajax_is_php_s() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let action = async |conn: &mut Connection, ()| {
        let post = "screen=edit-category&taxonomy=category&tag-name=Child+category&slug=&parent=1&description=";
        let answer = settled(
            &writer,
            conn,
            ajax_handler(b"add-tag").unwrap(),
            &Sent { post, ..POSTED },
        )
        .await;
        let Ajax::TermAdded(listed, added) = answer else {
            panic!("{answer:?}");
        };
        assert_eq!(listed, Listed::named(b"category").unwrap());
        let term = &added.term;
        assert_eq!(
            (&term.name[..], &term.slug[..]),
            (&b"Child category"[..], &b"child-category"[..])
        );
        assert_eq!((term.parent, term.count, added.term_group), (1, 0, 0));
        assert_ne!(added.term_taxonomy_id, 0);
        let post = "taxonomy=category&tax_ID=1&name=Uncategorized&slug=child-category";
        let answer = settled(
            &writer,
            conn,
            ajax_handler(b"inline-save-tax").unwrap(),
            &Sent { post, ..POSTED },
        )
        .await;
        assert_eq!(answer, Ajax::Text(php_answer("ajax-inline-duplicate").2));
        Ok(())
    };
    let ok = async |_: &mut Connection| Ok(());
    check_writes(&fixture_database(), "admin-terms-add-child", ok, action)
        .await
        .unwrap();
    let action = async |conn: &mut Connection, ()| {
        let post =
            "screen=edit-post_tag&taxonomy=post_tag&tag-name=Fresh+tag&slug=&description=About+it";
        let answer = settled(
            &writer,
            conn,
            ajax_handler(b"add-tag").unwrap(),
            &Sent { post, ..POSTED },
        )
        .await;
        let term = term_of(&answer);
        assert_eq!(
            (&term.name[..], &term.description[..]),
            (&b"Fresh tag"[..], &b"About it"[..])
        );
        Ok(())
    };
    check_writes(&fixture_database(), "admin-terms-add-tag", ok, action)
        .await
        .unwrap();
}

/// Quick Edit: the category renamed, its description stripped of its
/// backslash, answered with its row.
#[tokio::test]
async fn quick_edit_is_php_s() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        let terms = [("Oracle inline", Taxonomy::Category, None)];
        Ok(added(&writer, conn, &terms, br"Kept \\ text").await)
    };
    let action = async |conn: &mut Connection, ids: Vec<u64>| {
        let post = format!(
            "taxonomy=category&tax_ID={}&name=Quick+%26+named&slug=",
            ids[0]
        );
        let answer = settled(
            &writer,
            conn,
            ajax_handler(b"inline-save-tax").unwrap(),
            &Sent {
                post: &post,
                ..POSTED
            },
        )
        .await;
        let expected = Term {
            id: ids[0],
            name: b"Quick &amp; named".to_vec(),
            slug: b"quick-named".to_vec(),
            description: b"Kept  text".to_vec(),
            parent: 0,
            count: 0,
        };
        assert_eq!(
            answer,
            Ajax::TermRow(Listed::named(b"category").unwrap(), expected)
        );
        Ok(())
    };
    check_writes(&fixture_database(), "admin-terms-inline", setup, action)
        .await
        .unwrap();
}

/// The edit screen's save: the category renamed under a parent, then back
/// to the screen with its notice.
#[tokio::test]
async fn editing_is_php_s() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        let terms = [
            ("Oracle parent", Taxonomy::Category, None),
            ("Oracle edited", Taxonomy::Category, None),
        ];
        Ok(added(&writer, conn, &terms, b"").await)
    };
    let action = async |conn: &mut Connection, ids: Vec<u64>| {
        let post = format!(
            "action=editedtag&tag_ID={}&taxonomy=category&name=Oracle+%22edited%22&slug=Oracle+Edited+Slug&description=Line+%3Cscript%3Ex%3C%2Fscript%3E&parent={}&_wp_http_referer=%2Fhs-admin%2Fterm%3Ftaxonomy%3Dpost_tag%26tag_ID%3D801%26post_type%3Dpost",
            ids[1], ids[0]
        );
        let sent = Sent {
            post: &post,
            ..POSTED
        };
        let to = location(
            settled(
                &writer,
                conn,
                handler("edit-tags", b"editedtag").unwrap(),
                &sent,
            )
            .await,
        );
        assert_eq!(to, php_answer("terms-edited").1);
        Ok(())
    };
    check_writes(&fixture_database(), "admin-terms-edited", setup, action)
        .await
        .unwrap();
}

/// A row's Delete of a category with a child, then the list on its page.
#[tokio::test]
async fn deleting_is_php_s() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        let terms = [
            ("Oracle doomed", Taxonomy::Category, None),
            ("Oracle orphan", Taxonomy::Category, Some(0)),
        ];
        Ok(added(&writer, conn, &terms, b"").await)
    };
    let action = async |conn: &mut Connection, ids: Vec<u64>| {
        let get = format!("action=delete&taxonomy=category&tag_ID={}&paged=3", ids[0]);
        let uri = format!("/hs-admin/edit-tags?{get}&_wpnonce=x");
        let sent = Sent {
            get: &get,
            post: "",
            uri: &uri,
        };
        let to = location(
            settled(
                &writer,
                conn,
                handler("edit-tags", b"delete").unwrap(),
                &sent,
            )
            .await,
        );
        assert_eq!(to, php_answer("terms-delete-other").1);
        Ok(())
    };
    check_writes(&fixture_database(), "admin-terms-delete", setup, action)
        .await
        .unwrap();
}

/// The bulk Delete of two tags and a term no row has.
#[tokio::test]
async fn bulk_deleting_is_php_s() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        let terms = [
            ("Oracle one", Taxonomy::PostTag, None),
            ("Oracle two", Taxonomy::PostTag, None),
        ];
        Ok(added(&writer, conn, &terms, b"").await)
    };
    let action = async |conn: &mut Connection, ids: Vec<u64>| {
        let post = format!(
            "action=delete&taxonomy=post_tag&delete_tags[]={}&delete_tags[]={}&delete_tags[]={}&_wp_http_referer=%2Fhs-admin%2Fedit-tags%3Ftaxonomy%3Dcategory%26message%3D1%26error%3D1",
            ids[0],
            ids[1],
            1u64 << 40
        );
        let to = location(
            settled(
                &writer,
                conn,
                handler("edit-tags", b"bulk-delete").unwrap(),
                &Sent {
                    post: &post,
                    ..POSTED
                },
            )
            .await,
        );
        assert_eq!(to, php_answer("terms-bulk-missing").1);
        Ok(())
    };
    check_writes(
        &fixture_database(),
        "admin-terms-bulk-delete",
        setup,
        action,
    )
    .await
    .unwrap();
}

/// A row's Delete through admin-ajax.php.
#[tokio::test]
async fn deleting_by_ajax_is_php_s() {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let setup = async |conn: &mut Connection| {
        let terms = [("Oracle ajax doomed", Taxonomy::PostTag, None)];
        Ok(added(&writer, conn, &terms, b"").await)
    };
    let action = async |conn: &mut Connection, ids: Vec<u64>| {
        let post = format!("tag_ID={}&taxonomy=post_tag", ids[0]);
        let answer = settled(
            &writer,
            conn,
            ajax_handler(b"delete-tag").unwrap(),
            &Sent {
                post: &post,
                ..POSTED
            },
        )
        .await;
        assert_eq!(answer, Ajax::Text(php_answer("ajax-delete-tag").2));
        // A taxonomy not registered: `wp_die( 1 )`, nothing written.
        let post = format!("tag_ID={}&taxonomy=nope", ids[0]);
        let delete = ajax_handler(b"delete-tag").unwrap();
        let sent = Sent {
            post: &post,
            ..POSTED
        };
        let answer = settled(&writer, conn, delete, &sent).await;
        assert_eq!(answer, Ajax::Text(php_answer("ajax-delete-unknown").2));
        Ok(())
    };
    check_writes(
        &fixture_database(),
        "admin-terms-delete-ajax",
        setup,
        action,
    )
    .await
    .unwrap();
}

/// `handler`'s answer to `post`, with a tag the action adds first and
/// `{tag}` in `post` standing for it; nothing is kept.
async fn answer<T>(handler: Handler<T>, post: &str) -> Result<T, Stop<T>> {
    let _one = ONE_AT_A_TIME.lock().await;
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let args = TermArgs::default();
    let tag = wp_insert_term(&mut write, Taxonomy::PostTag, b"Answered", &args).await;
    let post = fields(&post.replace("{tag}", &tag.unwrap().term_id.to_string()));
    handler(
        &mut write,
        &asked("/hs-admin/edit-tags", &[], &post),
        &Unread::default(),
    )
    .await
}

/// The answers of `reference/admin-saves/golden.sh` that need a tag the
/// fixture lacks: the edit screen and Quick Edit of it as a category, and
/// its Delete in another taxonomy or one not registered.
#[tokio::test]
async fn answers_for_another_taxonomy_are_php_s() {
    let died = answer(edited_tag, "taxonomy=category&tag_ID={tag}&name=Other").await;
    let Err(Stop::Died(Answer::Died(message))) = died else {
        panic!("no wp_die()");
    };
    let page = hyscribe_admin::die::page(true, "WordPress &rsaquo; Error", message);
    assert_eq!(
        String::from_utf8(page).unwrap(),
        php_answer("terms-edited-other").2
    );
    let quick = answer(
        inline_save_tax,
        "taxonomy=category&tax_ID={tag}&name=Other&slug=other",
    )
    .await;
    assert!(matches!(quick, Ok(Ajax::Text(text)) if text == php_answer("ajax-inline-other").2));
    let post = "taxonomy=post_tag&screen=edit-post_tag&tag-name=answered";
    let exists = answer(ajax_add_tag, post).await;
    assert!(matches!(exists, Ok(Ajax::Xml(xml)) if xml == php_answer("ajax-add-tag-exists").2));
    // The default category is never deleted: `wp_die( 0 )`.
    let kept = answer(ajax_delete_tag, "tag_ID=1&taxonomy=category").await;
    assert!(matches!(kept, Ok(Ajax::Text(text)) if text == "0"));
    for (taxonomy, case) in [
        ("category", "ajax-delete-other"),
        ("nope", "ajax-delete-unknown"),
    ] {
        let post = format!("tag_ID={{tag}}&taxonomy={taxonomy}");
        let deleted = answer(ajax_delete_tag, &post).await;
        let died =
            matches!(deleted, Err(Stop::Died(Ajax::Text(text))) if text == php_answer(case).2);
        assert!(died, "{case}");
    }
}

/// What PHP cannot go on with, or what is not ported, is refused before
/// anything is written: taxonomies not written, arrays as fields, the rows
/// of another screen, and a Delete of no term.
#[tokio::test]
async fn unported_requests_write_nothing() {
    let unported =
        |result: Result<Ajax, Stop<Ajax>>| matches!(result, Err(Stop::Failed(Error::Unported(_))));
    for post in [
        "taxonomy=nav_menu&screen=edit-nav_menu&tag-name=X",
        "taxonomy[]=post_tag&tag-name=X",
        "taxonomy=post_tag&screen=edit-category&tag-name=X",
        "taxonomy=post_tag&screen=edit-post_tag&tag-name[]=X",
        "taxonomy=post_tag&screen=edit-post_tag&tag-name=X&alias_of[]=x",
    ] {
        assert!(unported(answer(ajax_add_tag, post).await), "{post}");
    }
    for post in [
        "taxonomy=link_category&tax_ID={tag}",
        "taxonomy=post_tag&tax_ID={tag}&description[]=x",
    ] {
        assert!(unported(answer(inline_save_tax, post).await), "{post}");
    }
    for post in [
        "tag_ID={tag}&taxonomy=nav_menu",
        "tag_ID={tag}&taxonomy[]=post_tag",
    ] {
        assert!(unported(answer(ajax_delete_tag, post).await), "{post}");
    }
    for (handler, post) in [
        (delete as Handler, "taxonomy=post_tag"),
        (delete, "taxonomy=nav_menu&tag_ID={tag}"),
        (bulk_delete, "taxonomy=nav_menu&delete_tags[]={tag}"),
        (add_tag, "taxonomy=nav_menu&tag-name=X"),
        (add_tag, "taxonomy=post_tag&tag-name=X&slug[]=x"),
        (edited_tag, "taxonomy=nav_menu&tag_ID={tag}"),
        (edited_tag, "taxonomy=post_tag&tag_ID={tag}&name[]=x"),
    ] {
        let refused = answer(handler, post).await;
        assert!(
            matches!(refused, Err(Stop::Failed(Error::Unported(_)))),
            "{post}"
        );
    }
}
