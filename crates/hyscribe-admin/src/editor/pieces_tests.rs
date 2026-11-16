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
    Post {
        id: id.parse().unwrap(),
        author: 1,
        post_type: kind.into(),
        status: status.into(),
        ..Post::default()
    }
}

/// The style and script handles `html` prints, as `editor-parts.php` lists them.
fn handles(html: &str) -> String {
    let suffixes = [
        "-inline-css",
        "-js-extra",
        "-js-before",
        "-js-after",
        "-js-module",
        "-css",
        "-js",
    ];
    let mut out = Vec::new();
    let mut rest = html;
    while let Some((_, after)) = rest.split_once('<') {
        rest = after;

        let tag = rest.split('>').next().unwrap_or_default();
        if !["link", "style", "script"]
            .iter()
            .any(|t| tag.starts_with(t))
        {
            continue;
        }
        if let Some((_, load)) = tag.split_once("/load-s") {
            let kind = if load.starts_with("tyles") {
                "styles"
            } else {
                "scripts"
            };
            let decoded = tag.replace("&#038;", "&").replace("&amp;", "&");
            let chunks: String = decoded
                .split("load%5Bchunk_")
                .skip(1)
                .map(|chunk| chunk.split_once('=').unwrap().1.split('&').next().unwrap())
                .collect();
            out.push(format!("{kind}:{}", chunks.trim_end_matches(',')));
            continue;
        }
        let id = [" id='", " id=\""].iter().find_map(|open| {
            let (_, after) = tag.split_once(open)?;
            after.split(['\'', '"']).next()
        });
        let Some(id) = id else { continue };
        for suffix in suffixes {
            if let Some(handle) = id.strip_suffix(suffix) {
                out.push(format!("{handle}{suffix}"));
                break;
            }
        }
    }
    out.join(" ")
}

/// The page's styles and scripts, in order, around an empty body.
#[test]
fn editor_closure_is_php_s() {
    let cases = golden_cases();
    let (case, php) = &cases[0];
    let (site, browser) = site_of(case);
    let post = post_of(case, &php[0].1);
    let uri = uri_of(&post);
    let editing = form::Editing {
        post: &post,
        lock: "%time%:1",
        browser,
        uri: &uri,
    };
    let settings =
        settings::get_block_editor_settings(&site, form::custom_settings(&site, &editing), &post);
    let after = form::scripts_after(&settings, &form::initial_edits(&post), &post);
    let canonical = format!("{}{uri}", crate::tests::SITE);
    let page = crate::page::Page {
        screen: crate::menu::Screen {
            pagenow: "post-new.php",
            typenow: "post",
            taxonomy: "",
            parent_file: None,
            submenu_file: None,
        },
        id: "post",
        title: "Add Post",
        help: &[],
        sidebar: "",
        screen_options: "",
        styles: &form::STYLES,
        scripts: &form::SCRIPTS,
        late_scripts: &[],
        script_data: &crate::media::enqueue_media(&site, browser, Some((post.id, Value::Int(-1))))
            .1,
        script_after: &after,
        late_styles: &[],
        admin_head: "",
        admin_footer: "",
        footer_scripts: "",
        content: "",
        canonical: &canonical,
        origin: crate::tests::SITE,
        view: None,
    };
    let html = crate::page::admin_page(&site, &page, browser).unwrap();
    let head = handles(html.split("</head>").next().unwrap());
    let footer = handles(html.split_once("<div id=\"wpfooter\"").unwrap().1);
    let golden = |part: &str| php.iter().find(|(p, _)| p == part).unwrap().1.clone();
    // The page adds the global stylesheet's variables inline itself.
    let php_head = golden("head").replace(" global-styles-css-custom-properties-inline-css", "");
    assert_eq!(head, php_head);
    // And its script module, the inline scripts before `wp-block-library`'s
    // and `wp-editor`'s, and `wp-block-styles`, a script alias.
    let mut php_footer = golden("footer");
    for token in [
        "@wordpress/latex-to-mathml/loader-js-module ",
        " wp-block-library-js-before",
        " wp-editor-js-before",
        " wp-block-styles-js-after",
    ] {
        php_footer = php_footer.replacen(token, "", 1);
    }
    assert_eq!(footer, php_footer);
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
            .filter(|(p, _)| !["head", "footer"].contains(&p.as_str()))
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
