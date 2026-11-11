// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::Request;
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// Every case of `reference/file/golden.sh`.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../tests/golden/file.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (markup, php) = line.split_once('\t').unwrap();
        let (markup, php) = (unhex(markup), unhex(php));
        let ours = Renderer::new(&theme, &site, &request).render(&markup);
        if ours != php {
            failures.push(format!(
                "{}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
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
    assert_eq!(golden.lines().count(), 29);
}

/// A preview enqueues the file block's view module.
#[test]
fn previews_enqueue_the_view() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let mut renderer = Renderer::new(&theme, &site, &request);
    renderer.render(br#"<!-- wp:file {"displayPreview":true} --><div class="wp-block-file"><object></object></div><!-- /wp:file -->"#);
    assert_eq!(
        renderer.assets.modules,
        ["@wordpress/block-library/file/view"]
    );
}
