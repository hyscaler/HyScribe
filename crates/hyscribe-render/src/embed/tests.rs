// SPDX-License-Identifier: AGPL-3.0-or-later

use std::cell::RefCell;

use hyscribe_model::{Post, PostQuery, Term};
use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::outbound::{EmbedHosts, Get, Hooks, NOT_ALLOWED, Redirects, Response, Size, testing};
use crate::site::{Request, Site};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// The default install fetching from the test site, with post 1's meta
/// kept in memory as written, and post 1's content given.
pub(crate) struct Embeds {
    pub(crate) install: DefaultInstall,
    pub(crate) meta: RefCell<Vec<(u64, String, Vec<u8>)>>,
    pub(crate) content: Option<Vec<u8>>,
    pub(crate) fetches: bool,
    pub(crate) hooks: Box<dyn Hooks>,
    pub(crate) asked: RefCell<Vec<Vec<u8>>>,
    pub(crate) limits: RefCell<Vec<Option<usize>>>,
    pub(crate) allowed: EmbedHosts,
    /// Whether every fetch is stopped by a redirect off the list.
    pub(crate) away: bool,
    pub(crate) redirects: RefCell<Vec<Redirects>>,
}

impl Embeds {
    pub(crate) fn new() -> Embeds {
        Embeds {
            install: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            meta: RefCell::new(Vec::new()),
            content: None,
            fetches: true,
            hooks: Box::new(()),
            asked: RefCell::new(Vec::new()),
            limits: RefCell::new(Vec::new()),
            away: false,
            redirects: RefCell::new(Vec::new()),
            allowed: EmbedHosts {
                providers: true,
                hosts: vec![
                    b"pages.test".to_vec(),
                    b"nowhere.example".to_vec(),
                    b".mp3".to_vec(),
                ],
            },
        }
    }

    /// The meta as "key=value" lines, in the order written.
    fn rows(&self) -> String {
        self.meta
            .borrow()
            .iter()
            .map(|(_, key, value)| format!("{key}={}\n", String::from_utf8_lossy(value)))
            .collect()
    }
}

impl Site for Embeds {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.install.option(name)
    }

    fn home_url(&self) -> Vec<u8> {
        self.install.home_url()
    }

    fn post(&self, id: u64) -> Option<Post> {
        let mut post = self.install.post(id)?;
        if let Some(content) = self.content.clone().filter(|_| id == 1) {
            post.content = content;
        }
        Some(post)
    }

    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        self.install.post_terms(post_id, taxonomy)
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        let meta = self.meta.borrow();
        let found = meta.iter().find(|(id, k, _)| *id == post_id && k == key);
        match found {
            Some((_, _, value)) => Some(value.clone()),
            None if key.starts_with("_oembed") => None,
            None => self.install.post_meta(post_id, key),
        }
    }

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.install.query(query)
    }

    fn remote_get(&self, url: &[u8], args: &Get) -> Option<Result<Response, Vec<u8>>> {
        self.asked.borrow_mut().push(url.to_vec());
        self.limits.borrow_mut().push(args.limit);
        self.redirects.borrow_mut().push(args.redirects.clone());
        if self.away {
            return Some(Err(NOT_ALLOWED.to_vec()));
        }
        assert!(
            args.user_agent
                .starts_with(b"WordPress/7.1.3; http://127.0.0.1:8081")
        );
        self.fetches.then(|| testing::get(url, args.limit))
    }

    fn update_post_meta(&self, post_id: u64, key: &str, value: &[u8]) {
        let mut meta = self.meta.borrow_mut();
        match meta
            .iter_mut()
            .find(|(id, k, _)| *id == post_id && k == key)
        {
            Some((_, _, old)) => *old = value.to_vec(),
            None => meta.push((post_id, key.to_string(), value.to_vec())),
        }
    }

    fn hooks(&self) -> &dyn Hooks {
        self.hooks.as_ref()
    }

    fn embed_hosts(&self) -> &EmbedHosts {
        &self.allowed
    }
}

/// The golden script's plugin: the YouTube provider pointed at the test site.
#[derive(Debug)]
struct TestSite;

impl Hooks for TestSite {
    fn oembed_fetch_url(&self, provider: Vec<u8>, url: &[u8], args: Size) -> Vec<u8> {
        assert_eq!((args.width, args.height), (500, 750));
        assert!(url.starts_with(b"https://youtu.be/"));
        let text = String::from_utf8_lossy(&provider).into_owned();
        match text.strip_prefix("https://www.youtube.com/oembed?") {
            Some(query) => {
                format!("http://pages.test:8080/oembed/trusted.json?{query}").into_bytes()
            }
            None => provider,
        }
    }
}

/// `html` with the secret `wp_filter_oembed_result()` drew masked.
pub(crate) fn masked(html: &[u8]) -> String {
    let mut text = String::from_utf8_lossy(html).into_owned();
    for marker in ["secret=", "data-secret=\"", "data-secret='"] {
        let mut at = 0;
        while let Some(found) = text.get(at..).and_then(|rest| rest.find(marker)) {
            let start = at + found + marker.len();
            let end = start + 10;
            if text
                .get(start..end)
                .is_some_and(|s| s.bytes().all(|b| b.is_ascii_alphanumeric()))
            {
                text.replace_range(start..end, "%secret%");
            }
            at = start;
        }
    }
    text
}

/// Every case of `reference/oembed/golden.sh`: the HTML, the meta written
/// (times as `%now%`) and whether `wp-embed` was enqueued.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/oembed.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let cells: Vec<&str> = line.split('\t').collect();
        let [name, content, html, meta, enqueued] = cells.as_slice() else {
            panic!("{line}");
        };
        let (content, html, meta) = (unhex(content), unhex(html), unhex(meta));
        let mut site = Embeds::new();
        let cached = name.starts_with("cached") || *name == "expired";
        if cached {
            for row in String::from_utf8_lossy(&meta).lines() {
                let (key, value) = row.split_once('=').unwrap();
                site.update_post_meta(1, key, value.as_bytes());
            }
        }
        if *name == "trusted" {
            site.hooks = Box::new(TestSite);
        }
        let request = Request {
            post: Some(1),
            now: 1_800_000_000,
            ..Request::default()
        };
        let content_mode = name.starts_with("content");
        if content_mode {
            site.content = Some(content.clone());
        }
        let mut renderer = Renderer::new(&theme, &site, &request);
        let ours = match content_mode {
            true => {
                let post = site.post(1).unwrap();
                renderer.the_content(&post)
            }
            false => renderer.autoembed(&content),
        };
        let queued = renderer.assets.scripts.iter().any(|s| s == "wp-embed");
        let rows = site.rows().replace("=1800000000\n", "=%now%\n");
        let ours = (masked(&ours), masked(rows.as_bytes()), queued);
        let php = (
            String::from_utf8_lossy(&html).into_owned(),
            String::from_utf8_lossy(&meta).into_owned(),
            *enqueued == "1",
        );
        if ours != php {
            failures.push(format!("{name}\n  php:  {php:?}\n  ours: {ours:?}"));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(golden.lines().count(), 35);
}

/// While fetches are off, an uncached URL stays and nothing is written,
/// where a cached one still shows.
#[test]
fn fetches_off_leave_urls() {
    let theme = twentytwentyfive();
    let mut site = Embeds::new();
    site.fetches = false;
    let request = Request {
        post: Some(1),
        ..Request::default()
    };
    // A page to discover, and a provider's URL, asked of nobody.
    for url in [
        &b"http://pages.test:8080/oembed/rich.html"[..],
        b"https://www.youtube.com/watch?v=x",
    ] {
        let ours = Renderer::new(&theme, &site, &request).autoembed(url);
        assert_eq!(ours, url);
    }
    assert!(site.meta.borrow().is_empty());
    assert_eq!(site.asked.borrow().len(), 2);
}

/// A host outside the list makes no request and writes no cache row; a
/// provider needs the providers on or its host listed.
#[test]
fn hosts_outside_the_list_are_not_fetched() {
    let theme = twentytwentyfive();
    let request = Request {
        post: Some(1),
        ..Request::default()
    };
    let page = &b"http://pages.test:8080/oembed/rich.html"[..];
    let provider = &b"https://www.youtube.com/watch?v=x"[..];
    let cases = [
        // (providers, hosts, url, fetches)
        (true, &[][..], page, false),
        (true, &["Elsewhere.test"][..], page, false),
        (false, &["pages.test"][..], page, true),
        (true, &["pages.test"][..], page, true),
        (false, &[][..], provider, false),
        (false, &["other.test"][..], provider, false),
        (true, &[][..], provider, true),
        (false, &["www.youtube.com"][..], provider, true),
    ];
    for (providers, hosts, url, fetches) in cases {
        let mut site = Embeds::new();
        site.fetches = false;
        site.allowed = EmbedHosts {
            providers,
            hosts: hosts
                .iter()
                .map(|h| h.to_ascii_lowercase().into_bytes())
                .collect(),
        };
        let ours = Renderer::new(&theme, &site, &request).autoembed(url);
        assert_eq!(ours, url);
        let asked = site.asked.borrow().len();
        assert_eq!(asked > 0, fetches, "{providers} {hosts:?} {url:?}");
        assert!(site.meta.borrow().is_empty());
    }
}

/// A fetch stopped by a redirect off the list leaves the URL as it is and
/// writes no cache row, not even the failure's; the hosts redirects
/// may reach are the listed ones and, when allowed, the providers'.
#[test]
fn redirects_off_the_list_leave_urls_uncached() {
    let theme = twentytwentyfive();
    let request = Request {
        post: Some(1),
        ..Request::default()
    };
    for url in [
        &b"http://pages.test:8080/oembed/rich.html"[..],
        b"https://www.youtube.com/watch?v=x",
    ] {
        let mut site = Embeds::new();
        site.away = true;
        let ours = Renderer::new(&theme, &site, &request).autoembed(url);
        assert_eq!(ours, url);
        assert!(site.meta.borrow().is_empty());
        let seen = site.redirects.borrow();
        let Some(Redirects::Hosts(hosts)) = seen.first() else {
            panic!("embeds restrict redirects to hosts");
        };
        assert!(hosts.contains(&b"pages.test".to_vec()));
        assert!(hosts.contains(&b"www.youtube.com".to_vec()));
    }
    let mut site = Embeds::new();
    site.away = true;
    site.allowed.providers = false;
    Renderer::new(&theme, &site, &request).autoembed(b"http://pages.test:8080/x");
    let seen = site.redirects.borrow();
    assert_eq!(
        seen.first(),
        Some(&Redirects::Hosts(vec![
            b"pages.test".to_vec(),
            b"nowhere.example".to_vec(),
            b".mp3".to_vec(),
        ]))
    );
}

/// The setting's default is the providers; a site that names none allows
/// nothing.
#[test]
fn defaults_allow_the_providers_or_nothing() {
    let default = EmbedHosts::default();
    assert!(default.providers && default.hosts.is_empty());
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let nowhere = EmbedHosts {
        providers: false,
        hosts: Vec::new(),
    };
    assert_eq!(*site.embed_hosts(), nowhere);
}

/// What is not ported fetches nothing: no post, the site's own host and
/// media files.
#[test]
fn unported_embeds_fetch_nothing() {
    let theme = twentytwentyfive();
    let site = Embeds::new();
    for (url, post) in [
        (&b"http://pages.test:8080/oembed/rich.html"[..], None),
        (b"http://127.0.0.1:8081/?p=1", Some(1)),
        (b"http://pages.test:8080/a.MP3", Some(1)),
        (b"http://pages.test:8080/v.webm", Some(1)),
    ] {
        let request = Request {
            post,
            ..Request::default()
        };
        let ours = Renderer::new(&theme, &site, &request).autoembed(url);
        assert_eq!(ours, url);
    }
    assert!(site.asked.borrow().is_empty());
    assert!(site.meta.borrow().is_empty());
}

/// What autoembed's patterns leave alone, and the YouTube handler's IDs.
#[test]
fn leaves_urls_that_are_not_alone() {
    let theme = twentytwentyfive();
    let site = Embeds::new();
    let request = Request {
        post: Some(1),
        ..Request::default()
    };
    let link = "http://pages.test:8080/oembed/link.html";
    for content in [
        format!("{link} here"),
        "x http://a.test/ <p".to_string(),
        "x http://a.test/ <p class".to_string(),
        format!("<p>{link}"),
    ] {
        let ours = Renderer::new(&theme, &site, &request).autoembed(content.as_bytes());
        assert_eq!(ours, content.as_bytes());
    }
    assert_eq!(
        super::youtube_id(b"http://www.youtube.com/v/ID/x"),
        Some(&b"ID"[..])
    );
    assert_eq!(
        super::youtube_id(b"xhttp://wwwXyoutube.com/EMBED/a"),
        Some(&b"a"[..])
    );
    assert_eq!(super::youtube_id(b"https://youtube.com/embed/"), None);
    assert_eq!(super::youtube_id(b"http://www.youtube.com/watch"), None);
}

#[test]
fn finds_embedded_quotes_and_media() {
    for (html, quoted) in [
        (&b"<blockquote class=\"wp-embedded-content\">"[..], true),
        (b"<blockquote\twp-embedded-content>", true),
        (b"<blockquotex class=\"wp-embedded-content\">", false),
        (b"<blockquote>wp-embedded-content", false),
        (b"<blockquote class=\"x\">wp-embedded-content", false),
    ] {
        assert_eq!(super::embedded_quote(html), quoted, "{html:?}");
    }
    let theme = twentytwentyfive();
    let site = Embeds::new();
    let request = Request {
        post: Some(1),
        ..Request::default()
    };
    // A media handler needs a name before its extension.
    for (url, asked) in [(&b"http://a.test/a.mp3"[..], 0), (b"http://.mp3", 1)] {
        Renderer::new(&theme, &site, &request).autoembed(url);
        assert_eq!(site.asked.borrow().len(), asked);
    }
}
