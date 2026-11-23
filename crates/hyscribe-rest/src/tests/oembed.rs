// SPDX-License-Identifier: AGPL-3.0-or-later

//! The oEmbed provider's goldens, `oembed.tsv`, replayed for a visitor and
//! each role, and what the default install does not show: a page on the
//! front, attachments, thumbnails, a post named `embed`, a cached proxy
//! answer, and how SimpleXML writes text.

use std::sync::OnceLock;

use super::content::default_install_with;
use super::*;
use crate::oembed::{create_xml, normalized, xml_text};

#[test]
fn oembed_matches_php() {
    let golden = include_str!("../../tests/golden/oembed.tsv");
    // What the rewrite rules would match, and `_fields` on the proxy's
    // object, on which PHP fails.
    let unported = |case: &RestCase| {
        let rewritten = case.flag("pretty") && !case.target.contains("%3Fp%3D");
        rewritten || case.target.contains("proxy") && case.target.contains("_fields")
    };
    let goldens = [(content::default_install(), golden, false)];
    let (failures, not_served) = mismatches(&goldens, true, &unported);
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!((rest_cases(golden).len(), not_served), (6 * 53, 3 * 6 + 4));
}

/// The default install with `state`'s rows, leaked for the tests.
fn with(state: &str) -> &'static Content {
    Box::leak(Box::new(default_install_with(state)))
}

fn embed(content: &'static Content, url: &str) -> Served {
    get(
        content,
        "visitor",
        &format!("/?rest_route=/oembed/1.0/embed&url={url}"),
    )
}

/// `url_to_postid()` gives a static front page for the home URL, but not
/// one that is not there.
#[test]
fn the_front_page_is_its_page() {
    static FRONT: OnceLock<&Content> = OnceLock::new();
    let front = FRONT.get_or_init(|| {
        with("INSERT INTO `wp_options` VALUES (9921,'show_on_front','page','on'),(9922,'page_on_front','2','on');")
    });
    let served = embed(front, "http://127.0.0.1:8081//");
    let data = hyscribe_phpser::json_decode(&served.body).unwrap();
    assert_eq!(data.get_str("title"), Some(&text("Sample Page")));
    let gone = with(
        "INSERT INTO `wp_options` VALUES (9921,'show_on_front','page','on'),(9922,'page_on_front','999','on');",
    );
    assert_eq!(embed(gone, "http://127.0.0.1:8081/").status, 404);
    assert_eq!(
        embed(content::default_install(), "http://127.0.0.1:8081/").status,
        404
    );
    // Another URL is not the front page's; under pretty permalinks the
    // rewrite rules would read it, and a front page that is not there.
    assert_eq!(embed(front, "http://127.0.0.1:8081/other/").status, 404);
    let pretty = |content, url: &str| {
        let case = RestCase {
            who: "visitor".into(),
            method: "GET".into(),
            target: format!("/wp-json/oembed/1.0/embed?url={url}"),
            flags: "pretty".into(),
            status: 0,
            headers: Vec::new(),
            body: Vec::new(),
            sent: Vec::new(),
        };
        serve_over(content, &case).status
    };
    assert_eq!(pretty(gone, "http://127.0.0.1:8081/"), 501);
}

/// An attachment, whose thumbnail is the image itself, and a post with a
/// thumbnail are not ported; nor is a post's embed URL where a post named
/// `embed` could sit at it.
#[test]
fn thumbnails_and_embed_paths_are_not_served() {
    let site = with(include_str!("../../../../reference/rest/index-site.sql"));
    assert_eq!(embed(site, "%2F%3Fattachment_id%3D951").status, 501);
    let thumbnail = with("INSERT INTO `wp_postmeta` VALUES (9931,1,'_thumbnail_id','951');");
    assert_eq!(embed(thumbnail, "%2F%3Fp%3D1").status, 501);
    let none = with("INSERT INTO `wp_postmeta` VALUES (9931,1,'_thumbnail_id','0');");
    assert_eq!(embed(none, "%2F%3Fp%3D1").status, 200);
    let named = with(
        "INSERT INTO `wp_posts` VALUES (961,1,'2026-11-01 10:00:00','2026-11-01 10:00:00','','Embed','','draft','open','closed','','embed','','','2026-11-01 10:00:00','2026-11-01 10:00:00','',0,'http://127.0.0.1:8081/?page_id=961',0,'page','',0);",
    );
    let pretty = |content| {
        let case = RestCase {
            who: "visitor".into(),
            method: "GET".into(),
            target: "/wp-json/oembed/1.0/embed?url=%2F%3Fp%3D1".into(),
            flags: "pretty".into(),
            status: 0,
            headers: Vec::new(),
            body: Vec::new(),
            sent: Vec::new(),
        };
        serve_over(content, &case).status
    };
    assert_eq!(pretty(named), 501);
    assert_eq!(pretty(content::default_install()), 200);
    // A synced pattern is no public post type's.
    let pattern = with(
        "INSERT INTO `wp_posts` VALUES (961,1,'2026-11-01 10:00:00','2026-11-01 10:00:00','','Embed','','publish','closed','closed','','embed','','','2026-11-01 10:00:00','2026-11-01 10:00:00','',0,'http://127.0.0.1:8081/?p=961',0,'wp_block','',0);",
    );
    assert_eq!(pretty(pattern), 200);
}

/// A proxy answer PHP cached for the same arguments is not ported.
#[test]
fn cached_proxy_answers_are_not_served() {
    let target = "/?rest_route=/oembed/1.0/proxy&url=%2F%3Fp%3D1";
    assert_eq!(
        get(content::default_install(), "author", target).status,
        200
    );
    // md5( serialize( array( 'format' => 'json', 'maxwidth' => 600,
    // 'discover' => true, 'url' => '/?p=1' ) ) ).
    let key = hyscribe_phpser::md5(
        br#"a:4:{s:6:"format";s:4:"json";s:8:"maxwidth";i:600;s:8:"discover";b:1;s:3:"url";s:5:"/?p=1";}"#,
    );
    let cached = with(&format!(
        "INSERT INTO `wp_options` VALUES (9941,'_transient_oembed_{key}','x','off');"
    ));
    assert_eq!(get(cached, "author", target).status, 501);
    // A URL that is none of this site's posts, which PHP would fetch, and
    // one the rewrite rules would match.
    let missing = "/?rest_route=/oembed/1.0/proxy&url=%2F%3Fp%3D999";
    assert_eq!(
        get(content::default_install(), "author", missing).status,
        501
    );
    let case = RestCase {
        who: "author".into(),
        method: "GET".into(),
        target: "/wp-json/oembed/1.0/proxy?url=%2Fhello-world%2F".into(),
        flags: "pretty".into(),
        status: 0,
        headers: Vec::new(),
        body: Vec::new(),
        sent: Vec::new(),
    };
    assert_eq!(serve_over(content::default_install(), &case).status, 501);
}

/// `url_to_postid()` reads the first of `p`, `page_id` and
/// `attachment_id` given digits.
#[test]
fn the_first_id_with_digits_is_read() {
    let served = embed(content::default_install(), "%2F%3Fp%3D%26page_id%3D2");
    let data = hyscribe_phpser::json_decode(&served.body).unwrap();
    assert_eq!(data.get_str("title"), Some(&text("Sample Page")));
}

/// `set_url_scheme()` and the `www.` `url_to_postid()` adds or takes away
/// to match the home URL.
#[test]
fn urls_take_the_home_url_s_scheme_and_www() {
    let cases: [(&str, &str, &str); 7] = [
        ("//x.test/a?b#c", "https://x.test", "https://x.test/a"),
        // `#^\w+://#` wants a scheme before its `://`.
        ("://x.test/a", "https://x.test", "://x.test/a"),
        ("ab/c", "https://x.test", "ab/c"),
        (
            "http://x.test/a/",
            "https://www.x.test",
            "https://www.x.test/a/",
        ),
        (
            "http://www.x.test/a",
            "http://www.x.test",
            "http://www.x.test/a",
        ),
        ("ftp://www.x.test/a", "http://x.test", "http://x.test/a"),
        (" /a#?p=1", "http://x.test", "/a"),
    ];
    for (url, home, expected) in cases {
        let url = normalized(url.as_bytes(), home.as_bytes());
        assert_eq!(String::from_utf8_lossy(&url), expected, "{home}");
    }
}

/// Text as `addChild()` reads escaped HTML and `asXML()` writes it: XML's
/// own entities and references read, other entities kept, controls, bad
/// references and a lone `&` refused.
#[test]
fn xml_text_is_written_as_simplexml_writes_it() {
    let written = |text: &str| xml_text(text.as_bytes()).map(|xml| String::from_utf8(xml).unwrap());
    assert_eq!(
        written(
            "a&amp;b &lt;c&gt; &quot;q&quot; &apos;&#039;s&#x41;&#X42; &nbsp; é \r\t\n&#60;&#62;"
        ),
        Some("a&amp;b &lt;c&gt; \"q\" ''sAB &nbsp; &#xE9; &#xD;\t\n&lt;&gt;".into())
    );
    for refused in [
        "a & b",
        "&#0;",
        "&#xD800;",
        "&#xZ;",
        "&;",
        "&a-b;",
        "\u{1}",
        "&amp;\u{1}",
    ] {
        assert_eq!(written(refused), None, "{refused:?}");
    }
    assert_eq!(xml_text(b"\xff"), None);
    let data = object([("a", Value::Array(Vec::new())), ("1.5", text("x"))]);
    assert_eq!(
        create_xml(&data).unwrap(),
        b"<?xml version=\"1.0\"?>\n<oembed><a/><oembed>x</oembed></oembed>\n"
    );
    assert_eq!(create_xml(&object([("a", text("\u{1}"))])), None);
    let nested = object([("a", object([("b", text("\u{1}"))]))]);
    assert_eq!(create_xml(&nested), None);
    assert_eq!(create_xml(&Value::Array(Vec::new())), None);
}

/// XML that SimpleXML would not write is not ported: here no data left
/// after `_fields`.
#[test]
fn empty_xml_is_not_served() {
    let url = "%2F%3Fp%3D1&format=xml&_fields=nope";
    assert_eq!(embed(content::default_install(), url).status, 501);
}
