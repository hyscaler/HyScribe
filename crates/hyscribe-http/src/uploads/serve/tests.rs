// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use hyscribe_config::ImageLimits;
use tower::ServiceExt;

use super::*;
use crate::Uploads;

/// The fixture's site serving the uploads in `dir`.
async fn serving(dir: &std::path::Path) -> Arc<Site> {
    let site = Arc::into_inner(crate::tests::oracle_site().await).unwrap();
    let uploads = Uploads::new(dir.into(), 1 << 20, ImageLimits::default());
    Arc::new(site.with_uploads(uploads))
}

async fn get(site: &Arc<Site>, path: &str, etag: Option<&str>) -> axum::response::Response {
    let mut request = Request::builder().uri(path);
    if let Some(etag) = etag {
        request = request.header(header::IF_NONE_MATCH, etag);
    }
    let request = request.body(Body::empty()).unwrap();
    crate::router(Arc::clone(site))
        .oneshot(request)
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn uploads_are_served_never_run() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("2026/11")).unwrap();
    for (name, bytes) in [
        ("2026/11/a b+c.jpg", &b"JPEG"[..]),
        ("2026/11/page.html", b"<script>x()</script>"),
        ("2026/11/notes.txt", b"notes"),
        ("2026/11/run.php", b"<?php"),
        ("2026/11/image.svg", b"<svg/>"),
        ("2026/11/.hidden.jpg", b"secret"),
    ] {
        std::fs::write(dir.path().join(name), bytes).unwrap();
    }
    std::fs::write(outside.path().join("secret.jpg"), b"secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("out")).unwrap();
    let site = serving(dir.path()).await;
    let shown = get(&site, "/wp-content/uploads/2026/11/a%20b+c.jpg", None).await;
    assert_eq!(shown.status(), StatusCode::OK);
    let headers = shown.headers().clone();
    assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
    assert_eq!(headers["x-content-type-options"], "nosniff");
    assert!(!headers.contains_key(header::CONTENT_DISPOSITION));
    let bytes = axum::body::to_bytes(shown.into_body(), usize::MAX)
        .await
        .unwrap();
    assert_eq!(&bytes[..], b"JPEG");
    let etag = headers[header::ETAG].to_str().unwrap();
    let again = get(&site, "/wp-content/uploads/2026/11/a%20b+c.jpg", Some(etag)).await;
    assert_eq!(again.status(), StatusCode::NOT_MODIFIED);
    let text = get(&site, "/wp-content/uploads/2026/11/notes.txt", None).await;
    assert_eq!(text.headers()[header::CONTENT_TYPE], "text/plain");
    for download in ["page.html", "run.php", "image.svg"] {
        let path = format!("/wp-content/uploads/2026/11/{download}");
        let response = get(&site, &path, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{download}");
        let headers = response.headers();
        assert_eq!(headers[header::CONTENT_TYPE], "application/octet-stream");
        assert_eq!(headers[header::CONTENT_DISPOSITION], "attachment");
    }
    for missing in [
        "/wp-content/uploads/2026/11/.hidden.jpg",
        "/wp-content/uploads/2026/11/../11/notes.txt",
        "/wp-content/uploads/2026/11/%2e%2e/11/notes.txt",
        "/wp-content/uploads/2026//11/notes.txt",
        "/wp-content/uploads/2026/11/a%5Cb.jpg",
        "/wp-content/uploads/2026/11/%FF.jpg",
        "/wp-content/uploads/2026/11",
        "/wp-content/uploads/2026/11/gone.jpg",
        "/wp-content/uploads/out/secret.jpg",
    ] {
        assert_eq!(
            get(&site, missing, None).await.status(),
            StatusCode::NOT_FOUND,
            "{missing}"
        );
    }
    // An uploads folder not made yet has nothing to serve.
    let missing = serving(&dir.path().join("missing")).await;
    let path = "/wp-content/uploads/2026/11/notes.txt";
    assert_eq!(
        get(&missing, path, None).await.status(),
        StatusCode::NOT_FOUND
    );
    // Without an uploads folder, nothing is served from one.
    let bare = crate::tests::oracle_site().await;
    let path = "/wp-content/uploads/2026/11/notes.txt";
    assert_eq!(get(&bare, path, None).await.status(), StatusCode::NOT_FOUND);
}
