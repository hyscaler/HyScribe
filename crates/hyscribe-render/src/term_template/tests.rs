// SPDX-License-Identifier: AGPL-3.0-or-later

use std::cell::RefCell;

use hyscribe_model::{Post, PostQuery, Term, TermOrder, TermQuery};

use crate::fixture::DefaultInstall;
use crate::site::{Archive, Request, Site, Taxonomy};
use crate::template::render_content;
use crate::term_template::{TermArgs, get_terms};
use crate::theme::twentytwentyfive;

const TEMPLATE: &[u8] = b"<!-- wp:terms-query {\"termQuery\":{\"taxonomy\":\"category\",\"hideEmpty\":false}} --><div class=\"wp-block-terms-query\"><!-- wp:term-template --><!-- wp:term-name /--><!-- /wp:term-template --></div><!-- /wp:terms-query -->";

/// A site whose term query finds term 7, which `get_term()` does not.
struct Lost(DefaultInstall);

impl Site for Lost {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.0.option(name)
    }
    fn home_url(&self) -> Vec<u8> {
        self.0.home_url()
    }
    fn post(&self, id: u64) -> Option<Post> {
        self.0.post(id)
    }
    fn post_terms(&self, id: u64, taxonomy: &str) -> Vec<Term> {
        self.0.post_terms(id, taxonomy)
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.0.post_meta(id, key)
    }
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.0.query(query)
    }
    fn term_query(&self, _: &TermQuery) -> Vec<Term> {
        vec![Term {
            id: 7,
            count: 1,
            ..Term::default()
        }]
    }
}

fn render(site: &dyn Site, request: &Request, markup: &[u8]) -> String {
    String::from_utf8(render_content(&twentytwentyfive(), site, request, markup)).unwrap()
}

/// A term the template lists but `get_term()` cannot find renders its
/// item empty; a site without term data lists nothing.
#[test]
fn terms_the_site_lacks() {
    let site = || DefaultInstall {
        home: b"http://s.test".to_vec(),
    };
    let request = Request::default();
    assert_eq!(
        render(&Lost(site()), &request, TEMPLATE),
        "<div class=\"wp-block-terms-query is-layout-flow wp-block-terms-query-is-layout-flow\"><ul class=\"wp-block-term-template is-layout-flow wp-block-term-template-is-layout-flow\"><li class=\"wp-block-term term-7 category taxonomy-category\"></li></ul></div>"
    );
    assert_eq!(
        render(&site(), &request, TEMPLATE),
        "<div class=\"wp-block-terms-query is-layout-flow wp-block-terms-query-is-layout-flow\"></div>"
    );
}

/// Inheriting on a term archive without its term lists nothing, where PHP
/// reads a null queried object; the term blocks on a date archive have no
/// term.
#[test]
fn archives_without_a_term() {
    let site = Lost(DefaultInstall {
        home: b"http://s.test".to_vec(),
    });
    let inherit = b"<!-- wp:terms-query {\"termQuery\":{\"inherit\":true}} --><div><!-- wp:term-template --><!-- wp:term-name /--><!-- /wp:term-template --></div><!-- /wp:terms-query -->";
    let request = Request {
        archive: Some(Archive::Term(Taxonomy::Category, None)),
        ..Request::default()
    };
    assert_eq!(
        render(&site, &request, inherit),
        "<div class=\"is-layout-flow wp-block-terms-query-is-layout-flow\"></div>"
    );
    let request = Request {
        archive: Some(Archive::Year),
        ..Request::default()
    };
    assert_eq!(render(&site, &request, b"<!-- wp:term-name /-->"), "");
}

/// A site that keeps the term query it is asked.
struct Asked(DefaultInstall, RefCell<Option<TermQuery>>);

impl Site for Asked {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.0.option(name)
    }
    fn home_url(&self) -> Vec<u8> {
        self.0.home_url()
    }
    fn post(&self, id: u64) -> Option<Post> {
        self.0.post(id)
    }
    fn post_terms(&self, id: u64, taxonomy: &str) -> Vec<Term> {
        self.0.post_terms(id, taxonomy)
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.0.post_meta(id, key)
    }
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.0.query(query)
    }
    fn term_query(&self, query: &TermQuery) -> Vec<Term> {
        self.1.replace(Some(query.clone()));
        Vec::new()
    }
}

/// `parse_orderby()`'s columns, by the name each comes from.
#[test]
fn orders_by_name() {
    let site = Asked(
        DefaultInstall {
            home: b"http://s.test".to_vec(),
        },
        RefCell::new(None),
    );
    for (name, order) in [
        ("SLUG", Some(TermOrder::Slug)),
        ("term_group", Some(TermOrder::TermGroup)),
        ("taxonomy", Some(TermOrder::Taxonomy)),
        ("term_taxonomy_id", Some(TermOrder::TermTaxonomyId)),
        ("count", Some(TermOrder::Count)),
        ("parent", Some(TermOrder::Parent)),
        ("description", Some(TermOrder::Description)),
        ("Id", Some(TermOrder::TermId)),
        ("none", None),
        ("rand", Some(TermOrder::Name)),
    ] {
        let args = TermArgs {
            taxonomy: "post_tag".into(),
            orderby: name.as_bytes().to_vec(),
            ..TermArgs::default()
        };
        get_terms(&site, &args);
        let asked = site.1.take().unwrap();
        assert_eq!(asked.order.map(|(order, _)| order), order, "{name}");
    }
}

/// A negative parent or `child_of` finds nothing without a query.
#[test]
fn negative_parents_find_nothing() {
    let site = Asked(
        DefaultInstall {
            home: b"http://s.test".to_vec(),
        },
        RefCell::new(None),
    );
    for (parent, child_of) in [(Some(-1), 0), (None, -1)] {
        let args = TermArgs {
            taxonomy: "category".into(),
            parent,
            child_of,
            ..TermArgs::default()
        };
        assert!(get_terms(&site, &args).is_empty());
        assert_eq!(site.1.take(), None);
    }
}
