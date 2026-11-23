// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{OrderBy, Post, PostQuery, Term};
use hyscribe_testkit::unhex;

use super::*;
use crate::fixture::DefaultInstall;
use crate::site::{Request, Site, Taxonomy};
use crate::theme::twentytwentyfive;

/// The fixture with `golden.php`'s in-memory state: its page tree as the
/// pages, with 101 as `page_on_front`, or a menu post the fallback finds.
struct Tree<'a> {
    base: &'a DefaultInstall,
    pages: Vec<Post>,
    menu: Option<Post>,
}

impl Tree<'_> {
    fn new(base: &DefaultInstall) -> Tree<'_> {
        let page = |id, parent, menu_order, title: &str| Post {
            id,
            parent,
            menu_order,
            title: title.as_bytes().to_vec(),
            post_type: "page".into(),
            status: "publish".into(),
            name: format!("page-{id}").into_bytes(),
            ..Post::default()
        };
        let pages = vec![
            page(101, 0, 0, "Alpha"),
            page(102, 101, 0, "Child"),
            page(103, 102, 0, "Grandchild"),
            page(104, 0, 1, "0"),
            page(105, 999, 0, "Orphan"),
            page(106, 0, 2, "<b>Bold</b> & co"),
            page(107, 101, 1, "Second child"),
        ];
        Tree {
            base,
            pages,
            menu: None,
        }
    }

    fn with_menu<'b>(base: &'b DefaultInstall, content: &[u8]) -> Tree<'b> {
        let menu = Post {
            id: 900,
            title: b"Menu".to_vec(),
            content: content.to_vec(),
            post_type: "wp_navigation".into(),
            status: "publish".into(),
            ..Post::default()
        };
        Tree {
            base,
            pages: Vec::new(),
            menu: Some(menu),
        }
    }
}

impl Site for Tree<'_> {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "page_on_front" if self.menu.is_none() => Some(b"101".to_vec()),
            _ => self.base.option(name),
        }
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<Post> {
        // The menu, and a draft of it as 901.
        let draft = self.menu.iter().map(|menu| Post {
            id: 901,
            status: "draft".into(),
            ..menu.clone()
        });
        let menus = self.menu.iter().cloned().chain(draft);
        let page = self
            .pages
            .iter()
            .cloned()
            .chain(menus)
            .find(|post| post.id == id);
        page.or_else(|| self.base.post(id))
    }
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        self.base.post_terms(post_id, taxonomy)
    }
    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.base.post_meta(post_id, key)
    }
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        match (
            &self.menu,
            query.order.first() == Some(&(OrderBy::MenuOrder, true)),
        ) {
            (Some(menu), _) if query.post_types == ["wp_navigation"] => vec![menu.clone()]
                .into_iter()
                .take(query.limit as usize)
                .collect(),
            (None, true) => self.pages.clone(),
            _ => self.base.query(query),
        }
    }
}

/// Every case of `reference/navigation/golden.sh`.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../tests/golden/navigation.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let (markup, counter, queried, pages, php) = (
            unhex(fields[0]),
            fields[1],
            fields[2],
            fields[3],
            unhex(fields[4]),
        );
        let mut request = Request::default();
        if let Some((kind, id)) = queried.split_once(':') {
            request.queried_object_id = id.parse().unwrap();
            if kind == "term" {
                request.archive = Some(Archive::Term(
                    Taxonomy::Category,
                    Some(Term {
                        id: request.queried_object_id,
                        name: "Uncategorized".into(),
                        slug: "uncategorized".into(),
                        ..Term::default()
                    }),
                ));
            }
        }
        // With `menu-global`, the menu is the global post.
        request.post = (pages == "menu-global").then_some(900);
        let tree = match pages {
            "tree" => Some(Tree::new(&site)),
            "menu-links" | "menu-global" => Some(Tree::with_menu(
                &site,
                b"\n\n<!-- wp:navigation-link {\"label\":\"Menu link\",\"url\":\"/m\"} /-->\n\n",
            )),
            "menu-blank" => Some(Tree::with_menu(&site, b"\n\n")),
            _ => None,
        };
        let site: &dyn Site = match &tree {
            Some(tree) => tree,
            None => &site,
        };
        let mut renderer = Renderer::new(&theme, site, &request);
        renderer.unique_id = counter.parse().unwrap();
        let ours = renderer.render(&markup);
        if ours != php {
            failures.push(format!(
                "{}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(4)].join("\n")
    );
    assert!(golden.lines().count() > 50);
}

/// `get_post_ancestors()` stops where the parents loop back.
#[test]
fn ancestors_stop_at_a_loop() {
    let base = DefaultInstall { home: Vec::new() };
    let page = |id, parent| Post {
        id,
        parent,
        post_type: "page".into(),
        ..Post::default()
    };
    let site = Tree {
        base: &base,
        pages: vec![
            page(201, 202),
            page(202, 201),
            page(203, 204),
            page(204, 205),
            page(205, 204),
        ],
        menu: None,
    };
    assert_eq!(post_ancestors(&site, &page(201, 202)), [202]);
    assert_eq!(post_ancestors(&site, &page(203, 204)), [204, 205]);
    assert!(post_ancestors(&site, &page(206, 0)).is_empty());
    assert_eq!(post_ancestors(&site, &page(207, 999)), [999]);
}

/// Every case of `reference/home-link/golden.sh`, in its order: the count
/// of navigation names carries over from case to case, as PHP's static
/// does for the request.
#[test]
fn home_links_match_php() {
    let theme = crate::theme::twentytwentyfive();
    let golden = include_str!("../../tests/golden/home-link.tsv");
    let mut names = std::collections::HashMap::new();
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let flag = |i: usize| fields[i] == "1";
        let site = PostsPage {
            site: crate::fixture::DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            posts_page: fields[5].as_bytes().to_vec(),
        };
        let request = crate::site::Request {
            front_page: flag(2),
            home: flag(3),
            queried_object_id: fields[4].parse().unwrap(),
            ..crate::site::Request::default()
        };
        let markup = hyscribe_testkit::unhex(fields[0]);
        let mut renderer = crate::template::Renderer::new(&theme, &site, &request);
        renderer.navigation_names = std::mem::take(&mut names);
        let ours = renderer.render(&markup);
        names = std::mem::take(&mut renderer.navigation_names);
        let php = hyscribe_testkit::unhex(fields[6]);
        if ours != php {
            failures.push(format!(
                "{} {}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                fields[1],
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(4)].join("\n")
    );
    assert_eq!(golden.lines().count(), 90);
}

/// The default install with `page_for_posts` set.
struct PostsPage {
    site: crate::fixture::DefaultInstall,
    posts_page: Vec<u8>,
}

impl crate::site::Site for PostsPage {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "page_for_posts" => Some(self.posts_page.clone()),
            _ => self.site.option(name),
        }
    }

    fn home_url(&self) -> Vec<u8> {
        self.site.home_url()
    }

    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        self.site.post(id)
    }

    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        self.site.post_terms(post_id, taxonomy)
    }

    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.site.post_meta(post_id, key)
    }

    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        self.site.query(query)
    }
}
