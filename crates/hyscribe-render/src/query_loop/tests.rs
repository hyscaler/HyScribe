// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::Request;
use crate::template::render_content;
use crate::theme::twentytwentyfive;

/// The `query` cases of `reference/core-blocks/golden.sh`, on the fixture's
/// posts.
#[test]
fn query_loops_match_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let mut count = 0;
    for line in golden.lines().filter(|line| line.starts_with("query\t")) {
        let mut fields = line.split('\t').skip(1).map(unhex);
        let mut field = || fields.next().unwrap();
        let (markup, request, home, php) = (field(), field(), field(), field());
        let site = DefaultInstall { home };
        let request = String::from_utf8(request).unwrap();
        let request = site.request(&request);
        let ours = render_content(&theme, &site, &request, &markup);
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&php),
            "{}",
            String::from_utf8_lossy(&markup)
        );
        count += 1;
    }
    assert!(count >= 50);
}

/// The default install with one sticky post, which its own data lacks.
struct Sticky(DefaultInstall);

impl crate::site::Site for Sticky {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "sticky_posts" => Some(b"a:1:{i:0;i:1;}".to_vec()),
            _ => self.0.option(name),
        }
    }

    fn home_url(&self) -> Vec<u8> {
        self.0.home_url()
    }

    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        self.0.post(id)
    }

    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        self.0.post_terms(post_id, taxonomy)
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.0.post_meta(post_id, key)
    }

    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        self.0.query(query)
    }
}

#[test]
fn sticky_posts_are_selected_or_left_out() {
    let theme = twentytwentyfive();
    let site = Sticky(DefaultInstall {
        home: b"http://example.test".to_vec(),
    });
    let request = Request::default();
    let html = |sticky: &str| {
        let markup = format!(
            r#"<!-- wp:query {{"query":{{"perPage":3,"sticky":"{sticky}","inherit":false}}}} --><div><!-- wp:post-template --><!-- wp:post-title /--><!-- /wp:post-template --></div><!-- /wp:query -->"#
        );
        String::from_utf8(render_content(&theme, &site, &request, markup.as_bytes())).unwrap()
    };
    assert!(html("only").contains("Hello world!"));
    assert_eq!(html("exclude"), "<div></div>");
}
