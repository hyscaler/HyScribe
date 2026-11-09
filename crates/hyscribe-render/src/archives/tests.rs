// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{DateCount, OrderBy, Period, Post, PostQuery, Term};
use hyscribe_phpser::parse_str;
use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::{Archive, Request, Site};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// The default install under another permalink structure and week start.
struct Structured {
    site: DefaultInstall,
    structure: Vec<u8>,
    start_of_week: Vec<u8>,
}

impl Site for Structured {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "permalink_structure" => Some(self.structure.clone()),
            "start_of_week" => Some(self.start_of_week.clone()),
            _ => self.site.option(name),
        }
    }

    fn home_url(&self) -> Vec<u8> {
        self.site.home_url()
    }

    fn post(&self, id: u64) -> Option<Post> {
        self.site.post(id)
    }

    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        self.site.post_terms(post_id, taxonomy)
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.site.post_meta(post_id, key)
    }

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.site.query(query)
    }

    fn date_counts(&self, period: Period) -> Vec<DateCount> {
        self.site.date_counts(period)
    }
}

/// Every case of `reference/archives/golden.sh`.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/archives.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (markup, counter, query, structure, start, post, php) = (
            unhex(fields[0]),
            fields[1],
            unhex(fields[2]),
            unhex(fields[3]),
            fields[4],
            fields[5].parse().unwrap(),
            unhex(fields[6]),
        );
        let site = Structured {
            site: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            structure,
            start_of_week: start.as_bytes().to_vec(),
        };
        // Every request here but the post's and the home page's is an
        // archive, or a 404 when it finds no post.
        let archive = !matches!(query.as_slice(), b"" | b"p=1") && post != 0;
        let request = Request {
            archive: archive.then_some(Archive::Date),
            post: Some(post).filter(|&id| id != 0),
            vars: parse_str(&query),
            ..Request::default()
        };
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.unique_id = counter.parse().unwrap();
        let ours = renderer.render(&markup);
        if ours != php {
            failures.push(format!(
                "{} {}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&query),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(4)].join("\n")
    );
    assert_eq!(golden.lines().count(), 1848);
}

/// A site with no posts, as `Site` answers by default, which it keeps
/// for `date_counts`.
struct Empty;

impl Site for Empty {
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

/// `render_block_core_archives()` with nothing for `wp_get_archives()` to list.
#[test]
fn no_posts_have_no_archives() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let mut renderer = Renderer::new(&theme, &Empty, &request);
    assert_eq!(
        renderer.render(b"<!-- wp:archives /-->"),
        b"<div class=\"wp-block-archives-list wp-block-archives\">No archives to show.</div>"
    );
}

/// Rows the default install has none like, and the orders posts were
/// asked for in.
struct Listed {
    counts: Vec<DateCount>,
    posts: Vec<Post>,
    orders: std::cell::RefCell<Vec<Vec<(OrderBy, bool)>>>,
}

impl Site for Listed {
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

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.orders.borrow_mut().push(query.order.clone());
        self.posts.clone()
    }

    fn date_counts(&self, _: Period) -> Vec<DateCount> {
        self.counts.clone()
    }
}

/// `wp_get_archives()` skips a week whose number the row before had, of
/// another year, and names a post without a title (or titled `0`) by ID.
#[test]
fn repeated_weeks_and_untitled_posts() {
    let week = |year, day| DateCount {
        year,
        month: 1,
        day,
        week: 1,
        posts: 1,
    };
    let post = |id, title: &[u8]| Post {
        id,
        title: title.to_vec(),
        post_type: "post".into(),
        status: "publish".into(),
        date: "2026-01-01 00:00:00".into(),
        ..Post::default()
    };
    let site = Listed {
        counts: vec![week(2026, 5), week(2025, 6)],
        posts: vec![post(7, b""), post(8, b"0")],
        orders: std::cell::RefCell::default(),
    };
    let theme = twentytwentyfive();
    let request = Request::default();
    let mut renderer = Renderer::new(&theme, &site, &request);
    let weeks = renderer.render(br#"<!-- wp:archives {"type":"weekly"} /-->"#);
    assert_eq!(String::from_utf8_lossy(&weeks).matches("<li>").count(), 1);
    let posts =
        String::from_utf8(renderer.render(br#"<!-- wp:archives {"type":"postbypost"} /-->"#))
            .unwrap();
    assert!(
        posts.contains(">7</a>") && posts.contains(">8</a>"),
        "{posts}"
    );
    renderer.render(br#"<!-- wp:archives {"type":"alpha"} /-->"#);
    assert_eq!(
        *site.orders.borrow(),
        [
            vec![(OrderBy::Date, false), (OrderBy::Id, false)],
            vec![(OrderBy::Title, true)]
        ]
    );
}
