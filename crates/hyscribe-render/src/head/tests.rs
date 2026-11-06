// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{Value, json_decode};
use hyscribe_testkit::unhex;

use super::*;

fn text(value: Option<&Value>) -> String {
    String::from_utf8_lossy(&value.map(Value::to_php_string).unwrap_or_default()).into_owned()
}

/// Every scenario of `reference/head/modules.php`: the import map, the
/// modules printed in the head and the preloads.
#[test]
fn script_modules_match_php() {
    let golden = include_str!("../../tests/golden/script-modules.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (scenario, php) = line.split_once('\t').unwrap();
        let scenario = json_decode(&unhex(scenario)).unwrap();
        let php = unhex(php);
        let siteurl = text(scenario.get_str("siteurl"));
        let registry: Vec<Module> = scenario
            .get_str("modules")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .map(|(_, m)| {
                let src = text(m.get_str("src"));
                Module {
                    id: text(m.get_str("id")),
                    src: if src.is_empty() {
                        src
                    } else {
                        format!("{siteurl}{src}")
                    },
                    version: m.get_str("version").cloned().unwrap_or_default(),
                    dependencies: m
                        .get_str("deps")
                        .and_then(Value::as_array)
                        .unwrap_or_default()
                        .iter()
                        .map(|(_, d)| (text(d.get_str("id")), text(d.get_str("import"))))
                        .collect(),
                    in_footer: m.get_str("in_footer") == Some(&Value::Bool(true)),
                    fetchpriority: text(m.get_str("fetchpriority")),
                }
            })
            .collect();
        let queue: Vec<String> = scenario
            .get_str("queue")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .map(|(_, id)| text(Some(id)))
            .collect();
        let ours = Modules {
            registry: &registry,
            queue: &queue,
        }
        .head();
        if ours != php {
            failures.push(format!(
                "{scenario:?}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(3)].join("\n")
    );
    assert_eq!(golden.lines().count(), 400);
}

/// Every case of `reference/head/fonts.php`: font faces from a
/// `fontFamilies` setting, relative CSS links and `<style>` text.
#[test]
fn fonts_and_css_links_match_php() {
    let golden = include_str!("../../tests/golden/fonts-css.tsv");
    let theme_uri = "http://127.0.0.1:8081/wp-content/themes/twentytwentyfive";
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<Vec<u8>> = line.split('\t').skip(1).map(unhex).collect();
        let (input, ours, php) = if line.starts_with("fonts") {
            let families = json_decode(&fields[0]).unwrap();
            (
                fields[0].clone(),
                font_faces(Some(&families), theme_uri),
                fields[1].clone(),
            )
        } else if line.starts_with("style") {
            let ours = [&b"<style>"[..], &style_text(&fields[0]), b"</style>"].concat();
            (fields[0].clone(), ours, fields[1].clone())
        } else {
            let css = String::from_utf8(fields[0].clone()).unwrap();
            let url = String::from_utf8(fields[1].clone()).unwrap();
            (
                fields[0].clone(),
                normalize_relative_css_links(&css, &url).into_bytes(),
                fields[2].clone(),
            )
        };
        if ours != php {
            failures.push(format!(
                "{}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&input),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(3)].join("\n")
    );
    assert_eq!(golden.lines().count(), 605);
}

/// The fixture with some options replaced and post 1's comments and pings
/// closed when `closed` is set.
struct Options {
    base: crate::fixture::DefaultInstall,
    options: Vec<(&'static str, &'static [u8])>,
    closed: bool,
}

impl Site for Options {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match self.options.iter().find(|(n, _)| *n == name) {
            Some((_, value)) => Some(value.to_vec()),
            None => self.base.option(name),
        }
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        self.base.post(id).map(|post| hyscribe_model::Post {
            comment_status: if self.closed {
                "closed".into()
            } else {
                post.comment_status.clone()
            },
            ping_status: if self.closed {
                "closed".into()
            } else {
                post.ping_status.clone()
            },
            ..post
        })
    }
    fn post_terms(&self, id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        self.base.post_terms(id, taxonomy)
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.base.post_meta(id, key)
    }
    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        self.base.query(query)
    }
    fn comments(&self, post_id: u64) -> Vec<hyscribe_model::Comment> {
        self.base.comments(post_id)
    }
    fn comment(&self, id: u64) -> Option<hyscribe_model::Comment> {
        self.base.comment(id)
    }
    fn user(&self, id: u64) -> Option<hyscribe_model::User> {
        self.base.user(id)
    }
}

fn head_for(site: &Options, request: &Request) -> String {
    let theme = crate::theme::twentytwentyfive();
    String::from_utf8(wp_head(&theme, site, request, &Assets::default(), b"T")).unwrap()
}

/// `wp_robots()`, `rel_canonical()`, `wp_shortlink_wp_head()` and the
/// post comments feed link where PHP prints them as shown.
#[test]
fn head_links_match_php() {
    let site = |options: Vec<(&'static str, &'static [u8])>, closed| Options {
        base: crate::fixture::DefaultInstall {
            home: b"http://127.0.0.1:8081".to_vec(),
        },
        options,
        closed,
    };
    let search = Request {
        search: Some(b"x".to_vec()),
        get: hyscribe_phpser::parse_str(b"s=x&replytocom=1"),
        ..Request::default()
    };
    let private = site(vec![("blog_public", b"0")], false);
    assert!(
        head_for(&private, &search)
            .contains("<meta name='robots' content='noindex, nofollow' />\n")
    );
    let public = site(vec![("blog_public", b"1")], false);
    assert!(
        head_for(&public, &search).contains(
            "<meta name='robots' content='noindex, follow, max-image-preview:large' />\n"
        )
    );

    let closed = site(Vec::new(), true);
    let mut single = closed.base.request("p=1");
    single.posts = vec![closed.post(1).unwrap()];
    single.page = 2;
    let head = head_for(&closed, &single);
    assert!(
        head.contains(
            "<link rel=\"canonical\" href=\"http://127.0.0.1:8081/?p=1&#038;page=2\" />\n"
        ),
        "{head}"
    );
    let mut private = single.clone();
    private.posts[0].status = "private".into();
    assert!(!head_for(&closed, &private).contains("rel=\"canonical\""));
    // Closed comments still have a feed once there is a comment.
    assert!(
        head.contains(
            "Hello world! Comments Feed\" href=\"http://127.0.0.1:8081/?feed=rss2&#038;p=1\""
        ),
        "{head}"
    );

    // Closed, with no comments: no comments feed.
    let mut page = closed.base.request("page_id=2");
    page.posts = vec![closed.post(2).unwrap()];
    assert!(!head_for(&closed, &page).contains("Sample Page Comments Feed"));
    // A front page shortlink needs show_on_front set to page.
    let posts = site(
        vec![("show_on_front", b"posts"), ("page_on_front", b"2")],
        false,
    );
    let head = head_for(&posts, &posts.base.request("page_id=2"));
    assert!(
        head.contains("<link rel='shortlink' href='http://127.0.0.1:8081/?p=2' />"),
        "{head}"
    );
    let front = site(
        vec![("show_on_front", b"page"), ("page_on_front", b"2")],
        false,
    );
    let page = front.base.request("page_id=2");
    let head = head_for(&front, &page);
    assert!(head.contains("<link rel=\"canonical\" href=\"http://127.0.0.1:8081/\" />\n<link rel='shortlink' href='http://127.0.0.1:8081/' />\n"), "{head}");
}

/// The default install seen from the oracle's address.
fn oracle_site() -> Options {
    Options {
        base: crate::fixture::DefaultInstall {
            home: b"http://127.0.0.1:8081".to_vec(),
        },
        options: vec![("siteurl", b"http://127.0.0.1:8081")],
        closed: false,
    }
}

/// What rendering the template the hierarchy picks for a golden URL the
/// fixture can render enqueued, and its request.
fn rendered(theme: &Theme, site: &Options, path: &str) -> Option<(Request, Assets)> {
    let query = path.strip_prefix("/?").unwrap_or_default();
    let template = match query {
        "" if path == "/" => "home",
        "page_id=2" => "page",
        "p=1" | "p=1&cpage=2" | "p=1&cpage=abc" => "single",
        // The fixture's empty search lists nothing; the HTTP test has it.
        "s=hello" => "search",
        "cat=1" | "m=2026" | "m=202610" | "m=20261027" => "archive",
        "p=999" | "cat=999" => "404",
        _ => return None,
    };
    let request = site.base.request(query);
    let (_, markup) = theme.template(&[template.to_string()]).unwrap();
    let (_, assets) = crate::template::render_template(theme, site, &request, markup);
    Some((request, assets))
}

/// The page heads of `reference/head/golden.sh` the fixture can render,
/// rendered here with the template the hierarchy picks; HyScribe is the
/// generator and the RSD tag is left out.
#[test]
fn page_heads_match_php() {
    let theme = crate::theme::twentytwentyfive();
    let site = oracle_site();
    let golden = include_str!("../../tests/golden/head.tsv");
    let mut checked = 0;
    for line in golden.lines() {
        let (path, php) = line.split_once('\t').unwrap();
        let path = String::from_utf8(unhex(path)).unwrap();
        let Some((request, assets)) = rendered(&theme, &site, &path) else {
            continue;
        };
        let mut php = String::from_utf8(unhex(php)).unwrap();
        if let Some(start) = php.find("<link rel=\"EditURI\"") {
            let end = start + php.get(start..).unwrap().find("/>\n").unwrap() + 3;
            php.replace_range(start..end, "");
        }
        let php = php.replace(
            "WordPress 7.1.3",
            &format!("HyScribe {}", env!("CARGO_PKG_VERSION")),
        );
        let title_start = php.find("<title>").unwrap() + 7;
        let title = php
            .get(
                title_start
                    ..title_start + php.get(title_start..).unwrap().find("</title>").unwrap(),
            )
            .unwrap();
        let ours =
            String::from_utf8(wp_head(&theme, &site, &request, &assets, title.as_bytes())).unwrap();
        assert_eq!(ours, php, "{path}");
        checked += 1;
    }
    assert_eq!(checked, 12);
}

/// The footers of `reference/head/golden.sh` the fixture can render.
#[test]
fn page_footers_match_php() {
    let theme = crate::theme::twentytwentyfive();
    let site = oracle_site();
    let golden = include_str!("../../tests/golden/footer.tsv");
    let mut checked = 0;
    for line in golden.lines() {
        let (path, php) = line.split_once('\t').unwrap();
        let path = String::from_utf8(unhex(path)).unwrap();
        let Some((_, assets)) = rendered(&theme, &site, &path) else {
            continue;
        };
        let ours = String::from_utf8(wp_footer(&site, &assets)).unwrap();
        assert_eq!(ours, String::from_utf8(unhex(php)).unwrap(), "{path}");
        checked += 1;
    }
    assert_eq!(checked, 12);
}

/// `wp_maybe_inline_styles()` inlines up to the limit itself: two halves
/// fit, and one byte more does not.
#[test]
fn inline_budget_is_inclusive() {
    let half: &'static str = Box::leak("a".repeat(INLINE_LIMIT / 2).into_boxed_str());
    let mut queue = vec![
        Style::file("a", "/a.css", half, None),
        Style::file("b", "/b.css", half, None),
    ];
    inline_styles(&mut queue);
    assert!(queue.iter().all(|style| style.src.is_none()));
    let more: &'static str = Box::leak("a".repeat(INLINE_LIMIT / 2 + 1).into_boxed_str());
    let mut queue = vec![
        Style::file("a", "/a.css", half, None),
        Style::file("b", "/b.css", more, None),
    ];
    inline_styles(&mut queue);
    assert_eq!(queue.iter().filter(|style| style.src.is_some()).count(), 1);
}

/// PHP's robots tag through its title for `path`, from
/// `reference/loader/golden.sh`.
fn loader_head(path: &str) -> String {
    let golden = include_str!("../../../hyscribe-http/tests/golden/loader.tsv");
    let key: String = path.bytes().map(|b| format!("{b:02x}")).collect();
    let line = golden
        .lines()
        .find(|l| l.starts_with(&format!("{key}\t")))
        .unwrap();
    String::from_utf8(unhex(line.rsplit('\t').next().unwrap())).unwrap()
}

fn loader_site() -> Options {
    Options {
        base: crate::fixture::DefaultInstall {
            home: b"http://127.0.0.1:8081".to_vec(),
        },
        options: Vec::new(),
        closed: false,
    }
}

/// `wp_post_preview_js()` names the global post, as PHP prints it for
/// `?preview=true&pagename=sample-page`, and a preview of nothing prints
/// none.
#[test]
fn preview_script_matches_php() {
    let php = loader_head("/?preview=true&pagename=sample-page");
    let script = php
        .get(php.find("<script>").unwrap()..php.find("<title>").unwrap())
        .unwrap();
    let page = hyscribe_model::Post {
        id: 2,
        ..hyscribe_model::Post::default()
    };
    let preview = Request {
        preview: true,
        posts: vec![page],
        ..Request::default()
    };
    assert!(head_for(&loader_site(), &preview).contains(script));
    let nothing = Request {
        preview: true,
        ..Request::default()
    };
    assert!(!head_for(&loader_site(), &nothing).contains("<script>\n\t( function"));
}

/// An unapproved comment's preview link is not indexed; either var alone
/// changes nothing.
#[test]
fn unapproved_comment_robots_match_php() {
    for query in [
        "p=1&unapproved=1&moderation-hash=x",
        "p=1&unapproved=1",
        "p=1&moderation-hash=x",
    ] {
        let php = loader_head(&format!("/?{query}"));
        let robots = php.lines().next().unwrap();
        let request = Request {
            get: hyscribe_phpser::parse_str(query.as_bytes()),
            ..Request::default()
        };
        assert!(
            head_for(&loader_site(), &request).contains(robots),
            "{query}"
        );
    }
}
