// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;

use hyscribe_model::{Post, PostQuery, Term};
use hyscribe_phpser::{Value, json_decode, parse_str};
use hyscribe_testkit::unhex;

use crate::site::{Archive, Request, Site, Taxonomy};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// The oracle's posts and terms, with the case's options.
struct Recorded {
    posts: Vec<Post>,
    terms: Vec<(String, Term)>,
    post_terms: Vec<(u64, String, u64)>,
    options: HashMap<String, Vec<u8>>,
}

impl Site for Recorded {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.options.get(name).cloned()
    }
    fn home_url(&self) -> Vec<u8> {
        b"http://127.0.0.1:8081".to_vec()
    }
    fn post(&self, id: u64) -> Option<Post> {
        self.posts.iter().find(|p| p.id == id).cloned()
    }
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        self.post_terms
            .iter()
            .filter(|(id, tax, _)| *id == post_id && tax == taxonomy)
            .filter_map(|(_, tax, term)| self.term(*term, tax))
            .collect()
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn query(&self, _: &PostQuery) -> Vec<Post> {
        Vec::new()
    }
    fn term(&self, id: u64, taxonomy: &str) -> Option<Term> {
        self.terms
            .iter()
            .find(|(tax, t)| t.id == id && tax == taxonomy)
            .map(|(_, t)| t.clone())
    }
}

fn int(v: &Value) -> u64 {
    v.to_int().unsigned_abs()
}

fn at(list: &Value, i: i64) -> &Value {
    list.get(&hyscribe_phpser::Key::Int(i)).unwrap()
}

/// Every case of `reference/breadcrumbs/golden.sh`.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../../tests/golden/breadcrumbs.tsv");
    let mut lines = golden.lines();
    let site = json_decode(&unhex(
        lines.next().unwrap().strip_prefix("site\t").unwrap(),
    ))
    .unwrap();
    let list = |key| site.get_str(key).unwrap().as_array().unwrap().to_vec();
    let posts: Vec<Post> = list("posts")
        .iter()
        .map(|(_, p)| Post {
            id: int(at(p, 0)),
            title: at(p, 1).to_php_string(),
            post_type: String::from_utf8(at(p, 2).to_php_string()).unwrap(),
            parent: int(at(p, 3)),
            name: at(p, 4).to_php_string(),
            status: "publish".into(),
            ..Post::default()
        })
        .collect();
    let terms: Vec<(String, Term)> = list("terms")
        .iter()
        .map(|(_, t)| {
            let term = Term {
                id: int(at(t, 0)),
                name: at(t, 1).to_php_string(),
                slug: at(t, 2).to_php_string(),
                parent: int(at(t, 3)),
                description: Vec::new(),
            };
            (String::from_utf8(at(t, 4).to_php_string()).unwrap(), term)
        })
        .collect();
    let post_terms: Vec<(u64, String, u64)> = list("post_terms")
        .iter()
        .map(|(_, r)| {
            (
                int(at(r, 0)),
                String::from_utf8(at(r, 1).to_php_string()).unwrap(),
                int(at(r, 2)),
            )
        })
        .collect();
    let mut failures = Vec::new();
    let mut count = 0;
    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        let (markup, state, php) = (unhex(fields[1]), unhex(fields[2]), unhex(fields[3]));
        let state = json_decode(&state).unwrap();
        let flag = |key| state.get_str(key).is_some_and(|v| !v.is_empty());
        let options = state
            .get_str("options")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .map(|(k, v)| {
                (
                    String::from_utf8(k.to_php_string()).unwrap(),
                    v.to_php_string(),
                )
            })
            .collect();
        let site = Recorded {
            posts: posts.clone(),
            terms: terms.clone(),
            post_terms: post_terms.clone(),
            options,
        };
        let term = state
            .get_str("term")
            .filter(|t| **t != Value::Null)
            .map(|t| {
                let taxonomy = match at(t, 0).to_php_string().as_slice() {
                    b"category" => Taxonomy::Category,
                    _ => Taxonomy::Tag,
                };
                Archive::Term(taxonomy, site.term(int(at(t, 1)), taxonomy.name()))
            });
        let uri = state.get_str("uri").unwrap().to_php_string();
        let post = int(state.get_str("post").unwrap());
        let request = Request {
            front_page: flag("front"),
            home: flag("home"),
            search: state
                .get_str("search")
                .filter(|s| **s != Value::Null)
                .map(Value::to_php_string),
            not_found: flag("not_found"),
            date: flag("date"),
            archive: term,
            paged: int(state.get_str("paged").unwrap()),
            post: (post != 0).then_some(post),
            vars: state.get_str("vars").unwrap().clone(),
            get: parse_str(uri.splitn(2, |&b| b == b'?').nth(1).unwrap_or_default()),
            uri,
            ..Request::default()
        };
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.context.post_id = request.post;
        let ours = renderer.render(&markup);
        count += 1;
        if ours != php {
            failures.push(format!(
                "{} {}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&request.uri),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(5)].join("\n")
    );
    assert_eq!(count, 224);
}

/// A site of `posts` and categories, the categories on post 1.
fn hand_made(posts: Vec<Post>, categories: Vec<Term>) -> Recorded {
    let post_terms = categories
        .iter()
        .map(|t| (1, "category".to_owned(), t.id))
        .take(1)
        .collect();
    Recorded {
        posts,
        terms: categories
            .into_iter()
            .map(|t| ("category".to_owned(), t))
            .collect(),
        post_terms,
        options: HashMap::new(),
    }
}

fn render(site: &Recorded, request: &Request, post: Option<u64>) -> String {
    let theme = twentytwentyfive();
    let mut renderer = Renderer::new(&theme, site, request);
    renderer.context.post_id = post;
    String::from_utf8(renderer.render(b"<!-- wp:breadcrumbs /-->")).unwrap()
}

/// Ancestor walks stop at a missing parent and at a loop, and a post
/// without terms or a request without a post have none.
#[test]
fn trails_stop_where_parents_do() {
    let page = |id, parent| Post {
        id,
        parent,
        title: format!("P{id}").into_bytes(),
        post_type: "page".into(),
        status: "publish".into(),
        ..Post::default()
    };
    let request = Request::default();
    // 1 under 2 under 3, whose parent 9 is missing.
    let site = hand_made(vec![page(1, 2), page(2, 3), page(3, 9)], vec![]);
    let html = render(&site, &request, Some(1));
    assert_eq!(html.matches("<a href").count(), 3);
    assert!(html.contains("<span aria-current=\"page\">(no title)</span>"));
    // 1 under 2 under 1: each once.
    let site = hand_made(vec![page(1, 2), page(2, 1)], vec![]);
    assert_eq!(render(&site, &request, Some(1)).matches("P2").count(), 1);
    // A page that is its own parent.
    let site = hand_made(vec![page(1, 1)], vec![]);
    assert_eq!(
        render(&site, &request, Some(1)).matches("<a href").count(),
        1
    );
    assert!(render(&site, &request, None).is_empty());
    assert!(render(&site, &request, Some(7)).is_empty());
    // A post without terms: Home and its title.
    let post = Post {
        post_type: "post".into(),
        ..page(1, 0)
    };
    let site = hand_made(vec![post.clone()], vec![]);
    assert_eq!(render(&site, &request, Some(1)).matches("<li>").count(), 2);
    let category = |id, parent| Term {
        id,
        parent,
        name: format!("C{id}").into_bytes(),
        slug: format!("c{id}").into_bytes(),
        description: Vec::new(),
    };
    // Category 4 under 5 under 4 and 8 under itself list themselves as
    // ancestors, as get_ancestors() does; 6 is under a missing 9.
    for (terms, links) in [
        (vec![category(4, 5), category(5, 4)], 4),
        (vec![category(6, 9)], 2),
        (vec![category(8, 8)], 3),
    ] {
        let site = hand_made(vec![post.clone()], terms);
        assert_eq!(
            render(&site, &request, Some(1)).matches("<a href").count(),
            links
        );
    }
}
