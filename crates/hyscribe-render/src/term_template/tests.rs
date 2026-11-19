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

/// A site that keeps the term query it is asked, answers it with its
/// terms and lists `taxonomy_terms` as its last field.
struct Asked(
    DefaultInstall,
    RefCell<Option<TermQuery>>,
    Vec<Term>,
    Vec<Term>,
);

fn asked(terms: Vec<Term>, all: Vec<Term>) -> Asked {
    Asked(
        DefaultInstall {
            home: b"http://s.test".to_vec(),
        },
        RefCell::new(None),
        terms,
        all,
    )
}

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
        self.2.clone()
    }
    fn taxonomy_terms(&self, _: &str) -> Vec<Term> {
        self.3.clone()
    }
}

/// `parse_orderby()`'s columns, by the name each comes from.
#[test]
fn orders_by_name() {
    let site = asked(Vec::new(), Vec::new());
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
        ("term_order", Some(TermOrder::TermId)),
        ("include", Some(TermOrder::Name)),
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

/// `include` orders by the IDs only when there are some; a `term_order`
/// spelled otherwise makes no query, as PHP's fails.
#[test]
fn include_and_term_order_spellings() {
    let site = asked(Vec::new(), Vec::new());
    let args = TermArgs {
        taxonomy: "post_tag".into(),
        orderby: b"include".to_vec(),
        include: vec![1],
        ..TermArgs::default()
    };
    get_terms(&site, &args);
    let asked = site.1.take().unwrap();
    assert_eq!(
        asked.order.map(|(order, _)| order),
        Some(TermOrder::Include)
    );
    let args = TermArgs {
        orderby: b"Term_Order".to_vec(),
        ..args
    };
    assert!(get_terms(&site, &args).is_empty());
    assert!(site.1.take().is_none());
}

/// A negative parent or `child_of` finds nothing without a query.
#[test]
fn negative_parents_find_nothing() {
    let site = asked(Vec::new(), Vec::new());
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

/// The term query the terms query block `attrs` makes on `archive`, where
/// `WP_Term_Query` would take its arguments from the block.
fn query_of(attrs: &str, archive: Option<Archive>) -> Option<TermQuery> {
    let site = asked(Vec::new(), Vec::new());
    let markup = format!(
        "<!-- wp:terms-query {{\"termQuery\":{attrs}}} --><div><!-- wp:term-template --><!-- wp:term-name /--><!-- /wp:term-template --></div><!-- /wp:terms-query -->"
    );
    let request = Request {
        archive,
        ..Request::default()
    };
    render(&site, &request, markup.as_bytes());
    site.1.take()
}

fn term(id: u64, parent: u64, count: u64) -> Term {
    Term {
        id,
        parent,
        count,
        ..Term::default()
    }
}

/// A tag list's block attributes reach the query: its taxonomy, order,
/// `perPage` as a limit and `hideEmpty` as a count test.
#[test]
fn tag_blocks_ask_for_their_arguments() {
    let query = query_of(
        r#"{"taxonomy":"post_tag","perPage":3,"order":"asc","orderBy":"slug","hideEmpty":true}"#,
        None,
    );
    assert_eq!(
        query,
        Some(TermQuery {
            taxonomy: "post_tag".into(),
            nonempty: true,
            order: Some((TermOrder::Slug, false)),
            limit: Some(3),
            ..TermQuery::default()
        })
    );
    let query = query_of(
        r#"{"taxonomy":"post_tag","order":"DESC","orderBy":"count"}"#,
        None,
    );
    assert_eq!(
        query,
        Some(TermQuery {
            taxonomy: "post_tag".into(),
            order: Some((TermOrder::Count, true)),
            ..TermQuery::default()
        })
    );
}

/// Categories list the top level unless nested, and are cut to `perPage`
/// after the query, not by it; an include list overrides the order and the
/// parent, each ID once and as its absolute value.
#[test]
fn category_blocks_ask_for_the_top_level() {
    let top = query_of(
        r#"{"taxonomy":"category","perPage":2,"hideEmpty":true}"#,
        None,
    )
    .unwrap();
    assert_eq!(top.parent, Some(0));
    assert_eq!((top.limit, top.nonempty), (None, false));
    let nested = query_of(r#"{"taxonomy":"category","showNested":true}"#, None).unwrap();
    assert_eq!(nested.parent, None);
    let included = query_of(
        r#"{"taxonomy":"category","include":[5,"5",-7],"orderBy":"name","order":"desc","showNested":false}"#,
        None,
    )
    .unwrap();
    assert_eq!(included.include, [5, 7]);
    assert_eq!(included.parent, None);
    assert_eq!(included.order, Some((TermOrder::Include, false)));
}

/// Inheriting on a term archive takes its taxonomy, and for a category
/// its children: its direct ones, or all below it when nested.
#[test]
fn inheriting_blocks_ask_for_the_archive_terms() {
    let archive = |taxonomy| Some(Archive::Term(taxonomy, Some(term(4, 0, 1))));
    let own = query_of(
        r#"{"inherit":true,"taxonomy":"post_tag"}"#,
        archive(Taxonomy::Category),
    )
    .unwrap();
    assert_eq!((own.taxonomy.as_str(), own.parent), ("category", Some(4)));
    let nested = query_of(
        r#"{"inherit":true,"showNested":true}"#,
        archive(Taxonomy::Category),
    )
    .unwrap();
    assert_eq!(
        (nested.taxonomy.as_str(), nested.parent),
        ("category", None)
    );
    let tag = query_of(r#"{"inherit":true}"#, archive(Taxonomy::Tag)).unwrap();
    assert_eq!((tag.taxonomy.as_str(), tag.parent), ("post_tag", None));
    // Without `inherit` the block's own taxonomy is read.
    let kept = query_of(r#"{"taxonomy":"post_tag"}"#, archive(Taxonomy::Category)).unwrap();
    assert_eq!(kept.taxonomy, "post_tag");
}

fn list(site: &Asked, args: &TermArgs) -> (Vec<u64>, TermQuery) {
    let terms = get_terms(site, args);
    (terms.iter().map(|t| t.id).collect(), site.1.take().unwrap())
}

/// `child_of` keeps the terms below it, each followed by its own, and a
/// `parent` above 0 overrides it.
#[test]
fn child_of_keeps_the_descendants() {
    let all = vec![term(1, 0, 1), term(2, 1, 1), term(3, 2, 1), term(4, 0, 1)];
    let site = asked(all.clone(), all);
    let args = TermArgs {
        taxonomy: "category".into(),
        child_of: 1,
        ..TermArgs::default()
    };
    assert_eq!(list(&site, &args).0, [2, 3]);
    let zero = TermArgs {
        parent: Some(0),
        ..args.clone()
    };
    assert_eq!(list(&site, &zero).0, [2, 3]);
    let above = TermArgs {
        parent: Some(1),
        ..args
    };
    let (found, query) = list(&site, &above);
    assert_eq!((found, query.parent), (vec![1, 2, 3, 4], Some(1)));
}

/// A loop of parents is followed once.
#[test]
fn child_of_stops_in_a_loop() {
    let all = vec![term(2, 3, 1), term(3, 2, 1), term(4, 3, 1)];
    let site = asked(all.clone(), all);
    let args = TermArgs {
        taxonomy: "category".into(),
        child_of: 3,
        ..TermArgs::default()
    };
    assert_eq!(list(&site, &args).0, [2, 4]);
}

/// Only a hierarchical query is cut to `number` and to its used terms
/// after the query; others leave that to the query.
#[test]
fn hierarchical_queries_are_cut_after_the_query() {
    let all = vec![term(1, 0, 0), term(2, 0, 3), term(3, 0, 0), term(4, 3, 1)];
    let site = asked(all.clone(), all);
    let category = TermArgs {
        taxonomy: "category".into(),
        ..TermArgs::default()
    };
    let cut = TermArgs {
        number: 2,
        ..category.clone()
    };
    assert_eq!(list(&site, &cut).0, [1, 2]);
    assert_eq!(list(&site, &category).0, [1, 2, 3, 4]);
    let used = TermArgs {
        hide_empty: true,
        ..category
    };
    // Term 3 has no posts but a used term below it.
    let (found, query) = list(&site, &used);
    assert_eq!((found, query.nonempty), (vec![2, 3, 4], false));
    let tags = TermArgs {
        taxonomy: "post_tag".into(),
        hide_empty: true,
        number: 1,
        ..TermArgs::default()
    };
    let (found, query) = list(&site, &tags);
    assert_eq!(
        (found, query.nonempty, query.limit),
        (vec![1, 2, 3, 4], true, Some(1))
    );
}

/// The query's own limit and arguments: the limit only where nothing is
/// cut afterwards.
#[test]
fn the_query_limit_and_arguments() {
    let site = asked(Vec::new(), Vec::new());
    let tags = TermArgs {
        taxonomy: "post_tag".into(),
        number: 5,
        include: vec![3],
        ..TermArgs::default()
    };
    let query = list(&site, &tags).1;
    assert_eq!(
        (query.taxonomy.as_str(), query.include, query.limit),
        ("post_tag", vec![3], Some(5))
    );
    for (child_of, parent) in [(1, None), (0, Some(0)), (0, Some(2))] {
        let args = TermArgs {
            child_of,
            parent,
            ..tags.clone()
        };
        let query = list(&site, &args).1;
        assert_eq!(
            (query.limit, query.parent),
            (None, parent.map(i64::unsigned_abs))
        );
    }
    let none = TermArgs { number: 0, ..tags };
    assert_eq!(list(&site, &none).1.limit, None);
}
