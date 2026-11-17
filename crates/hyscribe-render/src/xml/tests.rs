// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

/// Every event of `xml`, or the error.
fn events(xml: &[u8], html: bool) -> Result<Vec<Event>, Error> {
    let mut parser = Parser::new(xml, html);
    let mut events = Vec::new();
    loop {
        let event = parser.read()?;
        if event == Event::Eof {
            return Ok(events);
        }
        events.push(event);
    }
}

fn name(namespace: &[u8], local: &[u8]) -> Name {
    Name {
        namespace: namespace.to_vec(),
        local: local.to_vec(),
    }
}

#[test]
fn reads_namespaces_text_and_references() {
    let xml = b"\xEF\xBB\xBF<?xml version=\"1.0\"?>\n<!DOCTYPE r>\n<!-- c --><r xmlns=\"u:a\" xmlns:p=\"u:p\" p:x=\"1\r\n2\" y='&#x41;&lt;'><p:e xml:lang=\"en\"/><![CDATA[<raw>\r\n]]>a&amp;b&#233;\r\nc</r>\n";
    let found = events(xml, false).unwrap();
    assert_eq!(
        found,
        [
            Event::Doctype,
            Event::Start {
                name: name(b"u:a", b"r"),
                attributes: vec![
                    (name(b"u:p", b"x"), b"1 2".to_vec()),
                    (name(b"", b"y"), b"A<".to_vec())
                ],
            },
            Event::Start {
                name: name(b"u:p", b"e"),
                attributes: vec![(name(NAMESPACE_XML, b"lang"), b"en".to_vec())],
            },
            Event::End,
            Event::Text(b"<raw>\n".to_vec()),
            Event::Text("a&b\u{e9}\nc".as_bytes().to_vec()),
            Event::End,
        ]
    );
    // HTML 4's entities only where SimplePie declares them.
    let nbsp = events(b"<r>&nbsp;&hellip;</r>", true).unwrap();
    assert_eq!(
        nbsp.get(1),
        Some(&Event::Text("\u{a0}\u{2026}".as_bytes().to_vec()))
    );
    assert_eq!(
        events(b"<r>&nbsp;</r>", false).unwrap_err().message,
        "Undeclared entity"
    );
}

#[test]
fn refuses_what_is_not_well_formed() {
    let message = |xml: &[u8]| events(xml, true).unwrap_err().message;
    let broken = events(b"<a>\n<b></a>", false).unwrap_err();
    assert_eq!(
        (broken.message, broken.line, broken.column),
        ("Mismatched tag", 2, 8)
    );
    assert_eq!(message(b""), "Invalid document end");
    assert_eq!(message(b"<a>"), "Invalid document end");
    assert_eq!(message(b"text"), "Start tag expected, '<' not found");
    assert_eq!(
        message(b"<a/>x"),
        "Extra content at the end of the document"
    );
    assert_eq!(
        message(b"<a/><b/>"),
        "Extra content at the end of the document"
    );
    assert_eq!(
        message(b"<a/><![CDATA[x]]>"),
        "Extra content at the end of the document"
    );
    assert_eq!(message(b"<a><!-- x"), "Comment not terminated");
    assert_eq!(message(b"<a><![CDATA[x"), "CData section not finished");
    assert_eq!(message(b"<?xml"), "ParsePI: PI xml space expected");
    assert_eq!(message(b"<!DOCTYPE a"), "DOCTYPE improperly terminated");
    assert_eq!(
        message(b"<!DOCTYPE a [<!ENTITY x \"y\">]><a/>"),
        "Internal subsets are not read"
    );
    assert_eq!(message(b"<a></a x>"), "expected '>'");
    assert_eq!(message(b"<>"), "StartTag: invalid element name");
    assert_eq!(message(b"<a b=\"1\""), "Couldn't find end of Start Tag");
    assert_eq!(
        message(b"<a b=\"1\"c=\"2\"/>"),
        "attributes construct error"
    );
    assert_eq!(
        message(b"<a b/>"),
        "Specification mandates value for attribute"
    );
    assert_eq!(message(b"<a b=1/>"), "AttValue: \" or ' expected");
    assert_eq!(
        message(b"<a b=\"<\"/>"),
        "Unescaped '<' not allowed in attributes values"
    );
    assert_eq!(message(b"<a b=\"1\" b=\"2\"/>"), "Attribute redefined");
    assert_eq!(message(b"<p:a/>"), "Namespace prefix is not defined");
    assert_eq!(
        message(b"<a p:b=\"1\"/>"),
        "Namespace prefix is not defined"
    );
    assert_eq!(message(b"<a>&x</a>"), "EntityRef: expecting ';'");
    assert_eq!(
        message(b"<a>&#0;</a>"),
        "xmlParseCharRef: invalid xmlChar value"
    );
    assert_eq!(
        message(b"<a>&#xZ;</a>"),
        "xmlParseCharRef: invalid xmlChar value"
    );
    assert_eq!(
        message(b"<a>&#12a;</a>"),
        "xmlParseCharRef: invalid xmlChar value"
    );
    let deep = [b"<a>".repeat(256), b"</a>".repeat(256)].concat();
    assert!(events(&deep, false).is_ok());
    let deeper = b"<a>".repeat(257);
    assert_eq!(
        message(&deeper),
        "Excessive depth in document: 256 use XML_PARSE_HUGE option"
    );
}

#[test]
fn bindings_end_with_their_element() {
    let found = events(
        b"<a xmlns:p=\"u:1\"><p:b xmlns:p=\"u:2\"/><p:c/></a>",
        false,
    )
    .unwrap();
    assert_eq!(
        found.get(1),
        Some(&Event::Start {
            name: name(b"u:2", b"b"),
            attributes: Vec::new()
        })
    );
    assert_eq!(
        found.get(3),
        Some(&Event::Start {
            name: name(b"u:1", b"c"),
            attributes: Vec::new()
        })
    );
    assert_eq!(
        events(b"<a>\r</a>", false).unwrap().get(1),
        Some(&Event::Text(b"\n".to_vec()))
    );
}
