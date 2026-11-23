// SPDX-License-Identifier: AGPL-3.0-or-later

//! The buffer's own logic; `hyscribe-http`'s tests check the head and
//! footer against PHP's on the default install's pages
//! (`reference/classic-head`).

use super::*;

fn link(handle: &str) -> Link {
    Link {
        handle: handle.into(),
        href: format!("http://x/{handle}.css"),
        version: None,
    }
}

fn enqueued(late: &[u8]) -> Mutex<Enqueued> {
    Mutex::new(Enqueued {
        global: b"<global>".to_vec(),
        late: late.to_vec(),
        ..Enqueued::default()
    })
}

#[test]
fn the_buffer_fills_the_hole_and_puts_late_styles_before_the_head_ends() {
    let page = b"<head>\n<!-- h -->\n<title>t</title></HEAD\n><body></head>";
    let hoisted = hoist(page, &enqueued(b"<late>"), "<!-- h -->");
    assert_eq!(
        hoisted,
        b"<head>\n<global>\n<title>t</title><late>\n</HEAD\n><body></head>"
    );
    // No late styles: the hole only.
    let hoisted = hoist(page, &enqueued(b""), "<!-- h -->");
    assert_eq!(
        hoisted,
        b"<head>\n<global>\n<title>t</title></HEAD\n><body></head>"
    );
    // A page whose head printed no hole, or that has no end of its head.
    assert_eq!(hoist(page, &enqueued(b"<late>"), "<!-- x -->"), page);
    let hoisted = hoist(b"<!-- h --></header>", &enqueued(b"<late>"), "<!-- h -->");
    assert_eq!(hoisted, b"<global></header>");
}

#[test]
fn the_end_of_the_head_is_a_closer_in_any_case() {
    for (html, at) in [
        (&b"</head>"[..], Some(0)),
        (b"x</Head >", Some(1)),
        (b"</head/>", Some(0)),
        (b"</heading></head\t>", Some(10)),
        (b"</head", None),
        (b"<head>", None),
    ] {
        assert_eq!(head_end(html), at, "{html:?}");
    }
}

#[test]
fn a_handle_is_enqueued_once() {
    let mut enqueued = Enqueued::default();
    enqueued.enqueue_style(link("a"));
    enqueued.enqueue_style(link("a"));
    enqueued.enqueue_script("comment-reply");
    enqueued.enqueue_script("comment-reply");
    assert_eq!(enqueued.styles, [Queued::Link(link("a"))]);
    assert_eq!(enqueued.assets().scripts, ["comment-reply"]);
}

/// A site that writes nothing makes no lookup.
#[test]
fn a_site_that_writes_nothing_keeps_no_lookup() {
    let installed = crate::fixture::DefaultInstall { home: Vec::new() };
    installed.custom_css_post_lookup("hello-elementor");
}
