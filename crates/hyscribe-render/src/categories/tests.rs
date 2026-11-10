// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{Post, PostQuery, Term};
use hyscribe_phpser::{Key, Value, json_decode, parse_str};
use hyscribe_testkit::unhex;

use crate::fixture::{DefaultInstall, NoData};
use crate::site::{Archive, Request, Site, Taxonomy};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// The default install with the oracle's terms and term posts.
struct Categorized {
    site: DefaultInstall,
    state: Value,
}

impl Categorized {
    fn taxonomy(&self, taxonomy: &str, part: &str) -> Vec<Value> {
        self.state
            .get_path(&[taxonomy, part])
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .map(|(_, v)| v.clone())
            .collect()
    }
}

fn field(row: &Value, i: i64) -> &Value {
    row.get(&Key::Int(i)).unwrap()
}

fn term(row: &Value) -> (Term, u64) {
    let term = Term {
        id: field(row, 0).to_int() as u64,
        name: field(row, 1).to_php_string(),
        slug: field(row, 2).to_php_string(),
        parent: field(row, 3).to_int() as u64,
        ..Term::default()
    };
    (term, field(row, 4).to_int() as u64)
}

impl Site for Categorized {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.site.option(name)
    }

    fn home_url(&self) -> Vec<u8> {
        self.site.home_url()
    }

    fn post(&self, id: u64) -> Option<Post> {
        self.site.post(id)
    }

    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        self.site.post_terms(post_id, taxonomy)
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.site.post_meta(post_id, key)
    }

    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.site.query(query)
    }

    fn term(&self, id: u64, taxonomy: &str) -> Option<Term> {
        self.taxonomy_terms(taxonomy)
            .into_iter()
            .map(|(t, _)| t)
            .find(|t| t.id == id)
    }

    fn taxonomy_terms(&self, taxonomy: &str) -> Vec<(Term, u64)> {
        self.taxonomy(taxonomy, "terms").iter().map(term).collect()
    }

    fn term_posts(&self, taxonomy: &str) -> Vec<(u64, u64)> {
        let pair = |row: &Value| (field(row, 0).to_int() as u64, field(row, 1).to_int() as u64);
        self.taxonomy(taxonomy, "posts").iter().map(pair).collect()
    }
}

/// Every case of `reference/categories/golden.sh`, in its order: the
/// count of categories blocks, which numbers dropdowns, carries over as
/// PHP's static does.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/categories.tsv");
    let mut blocks = 0;
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (markup, query, php) = (unhex(fields[0]), unhex(fields[1]), unhex(fields[3]));
        let site = Categorized {
            site: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            state: json_decode(&unhex(fields[2])).unwrap(),
        };
        let archive = site
            .state
            .get_str("queried")
            .and_then(Value::as_array)
            .map(|queried| {
                let taxonomy = queried[0].1.to_php_string();
                let kind = if taxonomy == b"post_tag" {
                    Taxonomy::Tag
                } else {
                    Taxonomy::Category
                };
                let id = queried[1].1.to_int() as u64;
                let name = String::from_utf8(taxonomy).unwrap();
                Archive::Term(kind, site.term(id, &name))
            });
        let request = Request {
            archive,
            vars: parse_str(&query),
            ..Request::default()
        };
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.categories_blocks = blocks;
        let ours = renderer.render(&markup);
        blocks = renderer.categories_blocks;
        if ours != php {
            failures.push(format!(
                "{} {}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&query),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(3)].join("\n")
    );
    assert_eq!(golden.lines().count(), 168);
}

/// Terms whose parents are missing or loop, as only inconsistent data
/// has them, with the posts in each.
struct Inconsistent {
    terms: Vec<(Term, u64)>,
    posts: Vec<(u64, u64)>,
}

impl Site for Inconsistent {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        None
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

    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn query(&self, _: &PostQuery) -> Vec<Post> {
        Vec::new()
    }

    fn taxonomy_terms(&self, _: &str) -> Vec<(Term, u64)> {
        self.terms.clone()
    }

    fn term_posts(&self, _: &str) -> Vec<(u64, u64)> {
        self.posts.clone()
    }
}

fn render(site: &dyn Site, attributes: &str) -> String {
    let theme = twentytwentyfive();
    let request = Request::default();
    let markup = format!("<!-- wp:categories {attributes} /-->");
    let html = Renderer::new(&theme, site, &request).render(markup.as_bytes());
    String::from_utf8(html).unwrap()
}

/// `Walker::walk()` with no top level takes the first term's parent as
/// the root; terms under parents not listed come last, flat; a term with
/// no name prints only its closing tag.
#[test]
fn trees_with_missing_parents() {
    let term = |id, name: &str, parent| {
        let term = Term {
            id,
            name: name.as_bytes().to_vec(),
            slug: name.as_bytes().to_vec(),
            parent,
            ..Term::default()
        };
        (term, 1)
    };
    let tree = r#"{"showHierarchy":true}"#;
    let rooted = Inconsistent {
        terms: vec![
            term(10, "a", 99),
            term(11, "b", 99),
            term(12, "c", 10),
            term(16, "f", 10),
        ],
        posts: Vec::new(),
    };
    let html = render(&rooted, tree);
    let (a, b, c) = (
        html.find("cat-item-10").unwrap(),
        html.find("cat-item-11").unwrap(),
        html.find("cat-item-12").unwrap(),
    );
    let f = html.find("cat-item-16").unwrap();
    let levels = html.matches("<ul class='children'>").count();
    assert!(a < c && c < f && f < b && levels == 1, "{html}");
    let orphaned = Inconsistent {
        terms: vec![term(14, "e", 0), term(13, "d", 50), term(15, "", 0)],
        posts: Vec::new(),
    };
    let html = render(&orphaned, tree);
    assert!(html.find("cat-item-14").unwrap() < html.find("cat-item-13").unwrap());
    assert!(
        !html.contains("children") && !html.contains("cat-item-15"),
        "{html}"
    );
    assert_eq!(html.matches("</li>").count(), 3);
}

/// Parents that loop end `_pad_term_counts()`'s and `get_term_children()`'s
/// climbs; the walk shows each term once.
#[test]
fn parents_in_a_loop() {
    let term = |id, parent, count| {
        let term = Term {
            id,
            name: format!("t{id}").into_bytes(),
            slug: format!("t{id}").into_bytes(),
            parent,
            ..Term::default()
        };
        (term, count)
    };
    let site = Inconsistent {
        terms: vec![term(20, 21, 0), term(21, 20, 0)],
        posts: vec![(20, 1)],
    };
    let html = render(&site, r#"{"showHierarchy":true,"showPostCounts":true}"#);
    assert_eq!(html.matches("</a> (1)").count(), 2, "{html}");
    let empty = render(&site, r#"{"showHierarchy":true}"#);
    assert!(empty.contains("cat-item-none"), "{empty}");
}

/// For a taxonomy that is not registered PHP walks the error it gets as
/// terms; HyScribe prints the label and no select.
#[test]
fn unregistered_taxonomy_dropdown() {
    let site = Inconsistent {
        terms: Vec::new(),
        posts: Vec::new(),
    };
    let html = render(&site, r#"{"taxonomy":"nope","displayAsDropdown":true}"#);
    assert!(
        html.ends_with("for=\"wp-block-categories-1\"></label></div>"),
        "{html}"
    );
}

#[test]
fn no_term_data_lists_nothing() {
    let html = render(&NoData, r#"{"showHierarchy":true,"showPostCounts":true}"#);
    assert!(
        html.contains("<li class=\"cat-item-none\">No categories</li>"),
        "{html}"
    );
}

/// With no terms, the dropdown's only option is "none", selected; on a
/// post format archive, its format is the current term.
#[test]
fn empty_dropdowns_and_format_archives() {
    let html = render(&NoData, r#"{"displayAsDropdown":true}"#);
    assert!(
        html.contains("<option value='-1' selected='selected'>Select Category</option>"),
        "{html}"
    );
    let aside = Term {
        id: 30,
        name: b"Aside".to_vec(),
        slug: b"post-format-aside".to_vec(),
        ..Term::default()
    };
    let site = Inconsistent {
        terms: vec![(aside.clone(), 1)],
        posts: Vec::new(),
    };
    let theme = twentytwentyfive();
    let request = Request {
        archive: Some(Archive::Term(Taxonomy::Format, Some(aside))),
        ..Request::default()
    };
    let markup = br#"<!-- wp:categories {"taxonomy":"post_format"} /-->"#;
    let html = String::from_utf8(Renderer::new(&theme, &site, &request).render(markup)).unwrap();
    assert!(html.contains("cat-item-30 current-cat"), "{html}");
}

/// Terms only, with `Site`'s default answer for the posts in them.
struct TermsOnly(Vec<(Term, u64)>);

impl Site for TermsOnly {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        None
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

    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn query(&self, _: &PostQuery) -> Vec<Post> {
        Vec::new()
    }

    fn taxonomy_terms(&self, _: &str) -> Vec<(Term, u64)> {
        self.0.clone()
    }
}

/// An archive of another taxonomy marks nothing current, nor selects in
/// a dropdown of this one, though its term's ID or slug is the same.
#[test]
fn other_taxonomies_archives() {
    let term = |id, slug: &str| Term {
        id,
        name: slug.as_bytes().to_vec(),
        slug: slug.as_bytes().to_vec(),
        ..Term::default()
    };
    let site = TermsOnly(vec![(term(1, "shared"), 1)]);
    let theme = twentytwentyfive();
    let show = |kind, markup: &[u8]| {
        let request = Request {
            archive: Some(Archive::Term(kind, Some(term(1, "shared")))),
            ..Request::default()
        };
        String::from_utf8(Renderer::new(&theme, &site, &request).render(markup)).unwrap()
    };
    let list = show(Taxonomy::Tag, b"<!-- wp:categories /-->");
    assert!(!list.contains("current-cat"), "{list}");
    let tags = br#"<!-- wp:categories {"taxonomy":"post_tag","displayAsDropdown":true} /-->"#;
    assert!(!show(Taxonomy::Category, tags).contains("selected=\"selected\""));
    // Without posts in its terms, `Site` leaves every count as it is.
    let counts = TermsOnly(vec![(term(0, "zero"), 0), (term(1, "one"), 0)]);
    let html = render(
        &counts,
        r#"{"showHierarchy":true,"showPostCounts":true,"showEmpty":true}"#,
    );
    assert_eq!(html.matches("</a> (0)").count(), 2, "{html}");
    // Without term data, nothing to show even with the empty ones.
    assert!(render(&NoData, r#"{"showEmpty":true}"#).contains("No categories"));
}
