// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;
use sha2::{Digest, Sha256};

use crate::fixture::ThemeTestData;
use crate::site::Request;
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// Every case of `reference/featured-image/golden.sh`, by hash: the
/// fixture's content is not ours to commit.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let site = ThemeTestData {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../../tests/golden/featured-image.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let markup = unhex(fields[1]);
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.context.post_id = Some(fields[0].parse().unwrap());
        let ours = renderer.render(&markup);
        let hash: String = Sha256::digest(&ours)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if hash != fields[2] {
            failures.push(format!(
                "{} {}\n  ours: {}",
                fields[0],
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(8)].join("\n")
    );
    assert_eq!(golden.lines().count(), 130);
}

/// The default install with one post of the given content and no thumbnail.
struct WithContent {
    base: crate::fixture::DefaultInstall,
    content: Vec<u8>,
}

impl crate::site::Site for WithContent {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.base.option(name)
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        self.base.post(id).map(|post| hyscribe_model::Post {
            content: self.content.clone(),
            ..post
        })
    }
    fn post_terms(&self, id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        self.base.post_terms(id, taxonomy)
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        self.base.query(query)
    }
}

fn render(content: &str, markup: &str, post_id: Option<u64>) -> String {
    let theme = twentytwentyfive();
    let request = Request::default();
    let site = WithContent {
        base: crate::fixture::DefaultInstall {
            home: b"http://127.0.0.1:8081".to_vec(),
        },
        content: content.as_bytes().to_vec(),
    };
    let mut renderer = Renderer::new(&theme, &site, &request);
    renderer.context.post_id = post_id;
    String::from_utf8(renderer.render(markup.as_bytes())).unwrap()
}

#[test]
fn needs_a_post() {
    let markup = r#"<!-- wp:post-featured-image {"useFirstImageFromPost":true} /-->"#;
    assert_eq!(render(r#"<img src="a.png">"#, markup, None), "");
}

#[test]
fn first_images_keep_their_attributes() {
    let markup =
        r#"<!-- wp:post-featured-image {"useFirstImageFromPost":true,"aspectRatio":"1"} /-->"#;
    let html = render(
        r#"<p>x</p><img src="a.png" data-flag style="color:red;;">"#,
        markup,
        Some(1),
    );
    assert!(html.contains(" data-flag"), "{html}");
    let empty = render(r#"<img src="a.png" style="">"#, markup, Some(1));
    assert!(
        empty.contains(r#"style="aspect-ratio:1;width:100%;object-fit:cover;""#),
        "{empty}"
    );
    let plain = r#"<!-- wp:post-featured-image {"useFirstImageFromPost":true,"scale":""} /-->"#;
    assert_eq!(
        render(r#"<img src="a.png" style="x:y">"#, plain, Some(1)),
        r#"<figure class="wp-block-post-featured-image"><img src="http://a.png" style="x:y"></figure>"#
    );
    assert!(
        html.contains(r#"style="color:red;aspect-ratio:1;width:100%;object-fit:cover;""#),
        "{html}"
    );
}

#[test]
fn overlays_without_borders() {
    let html = |attrs: &str| {
        let markup =
            format!(r#"<!-- wp:post-featured-image {{"useFirstImageFromPost":true,{attrs}}} /-->"#);
        let html = render(r#"<img src="a.png">"#, &markup, Some(1));
        html.split("<span")
            .nth(1)
            .map(|s| format!("<span{s}"))
            .unwrap_or_default()
    };
    assert_eq!(
        html(r##""dimRatio":30,"customOverlayColor":"#123""##),
        r#"<span class="wp-block-post-featured-image__overlay has-background-dim has-background-dim-30" style="background-color: #123" aria-hidden="true"></span></figure>"#
    );
    assert_eq!(
        html(r#""dimRatio":10,"gradient":"g""#),
        r#"<span class="wp-block-post-featured-image__overlay has-background-dim has-background-dim-10 has-background-gradient has-g-gradient-background" style="" aria-hidden="true"></span></figure>"#
    );
}
