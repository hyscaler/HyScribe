// SPDX-License-Identifier: Apache-2.0 OR MIT

use alloc::string::ToString;
use alloc::vec;

use askama::Template;

use super::*;
use crate::{Plugin, Value, test};

/// `esc_html()` as PHP answers it, recorded by `reference/kses/golden.sh`:
/// hex of the text, then hex of what it gives.
const ESC_HTML: &str = include_str!("../../../hyscribe-kses/tests/golden/esc-html.tsv");

fn hex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|at| {
            u8::from_str_radix(text.get(at * 2..at * 2 + 2).unwrap_or_default(), 16).unwrap_or(0)
        })
        .collect()
}

#[test]
fn escapes_as_php_does() {
    let mut cases = 0;
    for line in ESC_HTML.lines() {
        let Some((text, expected)) = line.split_once('\t') else {
            continue;
        };
        let given = hex(text);
        assert_eq!(
            String::from_utf8_lossy(&esc_html(&given)),
            String::from_utf8_lossy(&hex(expected)),
            "{}",
            String::from_utf8_lossy(&given)
        );
        cases += 1;
    }
    assert_eq!(cases, 1815);
}

fn raw(bytes: &[u8]) -> Html {
    Safe(Raw(bytes.to_vec()))
}

/// A value through [`Esc`], as a port's `askama.toml` has every value of
/// its templates printed (the SDK's own tests go without that file).
struct Escaped<T>(T);

impl<T: fmt::Display> fmt::Display for Escaped<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = self.0.to_string();
        askama::filters::Escaper::write_escaped_str(&Esc, f, &text)
    }
}

fn escaped<T: fmt::Display>(value: T) -> Escaped<T> {
    Escaped(value)
}

#[derive(Template)]
#[template(
    source = "<p title=\"{{ title }}\">{{ head }}{{ text }}{{ number }}</p>{{ again }}",
    ext = "txt"
)]
struct Page<'a> {
    title: Escaped<&'a str>,
    head: Html,
    text: Escaped<Text>,
    number: u32,
    again: Html,
}

#[test]
fn a_page_is_bytes_with_its_values_escaped() {
    let page = Page {
        title: escaped("A \"q\" & <b> \u{E000}0\u{E001} &amp;"),
        head: raw(b"<head>\xff</head>"),
        text: escaped(Text(b"x\xffy".to_vec())),
        number: 7,
        again: raw(b"\xfe"),
    };
    let expected = [
        &b"<p title=\"A &quot;q&quot; &amp; &lt;b&gt; \xee\x80\x800\xee\x80\x81 &amp;\">"[..],
        b"<head>\xff</head>",
        b"7</p>\xfe",
    ]
    .concat();
    assert_eq!(render(&page), Ok(expected));
    // Text that is UTF-8 prints escaped.
    let page = Page {
        text: escaped(Text(b"<i>".to_vec())),
        ..page
    };
    let printed = render(&page).unwrap_or_default();
    assert!(printed.windows(10).any(|window| window == b"&lt;i&gt;7"));
}

/// Markup the host answered never prints as text, so it can only reach a
/// page through [`render`].
#[test]
fn markup_prints_only_through_render() {
    let page = Page {
        title: escaped(""),
        head: raw(b"<head>"),
        text: escaped(Text(Vec::new())),
        number: 0,
        again: raw(b""),
    };
    assert!(page.render().is_err());
    // Where it would be shown as text, a marker character prints.
    let formatted = Formatted { head: raw(b"<B>") };
    assert_eq!(render(&formatted), Ok("\u{E000}".as_bytes().to_vec()));
    // A value that fails to print fails the render.
    assert_eq!(
        render(&Failing { bad: Bad }),
        Err("an error occurred when formatting an argument".to_string())
    );
}

#[derive(Template)]
#[template(source = "{{ \"{}\"|format(head) }}", ext = "txt")]
struct Formatted {
    head: Html,
}

struct Bad;

impl fmt::Display for Bad {
    fn fmt(&self, _: &mut fmt::Formatter<'_>) -> fmt::Result {
        Err(fmt::Error)
    }
}

#[derive(Template)]
#[template(source = "{{ bad }}", ext = "txt")]
struct Failing {
    bad: Bad,
}

#[test]
fn markers_splice_their_bytes() {
    let kept = vec![b"\xff".to_vec(), b"<b>".to_vec()];
    assert_eq!(
        splice("a\u{E000}1\u{E001}b\u{E000}0\u{E001}", &kept),
        Ok(b"a<b>b\xff".to_vec())
    );
    // A marker character standing for itself, and a closing one alone.
    assert_eq!(
        splice("\u{E000}\u{E001}\u{E001}", &kept),
        Ok("\u{E000}\u{E001}".as_bytes().to_vec())
    );
    let refused = |out: &str| splice(out, &kept).unwrap_err();
    assert_eq!(refused("\u{E000}2\u{E001}"), "marker past the kept bytes");
    assert_eq!(
        refused("\u{E000}99999999999999999999999\u{E001}"),
        "marker past the kept bytes"
    );
    assert_eq!(refused("\u{E000}1"), "a marker not closed");
    assert_eq!(refused("\u{E000}x\u{E001}"), "a marker not closed");
}

struct Theme;

impl Plugin for Theme {
    fn init() -> Result<(), String> {
        Ok(())
    }
    fn activate() -> Result<(), String> {
        Ok(())
    }
    fn deactivate() -> Result<(), String> {
        Ok(())
    }
    fn invoke(_: u32, _: Vec<Value>) -> Result<Value, String> {
        Ok(Value::Null)
    }
}

#[derive(Template)]
#[template(source = "{{ head }}", ext = "txt")]
struct Head {
    head: Html,
}

#[test]
fn template_tags_print_markup() {
    let host = test::Host::<Theme>::new();
    host.state(|state| {
        state
            .printed
            .insert("wp_head".to_string(), b"<meta>\xff".to_vec());
    });
    let head = print("wp_head", &[]).unwrap_or_else(|_| raw(b"none"));
    assert!(!head.0.is_empty());
    assert_eq!(render(&Head { head }), Ok(b"<meta>\xff".to_vec()));
    assert!(print("wp_footer", &[]).is_err());
    assert!(raw(b"").0.is_empty());
}

/// A folder of its own under the system's temporary folder, with `files`.
#[cfg(unix)]
fn port(name: &str, files: &[(&str, &str)]) -> std::path::PathBuf {
    let root =
        std::env::temp_dir().join(format!("hyscribe-sdk-port-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (file, text) in files {
        let path = root.join(file);
        std::fs::create_dir_all(path.parent().unwrap_or(&root)).unwrap_or_default();
        std::fs::write(path, text).unwrap_or_default();
    }
    root
}

#[cfg(unix)]
#[test]
fn a_port_may_not_print_unescaped() {
    let clean = [
        ("templates/header.html", "<p>{{ title }}</p>"),
        ("templates/parts/a.html", "{{ head }}"),
        ("src/lib.rs", "struct Page;"),
    ];
    assert_eq!(check_port(&port("clean", &clean)), Ok(()));
    for (name, file, text) in [
        ("safe", "templates/a.html", "{{ x | safe }}"),
        (
            "filter",
            "templates/a.html",
            "{% filter safe %}x{% endfilter %}",
        ),
        ("marker", "templates/a.html", "\u{E001}"),
        ("wrapper", "src/lib.rs", "askama::filters::Safe (x)"),
        (
            "escape",
            "src/lib.rs",
            "#[template(path = \"a.html\", escape = \"none\")]",
        ),
    ] {
        let root = port(name, &[clean[0], clean[2], (file, text)]);
        let refused = check_port(&root).unwrap_err();
        assert!(refused.ends_with("is refused in a port"), "{refused}");
    }
    let missing = port("missing", &[clean[0]]);
    assert!(check_port(&missing).unwrap_err().starts_with("src: "));
    let unreadable = port("unreadable", &[clean[0], clean[2]]);
    std::fs::write(unreadable.join("src/bad.rs"), b"\xff").unwrap_or_default();
    assert!(check_port(&unreadable).unwrap_err().contains("bad.rs"));
    // A folder that cannot be listed.
    let closed = port(
        "closed",
        &[clean[0], clean[2], ("templates/shut/a.html", "")],
    );
    let shut = closed.join("templates/shut");
    std::fs::set_permissions(&shut, std::os::unix::fs::PermissionsExt::from_mode(0o0)).unwrap();
    let refused = check_port(&closed);
    std::fs::set_permissions(&shut, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    assert!(refused.unwrap_err().starts_with("templates: "));
}
