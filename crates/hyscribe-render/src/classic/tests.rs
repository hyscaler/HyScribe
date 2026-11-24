// SPDX-License-Identifier: AGPL-3.0-or-later

//! The tags' own logic; `hyscribe-http`'s tests check every tag against
//! PHP's answers on the default install's pages (`reference/theme-tags`).

use std::collections::HashMap;

use hyscribe_model::{Post, PostQuery, Term};
use hyscribe_phpser::{Key, parse_str};

use super::*;

/// Options and the template of post 2.
#[derive(Default)]
struct Fixture {
    options: HashMap<&'static str, &'static [u8]>,
    template: Option<&'static [u8]>,
}

impl Site for Fixture {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.options.get(name).map(|value| value.to_vec())
    }

    fn home_url(&self) -> Vec<u8> {
        Vec::new()
    }

    fn post(&self, _: u64) -> Option<Post> {
        None
    }

    fn post_terms(&self, _: u64, _: &str) -> Vec<Term> {
        Vec::new()
    }

    fn post_meta(&self, post: u64, key: &str) -> Option<Vec<u8>> {
        let template = (post == 2 && key == "_wp_page_template").then_some(self.template);
        template.flatten().map(<[u8]>::to_vec)
    }

    fn query(&self, _: &PostQuery) -> Vec<Post> {
        Vec::new()
    }
}

fn post(id: u64, post_type: &str) -> Post {
    Post {
        id,
        post_type: post_type.into(),
        ..Post::default()
    }
}

fn text(s: &str) -> Value {
    Value::String(s.as_bytes().to_vec())
}

fn classic() -> Classic {
    Classic::new(("t", "t"))
}

/// The main loop goes through the posts once, then rewinds and ends.
#[test]
fn the_loop_goes_through_the_main_query() {
    let request = Request {
        posts: vec![post(1, "post"), post(2, "post")],
        ..Request::default()
    };
    let mut page = classic();
    let mut seen = Vec::new();
    while page.have_posts(&request) {
        page.the_post();
        seen.push((page.current, page.in_the_loop()));
    }
    assert_eq!(seen, [(Some(0), true), (Some(1), true)]);
    assert_eq!((page.current, page.in_the_loop()), (None, false));
    page.the_post();
    page.rewind_posts();
    assert_eq!(page.current, None);
    assert!(!page.have_posts(&Request::default()));
}

/// The date archive each query var makes, as `parse_query()` decides.
#[test]
fn date_archives_follow_the_query_vars() {
    let of = |vars: &str| {
        let request = Request {
            date: true,
            vars: parse_str(vars.as_bytes()),
            ..Request::default()
        };
        let dates = Dates::of(&request);
        [dates.year, dates.month, dates.day, dates.time]
    };
    let none = [false; 4];
    assert_eq!(of("m=2026"), [true, false, false, false]);
    assert_eq!(of("m=2026-1"), [true, false, false, false]);
    assert_eq!(of("m=202610"), [false, true, false, false]);
    assert_eq!(of("m=2026102"), [false, true, false, false]);
    assert_eq!(of("m=20261027"), [false, false, true, false]);
    assert_eq!(of("m=202610271"), [false, false, true, false]);
    assert_eq!(of("m=2026102710"), [false, false, false, true]);
    assert_eq!(of("hour=1"), [false, false, false, true]);
    assert_eq!(of("minute=1&day=2"), [false, false, false, true]);
    assert_eq!(of("second=1"), [false, false, false, true]);
    assert_eq!(
        of("day=2&monthnum=10&year=2026"),
        [false, false, true, false]
    );
    assert_eq!(of("monthnum=10&year=2026"), [false, true, false, false]);
    assert_eq!(of("year=2026"), [true, false, false, false]);
    assert_eq!(of("w=3"), none);
    let undated = Request {
        vars: parse_str(b"m=2026"),
        ..Request::default()
    };
    let dates = Dates::of(&undated);
    assert_eq!([dates.year, dates.month, dates.day, dates.time], none);
}

/// `is_singular()`, `is_page_template()` and `is_privacy_policy()` with
/// their arguments.
#[test]
fn conditionals_take_their_arguments() {
    let mut site = Fixture::default();
    site.options.insert("wp_page_for_privacy_policy", b"2");
    let page = Request {
        posts: vec![post(2, "page")],
        singular: true,
        queried_object_id: 2,
        ..Request::default()
    };
    let is = |site: &Fixture, request: &Request, name: &str, args: &[Value]| {
        classic().is(site, request, name, args).unwrap()
    };
    let list = |names: &[&str]| {
        let items = names.iter().enumerate();
        Value::Array(
            items
                .map(|(at, name)| (Key::Int(at as i64), text(name)))
                .collect(),
        )
    };
    assert!(is(&site, &page, "is_singular", &[]));
    assert!(is(&site, &page, "is_singular", &[text("")]));
    assert!(is(&site, &page, "is_singular", &[list(&[])]));
    assert!(is(&site, &page, "is_singular", &[list(&["post", "page"])]));
    assert!(!is(&site, &page, "is_singular", &[list(&["post"])]));
    assert!(!is(&site, &page, "is_singular", &[text("post")]));
    assert!(is(&site, &page, "is_privacy_policy", &[]));
    assert!(!is(&site, &Request::default(), "is_privacy_policy", &[]));
    assert!(!is(&Fixture::default(), &page, "is_privacy_policy", &[]));
    assert!(!is(&site, &page, "is_page_template", &[]));
    assert!(is(&site, &page, "is_page_template", &[text("default")]));
    assert!(is(
        &site,
        &page,
        "is_page_template",
        &[list(&["wide.php", "default"])]
    ));
    assert!(!is(&site, &page, "is_page_template", &[text("wide.php")]));
    site.template = Some(b"wide.php");
    assert!(is(&site, &page, "is_page_template", &[]));
    assert!(is(&site, &page, "is_page_template", &[text("")]));
    assert!(is(&site, &page, "is_page_template", &[text("wide.php")]));
    assert!(is(
        &site,
        &page,
        "is_page_template",
        &[list(&["x", "wide.php"])]
    ));
    assert!(!is(&site, &page, "is_page_template", &[list(&["default"])]));
    assert!(!is(&site, &page, "is_page_template", &[text("default")]));
    assert!(!is(&site, &Request::default(), "is_page_template", &[]));
    assert_eq!(classic().is(&site, &page, "is_none", &[]), Err(Unserved));
}

/// `language_attributes()` for each document type and `html_type`.
#[test]
fn language_attributes_follow_the_document_type() {
    let attributes = |html_type: Option<&'static [u8]>, doctype: Option<&str>| {
        let mut site = Fixture::default();
        if let Some(html_type) = html_type {
            site.options.insert("html_type", html_type);
        }
        let args: Vec<Value> = doctype.map(text).into_iter().collect();
        let (theme, request) = (
            crate::theme::load("twentytwentyfive", None),
            Request::default(),
        );
        let scope = Scope {
            theme: &theme,
            site: &site,
            request: &request,
            supports: &|_| false,
        };
        let printed = classic().print(&scope, ("language_attributes", &args));
        String::from_utf8(printed.unwrap()).unwrap()
    };
    let xhtml: &[u8] = b"application/xhtml+xml";
    assert_eq!(attributes(Some(b"text/html"), None), "lang=\"en-US\"");
    assert_eq!(
        attributes(Some(b"text/html"), Some("xhtml")),
        "lang=\"en-US\" xml:lang=\"en-US\""
    );
    assert_eq!(attributes(Some(b"text/html"), Some("x")), "lang=\"en-US\"");
    assert_eq!(attributes(None, Some("x")), "");
    assert_eq!(attributes(Some(b"0"), Some("html")), "lang=\"en-US\"");
    assert_eq!(
        attributes(Some(xhtml), None),
        "lang=\"en-US\" xml:lang=\"en-US\""
    );
    assert_eq!(attributes(Some(xhtml), Some("x")), "xml:lang=\"en-US\"");
}

/// `body_class( $css_class )`, a menu falling back and the tags not served
/// answer so.
#[test]
fn unported_arguments_are_unserved() {
    let site = Fixture::default();
    let request = Request::default();
    let theme = crate::theme::load("twentytwentyfive", None);
    let scope = Scope {
        theme: &theme,
        site: &site,
        request: &request,
        supports: &|feature| feature == "responsive-embeds",
    };
    let mut page = classic();
    let print = |page: &mut Classic, name: &str, args: &[Value]| page.print(&scope, (name, args));
    assert_eq!(print(&mut page, "body_class", &[text("x")]), Err(Unserved));
    assert_eq!(print(&mut page, "the_title", &[]), Err(Unserved));
    let classes = print(&mut page, "body_class", &[text("")]).unwrap();
    assert_eq!(classes, b"class=\"wp-embed-responsive wp-theme-t\"");
    assert_eq!(page.get(&scope, "the_title", &[]), Err(Unserved));
    // No menu falls back to `wp_page_menu()`, which is not ported.
    assert_eq!(page.get(&scope, "wp_nav_menu", &[]), Err(Unserved));
}

/// `get_pagenum_link()` drops `page/<n>/?` at the end of the request, and
/// only with digits: `page/` with no number after it stays.
#[test]
fn page_numbers_are_dropped_only_with_digits() {
    let without = tags::without_page;
    assert_eq!(without(b"x/page/3/"), b"x/");
    assert_eq!(without(b"x/page/12"), b"x/");
    assert_eq!(without(b"x/page//"), b"x/page//");
    assert_eq!(without(b"x/page/"), b"x/page/");
    assert_eq!(without(b"x/3/"), b"x/3/");
}
