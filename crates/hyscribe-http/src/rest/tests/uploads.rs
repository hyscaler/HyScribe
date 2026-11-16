// SPDX-License-Identifier: AGPL-3.0-or-later

//! The upload routes' golden, `uploads.tsv` of
//! `reference/upload-routes/golden.sh`: each case of its `cases.list` sent
//! over HTTP as a visitor and each role to `wordpress_saves` with a writer
//! and an uploads folder of PHP's 2 MB limit, the answer, its headers and
//! the files written matched to PHP's, the database and the folder put
//! back after a case that writes. The types PHP checks with `fileinfo`
//! answer 501 once the checks before them pass.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use hyscribe_render::date::{Zone, format_unix};
use hyscribe_testkit::unhex;
use sqlx::Connection as _;

use super::writes::{fresh, now_masked, saves_site};
use super::*;
use crate::Uploads;

/// The identities of the golden: who, login and user ID.
const WHO: [(&str, &str, u64); 6] = [
    ("visitor", "", 0),
    ("administrator", "admin", 1),
    ("editor", "editor", 701),
    ("author", "author", 702),
    ("contributor", "contributor", 703),
    ("subscriber", "subscriber", 704),
];

/// The cases whose file PHP checks with `fileinfo`: a text file, and a
/// `.jpg` that is not an image.
const FILEINFO: [&str; 2] = ["notes.txt", "fake.jpg"];

/// The files `golden.sh` makes, by name.
fn made(name: &str) -> Option<Vec<u8>> {
    Some(match name {
        "evil.php" => b"<?php echo \"ran\";\n".to_vec(),
        "page.html" => b"<html><body><script>alert(1)</script></body></html>\n".to_vec(),
        "notes.txt" => b"Plain notes.\n".to_vec(),
        "fake.jpg" => b"Not a picture at all.\n".to_vec(),
        "empty.jpg" => Vec::new(),
        "over.jpg" => vec![0; 2_101_248],
        _ => return None,
    })
}

/// A file of the corpus, or one `golden.sh` makes.
fn source(name: &str) -> Vec<u8> {
    let corpus = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../hyscribe-image/tests/corpus/"
    );
    made(name).unwrap_or_else(|| std::fs::read(format!("{corpus}{name}")).unwrap())
}

/// The type curl gives a form's file by its name.
fn curl_type(name: &str) -> &'static str {
    let ext = name
        .rsplit_once('.')
        .map_or("", |(_, ext)| ext)
        .to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "txt" => "text/plain",
        "html" | "htm" => "text/html",
        _ => "application/octet-stream",
    }
}

/// A multipart form of `fields` with the file `name` of `kind` as `part`.
fn multipart(
    (part, kind): (&str, &str),
    name: &str,
    bytes: &[u8],
    fields: &[(String, String)],
) -> Vec<u8> {
    let mut body = format!(
        "--BOUNDARY\r\nContent-Disposition: form-data; name=\"{part}\"; filename=\"{name}\"\r\nContent-Type: {kind}\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(bytes);
    for (key, value) in fields {
        let field = format!(
            "\r\n--BOUNDARY\r\nContent-Disposition: form-data; name=\"{key}\"\r\n\r\n{value}"
        );
        body.extend_from_slice(field.as_bytes());
    }
    body.extend_from_slice(b"\r\n--BOUNDARY--\r\n");
    body
}

/// The cookies of `login`, user `id`, logged in at `/hs-login`, and their
/// REST and `media-form` nonces.
async fn logged_in(site: &Arc<Site>, login: &str, user: u64) -> (String, (String, String)) {
    let request = axum::http::Request::builder()
        .method("POST")
        .uri("/hs-login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    let form = format!("log={login}&pwd=reference-only-password");
    let response = send(site, request, &form).await;
    let cookies: Vec<String> = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|cookie| {
            cookie
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_string()
        })
        .collect();
    let logged_in = cookies
        .iter()
        .find(|c| c.starts_with("hyscribe_logged_in_"))
        .unwrap();
    let value = urldecode(logged_in.split_once('=').unwrap().1.as_bytes());
    let token = parse_auth_cookie(&value).unwrap().token.to_vec();
    let (site, handle) = (Arc::clone(site), Handle::current());
    let nonces = tokio::task::spawn_blocking(move || {
        let mut reads = LoginReads::new(&site, handle, now());
        reads.act_as(user, token);
        (reads.nonce("wp_rest"), reads.nonce("media-form"))
    })
    .await
    .unwrap();
    (cookies.join("; "), nonces)
}

/// `body` with what follows each `key`, as long as `span` reads it, as
/// `with`.
fn after(body: &str, key: &str, span: impl Fn(&str) -> usize, with: &str) -> String {
    let mut out = String::new();
    let mut rest = body;
    while let Some(at) = rest.find(key) {
        let (head, tail) = rest.split_at(at + key.len());
        out.push_str(head);
        let len = span(tail);
        if len > 0 {
            out.push_str(with);
        }
        rest = tail.get(len..).unwrap();
    }
    out + rest
}

/// The length of `n` hexadecimal digits at the start of `text`, else 0.
fn hex(text: &str, n: usize) -> usize {
    let found = text
        .get(..n)
        .is_some_and(|h| h.bytes().all(|b| b.is_ascii_hexdigit()));
    if found { n } else { 0 }
}

/// `body` masked as `reference/upload-routes/mask.php` masks PHP's.
fn masked(body: &str, (start, end): (i64, i64), nonces: &(String, String)) -> String {
    let mut body = body.to_string();
    for nonce in [&nonces.0, &nonces.1]
        .into_iter()
        .filter(|nonce| !nonce.is_empty())
    {
        body = body.replace(nonce.as_str(), "%nonce%");
    }
    for action in ["update", "delete", "edit"] {
        let key = format!("\"{action}\":\"");
        body = after(&body, &key, |tail| hex(tail, 10), "%nonce%");
    }
    for id in ["dismiss-", "error-description-"] {
        body = after(&body, id, |tail| hex(tail, 13), "%uniqid%");
    }
    let mut body = now_masked(&body, (start, end));
    for ms in start * 1000..=end * 1000 + 999 {
        body = body.replace(&ms.to_string(), "%nowms%");
    }
    let body = body.replace(&format_unix("F j, Y", start, Zone::Utc), "%today%");
    let (year, month) = (
        format_unix("Y", start, Zone::Utc),
        format_unix("m", start, Zone::Utc),
    );
    let month = |slash: &str| format!("{year}{slash}{month}{slash}");
    let body = body
        .replace(&month("\\/"), "%ym%\\/")
        .replace(&month("/"), "%ym%/");
    let digits = |tail: &str| tail.bytes().take_while(u8::is_ascii_digit).count();
    let body = after(&body, "\"filesize\":", digits, "%filesize%");
    let body = after(&body, "\"filesizeInBytes\":", digits, "%filesize%");
    let quoted = |tail: &str| tail.find('"').unwrap_or_default();
    after(&body, "\"filesizeHumanReadable\":\"", quoted, "%filesize%")
}

/// PHP's answer under hs-admin's paths.
fn hs_paths(php: &str) -> String {
    let php = php
        .replace("\\/wp-admin\\/post.php?", "\\/hs-admin\\/post?")
        .replace("\\/wp-admin\\/profile.php", "\\/hs-admin\\/profile")
        .replace("\\/wp-admin\\/user-edit.php?", "\\/hs-admin\\/user-edit?")
        .replace("/wp-admin/upload.php", "/hs-admin/upload")
        .replace(
            "%2Fwp-admin%2Fasync-upload.php",
            "%2Fhs-admin%2Fasync-upload",
        )
        .replace("%2Fwp-admin%2Fmedia-new.php", "%2Fhs-admin%2Fmedia-new");
    match php.starts_with("<!DOCTYPE") {
        true => hyscribe_testkit::hs_admin_page(&php),
        false => php.replace("/wp-login.php", "/hs-login"),
    }
}

/// The files under `dir`, by their path within it, sorted.
fn files(dir: &Path) -> Vec<String> {
    let mut found = Vec::new();
    let mut folders = vec![dir.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let relative = path.strip_prefix(dir).unwrap();
                found.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    found.sort();
    found
}

/// The site the golden replays on: `wordpress_saves` with a writer and
/// uploads into `uploads`.
async fn uploading(sessions: &Path, uploads: &Path) -> Arc<Site> {
    let site = Arc::into_inner(saves_site(sessions).await).unwrap();
    let limits = hyscribe_config::ImageLimits::default();
    Arc::new(site.with_uploads(Uploads::new(uploads.into(), 2 << 20, limits)))
}

/// `wordpress_saves` as the golden was recorded over: [`fresh`], and
/// `reference/upload-routes/state.sql`.
async fn reset() {
    fresh().await;
    run(include_str!(
        "../../../../../reference/upload-routes/state.sql"
    ))
    .await;
}

/// A logged in user's cookies and nonces.
type Login = (String, (String, String));

/// The request a line of `cases.list` makes, by `login` or a visitor.
fn request(line: &str, login: Option<&Login>) -> axum::http::Request<Body> {
    let case: Vec<&str> = line.split('\t').collect();
    let (method, target, flags) = (case[0], case[1], case[2]);
    let (src, sent) = case[3].split_once(':').unwrap_or((case[3], case[3]));
    let flag = |name: &str| flags.split(',').any(|f| f == name);
    let mut fields: Vec<(String, String)> = match case[4] {
        "-" => Vec::new(),
        given => given
            .split('&')
            .map(|pair| {
                let (key, value) = pair.split_once('=').unwrap();
                let value = urldecode(value.as_bytes());
                (key.to_string(), String::from_utf8(value).unwrap())
            })
            .collect(),
    };
    if flag("wrongnonce") {
        fields.push(("_wpnonce".into(), "0123456789".into()));
    } else if let Some((_, (_, form))) = login.filter(|_| flag("nonce")) {
        fields.push(("_wpnonce".into(), form.clone()));
    }
    let target = target
        .replace("/wp-admin/async-upload.php", "/hs-admin/async-upload")
        .replace("/wp-admin/media-new.php", "/hs-admin/media-new");
    let mut request = axum::http::Request::builder()
        .method(method)
        .header(header::HOST, "127.0.0.1:8081");
    if let Some((cookies, (rest, _))) = login {
        request = request.header(header::COOKIE, cookies);
        if target.contains("rest_route") {
            request = request.header("x-wp-nonce", rest);
        }
    }
    let bytes = source(src);
    let (uri, body) = if flag("raw") {
        let query: Vec<String> = fields
            .iter()
            .map(|(key, value)| {
                let value = hyscribe_phpser::urlencode(value.as_bytes());
                format!("{key}={}", String::from_utf8(value).unwrap())
            })
            .collect();
        let kind = flags.split(',').find_map(|f| f.strip_prefix("type="));
        if let Some(kind) = kind.filter(|_| !flag("notype")) {
            request = request.header(header::CONTENT_TYPE, kind);
        }
        if flag("baddisposition") {
            request = request.header(header::CONTENT_DISPOSITION, "attachment");
        } else if !flag("nodisposition") {
            let disposition = format!("attachment; filename={sent}");
            request = request.header(header::CONTENT_DISPOSITION, disposition);
        }
        if flag("md5") {
            request = request.header("content-md5", "0");
        }
        let uri = match query.is_empty() {
            true => target.clone(),
            false => format!("{target}&{}", query.join("&")),
        };
        (uri, bytes)
    } else {
        let part = flags.split(',').find_map(|f| f.strip_prefix("part="));
        let part = part.unwrap_or(if flag("restfile") {
            "file"
        } else {
            "async-upload"
        });
        let kind = flags.split(',').find_map(|f| f.strip_prefix("kind="));
        let kind = kind.unwrap_or_else(|| curl_type(sent));
        let form = "multipart/form-data; boundary=BOUNDARY";
        request = request.header(header::CONTENT_TYPE, form);
        (
            target.clone(),
            multipart((part, kind), sent, &bytes, &fields),
        )
    };
    request.uri(uri).body(Body::from(body)).unwrap()
}

/// The cases of `cases.list`.
fn cases() -> Vec<&'static str> {
    let list = include_str!("../../../../../reference/upload-routes/cases.list");
    list.lines().filter(|line| !line.starts_with('#')).collect()
}

/// The users of the golden logged in, by who they are.
async fn logins(site: &Arc<Site>) -> Vec<(&'static str, Login)> {
    let mut logins = Vec::new();
    for (who, login, id) in WHO.iter().skip(1) {
        logins.push((*who, logged_in(site, login, *id).await));
    }
    logins
}

/// Empties the uploads folder.
fn emptied(uploads: &Path) {
    for entry in std::fs::read_dir(uploads).unwrap() {
        std::fs::remove_dir_all(entry.unwrap().path()).unwrap();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn upload_routes_match_php() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    reset().await;
    let (sessions, uploads) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let mut site = uploading(sessions.path(), uploads.path()).await;
    let logins = logins(&site).await;
    let cases = cases();
    let golden = include_str!("../../../tests/golden/uploads.tsv");
    let (mut matched, mut unported) = (0, 0);
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let (who, n) = (f[0], f[1].parse::<usize>().unwrap());
        let case = cases[n - 1];
        let login = logins
            .iter()
            .find(|(name, _)| *name == who)
            .map(|(_, login)| login);
        let visitor = (String::new(), (String::new(), "%nonce%".to_string()));
        let nonces = &login.unwrap_or(&visitor).1;
        let start = now();
        let response = router(Arc::clone(&site))
            .oneshot(request(case, login))
            .await
            .unwrap();
        let end = now();
        let what = format!("{who} case {n}: {case}");
        let status = response.status();
        let written = files(uploads.path());
        let sent = case.split('\t').nth(3).unwrap();
        let past_checks = matches!(who, "administrator" | "editor" | "author");
        if FILEINFO.contains(&sent) && past_checks {
            assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{what}");
            assert!(written.is_empty(), "{what}");
            unported += 1;
            continue;
        }
        assert_eq!(status.as_u16().to_string(), f[2], "{what}");
        let mut php_headers: Vec<String> = String::from_utf8(unhex(f[3]))
            .unwrap()
            .lines()
            .map(hs_paths)
            .collect();
        php_headers.sort();
        let kept = |name: &str| {
            [
                "content-type",
                "location",
                "x-wp-upload-attachment-id",
                "x-redirect-by",
                "x-robots-tag",
                "cache-control",
                "expires",
            ]
            .contains(&name)
        };
        let mut ours: Vec<String> = response
            .headers()
            .iter()
            .filter(|(name, _)| kept(name.as_str()))
            .map(|(name, value)| format!("{name}: {}", value.to_str().unwrap()))
            .collect();
        ours.sort();
        assert_eq!(ours, php_headers, "{what}");
        let ours = masked(&body(response).await, (start, end), nonces);
        let php = hs_paths(&String::from_utf8(unhex(f[4])).unwrap());
        let page = php.starts_with("<!DOCTYPE");
        let settled = |text: String| match page {
            true => hyscribe_testkit::settled_page(&text),
            false => text,
        };
        assert_eq!(settled(ours), settled(php), "{what}");
        let month = format_unix("Y/m/", start, Zone::Utc);
        let written: Vec<String> = written.iter().map(|f| f.replace(&month, "%ym%/")).collect();
        let php_files: Vec<&str> = f[5].split(' ').filter(|f| !f.is_empty()).collect();
        assert_eq!(written, php_files, "{what}");
        matched += 1;
        if status.is_success() || status.is_redirection() {
            reset().await;
            emptied(uploads.path());
            site = uploading(sessions.path(), uploads.path()).await;
        }
    }
    assert_eq!((matched, unported), (6 * 34 - 3 * 2, 3 * 2));
}

/// Hooks that lock `0`, the folder of the month, once an attachment is
/// inserted, so its sizes cannot be written.
struct Locks(std::path::PathBuf);

impl hyscribe_image::hooks::Hooks for Locks {}

impl crate::uploads::UploadHooks for Locks {
    fn add_attachment(&self, _id: u64) {
        std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o500)).unwrap();
    }
}

/// `sql` run on `wordpress_saves`.
async fn run(sql: &str) {
    let db = super::writes::saves();
    let options = sqlx::mysql::MySqlConnectOptions::new()
        .host(&db.host)
        .port(db.port)
        .database(&db.name)
        .username(&db.user)
        .password(db.password.expose());
    let mut conn = sqlx::MySqlConnection::connect_with(&options).await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(sql.to_string()))
        .execute(&mut conn)
        .await
        .unwrap();
}

/// The status and body `site` answers the case `line` by `login`.
async fn sent_case(site: &Arc<Site>, line: &str, login: &Login) -> (StatusCode, String) {
    let response = router(Arc::clone(site))
        .oneshot(request(line, Some(login)))
        .await
        .unwrap();
    (response.status(), body(response).await)
}

/// What the golden cannot show: the arguments not ported answer 501 and
/// write nothing; without a writer or an uploads folder an upload is
/// refused; a write or a size that fails answers the error page and leaves
/// no upload without its attachment; and the author's name is their
/// nickname where their display name is empty.
#[tokio::test(flavor = "multi_thread")]
async fn uploads_stop_short() {
    let _saves = crate::admin::tests::SAVES.lock().await;
    reset().await;
    let (sessions, uploads) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let site = uploading(sessions.path(), uploads.path()).await;
    let logins = logins(&site).await;
    let admin = &logins[0].1;
    let async_upload = "POST\t/wp-admin/async-upload.php";
    let media_new = "POST\t/wp-admin/media-new.php";
    let rest = "POST\t/?rest_route=/wp/v2/media";
    for line in [
        format!("{async_upload}\tform,nonce,part=other\tsmall.jpg\taction=upload-attachment"),
        format!(
            "{async_upload}\tform,nonce\tsmall.jpg\taction=upload-attachment&post_data[context]=x"
        ),
        format!("{async_upload}\tform,nonce\tsmall.jpg\tattachment_id=1&fetch=3"),
        format!("{async_upload}\tform,nonce\tsmall.jpg\tpost_id=0"),
        format!("{async_upload}\tform,nonce,part=other\tsmall.jpg\tshort=1"),
        format!("{media_new}\tform\tsmall.jpg\t-"),
        format!("{media_new}\tform,nonce,part=other\tsmall.jpg\thtml-upload=Upload"),
        format!("{rest}\traw,type=image/jpeg,md5\tsmall.jpg\t-"),
        format!("{rest}\trestfile,part=other\tsmall.jpg\t-"),
        format!("{rest}\trestfile,kind=image/x-unknown\tsmall.jpg\t-"),
        format!("{rest}\traw,type=image/jpeg\tbig.jpg\t-"),
        format!("{rest}\traw,type=image/jpeg\torient-6.jpg\t-"),
        format!("{async_upload}\tform,nonce\tnotes.txt\tshort=1"),
        format!("{media_new}\tform,nonce\tnotes.txt\thtml-upload=Upload"),
    ] {
        let (status, _) = sent_case(&site, &line, admin).await;
        assert_eq!(status, StatusCode::NOT_IMPLEMENTED, "{line}");
        assert!(files(uploads.path()).is_empty(), "{line}");
    }
    let disposition = format!("{rest}\traw,type=image/jpeg,baddisposition\tsmall.jpg\t-");
    let (status, refused) = sent_case(&site, &disposition, admin).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(
        refused.contains("rest_upload_invalid_disposition"),
        "{refused}"
    );
    // A tall image's sizes are portraits.
    let tall = format!("{async_upload}\tform,nonce\ttall.jpg\taction=upload-attachment");
    let (status, json) = sent_case(&site, &tall, admin).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.contains("\"orientation\":\"portrait\""), "{json}");
    emptied(uploads.path());
    reset().await;
    let small = format!("{async_upload}\tform,nonce\tsmall.jpg\taction=upload-attachment");
    let photo = format!("{async_upload}\tform,nonce\tphoto.jpg\taction=upload-attachment");
    let raw = format!("{rest}\traw,type=image/jpeg\tphoto.jpg\t-");
    // Without an uploads folder, or a writer (shadow mode).
    let bare = Arc::into_inner(saves_site(sessions.path()).await).unwrap();
    let bare = Arc::new(bare);
    assert_eq!(
        sent_case(&bare, &small, admin).await.0,
        StatusCode::NOT_IMPLEMENTED
    );
    assert_eq!(
        sent_case(&bare, &raw, admin).await.0,
        StatusCode::NOT_IMPLEMENTED
    );
    let shadow = super::site(false, sessions.path()).await;
    assert_eq!(
        sent_case(&shadow, &small, admin).await.0,
        StatusCode::NOT_IMPLEMENTED
    );
    let (status, refused) = sent_case(&shadow, &raw, admin).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert!(refused.contains("\"code\":\"wp_die\""), "{refused}");
    // An insert, then the metadata, that the database refuses.
    let refuse = |table: &str, what: &str| {
        format!(
            "CREATE TRIGGER hyscribe_refuse BEFORE INSERT ON {table} FOR EACH ROW \
             IF {what} THEN SIGNAL SQLSTATE '45000' SET MESSAGE_TEXT = 'refused'; END IF"
        )
    };
    run(&refuse("wp_posts", "NEW.post_type = 'attachment'")).await;
    for line in [&small, &raw] {
        let (status, page) = sent_case(&site, line, admin).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{line}");
        assert!(
            page.contains("Error establishing a database connection"),
            "{line}"
        );
        assert!(files(uploads.path()).is_empty(), "{line}");
    }
    run("DROP TRIGGER hyscribe_refuse").await;
    run(&refuse(
        "wp_postmeta",
        "NEW.meta_key = '_wp_attachment_metadata'",
    ))
    .await;
    for line in [&small, &raw] {
        let (status, _) = sent_case(&site, line, admin).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{line}");
        emptied(uploads.path());
    }
    run("DROP TRIGGER hyscribe_refuse").await;
    // Sizes that cannot be written: `add_attachment` locks the folder.
    let folder = uploads.path().join(format_unix("Y/m", now(), Zone::Utc));
    let locking = Uploads::new(uploads.path().into(), 2 << 20, Default::default())
        .with_hooks(Arc::new(Locks(folder.clone())));
    let locked = Arc::into_inner(saves_site(sessions.path()).await).unwrap();
    let locked = Arc::new(locked.with_uploads(locking));
    for line in [&photo, &raw] {
        let (status, _) = sent_case(&locked, line, admin).await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR, "{line}");
        std::fs::set_permissions(&folder, std::fs::Permissions::from_mode(0o700)).unwrap();
        emptied(uploads.path());
    }
    // The author's nickname, without a display name.
    run("UPDATE wp_users SET display_name = '' WHERE ID = 1").await;
    let site = uploading(sessions.path(), uploads.path()).await;
    let (status, json) = sent_case(&site, &small, admin).await;
    assert_eq!(status, StatusCode::OK);
    assert!(json.contains("\"authorName\":\"admin\""), "{json}");
    emptied(uploads.path());
    reset().await;
}
