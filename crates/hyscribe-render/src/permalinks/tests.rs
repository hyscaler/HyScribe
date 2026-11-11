// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::site::Site;

const HOME: &str = "http://127.0.0.1:8081";

fn post(id: u64, post_type: &str, status: &str, name: &str, date: &str) -> Post {
    Post {
        id,
        post_type: post_type.into(),
        status: status.into(),
        name: name.as_bytes().to_vec(),
        date: date.into(),
        ..Post::default()
    }
}

/// The posts of `links-golden.php`, with the parts their links need.
fn posts() -> Vec<(&'static str, Post, &'static str)> {
    let date = "2026-05-06 07:08:09";
    vec![
        (
            "post 1",
            post(1, "post", "publish", "hello-world", "2026-10-27 08:08:15"),
            "",
        ),
        (
            "page 2",
            post(2, "page", "publish", "sample-page", date),
            "sample-page",
        ),
        (
            "draft page 3",
            post(3, "page", "draft", "privacy-policy", date),
            "privacy-policy",
        ),
        (
            "future post",
            post(7, "post", "future", "later", "2027-01-02 03:04:05"),
            "",
        ),
        (
            "draft post",
            post(8, "post", "draft", "", "2026-01-02 03:04:05"),
            "",
        ),
        (
            "private post",
            post(9, "post", "private", "secret", "2026-12-31 23:59:58"),
            "",
        ),
        (
            "nameless published",
            post(10, "post", "publish", "", date),
            "",
        ),
        (
            "unicode post",
            post(11, "post", "publish", "%e2%82%ac-price", date),
            "",
        ),
        (
            "child page",
            post(12, "page", "publish", "child", date),
            "sample-page/child",
        ),
    ]
}

/// Every setup of `reference/rewrite/golden.sh`, through
/// `links-golden.php`.
#[test]
fn links_match_php() {
    let golden = include_str!("../../tests/golden/pretty-links.tsv");
    let mut lines = 0;
    for setup in golden.split("setup\t").skip(1) {
        let (head, expected) = setup.split_once('\n').unwrap();
        let fields: Vec<&str> = head.split('\t').collect();
        let links = Permalinks::new(fields[0], fields[1], fields[2]);
        let front_page = fields[3].parse().ok();
        let plain = |post: &Post| {
            String::from_utf8(crate::link::permalink(HOME.as_bytes(), front_page, post)).unwrap()
        };
        let parts = |page_uri: &str| PostParts {
            category: "uncategorized".into(),
            author: "admin".into(),
            page_uri: page_uri.into(),
        };
        let link = |post: &Post, page_uri, front| {
            links
                .post_link(HOME, front, post, &parts(page_uri))
                .unwrap_or_else(|| plain(post))
        };
        let all = posts();
        let by_name = |name: &str| all.iter().find(|(n, _, _)| *n == name).unwrap();
        let permalink = |name: &str| {
            let (_, post, uri) = by_name(name);
            link(post, uri, front_page)
        };
        let page_link = |name: &str| {
            let (_, post, uri) = by_name(name);
            link(post, uri, None)
        };
        let term = |taxonomy: &str, slug: &str| links.term_link(HOME, taxonomy, &[slug]).unwrap();
        let mut ours = String::new();
        for line in expected.lines() {
            let (call, _) = line.split_once('\t').unwrap();
            let words: Vec<&str> = call.split(' ').collect();
            let rest = call.split_once(' ').map_or("", |(_, rest)| rest);
            let feed = |at: usize| words.get(at).copied().unwrap_or("rss2");
            let value = match words[0] {
                "permalink" => permalink(rest),
                "post_comments_feed_link" => {
                    let name = format!("{} {}", words[1], words[2]);
                    links.feed_under(&page_link(&name), feed(3))
                }
                "term_link" => match words[1] {
                    "category" => term("category", "uncategorized"),
                    "tag" => term("post_tag", "a-tag"),
                    _ => term("post_format", "post-format-aside"),
                },
                "term_feed_link" => links.feed_under(&term("category", "uncategorized"), feed(3)),
                "year_link" => links.date_link(HOME, 2026, None).unwrap(),
                "month_link" => links.date_link(HOME, 2026, Some((3, None))).unwrap(),
                "day_link" => links.date_link(HOME, 2026, Some((3, Some(7)))).unwrap(),
                "feed_link" => links
                    .feed_link(
                        HOME,
                        words
                            .get(1)
                            .filter(|f| !f.is_empty())
                            .copied()
                            .unwrap_or("rss2"),
                    )
                    .unwrap(),
                "search_link" => links.search_link(HOME, rest.as_bytes()).unwrap(),
                "search_feed_link" => {
                    let (search, feed) = match rest.strip_suffix(" atom") {
                        Some(search) => (search, "atom"),
                        None => (rest, "rss2"),
                    };
                    links
                        .search_feed_link(HOME, search.as_bytes(), feed)
                        .unwrap()
                }
                "search_comments_feed_link" => {
                    let search = rest.strip_suffix(" atom").unwrap();
                    links
                        .search_comments_feed_link(HOME, search.as_bytes(), "atom")
                        .unwrap()
                }
                "author_posts_url" => links.author_link(HOME, "admin").unwrap(),
                "author_feed_link" => {
                    links.feed_under(&links.author_link(HOME, "admin").unwrap(), "rss2")
                }
                "post_embed_url" => {
                    links.under(&permalink(&format!("{} {}", words[1], words[2])), "embed")
                }
                "trackback_url" => links.under(&permalink("post 1"), "trackback"),
                "sitemap_url" => {
                    let subtype = words.get(2).copied().unwrap_or_default();
                    let page = words.get(3).map_or(1, |p| p.parse().unwrap());
                    links.sitemap_url(HOME, words[1], subtype, page).unwrap()
                }
                "rest_url" => links.rest_url(HOME, words[1]).unwrap(),
                "pagenum_link" => links
                    .pagenum_link(HOME, words[1].as_bytes(), words[2].parse().unwrap())
                    .unwrap(),
                other => panic!("{other}"),
            };
            ours.push_str(&format!("{call}\t{value}\n"));
            lines += 1;
        }
        assert_eq!(ours, expected, "{head}");
    }
    assert_eq!(lines, 1024);
}

#[test]
fn plain_permalinks_have_no_pretty_links() {
    let links = Permalinks::default();
    let parts = PostParts::default();
    let page = post(2, "page", "publish", "sample-page", "");
    assert_eq!(
        links.post_link(HOME, Some(2), &page, &parts).unwrap(),
        format!("{HOME}/")
    );
    assert!(links.post_link(HOME, None, &page, &parts).is_none());
    assert!(links.term_link(HOME, "category", &["a"]).is_none());
    assert!(links.date_link(HOME, 2026, None).is_none());
    assert!(links.feed_link(HOME, "rss2").is_none());
    assert!(links.search_feed_link(HOME, b"a", "rss2").is_none());
    assert!(links.author_link(HOME, "a").is_none());
    assert!(links.sitemap_url(HOME, "index", "", 1).is_none());
    assert!(links.rest_url(HOME, "/").is_none());
    assert!(links.pagenum_link(HOME, b"/", 2).is_none());
    // `index.php`'s '.' is any character.
    assert!(Permalinks::new("/indexXphp/%postname%/", "", "").index());
    assert!(!Permalinks::new("/abcde.php/%postname%/", "", "").index());
    assert_eq!(tokens("/%year"), Vec::<&str>::new());
    assert_eq!(tokens("%%y%/%a%"), ["%%y%", "%a%"]);
    assert_eq!(strip_paging("x/page/"), "x/page/");
    assert_eq!(strip_paging("x/page//"), "x/page//");
    // Attachments and types core does not register keep their plain link,
    // as do taxonomies other than core's; only formats lose their prefix.
    let pretty = Permalinks::new("/%postname%/", "", "");
    for post_type in ["attachment", "book"] {
        let post = post(4, post_type, "publish", "x", "");
        assert!(pretty.post_link(HOME, None, &post, &parts).is_none());
    }
    assert!(pretty.term_link(HOME, "genre", &["x"]).is_none());
    assert_eq!(
        pretty
            .term_link(HOME, "category", &["post-format-x"])
            .unwrap(),
        format!("{HOME}/category/post-format-x/")
    );
    assert_eq!(
        pretty.sitemap_url(HOME, "users", "", 0).unwrap(),
        format!("{HOME}/wp-sitemap-users-1.xml")
    );
    let links = Permalinks::new("/%postname%/", "", "");
    let home = "http://example.test/blog";
    assert_eq!(home_path(home), "/blog");
    assert_eq!(home_path("http://example.test"), "");
    assert_eq!(
        links.pagenum_link(home, b"/BLOG/page/3/?a=1", 2).unwrap(),
        "http://example.test/blog/page/2/?a=1"
    );
    assert_eq!(collapse_slashes("//a///b"), "/a/b");
}

/// A site with nested categories and pages, for the walks up their parents.
struct Nested {
    structure: &'static str,
    default_category: &'static str,
    categories: Vec<u64>,
    /// Reads of users and post terms, which only `%author%` and
    /// `%category%` need.
    reads: std::cell::Cell<u32>,
}

fn term(id: u64, slug: &str, parent: u64) -> Term {
    Term {
        id,
        slug: slug.as_bytes().to_vec(),
        parent,
        ..Term::default()
    }
}

impl Site for Nested {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        let value = match name {
            "permalink_structure" => self.structure,
            "default_category" => self.default_category,
            "show_on_front" => "page",
            "page_on_front" => "4",
            _ => return None,
        };
        Some(value.as_bytes().to_vec())
    }

    fn home_url(&self) -> Vec<u8> {
        HOME.as_bytes().to_vec()
    }

    fn post(&self, id: u64) -> Option<Post> {
        let page = |id, name: &str, parent| Post {
            parent,
            ..post(id, "page", "publish", name, "")
        };
        // A loop: "a" sits under "c".
        [
            page(4, "a", 7),
            page(5, "", 4),
            page(6, "b", 5),
            page(7, "c", 6),
        ]
        .into_iter()
        .find(|p| p.id == id)
    }

    fn post_terms(&self, _post_id: u64, _taxonomy: &str) -> Vec<Term> {
        self.reads.set(self.reads.get() + 1);
        self.categories
            .iter()
            .filter_map(|id| self.term(*id, "category"))
            .collect()
    }

    fn post_meta(&self, _post_id: u64, _key: &str) -> Option<Vec<u8>> {
        None
    }

    fn query(&self, _query: &hyscribe_model::PostQuery) -> Vec<Post> {
        Vec::new()
    }

    fn term(&self, id: u64, _taxonomy: &str) -> Option<Term> {
        // A loop: 8 is its own parent's parent.
        [
            term(1, "top", 0),
            term(2, "mid", 1),
            term(3, "leaf", 2),
            term(8, "x", 9),
            term(9, "y", 8),
        ]
        .into_iter()
        .find(|t| t.id == id)
    }

    fn user(&self, id: u64) -> Option<hyscribe_model::User> {
        self.reads.set(self.reads.get() + 1);
        (id == 1).then(|| hyscribe_model::User {
            nicename: b"jo".to_vec(),
            ..hyscribe_model::User::default()
        })
    }
}

#[test]
fn links_walk_up_their_parents() {
    let site = |categories: Vec<u64>, default_category| Nested {
        structure: "/%category%/%author%/%postname%/",
        default_category,
        categories,
        reads: Default::default(),
    };
    let mut entry = post(10, "post", "publish", "entry", "2026-01-02 03:04:05");
    entry.author = 1;
    let link = |site: &Nested, post: &Post| String::from_utf8(permalink(site, post)).unwrap();
    // The category with the lowest ID, with those above it.
    assert_eq!(
        link(&site(vec![9, 3], ""), &entry),
        format!("{HOME}/top/mid/leaf/jo/entry/")
    );
    // A loop in the parents ends the walk; no category takes the default.
    assert_eq!(
        link(&site(vec![8], ""), &entry),
        format!("{HOME}/y/x/jo/entry/")
    );
    // The default category goes by its own slug alone.
    assert_eq!(
        link(&site(vec![], "2"), &entry),
        format!("{HOME}/mid/jo/entry/")
    );
    // `home_url()` drops the empty category's leading slash.
    assert_eq!(
        link(&site(vec![], "99"), &entry),
        format!("{HOME}/jo/entry/")
    );
    entry.author = 2;
    assert_eq!(
        link(&site(vec![1], ""), &entry),
        format!("{HOME}/top//entry/")
    );
    assert_eq!(
        category_paths(&site(vec![3, 1], ""), 10),
        ["top/mid/leaf", "top"]
    );
    // Pages: names from the top, an empty one skipped, a loop ended.
    let nested = site(vec![], "");
    let page = nested.post(7).unwrap();
    assert_eq!(link(&nested, &page), format!("{HOME}/a/b/c/"));
    let leaf = term(3, "leaf", 2);
    assert_eq!(
        String::from_utf8(term_link(&nested, "category", &leaf)).unwrap(),
        format!("{HOME}/category/top/mid/leaf/")
    );
    // Plain permalinks for the post types and taxonomies pretty ones skip.
    let book = post(11, "book", "publish", "b", "");
    assert_eq!(link(&nested, &book), format!("{HOME}/?post_type=book&p=11"));
    assert_eq!(
        String::from_utf8(author_posts_url(&nested, 1)).unwrap(),
        format!("{HOME}/author/jo/")
    );
    assert_eq!(
        String::from_utf8(author_posts_url(&nested, -1)).unwrap(),
        format!("{HOME}/author/")
    );
}

#[test]
fn comment_pages_under_pretty_permalinks() {
    let site = Nested {
        structure: "/%postname%/",
        default_category: "",
        categories: Vec::new(),
        reads: Default::default(),
    };
    let theme = crate::theme::twentytwentyfive();
    let request = crate::site::Request {
        post: Some(7),
        ..crate::site::Request::default()
    };
    let context = crate::core_blocks::Context {
        theme: &theme,
        site: &site,
        request: &request,
        post_id: None,
        query: None,
        comment: None,
        terms: Default::default(),
        loading: Default::default(),
    };
    let entry = post(10, "post", "publish", "entry", "2026-01-02 03:04:05");
    assert_eq!(
        String::from_utf8(permalink(&site, &entry)).unwrap(),
        format!("{HOME}/entry/")
    );
    // Neither the author nor the categories are read for this structure.
    assert_eq!(site.reads.get(), 0);
    let link = crate::comment_pages::comments_pagenum_link(&context, 2, 2);
    assert_eq!(
        String::from_utf8(link).unwrap(),
        format!("{HOME}/a/b/c/comment-page-2/#comments")
    );
}

#[test]
fn pages_of_a_link() {
    let site = |structure| Nested {
        structure,
        default_category: "",
        categories: Vec::new(),
        reads: Default::default(),
    };
    let link = format!("{HOME}/entry/");
    let paged = |structure| String::from_utf8(paged(&site(structure), link.as_bytes(), 2)).unwrap();
    assert_eq!(paged("/%postname%/"), format!("{HOME}/entry/2/"));
    assert_eq!(paged(""), format!("{HOME}/entry/?page=2"));
    // `_wp_link_page()`: a draft's in the query, the page on the front's
    // after `page/`.
    let page = |structure, post: &Post| {
        String::from_utf8(post_page(&site(structure), post, link.as_bytes(), 2)).unwrap()
    };
    let entry = post(10, "post", "publish", "entry", "");
    assert_eq!(page("/%postname%/", &entry), format!("{HOME}/entry/2/"));
    assert_eq!(page("", &entry), format!("{HOME}/entry/?page=2"));
    for status in ["draft", "pending"] {
        let draft = post(10, "post", status, "entry", "");
        assert_eq!(
            page("/%postname%/", &draft),
            format!("{HOME}/entry/?page=2")
        );
    }
    let front = post(4, "page", "publish", "a", "");
    assert_eq!(
        page("/%postname%/", &front),
        format!("{HOME}/entry/page/2/")
    );
}

#[test]
fn plain_term_feed_links() {
    let site = Nested {
        structure: "",
        default_category: "",
        categories: Vec::new(),
        reads: Default::default(),
    };
    let feed = |taxonomy, slug| {
        String::from_utf8(term_feed_link(&site, taxonomy, &term(5, slug, 0), "rss2")).unwrap()
    };
    assert_eq!(
        feed("post_tag", "a"),
        format!("{HOME}/?feed=rss2&amp;tag=a")
    );
    assert_eq!(
        feed("post_format", "post-format-aside"),
        format!("{HOME}/?feed=rss2&amp;post_format=post-format-aside")
    );
}
