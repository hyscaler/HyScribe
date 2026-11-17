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
        [format!("<!-- {post} -->").into_bytes(), html].concat()
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
    let atts = attributes(br#" a="1" B='2' c=3 "quoted" d=  e=5 f g="x"#);
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

#[test]
fn only_known_types_make_html() {
    let site = Embeds::new();
    let data = |json: &str| parse_json(json.as_bytes()).unwrap();
    assert_eq!(
        data2html(&site, &data(r#"{"type":1,"html":"x"}"#), b"u"),
        None
    );
    assert_eq!(
        data2html(&site, &data(r#"{"type":"other","html":"x"}"#), b"u"),
        None
    );
}

#[test]
fn incomplete_answers_make_no_html() {
    let site = Embeds::new();
    for json in [
        r#"{"a":1}"#,
        r#"{"type":"photo","width":1,"height":1}"#,
        r#"{"type":"photo","url":"u","width":1,"height":"x"}"#,
        r#"{"type":"rich"}"#,
        r#"{"type":"link"}"#,
    ] {
        let data = parse_json(json.as_bytes()).unwrap();
        assert_eq!(data2html(&site, &data, b"u"), None, "{json}");
    }
    assert!(parse_json(b"{x").is_none());
    let data = parse_json(br#"{"title":"T"}"#).unwrap();
    let titled = iframe_title(b"<iframe src=x".to_vec(), &data);
    assert_eq!(titled, b"<iframe title=\"T\" src=x");
    for html in [&b"<a></iframe>"[..], b"<b>x</b></iframe>"] {
        assert_eq!(filter_oembed_result(&site, html.to_vec(), b"u"), None);
    }
    let bare = filter_oembed_result(&site, b"<iframe></iframe>".to_vec(), b"u").unwrap();
    assert!(bare.starts_with(b"<iframe class=\"wp-embedded-content\""));
    for html in [&b" src=x"[..], b" src=\"abc", b" src=\"a\nb\""] {
        assert_eq!(source(html), None);
    }
}
