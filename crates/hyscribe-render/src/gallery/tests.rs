// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::layout::stylesheet;
use crate::site::Request;
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// Every case of `reference/gallery/golden.sh`: the block and the block
/// supports stylesheet it leaves.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../tests/golden/gallery.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let (counter, rest) = line.split_once('\t').unwrap();
        let fields: Vec<Vec<u8>> = rest.split('\t').map(unhex).collect();
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.unique_id = counter.parse().unwrap();
        let html = renderer.render(&fields[0]);
        let css = stylesheet(&renderer.assets.rules);
        if html != fields[1] || css.as_bytes() != fields[2] {
            failures.push(format!(
                "{}\n  php:  {}\n  ours: {}\n  php css:  {}\n  our css:  {css}",
                String::from_utf8_lossy(&fields[0]),
                String::from_utf8_lossy(&fields[1]),
                String::from_utf8_lossy(&html),
                String::from_utf8_lossy(&fields[2]),
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(4)].join("\n")
    );
    assert_eq!(golden.lines().count(), 76);
}

/// Only images get a `data-id`, and only from attributes that parsed.
#[test]
fn data_ids_for_images_only() {
    let mut blocks = crate::blocks::parse_blocks(br#"<!-- wp:paragraph {"id":5} /-->"#);
    // An image whose attributes were not valid JSON.
    blocks.push(crate::blocks::Block {
        name: Some("core/image".into()),
        attrs: None,
        inner_blocks: Vec::new(),
        inner_html: Vec::new(),
        inner_content: Vec::new(),
    });
    let before = blocks.clone();
    blocks.iter_mut().for_each(super::data_id);
    assert_eq!(blocks, before);
}
