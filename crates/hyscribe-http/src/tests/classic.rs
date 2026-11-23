// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::Query;
use axum::http::{HeaderMap, Uri};

use hyscribe_ext::{TemplateFile, locate};
use hyscribe_render::site::Request;
use hyscribe_render::site_templates::with_page_template;

use super::{NoFallback, fixture_config};
use crate::preview_nonce::{self, Entry};
use crate::{Outcome, Site, outcome, pretty_request};

fn file(name: &str) -> TemplateFile {
    TemplateFile {
        file: name.into(),
        name: None,
        post_types: Vec::new(),
    }
}

/// The site a golden line's options describe, as the page goldens build it.
async fn site_of(options: &str) -> Arc<Site> {
    site_on(&fixture_config(), options).await
}

/// The site on the database `config` that `options` describe.
pub(crate) async fn site_on(config: &hyscribe_config::Database, options: &str) -> Arc<Site> {
    let db = hyscribe_db::Db::connect(config).await.unwrap();
    let oracle = Some("http://127.0.0.1:8081".to_string());
    let mut site = Site::load(db, oracle.clone(), oracle).await.unwrap();
    let named = options.split('&').filter_map(|o| o.split_once('='));
    for (name, value) in named.filter(|(name, _)| !name.starts_with('_')) {
        let value = hyscribe_phpser::urldecode(value.as_bytes());
        site.options.insert(name.into(), value);
    }
    if !options
        .split('&')
        .any(|option| option.starts_with("_fallback="))
    {
        site.hooks = Arc::new(NoFallback);
    }
    site.build_rewriter();
    Arc::new(site)
}

/// The template `path` resolves to over `files`, as the front end asks
/// for it, `-` when the loader is not reached.
async fn resolved(site: &Arc<Site>, path: &str, files: &[TemplateFile]) -> String {
    let Some((_, candidates, _)) = page_of(site, path).await else {
        return "-".into();
    };
    locate(&candidates, files).unwrap_or("-").to_string()
}

/// The page `path` shows, its templates' candidates and its title, as the
/// front end asks for them, none when the loader is not reached.
pub(crate) async fn page_of(
    site: &Arc<Site>,
    path: &str,
) -> Option<(Request, Vec<String>, String)> {
    let uri: Uri = path.parse().unwrap();
    let Query(mut query) = Query::<HashMap<String, String>>::try_from_uri(&uri).unwrap_or_default();
    // As the front end, a search given as an array is an empty search.
    let get = hyscribe_phpser::parse_str(uri.query().unwrap_or_default().as_bytes());
    if get.get_str("s").is_some_and(|s| s.as_array().is_some()) {
        query.insert("s".into(), String::new());
    }
    let headers = HeaderMap::new();
    let refused = preview_nonce::refuse(site, &headers, uri.query(), false, Entry::Page);
    if refused.await.is_some() {
        return None;
    }
    let (asked, query) = pretty_request(site, uri.clone(), query).await.unwrap();
    let Ok((Outcome::Page(page), _)) = outcome(site, (&asked, &uri), &query).await else {
        return None;
    };
    // The query vars and the request as the front end sets them.
    let page = crate::requested(*page, &uri, &asked);
    let (site, request) = (site.clone(), page.request.clone());
    let found = tokio::task::spawn_blocking(move || {
        let reads = crate::reads::Reads::new(&site, tokio::runtime::Handle::current());
        let found = with_page_template(&reads, &request, &page.candidates);
        (request, found, page.title)
    })
    .await
    .unwrap();
    Some(found)
}

/// Twenty Twenty-One's templates for the default install's requests of
/// `reference/pages/default.list`, as PHP's template loader picks them
/// (`reference/themes/hierarchy-golden.sh`).
#[tokio::test(flavor = "multi_thread")]
async fn twenty_twenty_one_resolves_as_php() {
    let golden = include_str!("../../tests/golden/classic-hierarchy-twentytwentyone.tsv");
    let files: Vec<TemplateFile> = golden
        .lines()
        .filter_map(|line| line.strip_prefix("file\t"))
        .map(file)
        .collect();
    assert_eq!(files.len(), 48);
    let mut sites: HashMap<String, Arc<Site>> = HashMap::new();
    let (mut failures, mut pages) = (Vec::new(), 0);
    for line in golden
        .lines()
        .filter_map(|line| line.strip_prefix("page\t"))
    {
        let fields: Vec<&str> = line.split('\t').collect();
        let site = match sites.get(fields[0]) {
            Some(site) => site.clone(),
            None => {
                let site = site_of(fields[0]).await;
                sites.insert(fields[0].into(), site.clone());
                site
            }
        };
        let path = String::from_utf8(hyscribe_testkit::unhex(fields[1])).unwrap();
        let got = resolved(&site, &path, &files).await;
        if got != fields[3] {
            failures.push(format!(
                "{} {path}: PHP {} HyScribe {got}",
                fields[0], fields[3]
            ));
        }
        pages += 1;
    }
    assert_eq!(pages, 101);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
