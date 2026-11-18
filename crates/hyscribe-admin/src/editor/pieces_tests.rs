// SPDX-License-Identifier: AGPL-3.0-or-later

//! The block editor page's pieces against `reference/post-edit/editor-golden.sh`.

use hyscribe_model::Post;
use hyscribe_phpser::{Value, bin2hex};
use hyscribe_render::head::json_hex_tag;
use hyscribe_render::theme::Theme;
use sha2::{Digest, Sha256};

use super::*;
use crate::admin_bar::Browser;
use hyscribe_testkit::{hs_admin_page, unhex};

use crate::page::text;
use crate::tests::{BROWSER, Role, first_difference};

/// The bundled theme, as the editor reads it.
impl EditorSite for Role {
    fn theme(&self) -> &Theme {
        static THEME: std::sync::OnceLock<Theme> = std::sync::OnceLock::new();
        THEME.get_or_init(hyscribe_render::theme::twentytwentyfive)
    }
}

/// The browser the golden records Chrome with.
const CHROME: Browser = Browser {
    user_agent: b"Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36",
    mobile_hint: None,
};

/// A golden value: as PHP printed it, or its hash over 2000 bytes, which
/// is taken of the oracle's URL of its one admin file.
fn kept(value: &[u8]) -> String {
    if value.len() > 2000 {
        let text = String::from_utf8_lossy(value);
        let php = text.replace("/hs-admin/load-scripts?", "/wp-admin/load-scripts.php?");
        let hash = bin2hex(&Sha256::digest(php.as_bytes()));
        return format!("sha256:{hash} {}", php.len());
    }
    String::from_utf8_lossy(value).into_owned()
}

/// The site and browser of a case, named after its role or browser.
fn site_of(case: &str) -> (Role, &'static Browser<'static>) {
    let (role, browser) = match case.rsplit('-').next().unwrap_or_default() {
        "chrome" => ("administrator", &CHROME),
        role => (role, &BROWSER),
    };
    (
        Role(Box::leak(format!("e5:{role}").into_boxed_str())),
        browser,
    )
}

/// `$_SERVER['REQUEST_URI']` of the view of `post`.
fn uri_of(post: &Post) -> String {
    match (post.status.as_str(), post.post_type.as_str()) {
        ("auto-draft", "post") => "/hs-admin/post-new".into(),
        ("auto-draft", _) => "/hs-admin/post-new?post_type=page".into(),
        _ => format!("/hs-admin/post?post={}&action=edit", post.id),
    }
}

/// The parts of one case, as HyScribe builds them.
fn editor_parts(case: &str, post: &Post) -> Vec<(String, String)> {
    let (site, browser) = site_of(case);
    let uri = uri_of(post);
    let editing = form::Editing {
        post,
        lock: "%time%:1",
        browser,
        uri: &uri,
    };
    let custom = form::custom_settings(&site, &editing);
    let settings = settings::get_block_editor_settings(&site, custom, post);
    let mut parts = vec![(
        "post".to_string(),
        format!("{} {}", post.post_type, post.id),
    )];
    for (key, value) in settings.as_array().unwrap_or_default() {
        let part = format!("settings.{}", String::from_utf8_lossy(&key.to_php_string()));
        parts.push((part, kept(&json_hex_tag(value))));
    }
    let pieces: [(&str, Value); 4] = [
        ("edits", form::initial_edits(post)),
        ("categories", blocks::block_categories()),
        ("bindings", blocks::bindings_sources()),
        ("blocks", blocks::server_block_settings()),
    ];
    for (part, value) in pieces {
        parts.push((part.into(), kept(&json_hex_tag(&value))));
    }
    let body = form::body(&site, &editing);
    parts.push(("body".into(), bin2hex(body.as_bytes())));
    parts
}

/// The golden's cases, each with its parts but the page's handles.
fn golden_cases() -> Vec<(&'static str, Vec<(String, String)>)> {
    let golden = include_str!("../../tests/golden/post-editor.tsv");
    let mut cases: Vec<(&str, Vec<(String, String)>)> = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.splitn(3, '\t').collect();
        match cases.last_mut() {
            Some((case, parts)) if *case == f[0] => parts.push((f[1].into(), f[2].into())),
            _ => cases.push((f[0], vec![(f[1].into(), f[2].into())])),
        }
    }
    cases
}

/// The post of a case: its type and ID, new or published.
fn post_of(case: &str, post: &str) -> Post {
    let (kind, id) = post.split_once(" ").unwrap();
    let status = if case.starts_with("new") {
        "auto-draft"
    } else {
        "publish"
    };
    let (title, name): (&[u8], &[u8]) = match id {
        "1" => (b"Hello world!", b"hello-world"),
        "2" => (b"Sample Page", b"sample-page"),
        _ => (b"", b""),
    };
    Post {
        id: id.parse().unwrap(),
        author: 1,
        title: title.to_vec(),
        name: name.to_vec(),
        post_type: kind.into(),
        status: status.into(),
        ..Post::default()
    }
}

fn short(text: &str) -> &str {
    text.get(..300).unwrap_or(text)
}

/// Every case of `post-editor.tsv` but the page's styles and scripts.
#[test]
fn editor_pieces_are_php_s() {
    let cases = golden_cases();
    let mut failures = Vec::new();
    for (case, php) in &cases {
        let php: Vec<&(String, String)> = php
            .iter()
            .filter(|(p, _)| !["html", "preloads"].contains(&p.as_str()))
            .collect();
        let ours = editor_parts(case, &post_of(case, &php[0].1));
        for (i, (part, value)) in php.iter().enumerate() {
            // hs-admin's URLs, the body's as its hex.
            let value = match part.as_str() {
                "body" => {
                    bin2hex(hs_admin_page(&String::from_utf8(unhex(value)).unwrap()).as_bytes())
                }
                _ => hs_admin_page(value),
            };
            match ours.get(i) {
                Some((p, v)) if p == part && *v == value => {}
                Some((p, v)) if p == "body" => {
                    let (php, ours) = (unhex(&value), unhex(v));
                    let (php, ours) = (
                        String::from_utf8_lossy(&php),
                        String::from_utf8_lossy(&ours),
                    );
                    failures.push(format!("{case} body: {}", first_difference(&php, &ours)));
                }
                Some((p, v)) => failures.push(format!(
                    "{case} {part}: PHP {}\n  ours {p} {}",
                    short(&value),
                    short(v)
                )),
                None => failures.push(format!("{case} {part}: missing")),
            }
        }
        if ours.len() != php.len() {
            failures.push(format!("{case}: {} parts, PHP {}", ours.len(), php.len()));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(cases.len(), 12);
}

/// The page of each case of `post-editor.tsv`, its preloads' data left
/// out, and the paths it preloads, which PHP keeps when they answer.
#[test]
fn editor_pages_are_php_s() {
    let mut failures = Vec::new();
    for (case, php) in golden_cases() {
        let part = |name: &str| php.iter().find(|(p, _)| p == name).unwrap().1.clone();
        let post = post_of(case, &php[0].1);
        let (site, browser) = site_of(case);
        let uri = uri_of(&post);
        let editing = form::Editing {
            post: &post,
            lock: "%time%:1",
            browser,
            uri: &uri,
        };
        let canonical = format!("{}{uri}", crate::tests::SITE);
        let request = crate::tests::request(&canonical, &uri, browser);
        let ours = page::editor_page(&site, &editing, &Value::Null, &request).unwrap();
        let ours = ours.replace(
            "createPreloadingMiddleware( null )",
            "createPreloadingMiddleware( %preloads% )",
        );
        let php_page = hs_admin_page(&String::from_utf8(unhex(&part("html"))).unwrap());
        if ours != php_page {
            failures.push(format!("{case}: {}", first_difference(&php_page, &ours)));
        }
        let edit_theme = case.ends_with("administrator") || case.ends_with("chrome");
        let paths = page::preload_paths(&post, 6, edit_theme, "twentytwentyfive");
        let paths: Vec<String> = paths.iter().map(|(m, p)| format!("{m} {p}")).collect();
        // PHP leaves out what it refuses: the settings for all but administrators.
        let kept: Vec<&String> = paths
            .iter()
            .filter(|p| edit_theme || p.as_str() != "GET /wp/v2/settings")
            .collect();
        let tokens = part("preloads");
        let tokens: Vec<&str> = tokens.split(' ').collect();
        let php_paths: Vec<String> = tokens.chunks(2).map(|pair| pair.join(" ")).collect();
        let php_paths: Vec<&str> = php_paths.iter().map(String::as_str).collect();
        let missing: Vec<&&String> = kept
            .iter()
            .filter(|p| !php_paths.contains(&p.as_str()))
            .collect();
        let extra: Vec<&&str> = php_paths
            .iter()
            .filter(|p| !kept.iter().any(|k| k == *p))
            .collect();
        if !missing.is_empty() || !extra.is_empty() {
            failures.push(format!(
                "{case} preloads: ours only {missing:?}, PHP only {extra:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `rest_preload_api_request()`'s memo: 200 answers kept by path, the
/// `OPTIONS` ones under the place of the first, a path asked twice once.
#[test]
fn preloads_are_kept_by_path() {
    let paths: Vec<(&'static str, String)> = ["/a", "/b", "/a", "/c", "/b"]
        .iter()
        .zip(["GET", "OPTIONS", "GET", "GET", "OPTIONS"])
        .map(|(path, method)| (method, path.to_string()))
        .collect();
    let answered = |method: &str, path: &str| {
        let kept = path != "/c";
        Some(kept.then(|| (text(&format!("{method} {path}")), Value::Null)))
    };
    let memo = page::preloaded(&paths, &answered).unwrap();
    let json = String::from_utf8(json_hex_tag(&memo)).unwrap();
    assert_eq!(
        json,
        r#"{"/a":{"body":"GET /a","headers":null},"OPTIONS":{"/b":{"body":"OPTIONS /b","headers":null}}}"#
    );
    assert_eq!(page::preloaded(&paths, &|_, _| None), None);
    let gets = [("GET", "/a".to_string())];
    let memo = page::preloaded(&gets, &answered).unwrap();
    assert_eq!(memo.as_array().unwrap().len(), 1);
}

/// The page each screen and query asks for, once PHP's refusals pass.
#[test]
fn editor_targets() {
    let site = Role("default:administrator");
    let new = |kind: &str| Some(Target::New(kind.into()));
    assert_eq!(target(&site, "post-new.php", ""), new("post"));
    assert_eq!(target(&site, "post-new.php", "post_type=page"), new("page"));
    for refused in [
        "post_type=attachment",
        "post_type=nope",
        "x=1",
        "x=page",
        "post_type=page&x=post",
        "post_type[]=x",
    ] {
        assert_eq!(target(&site, "post-new.php", refused), None, "{refused}");
    }
    // An argument not ported never stands for the type, past the checks.
    let subscriber = Role("default:subscriber");
    for refused in ["", "x=post", "x=page"] {
        assert_eq!(
            target(&subscriber, "post-new.php", refused),
            None,
            "{refused}"
        );
    }
    let edit = target(&site, "post.php", "post=1&action=edit");
    assert!(matches!(edit, Some(Target::Edit(post)) if post.id == 1));
    for refused in [
        "post=1&action=edit&message=1",
        "post=999&action=edit",
        "post=-1&action=edit",
    ] {
        assert_eq!(target(&site, "post.php", refused), None, "{refused}");
    }
    assert_eq!(target(&subscriber, "post.php", "post=1&action=edit"), None);
    let locks = Role("a8lock:administrator");
    for refused in ["post=816&action=edit", "post=801&action=edit"] {
        assert_eq!(target(&locks, "post.php", refused), None, "{refused}");
    }
    // Another user's fresh lock: PHP's locked dialog, which takes no lock,
    // is not ported; a stale lock is taken over.
    assert_eq!(target(&locks, "post.php", "post=1&action=edit"), None);
    let stale = Role("a8lockold:administrator");
    assert!(target(&stale, "post.php", "post=1&action=edit").is_some());
}

/// A draft's page: its preview in the admin bar, no original referer in
/// its meta boxes area, and no author's preload without an author.
#[test]
fn a_draft_s_page() {
    let (site, browser) = site_of("x-administrator");
    let draft = Post {
        id: 1,
        post_type: "post".into(),
        status: "draft".into(),
        title: b"Hello world!".to_vec(),
        ..Post::default()
    };
    let uri = uri_of(&draft);
    let editing = form::Editing {
        post: &draft,
        lock: "%time%:1",
        browser,
        uri: &uri,
    };
    let canonical = format!("{}{uri}", crate::tests::SITE);
    let request = crate::tests::request(&canonical, &uri, browser);
    let html = page::editor_page(&site, &editing, &Value::Null, &request).unwrap();
    assert!(html.contains(">Preview Post</a>"));
    // A new post's stored title, "Auto Draft", is not the page's.
    let new = Post {
        status: "auto-draft".into(),
        title: b"Auto Draft".to_vec(),
        ..draft.clone()
    };
    let editing_new = form::Editing {
        post: &new,
        ..editing
    };
    let added = page::editor_page(&site, &editing_new, &Value::Null, &request).unwrap();
    assert!(added.contains("<title>Add Post &lsaquo;"));
    assert!(html.contains("preview=true"));
    assert!(!html.contains("_wp_original_http_referer"));
    let paths = page::preload_paths(&draft, 6, true, "twentytwentyfive");
    assert!(
        paths
            .iter()
            .all(|(_, path)| !path.starts_with("/wp/v2/users/0"))
    );
    assert_eq!(paths.len(), 23);
}
