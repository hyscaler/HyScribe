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
}

fn render(theme: &Theme, markup: &str) -> String {
    String::from_utf8(render_content(
        theme,
        &DefaultInstall,
        Request::default(),
        markup.as_bytes(),
    ))
    .unwrap()
}

#[test]
fn the_home_template_expands_parts_and_patterns() {
    let theme = twentytwentyfive();
    let (_, template) = theme.template(&["home".to_string()]).unwrap();
    let html = String::from_utf8(render_template(
        &theme,
        &DefaultInstall,
        Request::default(),
        template,
    ))
    .unwrap();
    assert!(
        html.starts_with("<div class=\"wp-site-blocks\"><header class=\"wp-block-template-part\">")
    );
    assert!(html.contains("<main class=\"wp-block-group\""));
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
        "a<div>1P2</div>b"
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
    let mut renderer = Renderer {
        context: Context {
            theme: &theme,
            site: &DefaultInstall,
            request: Request::default(),
        },
        open_parts: Vec::new(),
        open_patterns: Vec::new(),
    };
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
