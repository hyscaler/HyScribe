// SPDX-License-Identifier: AGPL-3.0-or-later

//! The dashboard's writes against the write oracle
//! (`reference/writes/actions-dashboard.php`), each statement budget short
//! of success failing first.

use hyscribe_db::Connection;
use hyscribe_testkit::fixture_database;
use hyscribe_testkit::writes::check_writes;

use super::super::Stop;
use super::super::screen::tests::{ajax, answered, cut, run};
use super::super::tests::unread::Unread;
use super::super::tests::{SITE, asked, fields, writer};
use super::*;

/// `$views_draft()`: the Quick Draft form's auto-draft, user 1's.
async fn drafted(write: &mut Write<'_>) -> Result<u64, Error> {
    quick_press_draft(write, &context(1, SITE, SITE, Default::default())).await
}

fn shown(err: Error) -> String {
    format!("{err:?}")
}

/// The save, from the auto-draft the form named, then the form's next one.
#[tokio::test]
async fn the_quick_draft_save_is_php_s() {
    let writer = writer().await;
    let setup = async |conn: &mut Connection| cut(&writer, conn, drafted).await.map_err(shown);
    let action = async |conn: &mut Connection, id: u64| {
        let query = format!(
            "action=post-quickdraft-save&post_ID={id}&post_type=post&post_title=A+quick+draft\
             &content=One+%26+%22two%22%0D%0Athree"
        );
        let saved = answered(&writer, conn, quickdraft_save, &query).await;
        assert!(matches!(saved, Ok(Answer::Page(page)) if page.is_empty()));
        cut(&writer, conn, drafted).await.map_err(shown)?;
        Ok(())
    };
    let report = check_writes(&fixture_database(), "admin-quickdraft-save", setup, action).await;
    report.unwrap();
}

#[tokio::test]
async fn the_welcome_panel_s_state_is_php_s() {
    let writer = writer().await;
    let hidden = async |conn: &mut Connection, ()| {
        cut(&writer, conn, async |w| welcome(w, 1, false).await)
            .await
            .map_err(shown)
    };
    let db = fixture_database();
    let report = check_writes(&db, "admin-welcome-hidden", async |_| Ok(()), hidden).await;
    report.unwrap();
    ajax(
        "update-welcome-panel",
        "admin-ajax-welcome-panel",
        "visible=",
        super::super::Ajax::Text("1".into()),
    )
    .await;
}

#[test]
fn line_breaks_are_php_s() {
    assert_eq!(
        breaks(b"a\r\nb\rc\nd\n\re"),
        b"a<br />b<br />c<br />d<br /><br />e"
    );
}

/// A post the form does not name is not ported; a title or content not
/// sent is kept, and content already a paragraph block is kept as sent.
#[tokio::test]
async fn quick_drafts_php_keeps_as_they_are() {
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    for id in ["1", "0", "-4"] {
        let query = format!("action=post-quickdraft-save&post_ID={id}");
        let saved = run(&mut write, quickdraft_save, &query).await;
        assert!(
            matches!(saved, Err(Stop::Failed(Error::Unported(_)))),
            "{id}"
        );
    }
    let saved = async |write: &mut Write<'_>, query: &str| {
        let id = drafted(write).await.unwrap();
        let saved = run(write, quickdraft_save, &format!("post_ID={id}&{query}")).await;
        assert!(saved.is_ok());
        let post = write.post(id).await.unwrap().unwrap();
        (post.title, post.content)
    };
    // `wp_filter_post_kses()` closes the block.
    let marked = "content=+<!--+wp:paragraph+-->x";
    let block = b" <!-- wp:paragraph -->x<!-- /wp:paragraph -->".to_vec();
    let kept = (b"Auto Draft".to_vec(), block);
    assert_eq!(saved(&mut write, marked).await, kept);
    let titled = (b"Titled".to_vec(), b"0".to_vec());
    assert_eq!(
        saved(&mut write, "post_title=Titled&content=0").await,
        titled
    );
    let long = "A line longer than the mark of a block";
    let wrapped = format!("<!-- wp:paragraph -->{long}<!-- /wp:paragraph -->");
    let ours = saved(&mut write, &format!("content={long}")).await;
    assert_eq!(ours, (b"Auto Draft".to_vec(), wrapped.into_bytes()));
    let untouched = (b"Only".to_vec(), Vec::new());
    assert_eq!(saved(&mut write, "post_title=Only").await, untouched);
}

/// A Quick Draft keeps a PDF from the uploads, which `wp_upload_dir()`
/// puts under `siteurl` (the site's `admin_url()`), not under a home URL
/// elsewhere.
#[tokio::test]
async fn quick_drafts_keep_pdfs_from_the_site_url() {
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let id = drafted(&mut write).await.unwrap();
    let pdf = "<object type=\"application/pdf\" data=\"http://127.0.0.1:8081/a.pdf\"></object>";
    let post = fields(&format!("post_ID={id}&content={}", pdf.replace(' ', "+")));
    let asked = Asked {
        home: "http://home.example.test",
        ..asked("/hs-admin/post", &[], &post)
    };
    assert!(
        quickdraft_save(&mut write, &asked, &Unread::default())
            .await
            .is_ok()
    );
    let content = write.post(id).await.unwrap().unwrap().content;
    let paragraph = format!("<!-- wp:paragraph -->{pdf}<!-- /wp:paragraph -->");
    assert_eq!(String::from_utf8(content).unwrap(), paragraph);
}

/// An administrator's Quick Draft is saved unfiltered where the `.env`
/// allows unfiltered HTML.
#[tokio::test]
async fn quick_drafts_follow_unfiltered_html() {
    let writer = writer().await;
    let mut write = writer.begin().await.unwrap();
    let id = drafted(&mut write).await.unwrap();
    let post = fields(&format!(
        "post_ID={id}&content=%3Cscript%3Ex%3C%2Fscript%3E"
    ));
    let asked = asked("/hs-admin/post", &[], &post);
    let site = crate::admin::saves::tests::unfiltered::Unfiltered { authors: false };
    assert!(quickdraft_save(&mut write, &asked, &site).await.is_ok());
    let content = write.post(id).await.unwrap().unwrap().content;
    let paragraph = "<!-- wp:paragraph --><script>x</script><!-- /wp:paragraph -->";
    assert_eq!(String::from_utf8(content).unwrap(), paragraph);
}
