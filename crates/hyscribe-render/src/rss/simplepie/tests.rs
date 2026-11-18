// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn absolutizes_as_rfc_3986_resolves() {
    let base = b"http://a.test/b/c/d;p?q#f";
    for (relative, resolved) in [
        (&b"g"[..], &b"http://a.test/b/c/g"[..]),
        (b"./g", b"http://a.test/b/c/g"),
        (b"g/", b"http://a.test/b/c/g/"),
        (b"/g", b"http://a.test/g"),
        (b"//g.test/x", b"http://g.test/x"),
        (b"?y", b"http://a.test/b/c/d;p?y"),
        (b"#s", b"http://a.test/b/c/d;p?q#s"),
        (b"", b"http://a.test/b/c/d;p?q"),
        (b"../g?x#y", b"http://a.test/b/g?x#y"),
        (b"../../../g", b"http://a.test/g"),
        (b".", b"http://a.test/b/c/"),
        (b"..", b"http://a.test/b/"),
        (b"https://other.test/x", b"https://other.test/x"),
    ] {
        assert_eq!(absolutize(relative, base), resolved, "{relative:?}");
    }
    assert_eq!(absolutize(b"x", b"http://a.test"), b"http://a.test/x");
    assert_eq!(absolutize(b"x", b"urn:a"), b"urn:x");
    assert_eq!(absolutize(b"x", b"not a base"), b"x");
    assert_eq!(absolutize(b"x", b"1a:b"), b"x");
}

#[test]
fn finds_each_feed_type() {
    let kind = |xml: &str| Feed::kind(&parse(xml.as_bytes()).unwrap());
    assert_eq!(
        kind("<feed xmlns='http://purl.org/atom/ns#'/>"),
        TYPE_ATOM_03
    );
    let rdf = "<r:RDF xmlns:r='http://www.w3.org/1999/02/22-rdf-syntax-ns#' xmlns='NS'><channel/></r:RDF>";
    assert_eq!(
        kind(&rdf.replace("NS", "http://purl.org/rss/1.0/")),
        TYPE_RSS_10
    );
    assert_eq!(
        kind(&rdf.replace("NS", "http://my.netscape.com/rdf/simple/0.9/")),
        TYPE_RSS_090
    );
    for (version, expected) in [
        ("0.91", TYPE_RSS_091),
        ("0.92", TYPE_RSS_092),
        ("0.93", TYPE_RSS_093),
        ("0.94", TYPE_RSS_094),
        ("9", TYPE_RSS_ALL),
    ] {
        assert_eq!(kind(&format!("<rss version=' {version} '/>")), expected);
    }
    assert_eq!(kind("<html/>"), 0);
}

#[test]
fn reads_items_of_older_feeds() {
    let rdf = br#"<r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns="http://purl.org/rss/1.0/" xmlns:dc="http://purl.org/dc/elements/1.0/" xmlns:i="http://www.itunes.com/dtds/podcast-1.0.dtd">
<channel><link>http://f.test/</link><dc:creator>Feed</dc:creator></channel>
<item><title>One &amp; two</title><link>one</link><dc:date>2026-01-02T03:04Z</dc:date><description>Text</description></item>
<item><dc:title>Dc</dc:title><i:author>Pod</i:author><i:summary>Sum</i:summary></item>
<item><i:subtitle>Sub</i:subtitle></item>
</r:RDF>"#;
    let data = Value::Array(parse(rdf).unwrap());
    let feed = Feed { data: &data };
    let items = feed.items();
    let titles: Vec<_> = items.iter().map(Item::title).collect();
    assert_eq!(titles[2], Some(b"One &amp; two".to_vec()));
    assert_eq!(titles[0], Some(b"Dc".to_vec()));
    assert_eq!(items[2].link(), Some(b"http://f.test/one".to_vec()));
    assert_eq!(items[2].date(), Some(1_767_323_040));
    assert_eq!(items[0].description(), Some(b"Sum".to_vec()));
    assert_eq!(items[1].description(), Some(b"Sub".to_vec()));
    assert_eq!(
        items[0].author().and_then(|a| a.name),
        Some(b"Pod".to_vec())
    );
    assert_eq!(
        items[1].author().and_then(|a| a.name),
        Some(b"Feed".to_vec())
    );
    let atom = br#"<feed xmlns="http://purl.org/atom/ns#"><entry><title type="text/html" mode="escaped">&lt;b&gt;x&lt;/b&gt;</title><link rel="alternate" href="http://e.test/"/><author><name>A</name><url>http://a.test/</url></author><issued>2026-01-01T00:00:00Z</issued><content type="application/xhtml+xml" mode="xml"><div xmlns="http://www.w3.org/1999/xhtml"><p class="c">P<br/></p></div></content></entry></feed>"#;
    let data = Value::Array(parse(atom).unwrap());
    let feed = Feed { data: &data };
    let item = &feed.items()[0];
    assert_eq!(item.title(), Some(b"<b>x</b>".to_vec()));
    assert_eq!(item.link(), Some(b"http://e.test/".to_vec()));
    assert_eq!(item.author().and_then(|a| a.name), Some(b"A".to_vec()));
    assert_eq!(
        item.description(),
        Some(b"<div><p class=\"c\">P<br></p></div>".to_vec())
    );
}

#[test]
fn sanitizes_by_construct() {
    assert_eq!(sanitize(b" a < b ", TEXT, b""), b"a &lt; b");
    // An entity or an end tag makes it HTML, through wp_kses_post().
    for html in [&b"a &amp b"[..], b"x</b >", b"&#x41;"] {
        assert_eq!(sanitize(html, MAYBE_HTML, b""), wp_kses_post(html, b""));
    }
    assert_eq!(sanitize(b"a & b", MAYBE_HTML, b""), b"a &amp; b");
    assert_eq!(sanitize(b"  ", TEXT, b""), b"");
    assert_eq!(sanitize(b"", IRI, b"http://a.test/"), b"http://a.test/");
    assert_eq!(sanitize(b"<i>", 0, b""), b"<i>");
    assert!(!looks_like_html(b"&# &#x a&"));
    assert_eq!(key(b"10"), Key::Int(10));
    assert_eq!(key(b"-3"), Key::Int(-3));
    for kept in [&b"010"[..], b"-0", b"1a", b"", b"99999999999999999999"] {
        assert_eq!(key(kept), Key::String(kept.to_vec()));
    }
}

#[test]
fn reads_what_feeds_leave_out() {
    assert_eq!(at(&Value::Int(1), b"x"), None);
    assert_eq!(first(Some(&Value::Int(1))), None);
    assert_eq!(absolutize(b"y", b"svn+ssh://h/x"), b"svn+ssh://h/y");
    assert_eq!(absolutize(b"y", b"a.b-c://h/x"), b"a.b-c://h/y");
    assert_eq!(absolutize(b"y", b"a_b://h/x"), b"y");
    let items = |xml: &[u8]| {
        let data = Value::Array(parse(xml).unwrap());
        let feed = Feed { data: &data };
        let found: Vec<_> = feed
            .items()
            .iter()
            .map(|item| {
                (
                    item.title(),
                    item.link(),
                    item.author().and_then(|a| a.name),
                )
            })
            .collect();
        found
    };
    assert!(items(b"<rss version='2.0'/>").is_empty());
    let rdf = |ns: &str, channel: &str| {
        format!(
            "<r:RDF xmlns:r='http://www.w3.org/1999/02/22-rdf-syntax-ns#' xmlns='{ns}'>{channel}<item><title>T</title><link>one</link></item></r:RDF>"
        )
    };
    for ns in [
        "http://purl.org/rss/1.0/",
        "http://my.netscape.com/rdf/simple/0.9/",
    ] {
        let bare = rdf(ns, "");
        assert_eq!(items(bare.as_bytes())[0].1, Some(b"one".to_vec()));
    }
    let old = rdf(
        "http://my.netscape.com/rdf/simple/0.9/",
        "<channel><link>http://o.test/</link></channel>",
    );
    assert_eq!(
        items(old.as_bytes())[0].1,
        Some(b"http://o.test/one".to_vec())
    );
    let atom = br#"<feed xmlns="http://www.w3.org/2005/Atom" xmlns:dc="http://purl.org/dc/elements/1.1/">
<link rel="self" href="http://f.test/self"/><link rel="alternate" href="http://f.test/"/>
<entry><title>A</title><link rel="edit" href="e"/><link href="one"/><author><email>e@x</email></author></entry>
<entry><title>B</title><author><uri>http://u.test/</uri></author><author/></entry>
<entry><title>C</title><dc:creator>D</dc:creator><dc:creator>D</dc:creator></entry>
</feed>"#;
    let found = items(atom);
    assert_eq!(found[0].1, Some(b"http://f.test/one".to_vec()));
    assert_eq!(found[0].2, None);
    assert_eq!(found[2].2, Some(b"D".to_vec()));
    let old_atom = br#"<feed xmlns="http://purl.org/atom/ns#"><entry><title>Plain &amp; text</title></entry><entry><title type="x/y">&lt;i&gt;</title></entry></feed>"#;
    let found = items(old_atom);
    assert_eq!(found[0].0, Some(b"Plain &amp; text".to_vec()));
    assert_eq!(found[1].0, Some(b"<i>".to_vec()));
    // Elements of no namespace in an RSS title give their text only.
    let nested =
        items(b"<rss version='2.0'><channel><item><title>a<b>c</b></title></item></channel></rss>");
    assert_eq!(nested[0].0, Some(b"ac".to_vec()));
}

#[test]
fn sanitizes_each_construct_type() {
    let title = |ns: &str, kind: &str, title: &str| {
        let xml = format!("<feed xmlns='{ns}'><entry><title{kind}>{title}</title></entry></feed>");
        let data = Value::Array(parse(xml.as_bytes()).unwrap());
        let feed = Feed { data: &data };
        feed.items()[0].title().unwrap()
    };
    let (atom, old) = ("http://www.w3.org/2005/Atom", "http://purl.org/atom/ns#");
    let script = "<div xmlns='http://www.w3.org/1999/xhtml'><script>s</script></div>";
    for (ns, kind, text, expected) in [
        (atom, "", "a &amp; b", &b"a &amp; b"[..]),
        (atom, " type='text'", "a &lt;", b"a &lt;"),
        (atom, " type='html'", "a &amp; b", b"a &amp; b"),
        (atom, " type='html'", "a &amp;amp b", b"a &amp;amp b"),
        (atom, " type='xhtml'", script, b"<div>s</div>"),
        (atom, " type='other'", "a &amp; b", b"a & b"),
        (old, "", "a &lt;", b"a &lt;"),
        (old, " type='text/html'", "a &amp; b", b"a &amp; b"),
        (
            old,
            " type='application/xhtml+xml' mode='xml'",
            script,
            b"<div>s</div>",
        ),
        (old, " type='other'", "a &amp; b", b"a & b"),
    ] {
        assert_eq!(title(ns, kind, text), expected, "{ns} {kind} {text}");
    }
    assert!(!looks_like_html(b"a</ b>"));
    assert!(!looks_like_html(b"a</b"));
    assert_eq!(
        absolutize(b"g#s", b"http://a.test/b/c"),
        b"http://a.test/b/g#s"
    );
}

#[test]
fn reads_feed_links_and_authors() {
    let atom = br#"<feed xmlns="http://www.w3.org/2005/Atom"><link rel="self" href="http://f.test/x/self"/><link href="http://f.test/"/><entry><link href="one"/><author><name>Only</name></author></entry></feed>"#;
    let data = Value::Array(parse(atom).unwrap());
    let feed = Feed { data: &data };
    let item = &feed.items()[0];
    assert_eq!(item.link(), Some(b"http://f.test/one".to_vec()));
    assert_eq!(item.author().and_then(|a| a.name), Some(b"Only".to_vec()));
}
