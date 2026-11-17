// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::embed::tests::Embeds;

/// `oembed_result` sees every embed and can drop one.
struct Dropping;

impl crate::outbound::Hooks for Dropping {
    fn oembed_result(&self, html: Option<Vec<u8>>, url: &[u8], _: Size) -> Option<Vec<u8>> {
        html.filter(|_| !url.ends_with(b"link.html"))
    }

    fn embed_oembed_html(&self, html: Vec<u8>, _: &[u8], _: Size, post: u64) -> Vec<u8> {
        [&format!("<!-- {post} -->")[..].as_bytes(), &html[..]].concat()
    }
}

#[test]
fn hooks_change_results() {
    let mut site = Embeds::new();
    site.hooks = Box::new(Dropping);
    let page = |name: &str| format!("http://pages.test:8080/oembed/{name}.html").into_bytes();
    assert!(matches!(
        wp_oembed_get(&site, &page("link"), crate::embed::DEFAULTS),
        Got::Failed
    ));
    assert!(matches!(
        wp_oembed_get(&site, &page("photo"), crate::embed::DEFAULTS),
        Got::Html(_)
    ));
    let theme = crate::theme::twentytwentyfive();
    let request = crate::site::Request {
        post: Some(1),
        ..Default::default()
    };
    let html = crate::template::Renderer::new(&theme, &site, &request).autoembed(&page("photo"));
    assert!(html.starts_with(b"<!-- 1 --><a href="));
}

#[test]
fn reads_link_attributes_as_shortcodes_do() {
    let atts = attributes(br#" a="1" B='2' c=3 "quoted" d=  e=5 f"#);
    let names: Vec<_> = atts
        .iter()
        .map(|(k, v)| (k.as_slice(), v.as_slice()))
        .collect();
    assert_eq!(
        names,
        [
            (&b"a"[..], &b"1"[..]),
            (b"b", b"2"),
            (b"c", b"3"),
            // `\s*` after `=` runs on to the next value, as in PHP.
            (b"d", b"e=5")
        ]
    );
    assert_eq!(
        links(b"<link a=b/><LINK c=d><link><link e=f"),
        [&b" a=b"[..], b" c=d"]
    );
    assert_eq!(
        htmlspecialchars_decode(b"&amp;&quot;&#039;&#39;&lt;&gt;&apos;"),
        b"&\"''<>&apos;"
    );
}

#[test]
fn reads_xml_answers() {
    let data =
        parse_xml(b"<oembed><type>rich</type><type>video</type><html>a<b>x</b>c</html></oembed>")
            .unwrap();
    assert_eq!(
        property(&data, b"type"),
        Some(&Value::String(b"video".to_vec()))
    );
    assert_eq!(
        property(&data, b"html"),
        Some(&Value::String(b"ac".to_vec()))
    );
    assert!(parse_xml(b"<oembed>").is_none());
    assert!(parse_json(b" [1] ").is_none());
    assert!(parse_json(b"{}").is_none());
}
