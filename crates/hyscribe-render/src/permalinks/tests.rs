// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

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
    assert_eq!(lines, 976);
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
