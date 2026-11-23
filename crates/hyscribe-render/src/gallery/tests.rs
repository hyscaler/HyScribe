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

/// Viewport gaps, the rules PHP stores for the gallery itself; the golden
/// leaves these cases out, as the layout support's own viewport rules are
/// not ported.
#[test]
fn viewport_gaps_like_php() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let image = r#"<!-- wp:image {"id":5} --><figure class="wp-block-image"><img src="a.jpg" alt="" class="wp-image-5"/></figure><!-- /wp:image -->"#;
    let mobile = "@media (width <= 480px){.wp-block-gallery.wp-block-gallery-1{--wp--style--unstable-gallery-gap:";
    let tablet = "@media (480px < width <= 782px){.wp-block-gallery.wp-block-gallery-1{--wp--style--unstable-gallery-gap:";
    for (style, expected) in [
        (
            r#"{"@mobile":{"spacing":{"blockGap":"2px"}}}"#,
            vec![format!("{mobile}2px;}}}}")],
        ),
        (
            r#"{"@tablet":{"spacing":{"blockGap":{"left":"3px"}}},"spacing":{"blockGap":"1em"}}"#,
            vec![format!("{tablet}3px;}}}}")],
        ),
        (
            r#"{"@mobile":{"spacing":{"blockGap":null}},"@tablet":{"spacing":{"blockGap":"0"}}}"#,
            vec![format!("{tablet}0px;}}}}")],
        ),
        (r#"{"@mobile":{"spacing":"x"},"@tablet":"y"}"#, vec![]),
        (r#"{"@mobile":{"spacing":{"padding":"1px"}}}"#, vec![]),
    ] {
        let markup = format!(
            r#"<!-- wp:gallery {{"style":{style}}} --><figure class="wp-block-gallery has-nested-images">{image}</figure><!-- /wp:gallery -->"#
        );
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.render(markup.as_bytes());
        let css = stylesheet(&renderer.assets.rules);
        let found: Vec<String> = css
            .split("@media")
            .skip(1)
            .map(|rule| format!("@media{}", rule.split_inclusive("}}").next().unwrap()))
            .filter(|rule| rule.contains(".wp-block-gallery.wp-block-gallery-1"))
            .collect();
        assert_eq!(found, expected, "{style}");
    }
}

/// A viewport gap from the site's global styles applies where the block
/// sets no gap of its own.
#[test]
fn global_viewport_gaps() {
    let user = br#"{"version":3,"isGlobalStylesUserThemeJSON":true,"styles":{"blocks":{"core/gallery":{"@mobile":{"spacing":{"blockGap":"4px"}}}}}}"#;
    let theme = crate::theme::twentytwentyfive_with(user, &[]);
    let request = Request::default();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let mut renderer = Renderer::new(&theme, &site, &request);
    renderer.render(
        br#"<!-- wp:gallery --><figure class="wp-block-gallery"></figure><!-- /wp:gallery -->"#,
    );
    let css = stylesheet(&renderer.assets.rules);
    assert!(css.contains("@media (width <= 480px){.wp-block-gallery.wp-block-gallery-1{--wp--style--unstable-gallery-gap:4px;}}"), "{css}");
}
