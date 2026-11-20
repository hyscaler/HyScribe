// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::bin2hex;
use hyscribe_testkit::{hs_admin_page, unhex};

use super::*;
use crate::editor::blocks::{bindings_sources, block_categories, server_block_settings};
use crate::page::PageSite;
use crate::tests::first_difference;

/// The administrator's redirects and refusals of
/// `reference/site-editor/golden.sh`, as PHP answered them; the HTTP route
/// sanitizes the one with a quote and a space.
#[test]
fn redirects_and_refusals_are_php_s() {
    let golden = include_str!("../../../hyscribe-http/tests/golden/site-editor.tsv");
    let rows = golden
        .lines()
        .map(|line| line.split('\t').collect::<Vec<_>>());
    let mut cases = 0;
    for f in rows.filter(|f| f[1] == "administrator" && !f[0].contains("%27")) {
        let query = f[0].strip_prefix('?').unwrap();
        let uri = format!("/wp-admin/site-editor.php{}", f[0]);
        let expected = match f[2] {
            "302" => Answer::Redirect(f[3].into()),
            _ => Answer::Died("Invalid post type."),
        };
        assert_eq!(site_editor(query, &uri), Some(expected), "{}", f[0]);
        cases += 1;
    }
    assert_eq!(cases, 31);
}

/// The post the arguments name: a numeric, non-empty `postId` before `p`'s
/// page; only posts and pages are ported.
#[test]
fn context_posts() {
    let site = crate::tests::Role("default:administrator");
    let id = |query: &str| context_post(&site, query).map(|post| post.map(|post| post.id));
    for (query, expected) in [
        ("postId=0&p=/page/2", Some(Some(2))),
        ("postId=x&p=/page/2", Some(Some(2))),
        ("postId=%202", Some(Some(2))),
        ("postId=1.9", Some(Some(1))),
        ("postId=-1", Some(None)),
        ("p=/page/2%0A", Some(Some(2))),
        ("p=/page/2x", Some(None)),
        ("p=/page/", Some(None)),
        ("p=/pages/2", Some(None)),
        ("postId[]=2", None),
    ] {
        assert_eq!(id(query), expected, "{query}");
    }
    let templates = crate::tests::Role("m4ttdtemplate:administrator");
    assert_eq!(context_post(&templates, "postId=9999"), None);
}

/// A page without a name looks its template up as `page`.
#[test]
fn a_nameless_page_s_lookup() {
    let page = Post {
        id: 7,
        post_type: "page".into(),
        ..Post::default()
    };
    let paths = preload_paths(Some(&page), 4, "twentytwentyfive");
    let last = paths.last().map(|(_, path)| path.as_str());
    assert_eq!(last, Some("/wp/v2/templates/lookup?slug=page"));
}

/// The editor's page, and arguments that are not strings, are left to
/// the page, which answers only once its view wrote.
#[test]
fn the_page_is_not_a_redirect() {
    for query in [
        "",
        "p=/",
        "p=/&postType=post",
        "p=/&postType=nope&postId=",
        "p[]=/",
    ] {
        assert_eq!(
            site_editor(query, "/wp-admin/site-editor.php"),
            None,
            "{query}"
        );
    }
}

/// The page golden's cases, each with its parts.
fn golden_cases() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    let golden = include_str!("../../tests/golden/site-editor.tsv");
    let mut cases: Vec<(&str, Vec<(&str, &str)>)> = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.splitn(3, '\t').collect();
        match cases.last_mut() {
            Some((case, parts)) if *case == f[0] => parts.push((f[1], f[2])),
            _ => cases.push((f[0], vec![(f[1], f[2])])),
        }
    }
    cases
}

/// The site of a case: another embedded theme active for a case ending in
/// `-tt4` or `-tt3`.
fn site_of(case: &str) -> crate::tests::Role {
    let variant = match case.rsplit_once('-') {
        Some((_, variant @ ("tt4" | "tt3"))) => variant,
        _ => "default",
    };
    crate::tests::Role(Box::leak(
        format!("{variant}:administrator").into_boxed_str(),
    ))
}

/// The query of each case of `reference/site-editor/page-golden.sh`.
fn query_of(case: &str, theme: &str) -> String {
    let case = case
        .strip_suffix("-tt4")
        .or(case.strip_suffix("-tt3"))
        .unwrap_or(case);
    match case {
        "se-page" => "p=%2Fpage".into(),
        "se-page-2" => "p=/page/2&canvas=edit".into(),
        "se-page-gone" => "p=/page/99".into(),
        "se-draft" => "p=/&postId=3".into(),
        "se-post" => "p=/&postId=1&postType=post".into(),
        "se-styles" => "p=/styles&postType=wp_global_styles".into(),
        "se-template" => format!("p=/wp_template/{theme}//index&canvas=edit"),
        _ => String::new(),
    }
}

/// A golden value: as PHP printed it, or its hash over 2000 bytes, taken
/// of the oracle's URL of its one admin file.
fn kept(value: &str) -> String {
    if value.len() > 2000 {
        let php = value.replace("/hs-admin/load-scripts?", "/wp-admin/load-scripts.php?");
        return format!("sha256:{} {}", crate::media::tests::sha256(&php), php.len());
    }
    value.into()
}

/// Each case's page, settings, block scripts and preloaded paths are PHP's.
#[test]
fn the_pages_are_php_s() {
    let browser = &crate::tests::BROWSER;
    let mut failures = Vec::new();
    let cases = golden_cases();
    for (case, php) in &cases {
        let site = site_of(case);
        let query = query_of(case, site.theme_slug());
        let query = query.as_str();
        let post = context_post(&site, query).unwrap();
        let uri = format!("/hs-admin/site-editor?{query}");
        let uri = uri.strip_suffix('?').unwrap_or(&uri);
        // `remove_query_arg()` builds the canonical URL's query again.
        let canonical = format!("{}{uri}", crate::tests::SITE);
        let canonical = remove_query_arg(&[b"message"], canonical.as_bytes());
        let canonical = String::from_utf8(canonical).unwrap();
        let request = crate::tests::request(&canonical, uri, browser);
        let page = site_editor_page(&site, post.as_ref(), &Value::Null, &request).unwrap();
        let mut masked = page.replace(
            "createPreloadingMiddleware( null )",
            "createPreloadingMiddleware( %preloads% )",
        );
        // The values the page prints, each masked as `parts.php` masks it.
        let mut custom = Settings::default();
        for (key, value) in custom_settings(crate::tests::SITE, b"10".to_vec()) {
            custom.set(key, value);
        }
        let settings = get_block_editor_settings(&site, custom, Editor::Site(post.as_ref()));
        let mut ours: Vec<(String, String)> = Vec::new();
        for (key, value) in settings.as_array().unwrap() {
            let key = String::from_utf8_lossy(&key.to_php_string()).into_owned();
            ours.push((format!("settings.{key}"), kept(&crate::page::json(value))));
        }
        let marker = "initializeEditor( \"site-editor\", ";
        let json = crate::page::json(&settings);
        masked = masked.replace(&format!("{marker}{json}"), &format!("{marker}%settings%"));
        let pieces = [
            (
                "categories",
                "wp.blocks.setCategories( ",
                block_categories(),
            ),
            (
                "bindings",
                "for ( const source of ",
                bindings_sources(site.active_theme()),
            ),
            (
                "blocks",
                "wp.blocks.unstable__bootstrapServerSideBlockDefinitions(",
                server_block_settings(site.active_theme()),
            ),
        ];
        for (part, marker, value) in pieces {
            let json = crate::page::json(&value);
            ours.push((part.into(), kept(&json)));
            masked = masked.replace(&format!("{marker}{json}"), &format!("{marker}%{part}%"));
        }
        // The theme's global styles post, which PHP made on the first view.
        let preloaded = php.iter().find(|(part, _)| *part == "preloads").unwrap().1;
        let styles = preloaded
            .split("/wp/v2/global-styles/")
            .nth(1)
            .and_then(|rest| rest.split(['?', ' ']).next()?.parse().ok());
        let paths = preload_paths(post.as_ref(), styles.unwrap(), site.theme_slug());
        let paths: Vec<String> = paths.iter().map(|(m, p)| format!("{m} {p}")).collect();
        let mut sorted: Vec<String> = paths.clone();
        // PHP's memo lists the `OPTIONS` paths first, at the first's place.
        sorted.sort_by_key(|path| !path.starts_with("OPTIONS"));
        ours.push(("preloads".into(), sorted.join(" ")));
        ours.push(("html".into(), bin2hex(masked.as_bytes())));
        for (part, value) in php {
            let value = match *part {
                "html" => {
                    let html = String::from_utf8(unhex(value)).unwrap();
                    bin2hex(hs_admin_page(&html).as_bytes())
                }
                _ => hs_admin_page(value),
            };
            match ours.iter().find(|(p, _)| p == part) {
                Some((_, v)) if *v == value => {}
                Some((_, v)) if *part == "html" => {
                    let (php, ours) = (unhex(&value), unhex(v));
                    let difference = first_difference(
                        &String::from_utf8_lossy(&php),
                        &String::from_utf8_lossy(&ours),
                    );
                    failures.push(format!("{case} html: {difference}"));
                }
                Some((_, v)) => failures.push(format!(
                    "{case} {part}:\n  PHP  {}\n  ours {}",
                    value.get(..300).unwrap_or(&value),
                    v.get(..300).unwrap_or(v)
                )),
                None => failures.push(format!("{case} {part}: missing")),
            }
        }
        let php_parts: Vec<&str> = php.iter().map(|(part, _)| *part).collect();
        let our_parts: Vec<&str> = ours.iter().map(|(part, _)| part.as_str()).collect();
        if our_parts != php_parts {
            failures.push(format!("{case}: parts {our_parts:?}, PHP {php_parts:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(cases.len(), 15);
}

/// The settings `site-editor.php` adds are PHP's, in its order.
#[test]
fn custom_settings_are_php_s() {
    let golden = include_str!("../../tests/golden/site-editor.tsv");
    let settings = custom_settings("http://127.0.0.1:8081", b"10".to_vec());
    for (key, value) in settings.iter().filter(|(key, _)| *key != "styles") {
        let prefix = format!("se-first\tsettings.{key}\t");
        let php = golden.lines().find_map(|line| line.strip_prefix(&prefix));
        let ours = crate::page::json(value);
        let ours = if ours.len() > 2000 {
            format!(
                "sha256:{} {}",
                crate::media::tests::sha256(&ours),
                ours.len()
            )
        } else {
            ours
        };
        assert_eq!(Some(ours.as_str()), php, "{key}");
    }
    let keys: Vec<&str> = settings.iter().map(|(key, _)| *key).collect();
    let order: Vec<&str> = golden
        .lines()
        .filter_map(|line| line.strip_prefix("se-first\tsettings."))
        .filter_map(|line| line.split('\t').next())
        .skip_while(|key| *key != "siteUrl")
        .take_while(|key| *key != "__experimentalBlockBindingsSupportedAttributes")
        .collect();
    assert_eq!(keys, order);
}
