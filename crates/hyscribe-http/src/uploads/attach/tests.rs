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
    let (actions, ctx) = (Actions::On(&writer, conn, None), context());
    let completed = uploads.complete(site, actions, &ctx, &prepared);
    completed.await.map(drop).map_err(|e| format!("{e:?}"))
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
    let text = Upload {
        title: Some(b"Notes".to_vec()),
        ..given.clone()
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
        (
            "upload-async-scaled-rotated",
            ("big-orient-6.jpg", "big-orient-6.jpg"),
            How::Media(0),
        ),
        (
            "upload-async-scaled-png",
            ("big.png", "big.png"),
            How::Media(0),
        ),
        (
            "upload-async-edge",
            ("edge-2560.jpg", "edge-2560.jpg"),
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
            "upload-rest-scaled",
            ("big.jpg", "big.jpg"),
            How::Rest(false, Upload::default()),
        ),
        (
            "upload-rest-rotated",
            ("orient-6.jpg", "orient-6.jpg"),
            How::Rest(false, Upload::default()),
        ),
        (
            "upload-rest-scaled-rotated",
            ("big-orient-3.jpg", "big-orient-3.jpg"),
            How::Rest(false, Upload::default()),
        ),
        (
            "upload-rest-form-scaled",
            ("big-rgba.png", "big-rgba.png"),
            How::Rest(
                true,
                Upload {
                    title: Some(b"Big form".to_vec()),
                    ..Upload::default()
                },
            ),
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
        (
            "upload-async-text",
            ("plain.txt", "plain.txt"),
            How::Media(0),
        ),
        (
            "upload-async-csv",
            ("words.csv", "words.csv"),
            How::Media(1),
        ),
        (
            "upload-async-vtt",
            ("captions.vtt", "captions.vtt"),
            How::Media(0),
        ),
        (
            "upload-async-docx",
            ("sample.docx", "sample.docx"),
            How::Media(0),
        ),
        (
            "upload-async-odt",
            ("sample.odt", "My Notes (1).odt"),
            How::Media(0),
        ),
        (
            "upload-async-zip",
            ("sample.zip", "sample.docx"),
            How::Media(0),
        ),
        (
            "upload-async-rtf",
            ("sample.rtf", "sample.rtf"),
            How::Media(0),
        ),
        (
            "upload-async-doc",
            ("sample.doc", "sample.doc"),
            How::Media(0),
        ),
        (
            "upload-async-xls",
            ("sample.xls", "My Sheet.xls"),
            How::Media(1),
        ),
        (
            "upload-async-ppt",
            ("sample.ppt", "sample.ppt"),
            How::Media(0),
        ),
        (
            "upload-async-ppt-pps",
            ("sample.ppt", "slides.pps"),
            How::Media(0),
        ),
        (
            "upload-async-xls-xlt",
            ("sample.xls", "sheet.xlt"),
            How::Media(0),
        ),
        (
            "upload-rest-raw-doc",
            ("sample.doc", "sample.doc"),
            How::Rest(
                false,
                Upload {
                    title: Some(b"A Word file".to_vec()),
                    post: 1,
                    ..Upload::default()
                },
            ),
        ),
        (
            "upload-rest-form-xls",
            ("sample.xls", "sample.xls"),
            How::Rest(true, Upload::default()),
        ),
        (
            "upload-async-pdf",
            ("sample.pdf", "sample.pdf"),
            How::Media(0),
        ),
        (
            "upload-rest-raw-pdf",
            ("sample.pdf", "sample.pdf"),
            How::Rest(
                false,
                Upload {
                    post: 1,
                    ..Upload::default()
                },
            ),
        ),
        (
            "upload-rest-raw-text",
            ("plain.txt", "plain.txt"),
            How::Rest(false, text),
        ),
        (
            "upload-rest-form-docx",
            ("sample.docx", "sample.docx"),
            How::Rest(
                true,
                Upload {
                    title: Some(b"A form document".to_vec()),
                    ..Upload::default()
                },
            ),
        ),
        (
            "upload-rest-raw-xlsx",
            ("sample.xlsx", "sample.xlsx"),
            How::Rest(false, Upload::default()),
        ),
        ("upload-async-wav", ("tone.wav", "tone.wav"), How::Media(0)),
        ("upload-async-aac", ("tone.aac", "tone.aac"), How::Media(1)),
        (
            "upload-rest-raw-wav",
            ("tone.wav", "tone.wav"),
            How::Rest(
                false,
                Upload {
                    title: Some(b"A tone".to_vec()),
                    post: 1,
                    ..Upload::default()
                },
            ),
        ),
        (
            "upload-rest-form-aac",
            ("tone.aac", "tone.aac"),
            How::Rest(true, Upload::default()),
        ),
        (
            "upload-async-mp3",
            ("tagged.mp3", "tagged.mp3"),
            How::Media(0),
        ),
        (
            "upload-async-mp3-cover",
            ("covered.mp3", "covered.mp3"),
            How::Media(1),
        ),
        (
            "upload-rest-raw-mp3",
            ("cbr.mp3", "cbr.mp3"),
            How::Rest(
                false,
                Upload {
                    title: Some(b"A song".to_vec()),
                    ..Upload::default()
                },
            ),
        ),
        (
            "upload-rest-form-mp3-cover",
            ("covered.mp3", "covered.mp3"),
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
    // The same cover picture again is the attachment made the first time.
    let dir = tempfile::tempdir().unwrap();
    let uploads = Uploads::new(dir.path().into(), 64 << 20, ImageLimits::default());
    let covered = ("covered.mp3", "covered.mp3");
    let setup = async |conn: &mut Connection| {
        uploaded((&site, &uploads), conn, covered, How::Media(0)).await
    };
    let again = async |conn: &mut Connection, ()| {
        uploaded((&site, &uploads), conn, covered, How::Media(0)).await
    };
    let golden = "upload-async-mp3-cover-again";
    check_writes(&fixture_database(), golden, setup, again)
        .await
        .unwrap();
    // A text file's number: its copies are not looked for.
    let dir = tempfile::tempdir().unwrap();
    let uploads = Uploads::new(dir.path().into(), 64 << 20, ImageLimits::default());
    let plain = ("plain.txt", "plain.txt");
    let setup =
        async |conn: &mut Connection| uploaded((&site, &uploads), conn, plain, How::Media(0)).await;
    let again = async |conn: &mut Connection, ()| {
        uploaded((&site, &uploads), conn, plain, How::Media(0)).await
    };
    let report = check_writes(&fixture_database(), "upload-async-text-again", setup, again).await;
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
    loop {
        let mut write = writer.begin_on(&mut outer).await.unwrap();
        write.fail_after(budget);
        match inserted(Ok(write), &ctx, &prepared).await {
            Ok(_) => break,
            Err(err) => {
                assert!(spent(&err), "{err:?}");
                budget += 1;
            }
        }
    }
    // A scaled image's attached file is saved with its metadata.
    let big = uploads.media_handle_upload(&site, sent("big.jpg", "big.jpg"), 0, (&time, WHO));
    let big = big.unwrap();
    let id = inserted(writer.begin_on(&mut outer).await, &ctx, &big)
        .await
        .unwrap();
    let generated = uploads.generate(&site, &big.handled, id).unwrap();
    assert!(generated.attached_file.is_some());
    for budget in 0.. {
        let mut write = writer.begin_on(&mut outer).await.unwrap();
        write.fail_after(budget);
        match saved(Ok(write), id, &generated, None).await {
            Ok(()) => break,
            Err(err) => assert!(spent(&err), "{err:?}"),
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
    assert!(saved(refused, id, &generated, None).await.is_err());
    let refused = writer.begin_on(&mut outer).await;
    assert!(found(refused, "hash", b"image/png").await.is_err());
    let refused = writer.begin_on(&mut outer).await;
    let args = PostArgs::default();
    assert!(
        covered(refused, &ctx, args, &big.handled, "hash")
            .await
            .is_err()
    );
}

/// Hooks that record the prefilter run and give their own scaling threshold.
struct Recorded {
    threshold: i64,
    prefilter: std::sync::Mutex<&'static str>,
}

impl hyscribe_image::hooks::Hooks for Recorded {
    fn big_image_size_threshold(&self, _: i64, _: (u32, u32), _: &str, _: u64) -> i64 {
        self.threshold
    }
}

impl crate::uploads::UploadHooks for Recorded {
    fn wp_handle_upload_prefilter(&self, file: Sent) -> Sent {
        *self.prefilter.lock().unwrap() = "upload";
        file
    }

    fn wp_handle_sideload_prefilter(&self, file: Sent) -> Sent {
        *self.prefilter.lock().unwrap() = "sideload";
        file
    }
}

/// Uploads into a new folder with [`Recorded`] hooks of `threshold`.
fn hooked(threshold: i64) -> (tempfile::TempDir, std::sync::Arc<Recorded>, Uploads) {
    let dir = tempfile::tempdir().unwrap();
    let prefilter = std::sync::Mutex::new("");
    let hooks = std::sync::Arc::new(Recorded {
        threshold,
        prefilter,
    });
    let uploads = Uploads::new(dir.path().into(), 64 << 20, ImageLimits::default());
    let uploads = uploads.with_hooks(std::sync::Arc::clone(&hooks) as _);
    (dir, hooks, uploads)
}

/// The titles PHP gives a name that is all extension, a body and a form,
/// the prefilter each runs, and the scaling threshold as hooks give it.
#[tokio::test(flavor = "multi_thread")]
async fn uploads_are_titled_and_hooked_as_php_s() {
    let site = crate::tests::oracle_site().await;
    let time = ("2026-11-17 00:00:00", WHO);
    let (_dir, hooks, uploads) = hooked(2560);
    // `basename()` keeps a name that is its whole suffix.
    let dotted = uploads.media_handle_upload(&site, sent("small.jpg", ".jpg"), 0, time);
    assert_eq!(dotted.unwrap().args.title, b".jpg");
    // A body is titled by its file, a form by the name it was sent as.
    let none = Upload::default();
    let raw = uploads.rest_upload(
        &site,
        (sent("small.jpg", "My Photo.jpg"), false),
        &none,
        time,
    );
    assert_eq!(raw.unwrap().args.title, b"My-Photo");
    assert_eq!(*hooks.prefilter.lock().unwrap(), "sideload");
    let form = uploads.rest_upload(
        &site,
        (sent("small.jpg", "My Photo.jpg"), true),
        &none,
        time,
    );
    assert_eq!(form.unwrap().args.title, b"My Photo");
    assert_eq!(*hooks.prefilter.lock().unwrap(), "upload");
    // A threshold of 0 turns scaling off; an image as large as it is kept.
    let small = corpus("small.jpg");
    let size = getimagesize(&small).unwrap();
    for threshold in [0, i64::from(size.width.max(size.height))] {
        let (_dir, _, uploads) = hooked(threshold);
        let kept = uploads.rest_upload(&site, (sent("small.jpg", "a.jpg"), false), &none, time);
        assert!(kept.is_ok(), "{threshold}");
    }
}

/// Audio and video whose metadata is getID3's answer 501 and leave no file
/// behind.
#[tokio::test(flavor = "multi_thread")]
async fn unported_types_leave_no_file() {
    let site = crate::tests::oracle_site().await;
    let time = (upload_time(&site, None, now()), WHO);
    for name in ["tone.flac", "clip.mp4"] {
        let dir = tempfile::tempdir().unwrap();
        let uploads = Uploads::new(dir.path().into(), 64 << 20, ImageLimits::default());
        let sent = sent(name, name);
        let media = uploads.media_handle_upload(&site, sent.clone(), 0, (&time.0, WHO));
        assert_eq!(media.unwrap_err(), Refused::Unported, "{name}");
        let rest = uploads.rest_upload(&site, (sent, true), &Upload::default(), (&time.0, WHO));
        assert_eq!(rest.unwrap_err(), Refused::Unported, "{name}");
        let mut found = Vec::new();
        let mut folders = vec![dir.path().to_path_buf()];
        while let Some(folder) = folders.pop() {
            for entry in std::fs::read_dir(folder).unwrap() {
                let path = entry.unwrap().path();
                match path.is_dir() {
                    true => folders.push(path),
                    false => found.push(path),
                }
            }
        }
        assert!(found.is_empty(), "{name}: {found:?}");
    }
}

/// Whether `err` is the test's statement budget running out: only that may
/// stop a write a budget loop retries, so one that always fails ends it.
fn spent(err: &hyscribe_write::Error) -> bool {
    matches!(err, hyscribe_write::Error::Db(db) if db.to_string().contains("budget is spent"))
}

/// A track number `number_format()` would give past a float's exact
/// integers is not ported; one below is written as PHP writes it.
#[test]
fn audio_text_keeps_to_exact_track_numbers() {
    let meta = |track: &str| {
        let key = Key::String(b"track_number".to_vec());
        vec![(key, Value::String(track.into()))]
    };
    let (_, content) = audio_text(&meta("999999999999999/2"), Vec::new()).unwrap();
    assert_eq!(content, b" Track 999,999,999,999,999 of 2.");
    // A part that is no number: the total left out, or no track at all.
    let text = |track: &str| audio_text(&meta(track), Vec::new()).unwrap().1;
    assert_eq!(text("3/x"), b" Track 3.");
    assert_eq!(text("x/2"), b"");
    assert_eq!(text("x"), b"");
    for track in ["1000000000000000", "1/1e15"] {
        assert_eq!(audio_text(&meta(track), Vec::new()), Err(Refused::Unported));
    }
}

/// Cover hooks: `wp_upload_bits` refuses the picture, or
/// `attachment_thumbnail_args` gives the cover a text.
struct Covers(bool);

impl hyscribe_image::hooks::Hooks for Covers {}

impl crate::uploads::UploadHooks for Covers {
    fn wp_upload_bits(&self, name: &str, bits: &[u8]) -> Option<String> {
        assert!(name == "covered-mp3-image.png" && bits.starts_with(b"\x89PNG"));
        self.0.then(|| "Refused.".to_string())
    }

    fn attachment_thumbnail_args(&self, args: PostArgs, uploaded: &Handled) -> PostArgs {
        assert_eq!(uploaded.mime, "image/png");
        PostArgs {
            content: b"A cover".to_vec(),
            ..args
        }
    }
}

/// A cover `wp_upload_bits` refuses is no attachment and no thumbnail; the
/// arguments `attachment_thumbnail_args` gives are the cover's.
#[tokio::test(flavor = "multi_thread")]
async fn covers_follow_their_hooks() {
    use sqlx::Connection as _;
    let site = crate::tests::oracle_site().await;
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
    let time = upload_time(&site, None, now());
    for refused in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let uploads = Uploads::new(dir.path().into(), 64 << 20, ImageLimits::default());
        let uploads = uploads.with_hooks(std::sync::Arc::new(Covers(refused)));
        let sent = sent("covered.mp3", "covered.mp3");
        let prepared = uploads
            .media_handle_upload(&site, sent, 0, (&time, WHO))
            .unwrap();
        let mut conn = Connection::connect_with(&options).await.unwrap();
        let mut outer = conn.begin().await.unwrap();
        let actions = Actions::On(&writer, &mut outer, None);
        let ctx = context();
        let id = uploads.complete(&site, actions, &ctx, &prepared);
        let id = id.await.unwrap();
        let thumbnail: Option<(String,)> = sqlx::query_as(
            "SELECT meta_value FROM wp_postmeta WHERE post_id = ? AND meta_key = '_thumbnail_id'",
        )
        .bind(id)
        .fetch_optional(&mut *outer)
        .await
        .unwrap();
        let content: Option<(String,)> = sqlx::query_as(
            "SELECT post_content FROM wp_posts WHERE post_mime_type = 'image/png' AND ID > ?",
        )
        .bind(id)
        .fetch_optional(&mut *outer)
        .await
        .unwrap();
        match refused {
            true => assert_eq!((thumbnail, content), (None, None)),
            false => {
                assert!(thumbnail.is_some());
                assert_eq!(content, Some(("A cover".to_string(),)));
            }
        }
    }
}

/// Each action of an upload with a cover picture stops whole at any
/// statement that fails: the audio's insert, the cover looked for, the
/// cover's insert, its metadata and the audio's.
#[tokio::test(flavor = "multi_thread")]
async fn cover_actions_stop_whole() {
    use sqlx::Connection as _;
    let site = crate::tests::oracle_site().await;
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
    let time = upload_time(&site, None, now());
    let ctx = context();
    for action in 0..5 {
        for statements in 0.. {
            let dir = tempfile::tempdir().unwrap();
            let uploads = Uploads::new(dir.path().into(), 64 << 20, ImageLimits::default());
            let sent = sent("covered.mp3", "covered.mp3");
            let prepared = uploads
                .media_handle_upload(&site, sent, 0, (&time, WHO))
                .unwrap();
            let mut conn = Connection::connect_with(&options).await.unwrap();
            let mut outer = conn.begin().await.unwrap();
            let actions = Actions::On(&writer, &mut outer, Some((action, statements)));
            match uploads.complete(&site, actions, &ctx, &prepared).await {
                Ok(_) => break,
                Err(refused) => assert!(matches!(refused, Refused::Failed(_)), "{refused:?}"),
            }
        }
    }
}

/// Hooks whose audio filter gives a track number past a float's exact
/// integers.
struct Track;

impl hyscribe_image::hooks::Hooks for Track {
    fn wp_read_audio_metadata(&self, _meta: Value, _format: Option<&str>) -> Value {
        let key = Key::String(b"track_number".to_vec());
        Value::Array(vec![(key, Value::String(b"1e20".to_vec()))])
    }
}

impl crate::uploads::UploadHooks for Track {}

/// An audio file whose text is not ported answers 501 and leaves no file;
/// a cover the month's folder cannot take is no attachment.
#[tokio::test(flavor = "multi_thread")]
async fn uploads_stop_where_their_cover_or_text_does() {
    let site = crate::tests::oracle_site().await;
    let time = upload_time(&site, None, now());
    let (year, month) = (time.get(..4).unwrap(), time.get(5..7).unwrap());
    let dir = tempfile::tempdir().unwrap();
    let uploads = Uploads::new(dir.path().into(), 64 << 20, ImageLimits::default());
    let uploads = uploads.with_hooks(std::sync::Arc::new(Track));
    let tagged = sent("tagged.mp3", "tagged.mp3");
    let refused = uploads.media_handle_upload(&site, tagged, 0, (&time, WHO));
    assert_eq!(refused.unwrap_err(), Refused::Unported);
    assert_eq!(std::fs::read_dir(dir.path().join(year)).unwrap().count(), 1);
    // The cover goes to the folder of this month, here a file.
    let dir = tempfile::tempdir().unwrap();
    let uploads = Uploads::new(dir.path().into(), 64 << 20, ImageLimits::default());
    let parent = site.db.post(1).await.unwrap();
    let dated = upload_time(&site, parent, now());
    assert_ne!(dated.get(..7), time.get(..7));
    let prepared = uploads
        .media_handle_upload(&site, sent("covered.mp3", "covered.mp3"), 1, (&dated, WHO))
        .unwrap();
    std::fs::create_dir_all(dir.path().join(year)).unwrap();
    std::fs::write(dir.path().join(year).join(month), b"").unwrap();
    let generated = uploads.generate(&site, &prepared.handled, 1).unwrap();
    let cover = generated.cover.unwrap();
    let name = ("covered-mp3-image.png", cover.mime.as_slice());
    let bits = cover.data.clone().into();
    assert!(uploads.upload_bits(&site, name, &bits, &time).is_none());
}

/// A cover's extension follows its type, `.jpg` for any other.
#[test]
fn covers_are_named_by_type() {
    let types: [&[u8]; 5] = [
        b"image/gif",
        b"image/png",
        b"image/webp",
        b"image/jpeg",
        b"",
    ];
    let names = types.map(cover_extension);
    assert_eq!(names, ["gif", "png", "webp", "jpg", "jpg"]);
}

/// The text `media_handle_upload()` writes from a title, album and artist,
/// each present or not, as its `sprintf()` formats them.
#[test]
fn audio_text_names_what_the_tags_give() {
    let meta = |pairs: &[(&str, &str)]| -> Vec<(Key, Value)> {
        pairs
            .iter()
            .map(|(k, v)| {
                (
                    Key::String(k.as_bytes().to_vec()),
                    Value::String(v.as_bytes().to_vec()),
                )
            })
            .collect()
    };
    let text = |pairs: &[(&str, &str)], name: &str| {
        let (title, content) = audio_text(&meta(pairs), name.into()).unwrap();
        (
            String::from_utf8(title).unwrap(),
            String::from_utf8(content).unwrap(),
        )
    };
    let named = |content: &str| ("song".to_string(), content.to_string());
    let untitled = |content: &str| (String::new(), content.to_string());
    assert_eq!(text(&[("album", "Al")], "song"), named("\"song\" from Al."));
    assert_eq!(text(&[("artist", "Ar")], "song"), named("\"song\" by Ar."));
    assert_eq!(text(&[], "song"), named("\"song\"."));
    let both = [("album", "Al"), ("artist", "Ar")];
    assert_eq!(text(&both, ""), untitled("Al by Ar."));
    assert_eq!(text(&[("album", "Al")], ""), untitled("Al."));
    assert_eq!(text(&[("artist", "Ar")], ""), untitled("Ar."));
    assert_eq!(text(&[], ""), untitled(""));
}
