// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{CalendarMonth, Post, PostQuery, Term};
use hyscribe_phpser::{json_decode, parse_str};
use hyscribe_testkit::unhex;

use crate::blocks::parse_blocks;
use crate::core_blocks::render;
use crate::fixture::DefaultInstall;
use crate::site::{Request, Site};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// The default install under a permalink structure and week start, with
/// the oracle's published post dates.
struct Dated {
    site: DefaultInstall,
    structure: Vec<u8>,
    start_of_week: Vec<u8>,
    dates: Vec<String>,
}

impl Site for Dated {
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

    fn has_published_posts(&self) -> bool {
        !self.dates.is_empty()
    }

    /// The month's reads over the dates, compared as strings as MySQL
    /// compares them with get_calendar()'s, which turns a date in a month
    /// past 12 into the zero date.
    fn calendar_month(&self, year: i64, month: u64, last_day: u64) -> CalendarMonth {
        let zero = "0000-00-00 00:00:00".to_string();
        let valid = (1..=12).contains(&month);
        let first = if valid {
            format!("{year}-{month:02}-01")
        } else {
            zero.clone()
        };
        let last = if valid {
            format!("{year}-{month:02}-{last_day} 23:59:59")
        } else {
            zero
        };
        let parts = |date: &String| {
            let number = |at: std::ops::Range<usize>| date.get(at).unwrap().parse::<u64>().unwrap();
            (number(0..4), number(5..7), number(8..10))
        };
        let previous = self
            .dates
            .iter()
            .rev()
            .find(|d| d.as_str() < first.as_str());
        let next = self.dates.iter().find(|d| d.as_str() > last.as_str());
        let mut days: Vec<u64> = self
            .dates
            .iter()
            .filter(|d| {
                d.as_str() >= format!("{first} 00:00:00").as_str() && d.as_str() <= last.as_str()
            })
            .map(|d| parts(d).2)
            .collect();
        days.dedup();
        CalendarMonth {
            previous: previous.map(|d| (parts(d).0, parts(d).1)),
            next: next.map(|d| (parts(d).0, parts(d).1)),
            days,
        }
    }
}

/// Every case of `reference/calendar/golden.sh`, through the render
/// callback, as the oracle leaves out the elements support.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../../tests/golden/calendar.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let query = unhex(fields[1]);
        let dates = json_decode(&unhex(fields[5])).unwrap();
        let site = Dated {
            site: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            structure: unhex(fields[2]),
            start_of_week: fields[3].as_bytes().to_vec(),
            dates: dates
                .as_array()
                .unwrap()
                .iter()
                .map(|(_, d)| String::from_utf8(d.to_php_string()).unwrap())
                .collect(),
        };
        let request = Request {
            vars: parse_str(&query),
            get: parse_str(&query),
            now: fields[4].parse().unwrap(),
            ..Request::default()
        };
        let context = Renderer::new(&theme, &site, &request).context;
        let markup = unhex(fields[0]);
        let block = &parse_blocks(&markup)[0];
        let attrs = block.attrs.as_deref().unwrap_or_default();
        let ours = render(&context, "core/calendar", attrs, b"").unwrap();
        let php = unhex(fields[6]);
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
        failures[..failures.len().min(2)].join("\n")
    );
    assert_eq!(golden.lines().count(), 420);
}

/// No post data, and the published posts option as given.
struct Cached(Option<&'static [u8]>);

impl Site for Cached {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.0
            .filter(|_| name == "wp_calendar_block_has_published_posts")
            .map(<[u8]>::to_vec)
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

/// The cached option decides whether there are posts, else the site does:
/// without them, no calendar; with it set, a month without posts.
#[test]
fn published_posts_option() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let show = |site: &dyn Site| {
        let context = Renderer::new(&theme, site, &request).context;
        String::from_utf8(render(&context, "core/calendar", &[], b"").unwrap()).unwrap()
    };
    assert_eq!(show(&Cached(None)), "");
    assert_eq!(show(&Cached(Some(b"0"))), "");
    let calendar = show(&Cached(Some(b"1")));
    assert!(
        calendar.contains("<caption>January 1970</caption>") && !calendar.contains("<a "),
        "{calendar}"
    );
}
