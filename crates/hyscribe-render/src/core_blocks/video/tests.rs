// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;
use sha2::{Digest, Sha256};

use crate::fixture::ThemeTestData;
use crate::site::Request;
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// Every case of `reference/video/golden.sh`, by hash, as for every golden
/// over the fixture.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let site = ThemeTestData {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../../tests/golden/video.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (markup, php) = line.split_once('\t').unwrap();
        let markup = unhex(markup);
        let ours = Renderer::new(&theme, &site, &request).render(&markup);
        let hash: String = Sha256::digest(&ours)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if hash != php {
            failures.push(format!("{}: ours {hash}", String::from_utf8_lossy(&markup)));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(golden.lines().count(), 40);
}

/// Sizes of zero or below are left alone.
#[test]
fn needs_positive_sizes() {
    use crate::fixture::{attachment, meta, site};
    let theme = twentytwentyfive();
    let request = Request::default();
    let mut site = site(vec![]);
    site.posts = vec![
        attachment(7, "video/mp4", ""),
        attachment(8, "video/mp4", ""),
    ];
    site.meta = vec![
        (
            7,
            "_wp_attachment_metadata",
            meta(r#"{"width":0,"height":5}"#),
        ),
        (
            8,
            "_wp_attachment_metadata",
            meta(r#"{"width":-5,"height":5}"#),
        ),
    ];
    for id in [7, 8] {
        let markup = format!(
            r#"<!-- wp:video {{"id":{id}}} --><figure class="wp-block-video"><video src="v.mp4"></video></figure><!-- /wp:video -->"#
        );
        let html = Renderer::new(&theme, &site, &request).render(markup.as_bytes());
        assert_eq!(
            html, br#"<figure class="wp-block-video"><video src="v.mp4"></video></figure>"#,
            "{id}"
        );
    }
}
