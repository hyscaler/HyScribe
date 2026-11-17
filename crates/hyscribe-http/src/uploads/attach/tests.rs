// SPDX-License-Identifier: AGPL-3.0-or-later

//! The uploads' rows against the write oracle
//! (`reference/writes/actions-uploads.php`): each file of the corpus
//! prepared as `media_handle_upload()` or the attachments controller
//! prepares it, inserted and its metadata saved in two actions as the
//! routes write them, on the fixture inside a transaction rolled back.

use hyscribe_config::{Constants, ImageLimits, Mode};
use hyscribe_db::Connection;
use hyscribe_testkit::{fixture_database, writes::check_writes};

use super::*;
use crate::now;

/// A file of the image corpus.
fn corpus(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../hyscribe-image/tests/corpus/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).unwrap()
}

/// The corpus file `source` sent as `name`.
fn sent(source: &str, name: &str) -> Sent {
    Sent::new(name.into(), Vec::new(), corpus(source).into(), 64 << 20)
}

/// The oracle's user 1 writing at `now`, who may not post unfiltered HTML.
fn context() -> Context {
    Context {
        user: 1,
        can_publish: true,
        home: b"http://127.0.0.1:8081".to_vec(),
        upload_host: b"127.0.0.1:8081".to_vec(),
        unfiltered_html: false,
        constants: Constants::default(),
        now: now(),
    }
}

const WHO: Who = Who {
    unfiltered_html: false,
    unfiltered_upload: false,
};

/// How an upload is prepared: `media_handle_upload()` to a post, or the
/// controller's insert from a form or a body.
#[derive(Clone)]
enum How {
    Media(u64),
    Rest(bool, Upload),
}

/// The upload of `source` as `name`, written to `conn` as the routes write
/// it: the attachment in one action, its metadata in another.
async fn uploaded(
    (site, uploads): (&Site, &Uploads),
    conn: &mut Connection,
    (source, name): (&str, &str),
    how: How,
) -> Result<(), String> {
    let writer = hyscribe_db::Writer::open(&fixture_database(), Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    let post = match &how {
        How::Media(post) => *post,
        How::Rest(_, upload) => upload.post,
    };
    let parent = site.db.post(post).await.unwrap();
    let time = upload_time(site, parent, now());
    let prepared = match how {
        How::Media(post) => {
            uploads.media_handle_upload(site, sent(source, name), post, (&time, WHO))
        }
        How::Rest(form, upload) => {
            uploads.rest_upload(site, (sent(source, name), form), &upload, (&time, WHO))
        }
    };
    let prepared = prepared.map_err(|refused| format!("{refused:?}"))?;
    let ctx = context();
    let mut write = writer.begin_on(conn).await.map_err(|e| e.to_string())?;
    let id = attach(&mut write, &ctx, &prepared)
        .await
        .map_err(|e| format!("{e:?}"))?;
    write.commit().await.map_err(|e| e.to_string())?;
    let generated = uploads.generate(site, &prepared.handled, id).unwrap();
    let mut write = writer.begin_on(conn).await.map_err(|e| e.to_string())?;
    save(&mut write, id, &generated)
        .await
        .map_err(|e| format!("{e:?}"))?;
    write.commit().await.map_err(|e| e.to_string())
}

/// Each upload of `reference/writes/actions-uploads.php` writes PHP's rows.
#[tokio::test(flavor = "multi_thread")]
async fn uploads_write_php_s_rows() {
    let site = crate::tests::oracle_site().await;
    let given = Upload {
        title: Some(b"Raw title".to_vec()),
        caption: Some(b"A caption".to_vec()),
        description: Some(b"Described".to_vec()),
        alt_text: Some(b"An alt".to_vec()),
        post: 1,
    };
    let form = Upload {
        title: Some(b"A form upload".to_vec()),
        alt_text: Some(b"Alt".to_vec()),
        post: 1,
        ..Upload::default()
    };
    let cases = [
        ("upload-async", ("photo.jpg", "photo.jpg"), How::Media(0)),
        (
            "upload-async-parent",
            ("small.jpg", "small.jpg"),
            How::Media(1),
        ),
        (
            "upload-async-page",
            ("small.jpg", "small.jpg"),
            How::Media(2),
        ),
        (
            "upload-async-name",
            ("rgba.png", "My Picture (1).PNG"),
            How::Media(0),
        ),
        ("upload-async-scaled", ("big.jpg", "big.jpg"), How::Media(0)),
        (
            "upload-async-rotated",
            ("orient-6.jpg", "orient-6.jpg"),
            How::Media(0),
        ),
        ("upload-async-iptc", ("iptc.jpg", "iptc.jpg"), How::Media(0)),
        ("upload-async-gif", ("anim.gif", "anim.gif"), How::Media(0)),
        (
            "upload-async-webp",
            ("lossy.webp", "lossy.webp"),
            How::Media(0),
        ),
        (
            "upload-async-avif",
            ("photo.avif", "photo.avif"),
            How::Media(0),
        ),
        (
            "upload-rest-raw",
            ("photo.jpg", "photo.jpg"),
            How::Rest(false, Upload::default()),
        ),
        (
            "upload-rest-raw-given",
            ("small.jpg", "small.jpg"),
            How::Rest(false, given),
        ),
        (
            "upload-rest-form",
            ("rgba.png", "rgba.png"),
            How::Rest(true, form),
        ),
        (
            "upload-rest-form-name",
            ("small.jpg", "small.jpg"),
            How::Rest(true, Upload::default()),
        ),
    ];
    for (golden, file, how) in cases {
        let dir = tempfile::tempdir().unwrap();
        let uploads = Uploads::new(dir.path().into(), 64 << 20, ImageLimits::default());
        let nothing = async |_: &mut Connection| Ok::<_, String>(());
        let action = async |conn: &mut Connection, ()| {
            uploaded((&site, &uploads), conn, file, how.clone()).await
        };
        let report = check_writes(&fixture_database(), golden, nothing, action).await;
        if golden != "upload-async-avif" {
            report.unwrap();
            continue;
        }
        // An AVIF is kept without the sizes PHP's editor makes (ADR 0015).
        let report = report.unwrap_err();
        let differences: Vec<&str> = report.lines().skip(1).collect();
        assert_eq!(differences.len(), 1, "{report}");
        assert!(differences[0].starts_with("wp_postmeta meta_value (+ %wp_postmeta:2%)"));
        assert!(differences[0].contains(r#"HyScribe "a:6:{s:5:\"width\";i:400;"#));
        assert!(differences[0].contains(r#"s:5:\"sizes\";a:0:{}"#));
    }
    // The same name again in the folder of the month is numbered.
    let dir = tempfile::tempdir().unwrap();
    let uploads = Uploads::new(dir.path().into(), 64 << 20, ImageLimits::default());
    let small = ("small.jpg", "small.jpg");
    let setup =
        async |conn: &mut Connection| uploaded((&site, &uploads), conn, small, How::Media(0)).await;
    let again = async |conn: &mut Connection, ()| {
        uploaded((&site, &uploads), conn, small, How::Media(0)).await
    };
    let report = check_writes(&fixture_database(), "upload-async-again", setup, again).await;
    report.unwrap();
}

/// The date an upload is filed under: a post's, not a page's or an
/// undated one's, else the time of the upload in the site's zone.
#[tokio::test(flavor = "multi_thread")]
async fn uploads_are_filed_by_their_post_s_date() {
    let site = crate::tests::oracle_site().await;
    let post = site.db.post(1).await.unwrap().unwrap();
    assert_eq!(upload_time(&site, Some(post.clone()), 0), post.date);
    let page = Post {
        post_type: "page".into(),
        ..post.clone()
    };
    let undated = Post {
        date: "0000-00-00 00:00:00".into(),
        ..post
    };
    for parent in [Some(page), Some(undated), None] {
        assert_eq!(upload_time(&site, parent, 86_400), "1970-01-02 00:00:00");
    }
}

/// Each action stops whole at any statement that fails, and at a writer
/// that refuses to begin, on the fixture inside a transaction rolled back.
#[tokio::test(flavor = "multi_thread")]
async fn actions_stop_whole() {
    use sqlx::Connection as _;
    let site = crate::tests::oracle_site().await;
    let dir = tempfile::tempdir().unwrap();
    let uploads = Uploads::new(dir.path().into(), 64 << 20, ImageLimits::default());
    let upload = Upload {
        alt_text: Some(b"Alt".to_vec()),
        ..Upload::default()
    };
    let time = upload_time(&site, None, now());
    let photo = (sent("photo.jpg", "photo.jpg"), false);
    let prepared = uploads
        .rest_upload(&site, photo, &upload, (&time, WHO))
        .unwrap();
    let writer = hyscribe_db::Writer::open(&fixture_database(), Mode::Primary)
        .await
        .unwrap()
        .unwrap();
    let db = fixture_database();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = Connection::connect_with(&options).await.unwrap();
    let mut outer = conn.begin().await.unwrap();
    let ctx = context();
    let mut budget = 0;
    let id = loop {
        let mut write = writer.begin_on(&mut outer).await.unwrap();
        write.fail_after(budget);
        match inserted(Ok(write), &ctx, &prepared).await {
            Ok(id) => break id,
            Err(hyscribe_write::Error::Db(_)) => budget += 1,
            Err(err) => panic!("{err:?}"),
        }
    };
    let generated = uploads.generate(&site, &prepared.handled, id).unwrap();
    for budget in 0.. {
        let mut write = writer.begin_on(&mut outer).await.unwrap();
        write.fail_after(budget);
        if saved(Ok(write), id, &generated).await.is_ok() {
            break;
        }
    }
    // A database whose version the writer does not support.
    sqlx::query("UPDATE wp_options SET option_value = '1' WHERE option_name = 'db_version'")
        .execute(&mut *outer)
        .await
        .unwrap();
    let refused = writer.begin_on(&mut outer).await;
    assert!(inserted(refused, &ctx, &prepared).await.is_err());
    let refused = writer.begin_on(&mut outer).await;
    assert!(saved(refused, id, &generated).await.is_err());
}
