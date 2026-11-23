// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{Post, PostQuery, Term};
use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::{Request, Site};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// The default install with the case's post.
struct WithPost {
    base: DefaultInstall,
    post: Post,
}

impl Site for WithPost {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.base.option(name)
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<Post> {
        (id == self.post.id).then(|| self.post.clone())
    }
    fn post_terms(&self, id: u64, taxonomy: &str) -> Vec<Term> {
        self.base.post_terms(id, taxonomy)
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.base.post_meta(id, key)
    }
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.base.query(query)
    }
}

/// Every case of `reference/time-to-read/golden.sh`, on the home page.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let golden = include_str!("../../../tests/golden/time-to-read.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<Vec<u8>> = line.split('\t').map(unhex).collect();
        let base = DefaultInstall {
            home: b"http://127.0.0.1:8081".to_vec(),
        };
        let post = Post {
            id: 9000,
            title: b"Case".to_vec(),
            content: fields[1].clone(),
            ..base.post(1).unwrap()
        };
        let site = WithPost { base, post };
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.context.post_id = Some(9000);
        let ours = renderer.render(&fields[0]);
        if ours != fields[2] {
            failures.push(format!(
                "{} {:?}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&fields[0]),
                String::from_utf8_lossy(&fields[1]),
                String::from_utf8_lossy(&fields[2]),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(6)].join("\n")
    );
    assert_eq!(golden.lines().count(), 182);
}

/// Without a post there is nothing to count.
#[test]
fn needs_a_post() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let mut renderer = Renderer::new(&theme, &site, &request);
    assert!(
        renderer
            .render(b"<!-- wp:post-time-to-read /-->")
            .is_empty()
    );
    renderer.context.post_id = Some(77);
    assert_eq!(
        renderer.render(b"<!-- wp:post-time-to-read {\"displayMode\":\"words\"} /-->"),
        b"<div class=\"wp-block-post-time-to-read\">0 words</div>"
    );
}

/// The tag and entity patterns as `preg_replace()` applies them.
#[test]
fn tags_and_entities_go_whole() {
    use super::{strip_entities, strip_tags};
    assert_eq!(strip_tags(b"<b>x</B>y<1>"), b"\nx\ny<1>");
    assert_eq!(strip_entities(b"a&amp;b&;c&x y;"), b"ab&;c&x y;");
}
