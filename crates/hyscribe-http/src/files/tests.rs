// SPDX-License-Identifier: AGPL-3.0-or-later

use std::sync::Arc;

use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode, header};
use tower::ServiceExt;

use super::{http_date, installed_theme_file, static_file};

/// The forms and checks of APR's `apr_date_parse_http()`.
#[test]
fn reads_dates_as_apache_does() {
    for (text, unix) in [
        ("Sun, 06 Nov 1994 08:49:37 GMT", Some(784_111_777)),
        ("  Sun, 6 Nov 1994 08:49:37 GMT", Some(784_111_777)),
        ("Sunday, 06-Nov-94 08:49:37 GMT", Some(784_111_777)),
        ("Sun Nov  6 08:49:37 1994", Some(784_111_777)),
        ("Sun Nov 06 08:49:37 1994", Some(784_111_777)),
        ("x 01-Jan-26 00:00:00 GMT", Some(1_767_225_600)),
        ("x 01-Jan-70 00:00:00 GMT", Some(0)),
        ("x 29 Feb 2024 00:00:00 x", Some(1_709_164_800)),
        ("x 29 Feb 2000 00:00:00 x", Some(951_782_400)),
        ("x 30 Apr 2026 00:00:00 x", Some(1_777_507_200)),
        ("x 01 Jan 1970 00:00:00 x", Some(0)),
        ("x 31 Dec 2025 23:59:61 x", Some(1_767_225_601)),
        ("x 29 Feb 2100 00:00:00 x", None),
        ("x 29 Feb 2026 00:00:00 x", None),
        ("x 31 Apr 2026 00:00:00 x", None),
        ("x 00 Jan 2026 00:00:00 x", None),
        ("x 32 Jan 2026 00:00:00 x", None),
        ("x 01 Foo 2026 00:00:00 x", None),
        ("x 01 Jan 1899 00:00:00 x", None),
        ("x 31 Dec 1969 23:59:59 x", None),
        ("x 01 Jan 2026 24:00:00 x", None),
        ("x 01 Jan 2026 00:60:00 x", None),
        ("x 01 Jan 2026 00:00:62 x", None),
        ("x 01 Jan 2026 00:00:00", None),
        ("nospace", None),
        ("", None),
    ] {
        assert_eq!(http_date(text), unix, "{text}");
    }
}

/// HyScribe's own files under `/hs-admin/` (ADR 0008): an `ETag` from their
/// SHA-256 and no `Last-Modified`, so only the tag answers a revalidation.
#[test]
fn serves_hyscribe_files() {
    let now = 1_800_000_000;
    let get = |pairs: &[(header::HeaderName, &str)]| {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(name, value.parse().unwrap());
        }
        static_file("/hs-admin/images/hyscribe-mark.svg", &headers, now).unwrap()
    };
    let (_, _, sha) = hyscribe_assets::hyscribe_file("images/hyscribe-mark.svg").unwrap();
    let etag = format!("\"{sha}\"");
    let plain = get(&[]);
    assert_eq!(plain.status(), StatusCode::OK);
    assert_eq!(plain.headers()[header::CONTENT_TYPE], "image/svg+xml");
    assert_eq!(plain.headers()[header::ETAG], etag.as_str());
    assert!(plain.headers().get(header::LAST_MODIFIED).is_none());
    let tagged = get(&[(header::IF_NONE_MATCH, &etag)]);
    assert_eq!(tagged.status(), StatusCode::NOT_MODIFIED);
    let date = "Sat, 01 Jan 2028 00:00:00 GMT";
    for name in [header::IF_MODIFIED_SINCE, header::IF_UNMODIFIED_SINCE] {
        assert_eq!(get(&[(name, date)]).status(), StatusCode::OK);
    }
    // A release file keeps its date.
    let core = static_file(
        "hs-admin/css/colors/coffee/colors.min.css",
        &HeaderMap::new(),
        now,
    );
    assert!(core.unwrap().headers().get(header::LAST_MODIFIED).is_some());
}

/// An installed theme's files are served from its package's `public/`,
/// with their type and caching headers; nothing outside it, no hidden
/// file and no PHP source.
#[tokio::test(flavor = "multi_thread")]
async fn installed_themes_serve_their_public_files() {
    let (plugins, themes) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let outside = tempfile::tempdir().unwrap();
    let public = themes.path().join("classic/public");
    std::fs::create_dir_all(public.join("assets/js")).unwrap();
    std::fs::create_dir_all(themes.path().join("bare")).unwrap();
    for (name, bytes) in [
        ("style.css", &b"body{}"[..]),
        ("assets/js/a b+c.js", b"x()"),
        ("FONT.WOFF2", b"wOF2"),
        ("LICENSE", b"GPL"),
        ("index.php", b"<?php echo 1;"),
        ("old.phtml", b"<?php"),
        (".env", b"secret"),
    ] {
        std::fs::write(public.join(name), bytes).unwrap();
    }
    std::fs::write(themes.path().join("classic/extension.wasm"), b"\0asm").unwrap();
    std::fs::write(outside.path().join("secret.css"), b"secret").unwrap();
    std::os::unix::fs::symlink(outside.path(), public.join("out")).unwrap();
    let site = Arc::into_inner(crate::tests::oracle_site().await).unwrap();
    let site = Arc::new(site.with_extensions(plugins.path(), themes.path()));
    let get = |path: &str, etag: Option<&str>| {
        let mut request = Request::builder().uri(path);
        if let Some(etag) = etag {
            request = request.header(header::IF_NONE_MATCH, etag);
        }
        let request = request.body(Body::empty()).unwrap();
        crate::router(Arc::clone(&site)).oneshot(request)
    };
    let style = get("/wp-content/themes/classic/style.css", None)
        .await
        .unwrap();
    assert_eq!(style.status(), StatusCode::OK);
    let headers = style.headers().clone();
    assert_eq!(headers[header::CONTENT_TYPE], "text/css");
    assert!(headers.contains_key(header::LAST_MODIFIED));
    assert_eq!(&body(style).await[..], b"body{}");
    let etag = headers[header::ETAG].to_str().unwrap();
    assert!(etag.starts_with("\"6-"), "{etag}");
    let again = get("/wp-content/themes/classic/style.css", Some(etag)).await;
    assert_eq!(again.unwrap().status(), StatusCode::NOT_MODIFIED);
    for (path, kind, bytes) in [
        ("assets/js/a%20b+c.js", Some("text/javascript"), &b"x()"[..]),
        ("FONT.WOFF2", Some("font/woff2"), b"wOF2"),
        ("LICENSE", None, b"GPL"),
    ] {
        let response = get(&format!("/wp-content/themes/classic/{path}"), None)
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        let given = response.headers().get(header::CONTENT_TYPE).cloned();
        assert_eq!(given.as_ref().map(|k| k.to_str().unwrap()), kind, "{path}");
        assert_eq!(&body(response).await[..], bytes, "{path}");
    }
    for refused in [
        "classic/index.php",
        "classic/old.phtml",
        "classic/.env",
        "classic/out/secret.css",
        "classic/assets/..%2F..%2Fextension.wasm",
        "classic/extension.wasm",
        "classic/assets",
        "classic/missing.css",
        "classic/%FF.css",
        "bare/style.css",
        "..%2Fclassic/style.css",
        "classic",
    ] {
        let response = get(&format!("/wp-content/themes/{refused}"), None)
            .await
            .unwrap();
        let served = body(response).await;
        for secret in [&b"<?php"[..], b"secret", b"\0asm"] {
            let leaked = served.windows(secret.len()).any(|w| w == secret);
            assert!(!leaked, "{refused}");
        }
    }
    let plain = crate::tests::oracle_site().await;
    let path = "/wp-content/themes/classic/style.css";
    assert!(installed_theme_file(&plain, path, &HeaderMap::new(), 0).is_none());
}

async fn body(response: axum::response::Response) -> axum::body::Bytes {
    axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
}
