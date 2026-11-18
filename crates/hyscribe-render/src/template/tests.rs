// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;

use super::*;
use crate::theme::twentytwentyfive;

/// A theme with the given parts and patterns, for structure tests.
fn theme(parts: &[(&'static str, &'static [u8])], patterns: &[(&str, &str)]) -> Theme {
    Theme {
        slug: "test",
        templates: HashMap::new(),
        parts: parts.iter().copied().collect(),
        areas: [(b"top".to_vec(), b"header".to_vec())]
            .into_iter()
            .collect(),
        patterns: patterns
            .iter()
            .map(|(slug, content)| (slug.as_bytes().to_vec(), content.as_bytes().to_vec()))
            .collect(),
        global: twentytwentyfive().global,
        style_handles: HashMap::new(),
        inline_block_styles: Vec::new(),
        stylesheet: "",
        version: String::new(),
    }
}

/// The default install's title and tagline, nothing else.
struct DefaultInstall;

impl Site for DefaultInstall {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "blogname" => Some(b"HyScribe Default Install".to_vec()),
            _ => None,
        }
    }

    fn home_url(&self) -> Vec<u8> {
        b"http://example.test".to_vec()
    }

    fn post(&self, _: u64) -> Option<hyscribe_model::Post> {
        None
    }

    fn post_terms(&self, _: u64, _: &str) -> Vec<hyscribe_model::Term> {
        Vec::new()
    }

    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn query(&self, _: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        Vec::new()
    }
}

/// A feed's posts are in the loop, as a singular page's are: the first
/// large image has the high priority and a later one none; a page of other
/// posts is outside the loop and its images are lazy.
#[test]
fn images_of_a_feed_are_in_the_loop() {
    let theme = twentytwentyfive();
    let image = br#"<img width="600" height="400" src="http://example.test/a.jpg">"#;
    let filtered = |request: Request| {
        let renderer = Renderer::new(&theme, &DefaultInstall, &request);
        let loading = renderer.context.loading.clone();
        let first = crate::content_tags::wp_filter_content_tags(&DefaultInstall, &loading, image);
        let second = crate::content_tags::wp_filter_content_tags(&DefaultInstall, &loading, image);
        (
            String::from_utf8(first).unwrap(),
            String::from_utf8(second).unwrap(),
        )
    };
    let feed = filtered(Request {
        feed: true,
        ..Request::default()
    });
    assert!(feed.0.contains(r#"fetchpriority="high""#), "{}", feed.0);
    assert!(!feed.1.contains("fetchpriority"), "{}", feed.1);
    let archive = filtered(Request::default());
    assert!(archive.0.contains(r#"loading="lazy""#), "{}", archive.0);
}

fn render(theme: &Theme, markup: &str) -> String {
    String::from_utf8(render_content(
        theme,
        &DefaultInstall,
        &Request::default(),
        markup.as_bytes(),
    ))
    .unwrap()
}

#[test]
fn the_home_template_expands_parts_and_patterns() {
    let theme = twentytwentyfive();
    let (_, template) = theme.template(&["home".to_string()]).unwrap();
    let html = String::from_utf8(
        render_template(&theme, &DefaultInstall, &Request::default(), template).0,
    )
    .unwrap();
    assert!(html.starts_with(
        r##"<a class="skip-link screen-reader-text" id="wp-skip-link" href="#wp--skip-link--target">Skip to content</a><div class="wp-site-blocks"><header class="wp-block-template-part">"##
    ));
    assert!(html.contains(
        r#"<main id="wp--skip-link--target" class="wp-block-group has-global-padding is-layout-constrained wp-block-group-is-layout-constrained""#
    ));
    assert!(html.contains("<footer class=\"wp-block-template-part\">"));
    assert!(html.ends_with("</footer>\n</div>"));
    assert!(!html.contains("wp:pattern"));
    assert!(html.contains(
        r#"<p class="wp-block-site-title"><a href="http://example.test" target="_self" rel="home">HyScribe Default Install</a></p>"#
    ));
}

#[test]
fn template_parts() {
    let theme = theme(
        &[
            ("top", b"<p>top</p>"),
            ("plain", b"plain"),
            ("loop", b"<!-- wp:template-part {\"slug\":\"loop\"} /-->x"),
        ],
        &[],
    );
    assert_eq!(
        render(&theme, "<!-- wp:template-part {\"slug\":\"top\"} /-->"),
        "<header class=\"wp-block-template-part\"><p>top</p></header>"
    );
    assert_eq!(
        render(&theme, "<!-- wp:template-part {\"slug\":\"plain\"} /-->"),
        "<div class=\"wp-block-template-part\">plain</div>"
    );
    assert_eq!(
        render(
            &theme,
            "<!-- wp:template-part {\"slug\":\"plain\",\"tagName\":\"aside\"} /-->"
        ),
        "<aside class=\"wp-block-template-part\">plain</aside>"
    );
    // A tag tag_escape() would change falls back to the area's.
    assert_eq!(
        render(
            &theme,
            "<!-- wp:template-part {\"slug\":\"top\",\"tagName\":\"Div\"} /-->"
        ),
        "<header class=\"wp-block-template-part\"><p>top</p></header>"
    );
    assert_eq!(
        render(
            &theme,
            "<!-- wp:template-part {\"slug\":\"top\",\"tagName\":\"\"} /-->"
        ),
        "<header class=\"wp-block-template-part\"><p>top</p></header>"
    );
    // A part that includes itself stops at the second level.
    assert_eq!(
        render(&theme, "<!-- wp:template-part {\"slug\":\"loop\"} /-->"),
        "<div class=\"wp-block-template-part\">x</div>"
    );
    for missing in [
        "<!-- wp:template-part {\"slug\":\"none\"} /-->",
        "<!-- wp:template-part /-->",
        "<!-- wp:template-part {\"slug\":\"top\",\"theme\":\"other\"} /-->",
    ] {
        assert_eq!(render(&theme, missing), "", "{missing}");
    }
    assert!(
        render(
            &theme,
            "<!-- wp:template-part {\"slug\":\"top\",\"theme\":\"test\"} /-->"
        )
        .contains("top")
    );
}

#[test]
fn patterns() {
    let theme = theme(
        &[],
        &[
            ("a", "<!-- wp:pattern {\"slug\":\"b\"} /-->!"),
            ("b", "<p>b</p>"),
            ("self", "<!-- wp:pattern {\"slug\":\"self\"} /-->s"),
        ],
    );
    assert_eq!(
        render(&theme, "<!-- wp:pattern {\"slug\":\"a\"} /-->"),
        "<p>b</p>!"
    );
    assert_eq!(
        render(&theme, "<!-- wp:pattern {\"slug\":\"self\"} /-->"),
        "s"
    );
    assert_eq!(
        render(&theme, "<!-- wp:pattern {\"slug\":\"none\"} /-->"),
        ""
    );
    assert_eq!(render(&theme, "<!-- wp:pattern {\"slug\":1} /-->"), "");
}

#[test]
fn saved_markup_keeps_inner_blocks_in_place() {
    let theme = theme(&[], &[("p", "P")]);
    assert_eq!(
        render(
            &theme,
            "a<!-- wp:group --><div>1<!-- wp:pattern {\"slug\":\"p\"} /-->2</div><!-- /wp:group -->b"
        ),
        "a<div class=\"is-layout-flow wp-block-group-is-layout-flow\">1P2</div>b"
    );
    // More placeholders than inner blocks cannot come from the parser, but
    // must not panic.
    let mut odd = Block {
        name: Some("core/group".into()),
        attrs: Some(Vec::new()),
        inner_blocks: Vec::new(),
        inner_html: Vec::new(),
        inner_content: vec![None, Some(b"x".to_vec())],
    };
    let request = Request::default();
    let mut renderer = Renderer::new(&theme, &DefaultInstall, &request);
    assert_eq!(renderer.block(&odd), b"x");
    odd.attrs = None;
    assert_eq!(attr(&odd, "slug"), None);
}

#[test]
fn tag_names() {
    assert!(is_tag_name(b"header"));
    assert!(is_tag_name(b"my-tag_1:x"));
    for bad in [&b""[..], b"Header", b"a b", b"a>"] {
        assert!(!is_tag_name(bad), "{bad:?}");
    }
    assert_eq!(area_tag(b"footer"), b"footer");
    assert_eq!(area_tag(b"menu"), b"div");
}

#[test]
fn skip_links_follow_the_main_element() {
    assert_eq!(
        add_skip_link(b"<div>no main</div>".to_vec()),
        b"<div>no main</div>"
    );
    let linked = |html: &str| String::from_utf8(add_skip_link(html.as_bytes().to_vec())).unwrap();
    assert_eq!(
        linked(r#"<div><main id="top">x</main></div>"#),
        r##"<a class="skip-link screen-reader-text" id="wp-skip-link" href="#top">Skip to content</a><div><main id="top">x</main></div>"##
    );
    assert!(linked(r#"<main id="">x</main>"#).contains(r#"<main id="wp--skip-link--target">"#));
    assert!(linked(r#"<main id>x</main>"#).contains(r#"<main id="wp--skip-link--target">"#));
}

#[test]
fn template_filters_run_on_the_rendered_template() {
    struct Smiling;
    impl Site for Smiling {
        fn option(&self, name: &str) -> Option<Vec<u8>> {
            match name {
                "use_smilies" => Some(b"1".to_vec()),
                "siteurl" => Some(b"http://example.test".to_vec()),
                _ => None,
            }
        }
        fn home_url(&self) -> Vec<u8> {
            Vec::new()
        }
        fn post(&self, _: u64) -> Option<hyscribe_model::Post> {
            None
        }
        fn post_terms(&self, _: u64, _: &str) -> Vec<hyscribe_model::Term> {
            Vec::new()
        }
        fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
            None
        }
        fn query(&self, _: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
            Vec::new()
        }
    }
    let theme = twentytwentyfive();
    let html = |site: &dyn Site| {
        String::from_utf8(
            render_template(
                &theme,
                site,
                &Request::default(),
                b"<p>\"Hi\" :mrgreen: ]]></p>",
            )
            .0,
        )
        .unwrap()
    };
    assert_eq!(
        html(&Smiling),
        r#"<div class="wp-site-blocks"><p>&#8220;Hi&#8221; <img src="http://example.test/wp-includes/images/smilies/mrgreen.png" alt=":mrgreen:" class="wp-smiley" style="height: 1em; max-height: 1em;" /> ]]&gt;</p></div>"#
    );
    assert_eq!(
        html(&DefaultInstall),
        r#"<div class="wp-site-blocks"><p>&#8220;Hi&#8221; :mrgreen: ]]&gt;</p></div>"#
    );
}

#[test]
fn unregistered_blocks_render_their_markup() {
    let theme = twentytwentyfive();
    assert_eq!(
        render(
            &theme,
            "<!-- wp:my/block --><div>x</div><!-- /wp:my/block -->"
        ),
        "<div>x</div>"
    );
}

#[test]
fn layout_classes_go_on_the_inner_wrapper() {
    let theme = twentytwentyfive();
    let html = render(
        &theme,
        r#"<!-- wp:group {"layout":{"type":"flex"}} --><div class="wp-block-group"><div class="wp-block-group__inner-container"><!-- wp:paragraph --><p>x</p><!-- /wp:paragraph --></div></div><!-- /wp:group -->"#,
    );
    assert!(html.starts_with(r#"<div class="wp-block-group"><div class="wp-block-group__inner-container is-layout-flex "#), "{html}");
}

/// HTTP requests render on 2 MiB worker stacks.
#[test]
fn the_deepest_blocks_fit_a_worker_stack() {
    let depth = crate::blocks::MAX_BLOCK_DEPTH;
    let markup = [
        r#"<!-- wp:group {"layout":{"type":"constrained"}} --><div class="wp-block-group">"#
            .repeat(depth),
        "</div><!-- /wp:group -->".repeat(depth),
    ]
    .concat();
    let html = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || render(&twentytwentyfive(), &markup))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(html.matches("<div class=\"wp-block-group").count(), depth);
}

/// Published synced patterns, each as deep as the parser allows with the
/// next pattern at the bottom, without end.
struct PatternChain;

impl Site for PatternChain {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn home_url(&self) -> Vec<u8> {
        b"http://example.test".to_vec()
    }

    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        let depth = crate::blocks::MAX_BLOCK_DEPTH - 1;
        let content = [
            "<!-- wp:group --><div class=\"wp-block-group\">".repeat(depth),
            format!("<!-- wp:block {{\"ref\":{}}} /-->", id + 1),
            "</div><!-- /wp:group -->".repeat(depth),
        ]
        .concat();
        Some(hyscribe_model::Post {
            id,
            post_type: "wp_block".into(),
            status: "publish".into(),
            content: content.into_bytes(),
            ..Default::default()
        })
    }

    fn post_terms(&self, _: u64, _: &str) -> Vec<hyscribe_model::Term> {
        Vec::new()
    }

    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn query(&self, _: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        Vec::new()
    }
}

/// Every parse starts its own nesting count, so rendering counts the
/// blocks it is inside across synced patterns, post content and template
/// parts, and renders nothing past [`MAX_RENDER_DEPTH`].
#[test]
fn chained_patterns_stop_at_the_render_depth() {
    let html = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(|| {
            let html = render_content(
                &twentytwentyfive(),
                &PatternChain,
                &Request::default(),
                br#"<!-- wp:block {"ref":1} /-->"#,
            );
            String::from_utf8(html).unwrap()
        })
        .unwrap()
        .join()
        .unwrap();
    // Each pattern's groups, and the pattern blocks themselves, which
    // render no wrapper of their own.
    let groups = MAX_RENDER_DEPTH - MAX_RENDER_DEPTH / crate::blocks::MAX_BLOCK_DEPTH;
    assert_eq!(html.matches("<div class=\"wp-block-group").count(), groups);
}

/// The depth counts blocks rendered inside others, not blocks rendered
/// before: a page of more blocks side by side renders them all.
#[test]
fn blocks_side_by_side_do_not_count_as_depth() {
    let blocks = MAX_RENDER_DEPTH + 1;
    let markup =
        "<!-- wp:group --><div class=\"wp-block-group\"></div><!-- /wp:group -->".repeat(blocks);
    let html = render(&twentytwentyfive(), &markup);
    assert_eq!(html.matches("<div class=\"wp-block-group").count(), blocks);
}

#[test]
fn replacing_nothing_keeps_the_subject() {
    assert_eq!(replace(b"abc", b"", b"x"), b"abc");
}

#[test]
fn sites_without_user_data_have_no_users() {
    assert_eq!(DefaultInstall.user(1), None);
    let post = hyscribe_model::Post::default();
    assert_eq!(DefaultInstall.adjacent_post(&post, true, None), None);
    assert!(DefaultInstall.comments(1).is_empty());
}

/// Every case of `reference/details/golden.sh`: images of a details block
/// shown closed load with low priority.
#[test]
fn details_images_match_php() {
    let theme = crate::theme::twentytwentyfive();
    let request = crate::site::Request::default();
    let site = crate::fixture::DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../tests/golden/details.tsv");
    for line in golden.lines() {
        let (markup, php) = line.split_once('\t').unwrap();
        let (markup, php) = (
            hyscribe_testkit::unhex(markup),
            hyscribe_testkit::unhex(php),
        );
        let ours = Renderer::new(&theme, &site, &request).render(&markup);
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&php),
            "{}",
            String::from_utf8_lossy(&markup)
        );
    }
    assert_eq!(golden.lines().count(), 7);
}

/// Every case of `reference/widget-group/golden.sh`: the title, then the
/// inner blocks without the group's own markup.
#[test]
fn widget_groups_match_php() {
    let theme = crate::theme::twentytwentyfive();
    let request = crate::site::Request::default();
    let site = crate::fixture::DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../tests/golden/widget-group.tsv");
    for line in golden.lines() {
        let (markup, php) = line.split_once('\t').unwrap();
        let (markup, php) = (
            hyscribe_testkit::unhex(markup),
            hyscribe_testkit::unhex(php),
        );
        let ours = Renderer::new(&theme, &site, &request).render(&markup);
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(&php),
            "{}",
            String::from_utf8_lossy(&markup)
        );
    }
    assert_eq!(golden.lines().count(), 6);
}

/// Every case of `reference/synced-pattern/golden.sh`, over the patterns
/// it inserted and rolled back.
#[test]
fn synced_patterns_match_php() {
    let golden = include_str!("../../tests/golden/synced-pattern.tsv");
    let mut site = crate::fixture::site(vec![]);
    let mut cases = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields[0] == "post" {
            site.posts.push(hyscribe_model::Post {
                id: fields[1].parse().unwrap(),
                post_type: fields[2].into(),
                status: fields[3].into(),
                password: hyscribe_testkit::unhex(fields[4]),
                content: hyscribe_testkit::unhex(fields[5]),
                ..Default::default()
            });
        } else {
            cases.push((
                hyscribe_testkit::unhex(fields[1]),
                hyscribe_testkit::unhex(fields[2]),
            ));
        }
    }
    let theme = crate::theme::twentytwentyfive();
    let request = crate::site::Request::default();
    for (markup, php) in &cases {
        let ours = render_content(&theme, &site, &request, markup);
        assert_eq!(
            String::from_utf8_lossy(&ours),
            String::from_utf8_lossy(php),
            "{}",
            String::from_utf8_lossy(markup)
        );
    }
    assert_eq!(cases.len(), 13);
}

/// An empty template renders nothing and enqueues no skip link, as the
/// blank page of `reference/pages/golden.sh site-edits` shows.
#[test]
fn empty_templates_render_nothing() {
    let theme = twentytwentyfive();
    let site = crate::fixture::DefaultInstall { home: Vec::new() };
    let request = Request::default();
    for template in [&b""[..], b"0"] {
        let (html, assets) = render_template(&theme, &site, &request, template);
        assert!(html.is_empty());
        let head = crate::head::wp_head(&theme, &site, &request, &assets, b"T");
        assert!(!String::from_utf8(head).unwrap().contains("skip-link"));
    }
    let (_, assets) = render_template(&theme, &site, &request, b"<p>x</p>");
    let head = crate::head::wp_head(&theme, &site, &request, &assets, b"T");
    assert!(String::from_utf8(head).unwrap().contains("skip-link"));
}
