// SPDX-License-Identifier: AGPL-3.0-or-later

use std::os::unix::fs::PermissionsExt;

use axum::http::{HeaderMap, HeaderValue, Uri, header};
use hyscribe_config::ImageLimits;

use super::*;

/// The uploads of a test into `dir`.
fn uploads(dir: &Path) -> Uploads {
    Uploads::new(dir.into(), 1 << 20, ImageLimits::default())
}

fn corpus(name: &str) -> Bytes {
    let path = format!(
        "{}/../hyscribe-image/tests/corpus/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(path).unwrap().into()
}

#[test]
fn only_the_upload_routes_take_uploads() {
    for (uri, takes) in [
        ("/hs-admin/async-upload", true),
        ("/hs-admin/media-new?post_id=1", true),
        ("/?rest_route=/wp/v2/media", true),
        ("/index.php?rest_route=%2Fwp%2Fv2%2Fmedia&x=1", true),
        ("/wp-json/wp/v2/media", true),
        ("/index.php/wp-json/wp/v2/media", true),
        ("/hs-admin/upload", false),
        ("/?rest_route=/wp/v2/media/5", false),
        ("/?rest_route[]=/wp/v2/media", false),
        ("/wp-json/wp/v2/media/5", false),
        ("/wp-comments-post.php", false),
    ] {
        assert_eq!(takes_upload(&uri.parse::<Uri>().unwrap()), takes, "{uri}");
    }
}

#[test]
fn forms_give_post_and_files() {
    let body = Bytes::from_static(
        b"--B\r\nContent-Disposition: form-data; name=\"async-upload\"; filename=\"a.jpg\"\r\nContent-Type: image/jpeg\r\n\r\nJPEG\r\n\
          --B\r\nContent-Disposition: form-data; name=\"big\"; filename=\"b.jpg\"\r\n\r\n0123456789\r\n\
          --B\r\nContent-Disposition: form-data; name=\"none\"; filename=\"\"\r\n\r\n\r\n\
          --B\r\nContent-Disposition: form-data; name=\"post_id\"\r\n\r\n1\r\n--B--",
    );
    let mut headers = HeaderMap::new();
    assert!(form(&headers, &body, 8).is_none());
    let kind = HeaderValue::from_static("multipart/form-data; boundary=B");
    headers.insert(header::CONTENT_TYPE, kind);
    let (post, files) = form(&headers, &body, 8).unwrap();
    assert_eq!(post.get_str("post_id"), Some(&Value::String(b"1".to_vec())));
    let [(field, sent), (_, big), (_, none)] = &files[..] else {
        panic!("{files:?}");
    };
    assert_eq!(
        (&field[..], &sent.name[..]),
        (&b"async-upload"[..], &b"a.jpg"[..])
    );
    assert_eq!(
        (&sent.kind[..], &sent.bytes[..]),
        (&b"image/jpeg"[..], &b"JPEG"[..])
    );
    assert_eq!(sent.error, None);
    assert_eq!(big.error.as_deref(), Some(TOO_LARGE));
    assert!(big.bytes.is_empty());
    assert_eq!(none.error.as_deref(), Some(NO_FILE));
    let broken = HeaderValue::from_static("multipart/form-data; boundary=C");
    headers.insert(header::CONTENT_TYPE, broken);
    assert!(form(&headers, &body, 8).is_none());
}

#[test]
fn files_are_placed_whole_and_only_in_their_folder() {
    let dir = tempfile::tempdir().unwrap();
    for bad in ["", ".hidden", "..", "a/b", "a\\b", "a\0b"] {
        assert!(!stays(bad), "{bad:?}");
        let refused = place(dir.path(), bad, b"x", false).unwrap_err();
        assert_eq!(refused.kind(), std::io::ErrorKind::InvalidInput);
    }
    place(dir.path(), "a.jpg", b"one", false).unwrap();
    let taken = place(dir.path(), "a.jpg", b"two", false).unwrap_err();
    assert_eq!(taken.kind(), std::io::ErrorKind::AlreadyExists);
    place(dir.path(), "a.jpg", b"three", true).unwrap();
    assert_eq!(std::fs::read(dir.path().join("a.jpg")).unwrap(), b"three");
    // No temporary file is left behind.
    assert_eq!(names(dir.path()).unwrap(), ["a.jpg"]);
    assert!(names(&dir.path().join("missing")).is_err());
}

#[test]
fn folders_stay_inside_the_uploads() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("uploads");
    assert!(folder(&base, "/2026/11").is_some_and(|f| f.ends_with("2026/11")));
    assert!(folder(&base, "").is_some());
    let outside = dir.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, base.join("2025")).unwrap();
    assert!(folder(&base, "/2025/01").is_none());
    std::fs::write(base.join("file"), b"").unwrap();
    assert!(folder(&base, "/file/01").is_none());
    assert!(folder(&dir.path().join("missing\0"), "").is_none());
}

/// A site over the fixture with `options` changed.
async fn site_with(options: &[(&str, &[u8])]) -> Site {
    let site = crate::tests::oracle_site().await;
    let mut site = Arc::into_inner(site).unwrap();
    for (name, value) in options {
        site.options.insert((*name).into(), value.to_vec());
    }
    site
}

const WHO: Who = Who {
    unfiltered_html: false,
    unfiltered_upload: false,
};

fn sent(name: &str, bytes: Bytes) -> Sent {
    Sent::new(name.as_bytes().to_vec(), Vec::new(), bytes, 1 << 20)
}

#[tokio::test(flavor = "multi_thread")]
async fn uploads_are_refused_as_php_refuses_them() {
    let dir = tempfile::tempdir().unwrap();
    let uploads = uploads(dir.path());
    let site = site_with(&[]).await;
    let time = ("2026-11-17 00:00:00", WHO);
    let refused = |sent, who| {
        uploads
            .handle(&site, sent, (time.0, who), false)
            .unwrap_err()
    };
    let message = |text: &str| Refused::Message(text.into());
    assert_eq!(refused(sent("a.jpg", Bytes::new()), WHO), message(EMPTY));
    let over = Sent::new(b"a.jpg".to_vec(), Vec::new(), corpus("small.jpg"), 4);
    assert_eq!(refused(over, WHO), message(TOO_LARGE));
    assert_eq!(
        refused(sent("a.php", corpus("small.jpg")), WHO),
        message(NOT_ALLOWED)
    );
    let unfiltered = Who {
        unfiltered_upload: true,
        ..WHO
    };
    assert_eq!(
        refused(sent("a.php", corpus("small.jpg")), unfiltered),
        Refused::Unported
    );
    // A type PHP checks with `fileinfo`.
    assert_eq!(
        refused(sent("a.txt", corpus("small.jpg")), WHO),
        Refused::Unported
    );
    for (name, value) in [
        ("upload_path", &b"/elsewhere"[..]),
        ("upload_url_path", b"https://cdn.example.test/u"),
    ] {
        let site = site_with(&[(name, value)]).await;
        let handled = uploads.handle(&site, sent("a.jpg", corpus("small.jpg")), time, false);
        assert_eq!(handled.unwrap_err(), Refused::Unported, "{name}");
    }
    // The folder of the month is a file, then not writable.
    std::fs::create_dir_all(dir.path().join("2026")).unwrap();
    std::fs::write(dir.path().join("2026/11"), b"").unwrap();
    let mkdir = "Unable to create directory wp-content/uploads/2026/11. Is its parent directory writable by the server?";
    assert_eq!(
        refused(sent("a.jpg", corpus("small.jpg")), WHO),
        message(mkdir)
    );
    std::fs::remove_file(dir.path().join("2026/11")).unwrap();
    std::fs::create_dir(dir.path().join("2026/11")).unwrap();
    let locked = std::fs::Permissions::from_mode(0o500);
    std::fs::set_permissions(dir.path().join("2026/11"), locked).unwrap();
    let moved = "The uploaded file could not be moved to wp-content/uploads/2026/11.";
    assert_eq!(
        refused(sent("a.jpg", corpus("small.jpg")), WHO),
        message(moved)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn uploads_are_written_by_month_with_unique_names() {
    let dir = tempfile::tempdir().unwrap();
    let uploads = uploads(dir.path());
    let site = site_with(&[]).await;
    let time = ("2026-11-17 00:00:00", WHO);
    let handled = uploads.handle(&site, sent("a b.jpeg", corpus("small.jpg")), time, true);
    let handled = handled.unwrap();
    assert_eq!(handled.file, "2026/11/a-b.jpeg");
    assert_eq!(
        handled.url,
        b"http://127.0.0.1:8081/wp-content/uploads/2026/11/a-b.jpeg"
    );
    assert_eq!(
        (handled.mime.as_str(), &handled.name[..]),
        ("image/jpeg", &b"a b.jpeg"[..])
    );
    // A PNG named as a JPEG takes its type's extension.
    let again = uploads.handle(&site, sent("a b.jpeg", corpus("rgba.png")), time, false);
    let again = again.unwrap();
    assert_eq!(
        (again.file.as_str(), again.mime.as_str()),
        ("2026/11/a-b.png", "image/png")
    );
    let third = uploads.handle(&site, sent("a b.jpeg", corpus("small.jpg")), time, false);
    assert_eq!(third.unwrap().file, "2026/11/a-b-1.jpeg");
    uploads.discard(&handled);
    assert!(!dir.path().join("2026/11/a-b.jpeg").exists());
    // Without folders by month, in the uploads folder itself.
    let flat = site_with(&[("uploads_use_yearmonth_folders", b"")]).await;
    let handled = uploads.handle(&flat, sent("c.jpg", corpus("small.jpg")), time, false);
    assert_eq!(handled.unwrap().file, "c.jpg");
    // The hooks attached by default change nothing.
    uploads.added(1);
}

#[tokio::test(flavor = "multi_thread")]
async fn sizes_are_made_on_the_pool_and_written_beside_the_upload() {
    let dir = tempfile::tempdir().unwrap();
    let uploads = uploads(dir.path());
    let site = site_with(&[]).await;
    let time = ("2026-11-17 00:00:00", WHO);
    let handled = uploads.handle(&site, sent("photo.jpg", corpus("photo.jpg")), time, false);
    let handled = handled.unwrap();
    let generated = uploads.generate(&site, &handled, 5).unwrap().unwrap();
    assert_eq!(generated.files.len(), 2);
    for size in ["photo-150x150.jpg", "photo-300x225.jpg"] {
        assert!(dir.path().join("2026/11").join(size).is_file(), "{size}");
    }
    // A size that cannot be written stops the upload.
    let locked = std::fs::Permissions::from_mode(0o500);
    std::fs::set_permissions(dir.path().join("2026/11"), locked).unwrap();
    assert!(uploads.generate(&site, &handled, 5).is_err());
}

/// A body is refused with 413 once past its limit: an upload's
/// `UPLOAD_MAX_SIZE` and its form's allowance, any other's 2 MB.
#[tokio::test(flavor = "multi_thread")]
async fn bodies_are_refused_past_their_limits() {
    use tower::ServiceExt;
    let dir = tempfile::tempdir().unwrap();
    let uploads = Uploads::new(dir.path().into(), 3 << 20, ImageLimits::default());
    let site = Arc::new(site_with(&[]).await.with_uploads(uploads));
    let status = |uri: &str, len: usize| {
        let site = Arc::clone(&site);
        let request = axum::http::Request::builder()
            .method("POST")
            .uri(uri)
            .body(axum::body::Body::from(vec![b'x'; len]))
            .unwrap();
        async move {
            let response = crate::router(site).oneshot(request).await.unwrap();
            response.status()
        }
    };
    let large = axum::http::StatusCode::PAYLOAD_TOO_LARGE;
    assert_eq!(status("/hs-admin/async-upload", (4 << 20) + 1).await, large);
    assert_ne!(status("/hs-admin/async-upload", 4 << 20).await, large);
    assert_eq!(status("/", (2 << 20) + 1).await, large);
    assert_ne!(status("/", 2 << 20).await, large);
}
