// SPDX-License-Identifier: AGPL-3.0-or-later

use std::cell::RefCell;

use hyscribe_model::{Post, PostQuery, Term};
use hyscribe_phpser::{Key, Value, decode, encode};
use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::outbound::{Get, Hooks, Response, testing};
use crate::site::{Request, Site};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// The test clock.
const NOW: i64 = 1_800_000_000;

/// The default install with its options in memory, written as
/// `set_site_transient()` and `delete_site_transient()` write them, and
/// fetching from the test site, the one allowed host.
pub(crate) struct Feeds {
    install: DefaultInstall,
    pub(crate) options: RefCell<Vec<(String, Vec<u8>)>>,
    pub(crate) hosts: Vec<Vec<u8>>,
    pub(crate) fetches: bool,
    pub(crate) hooks: Box<dyn Hooks>,
}

impl Feeds {
    pub(crate) fn new() -> Feeds {
        Feeds {
            install: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            options: RefCell::new(Vec::new()),
            hosts: vec![b"pages.test".to_vec()],
            fetches: true,
            hooks: Box::new(()),
        }
    }

    fn set(&self, name: &str, value: &[u8]) {
        let mut options = self.options.borrow_mut();
        match options.iter_mut().find(|(n, _)| n == name) {
            Some((_, old)) => *old = value.to_vec(),
            None => options.push((name.to_string(), value.to_vec())),
        }
    }

    fn delete(&self, name: &str) {
        self.options.borrow_mut().retain(|(n, _)| n != name);
    }
}

impl Site for Feeds {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        let options = self.options.borrow();
        match options.iter().find(|(n, _)| n == name) {
            Some((_, value)) => Some(value.clone()),
            None => self.install.option(name),
        }
    }

    fn home_url(&self) -> Vec<u8> {
        self.install.home_url()
    }

    fn post(&self, id: u64) -> Option<Post> {
        self.install.post(id)
    }

    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        self.install.post_terms(post_id, taxonomy)
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.install.post_meta(post_id, key)
    }

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.install.query(query)
    }

    fn remote_get(&self, url: &[u8], args: &Get) -> Option<Result<Response, Vec<u8>>> {
        assert_eq!(args.timeout, 10);
        self.fetches.then(|| testing::get(url, args.limit))
    }

    fn feed_hosts(&self) -> Vec<Vec<u8>> {
        self.hosts.clone()
    }

    fn set_site_transient(&self, name: &str, value: &[u8], expiration: i64) {
        let (option, timeout) = (
            format!("_site_transient_{name}"),
            format!("_site_transient_timeout_{name}"),
        );
        let expires = (NOW + expiration).to_string();
        self.set(&timeout, expires.as_bytes());
        // As `hyscribe-write` stores it: an integer as its digits.
        match decode(value) {
            Ok(Value::Int(int)) => self.set(&option, int.to_string().as_bytes()),
            _ => self.set(&option, value),
        }
    }

    fn delete_site_transient(&self, name: &str) {
        self.delete(&format!("_site_transient_{name}"));
        self.delete(&format!("_site_transient_timeout_{name}"));
    }

    fn hooks(&self) -> &dyn Hooks {
        self.hooks.as_ref()
    }
}

/// A cached feed with its times and headers as the golden script masks
/// them, or the times put back for `NOW` when `seed`.
fn mask(value: &[u8], seed: bool) -> Vec<u8> {
    let text = String::from_utf8_lossy(value);
    if let Ok(time) = text.parse::<i64>() {
        return match ((time - NOW).abs() < 50_000, seed) {
            (true, false) => b"%t%".to_vec(),
            _ => value.to_vec(),
        };
    }
    if text == "%t%" && seed {
        return (NOW + 43_200).to_string().into_bytes();
    }
    let Ok(Value::Array(entries)) = decode(value) else {
        return value.to_vec();
    };
    let entries = entries
        .into_iter()
        .map(|(key, value)| {
            let name = key.to_php_string();
            let value = match (name.as_slice(), &value) {
                (b"headers", _) => Value::Array(Vec::new()),
                (b"cache_expiration_time" | b"__cache_expiration_time", Value::Int(t))
                    if *t > 1 && !seed =>
                {
                    Value::Int(0)
                }
                (b"cache_expiration_time" | b"__cache_expiration_time", Value::Int(0)) if seed => {
                    Value::Int(NOW + 43_200)
                }
                _ => value,
            };
            (key, value)
        })
        .collect::<Vec<(Key, Value)>>();
    encode(&Value::Array(entries))
}

/// "name=value" lines, masked.
fn rows(options: &[(String, Vec<u8>)]) -> String {
    options
        .iter()
        .map(|(name, value)| format!("{name}={}\n", String::from_utf8_lossy(&mask(value, false))))
        .collect()
}

/// Every case of `reference/rss/golden.sh`: the block and the cache.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/rss.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let cells: Vec<&str> = line.split('\t').collect();
        let [name, markup, html, after, before] = cells.as_slice() else {
            panic!("{line}");
        };
        let site = Feeds::new();
        // The rows a case starts from, its times put back for the clock.
        let setup = String::from_utf8_lossy(&unhex(before)).into_owned();
        for row in setup.lines() {
            let (option, value) = row.split_once('=').unwrap();
            site.set(option, &mask(&unhex(value), true));
        }
        let request = Request {
            now: NOW,
            ..Request::default()
        };
        let ours = Renderer::new(&theme, &site, &request).render(&unhex(markup));
        let after = String::from_utf8_lossy(&unhex(after)).into_owned();
        let php: String = after
            .lines()
            .map(|row| {
                let (option, value) = row.split_once('=').unwrap();
                format!(
                    "{option}={}\n",
                    String::from_utf8_lossy(&mask(&unhex(value), false))
                )
            })
            .collect();
        let ours = (
            String::from_utf8_lossy(&ours).into_owned(),
            rows(&site.options.borrow()),
        );
        let php = (String::from_utf8_lossy(&unhex(html)).into_owned(), php);
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
    assert_eq!(golden.lines().count(), 23);
}

/// A site with nothing but what every site must give.
struct Bare;

impl Site for Bare {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn home_url(&self) -> Vec<u8> {
        Vec::new()
    }

    fn post(&self, _: u64) -> Option<Post> {
        None
    }

    fn post_terms(&self, _: u64, _: &str) -> Vec<Term> {
        Vec::new()
    }

    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn query(&self, _: &PostQuery) -> Vec<Post> {
        Vec::new()
    }
}

/// Sites fetch, write and filter nothing unless they say so.
#[test]
fn sites_fetch_and_write_nothing_by_default() {
    let site = Bare;
    assert!(
        site.remote_get(b"http://a.test/", &Get::new(&site))
            .is_none()
    );
    assert!(site.feed_hosts().is_empty());
    site.update_post_meta(1, "k", b"v");
    site.set_site_transient("t", b"v", 1);
    site.delete_site_transient("t");
    let hooks = site.hooks();
    let size = crate::outbound::Size {
        width: 1,
        height: 2,
    };
    assert_eq!(hooks.oembed_fetch_url(b"p".to_vec(), b"u", size), b"p");
    assert_eq!(hooks.oembed_result(None, b"u", size), None);
    assert_eq!(hooks.embed_oembed_html(b"h".to_vec(), b"u", size, 1), b"h");
    assert_eq!(hooks.wp_feed_cache_transient_lifetime(5, b"u"), 5);
}

/// A transient as `get_site_transient()` reads it: a plain value as is,
/// one that does not unserialize as nothing.
#[test]
fn reads_plain_and_broken_transients() {
    let site = Feeds::new();
    site.set("_site_transient_plain", b"text");
    site.set("_site_transient_broken", b"a:1:{x}");
    let read = |name| crate::outbound::get_site_transient(&site, name, NOW);
    assert_eq!(read("plain"), Some(Value::String(b"text".to_vec())));
    assert_eq!(read("broken"), None);
}

/// The feed's host must be allowed, fetches on, and the fetch made; a
/// feed that is not UTF-8 reads as ISO-8859-1, SimplePie's last try.
#[test]
fn unreachable_feeds_render_as_php_renders_them() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let block = br#"<!-- wp:rss {"feedURL":"http://pages.test:8080/feed/rss2.xml"} /-->"#;
    let unreachable = "<strong>RSS Error:</strong> WP HTTP Error: A valid URL was not provided.";
    let mut off = Feeds::new();
    off.fetches = false;
    let mut elsewhere = Feeds::new();
    elsewhere.hosts = vec![b"other.test".to_vec()];
    for site in [off, elsewhere] {
        let html = Renderer::new(&theme, &site, &request).render(block);
        assert!(String::from_utf8_lossy(&html).contains(unreachable));
        assert!(site.options.borrow().is_empty());
    }
    let mut refused = Feeds::new();
    refused.hosts = vec![b"nowhere.test".to_vec()];
    let block = br#"<!-- wp:rss {"feedURL":"http://nowhere.test/feed"} /-->"#;
    let html = Renderer::new(&theme, &refused, &request).render(block);
    assert!(String::from_utf8_lossy(&html).contains(unreachable));
    let latin = br#"<!-- wp:rss {"feedURL":"http://pages.test:8080/feed/latin.xml"} /-->"#;
    let html = Renderer::new(&theme, &Feeds::new(), &request).render(latin);
    assert!(String::from_utf8_lossy(&html).contains("Caf\u{e9}"));
}

#[test]
fn sniffs_feeds_as_the_locator_does() {
    for (kind, body, feed) in [
        (&b"application/atom+xml; charset=utf-8"[..], &b""[..], true),
        (b"text/xml", b"", true),
        (b"image/svg+xml", b"<rss", false),
        (
            b"text/html",
            b"\xEF\xBB\xBF <!-- c --> <!DOCTYPE x> <?xml ?>\n<rdf:RDF",
            true,
        ),
        (b"text/html", b"<feed", true),
        (b"text/html", b"<html><rss>", false),
        (b"text/html", b"text", false),
        (b"text/html", b"<!-- open", false),
        (b"text/html", b"<!x", false),
        (b"text/html", b"<?x", false),
        (b"", b" ", false),
    ] {
        assert_eq!(super::is_feed(kind, body), feed, "{kind:?} {body:?}");
    }
}
