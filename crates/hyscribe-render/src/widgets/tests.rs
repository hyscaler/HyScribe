// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use crate::site::{Request, Site};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// The default install with the options `reference/legacy-widget/golden.sh`
/// set and rolled back, signing as its `salt` filter does.
struct Widgets {
    install: crate::fixture::DefaultInstall,
    options: Vec<(String, Vec<u8>)>,
}

impl Site for Widgets {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        let set = self.options.iter().find(|(n, _)| n == name);
        set.map(|(_, value)| value.clone())
            .or_else(|| self.install.option(name))
    }
    fn home_url(&self) -> Vec<u8> {
        self.install.home_url()
    }
    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        self.install.post(id)
    }
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        self.install.post_terms(post_id, taxonomy)
    }
    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.install.post_meta(post_id, key)
    }
    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        self.install.query(query)
    }
    fn date_counts(&self, period: hyscribe_model::Period) -> Vec<hyscribe_model::DateCount> {
        self.install.date_counts(period)
    }
    fn wp_hash(&self, data: &[u8]) -> Option<String> {
        Some(hyscribe_phpser::hash_hmac_md5(data, b"test-salt"))
    }
}

/// Every case of `reference/legacy-widget/golden.sh`.
#[test]
fn legacy_widgets_match_php() {
    let golden = include_str!("../../tests/golden/legacy-widget.tsv");
    let lines: Vec<Vec<&str>> = golden.lines().map(|l| l.split('\t').collect()).collect();
    // Options apply from where the oracle set them; its `WP_SITEURL`
    // overrides the stored one.
    let mut options = vec![("siteurl".to_string(), b"http://127.0.0.1:8081".to_vec())];
    let theme = twentytwentyfive();
    let mut failures = Vec::new();
    for f in &lines {
        if f[0] == "option" {
            options.insert(0, (f[1].to_string(), unhex(f[2])));
            continue;
        }
        let site = Widgets {
            install: crate::fixture::DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            options: options.clone(),
        };
        let request = Request {
            search_query: unhex(f[1]),
            ..Default::default()
        };
        let (markup, php) = (unhex(f[2]), unhex(f[3]));
        let ours = Renderer::new(&theme, &site, &request).render(&markup);
        if ours != php {
            failures.push(format!(
                "{} / {}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                f[1],
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(lines.len(), 108);
}

/// Without the salts `wp_hash()` signs with, no instance checks out.
#[test]
fn no_salts_no_signed_instances() {
    let site = crate::fixture::DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let request = Request::default();
    let markup = br#"<!-- wp:legacy-widget {"idBase":"search","instance":{"encoded":"YTowOnt9","hash":"x"}} /-->"#;
    assert_eq!(site.wp_hash(b"a:0:{}"), None);
    let html = Renderer::new(&twentytwentyfive(), &site, &request).render(markup);
    assert!(html.is_empty());
}

/// The default install with `reference/recent-posts-widget/golden.sh`'s
/// published posts and pages, newest first.
struct Recent {
    install: crate::fixture::DefaultInstall,
    posts: Vec<hyscribe_model::Post>,
}

impl Site for Recent {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.install.option(name)
    }
    fn home_url(&self) -> Vec<u8> {
        self.install.home_url()
    }
    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        self.posts.iter().find(|post| post.id == id).cloned()
    }
    fn post_terms(&self, _: u64, _: &str) -> Vec<hyscribe_model::Term> {
        Vec::new()
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    /// Those of the types asked for, newest first, as many as asked for.
    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        let limit = usize::try_from(query.limit).unwrap();
        let typed = |post: &&hyscribe_model::Post| {
            query.post_types.is_empty() || query.post_types.contains(&post.post_type)
        };
        self.posts
            .iter()
            .filter(typed)
            .take(limit)
            .cloned()
            .collect()
    }
    fn wp_hash(&self, data: &[u8]) -> Option<String> {
        Some(hyscribe_phpser::hash_hmac_md5(data, b"test-salt"))
    }
}

/// Every case of `reference/recent-posts-widget/golden.sh`.
#[test]
fn recent_posts_match_php() {
    let golden = include_str!("../../tests/golden/recent-posts-widget.tsv");
    let theme = twentytwentyfive();
    let mut failures = Vec::new();
    for line in golden.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        let state = hyscribe_phpser::json_decode(&unhex(f[2])).unwrap();
        let field = |row: &hyscribe_phpser::Value, i| {
            row.get(&hyscribe_phpser::Key::Int(i))
                .unwrap()
                .to_php_string()
        };
        let posts = state.get_str("posts").unwrap().as_array().unwrap();
        let site = Recent {
            install: crate::fixture::DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            posts: posts
                .iter()
                .map(|(_, row)| hyscribe_model::Post {
                    id: String::from_utf8(field(row, 0)).unwrap().parse().unwrap(),
                    title: field(row, 1),
                    date: String::from_utf8(field(row, 2)).unwrap(),
                    password: field(row, 3),
                    status: "publish".into(),
                    post_type: String::from_utf8(field(row, 4)).unwrap(),
                    ..hyscribe_model::Post::default()
                })
                .collect(),
        };
        let request = Request {
            queried_object_id: state.get_str("queried").unwrap().to_int() as u64,
            ..Default::default()
        };
        let (markup, php) = (unhex(f[0]), unhex(f[3]));
        let ours = Renderer::new(&theme, &site, &request).render(&markup);
        if ours != php {
            failures.push(format!(
                "{} {}\n  php:  {:?}\n  ours: {:?}",
                String::from_utf8_lossy(&markup),
                f[1],
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
    assert_eq!(golden.lines().count(), 34);
}
