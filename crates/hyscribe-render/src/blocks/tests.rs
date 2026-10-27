// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

fn php(document: &str) -> String {
    String::from_utf8(hyscribe_phpser::encode(&list(&parse_blocks(
        document.as_bytes(),
    ))))
    .unwrap()
}

/// A compact view: names, HTML and inner content, for structure tests.
fn shape(blocks: &[Block]) -> Vec<String> {
    blocks
        .iter()
        .map(|b| {
            let content: Vec<String> = b
                .inner_content
                .iter()
                .map(|c| {
                    c.as_deref()
                        .map_or("*".to_string(), |h| String::from_utf8_lossy(h).into_owned())
                })
                .collect();
            format!(
                "{}[{}]{{{}}}",
                b.name.as_deref().unwrap_or("html"),
                content.join("|"),
                shape(&b.inner_blocks).join(",")
            )
        })
        .collect()
}

fn parse(document: &str) -> Vec<String> {
    shape(&parse_blocks(document.as_bytes()))
}

#[test]
fn serializes_like_php() {
    assert_eq!(
        php("plain text"),
        "a:1:{i:0;a:5:{s:9:\"blockName\";N;s:5:\"attrs\";a:0:{}s:11:\"innerBlocks\";a:0:{}\
         s:9:\"innerHTML\";s:10:\"plain text\";s:12:\"innerContent\";a:1:{i:0;s:10:\"plain text\";}}}"
    );
    assert_eq!(
        php("<!-- wp:a {\"x\":1} -->y<!-- /wp:a -->"),
        "a:1:{i:0;a:5:{s:9:\"blockName\";s:6:\"core/a\";s:5:\"attrs\";a:1:{s:1:\"x\";i:1;}\
         s:11:\"innerBlocks\";a:0:{}s:9:\"innerHTML\";s:1:\"y\";s:12:\"innerContent\";a:1:{i:0;s:1:\"y\";}}}"
    );
    assert_eq!(
        php("<!-- wp:a {bad json} /-->"),
        "a:1:{i:0;a:5:{s:9:\"blockName\";s:6:\"core/a\";s:5:\"attrs\";N;s:11:\"innerBlocks\";a:0:{}\
         s:9:\"innerHTML\";s:0:\"\";s:12:\"innerContent\";a:0:{}}}"
    );
    assert_eq!(php(""), "a:0:{}");
}

#[test]
fn nesting_and_inner_content() {
    assert_eq!(
        parse(
            "before<!-- wp:a -->1<!-- wp:b -->2<!-- wp:c /-->3<!-- /wp:b -->4<!-- /wp:a -->after"
        ),
        [
            "html[before]{}",
            "core/a[1|*|4]{core/b[2|*|3]{core/c[]{}}}",
            "html[after]{}",
        ]
    );
    // HTML that PHP's empty() calls empty, "0", is dropped between blocks.
    assert_eq!(
        parse("<!-- wp:a -->0<!-- wp:b /-->0<!-- /wp:a -->"),
        ["core/a[*]{core/b[]{}}"]
    );
    // ...but kept when a nested block closes, which has no empty() check.
    assert_eq!(
        parse("<!-- wp:a --><!-- wp:b --><!-- /wp:b --><!-- /wp:a -->"),
        ["core/a[*]{core/b[]{}}"]
    );
    assert_eq!(
        parse("<!-- wp:a --><!-- wp:b -->0<!-- /wp:b --><!-- /wp:a -->"),
        ["core/a[*]{core/b[0]{}}"]
    );
}

#[test]
fn void_blocks() {
    assert_eq!(
        parse("x<!-- wp:a /--><!-- wp:b /-->"),
        ["html[x]{}", "core/a[]{}", "core/b[]{}"]
    );
    // A void marker wins over a closer.
    assert_eq!(parse("<!-- /wp:a /-->"), ["core/a[]{}"]);
}

#[test]
fn malformed_documents() {
    // Unclosed blocks close at the end, each straight into the output.
    assert_eq!(parse("<!-- wp:a -->x"), ["core/a[x]{}"]);
    // Each keeps the rest of the document, after a chunk of the HTML
    // before it, as WordPress does.
    assert_eq!(
        parse("<!-- wp:a -->1<!-- wp:b -->2<!-- wp:c -->3"),
        [
            "html[2]{}",
            "core/c[3]{}",
            "html[1]{}",
            "core/b[2<!-- wp:c -->3]{}",
            "core/a[1<!-- wp:b -->2<!-- wp:c -->3]{}",
        ]
    );
    // A closer with nothing open turns the rest into one HTML chunk.
    assert_eq!(
        parse("x<!-- wp:a -->y<!-- /wp:a --><!-- /wp:b -->z"),
        ["html[x]{}", "core/a[y]{}", "html[<!-- /wp:b -->z]{}"]
    );
    assert_eq!(parse("<!-- /wp:a -->rest"), ["html[<!-- /wp:a -->rest]{}"]);
    // Closer names are not checked.
    assert_eq!(parse("<!-- wp:a -->x<!-- /wp:z -->"), ["core/a[x]{}"]);
}

#[test]
fn delimiter_syntax() {
    let names = |document: &str| -> Vec<Option<String>> {
        parse_blocks(document.as_bytes())
            .into_iter()
            .map(|b| b.name)
            .collect()
    };
    let one = |name: &str| vec![Some(name.to_string())];
    assert_eq!(
        names("<!-- wp:my-plugin/block_1 /-->"),
        one("my-plugin/block_1")
    );
    assert_eq!(names("<!--\twp:a\x0b/-->"), one("core/a"));
    for not_a_block in [
        "<!--wp:a /-->",
        "<!-- wp:A /-->",
        "<!-- wp: a /-->",
        "<!-- wp:a/b/c /-->",
        "<!-- wp:a/ /-->",
        "<!-- wp:a/-->",
        "<!-- wp:a -- >",
        "<!-- wp:a {\"x\":1}-->",
        "<!-- wp:a {\"x\":1} --",
        "<!-- xp:a /-->",
        "abcd wp:a /-->",
        "<!-x wp:a /-->",
        "<!-- wp:",
        "<!-- /",
        "<!--",
    ] {
        assert_eq!(names(not_a_block), [None], "{not_a_block:?}");
    }
}

#[test]
fn attributes() {
    let attrs = |document: &str| parse_blocks(document.as_bytes()).remove(0).attrs;
    let x = |value: Value| Some(vec![(Key::String(b"x".to_vec()), value)]);
    assert_eq!(attrs("<!-- wp:a /-->"), Some(Vec::new()));
    assert_eq!(
        attrs("<!-- wp:a {\"x\":\"}\"} /-->"),
        x(Value::String(b"}".to_vec()))
    );
    assert_eq!(
        attrs("<!-- wp:a {\"x\":\"-->\"} /-->"),
        x(Value::String(b"-->".to_vec()))
    );
    assert_eq!(
        attrs("<!-- wp:a {\"x\":{\"y\":{}}} /-->"),
        x(Value::Array(vec![(
            Key::String(b"y".to_vec()),
            Value::Array(Vec::new())
        )]))
    );
    assert_eq!(attrs("<!-- wp:a {\"x\":1}\n /-->"), x(Value::Int(1)));
    // PCRE's \s takes \f into the JSON, which JSON rejects.
    assert_eq!(attrs("<!-- wp:a {\"x\":1}\x0c/-->"), None);
    assert_eq!(attrs("<!-- wp:a {\"x\":1}} /-->"), None);
    // Everything up to the last `} /-->` is one attribute text.
    assert_eq!(attrs("<!-- wp:a {\"x\":1} {\"y\":2} /-->"), None);
    assert_eq!(
        attrs("<!-- wp:a {\"x\":\"}} -->\"} -->y<!-- /wp:a -->"),
        None
    );
}

#[test]
fn a_failed_attribute_scan_is_not_repeated() {
    let document = "<!-- wp:a {\"x\": <!-- wp:b {\"y\": tail".as_bytes();
    let mut parser = Parser {
        doc: document,
        offset: 0,
        output: Vec::new(),
        stack: Vec::new(),
        no_terminator_from: None,
    };
    assert!(matches!(parser.next_token(), Token::NoMore));
    // The first scan found no terminator, so the second never ran.
    assert_eq!(parser.no_terminator_from, Some(10));
    assert_eq!(parser.attributes(30), None);
    assert_eq!(parser.no_terminator_from, Some(10));
    // Many unterminated delimiters still parse.
    let hostile = "<!-- wp:a {\"x\":".repeat(500);
    assert_eq!(parse_blocks(hostile.as_bytes()).len(), 1);
}

#[test]
fn render_puts_inner_blocks_back() {
    let document =
        "before<!-- wp:a -->1<!-- wp:b -->2<!-- wp:c /-->3<!-- /wp:b -->4<!-- /wp:a -->after";
    assert_eq!(
        render(&parse_blocks(document.as_bytes())),
        b"before1234after"
    );
    // More placeholders than inner blocks cannot happen, but must not panic.
    let mut odd = Block::named("core/a".into(), Some(Vec::new()));
    odd.inner_content = vec![None, Some(b"x".to_vec())];
    assert_eq!(render(&[odd]), b"x");
}
