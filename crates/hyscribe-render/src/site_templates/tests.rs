// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::Term;
use hyscribe_testkit::unhex;

use super::*;
use crate::fixture::DefaultInstall;
use crate::post::body_class;
use crate::theme::twentytwentyfive;

/// The default install with post 1's and page 2's `_wp_page_template` as
/// given, `None` for none.
struct Templated {
    base: DefaultInstall,
    template: Option<Vec<u8>>,
}

impl Site for Templated {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.base.option(name)
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<Post> {
        self.base.post(id)
    }
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        self.base.post_terms(post_id, taxonomy)
    }
    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        match key {
            "_wp_page_template" => self.template.clone(),
            _ => self.base.post_meta(post_id, key),
        }
    }
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        self.base.query(query)
    }
}

/// Every line of `reference/site-edits/page-templates.php`: the template
/// `_wp_page_template` puts first in a post's and a page's hierarchy, and
/// their body classes.
#[test]
fn page_templates_match_php() {
    let golden = include_str!("../../tests/golden/page-templates.tsv");
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let site = Templated {
            base: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            template: (fields[0] != "-").then(|| unhex(fields[0])),
        };
        let request = site.base.request(match fields[1] {
            "1" => "p=1",
            _ => "page_id=2",
        });
        // PHP's hierarchy by priority, a repeated slug where it is last.
        let php: Vec<String> = fields[2]
            .split(',')
            .map(|slug| {
                slug.trim_end_matches(".php")
                    .trim_end_matches(".html")
                    .into()
            })
            .collect();
        let php: Vec<String> = (0..php.len())
            .filter(|at| !php[at + 1..].contains(&php[*at]))
            .map(|at| php[at].clone())
            .collect();
        // The type's own three, then `singular` and `index`.
        let own: Vec<String> = php[php.len() - 3..]
            .iter()
            .cloned()
            .chain(["singular".into(), "index".into()])
            .collect();
        let ours = with_page_template(&site, &request, &own);
        assert_eq!(ours[..ours.len() - 2], php[..], "{line}");
        let classes = body_class(&site, &request, "twentytwentyfive");
        assert_eq!(String::from_utf8_lossy(&classes), fields[3], "{line}");
    }
    assert_eq!(golden.lines().count(), 36);
}

/// An attachment's page template is not tried.
#[test]
fn attachments_have_no_page_template() {
    let site = Templated {
        base: DefaultInstall { home: Vec::new() },
        template: Some(b"wp-custom-template-landing".to_vec()),
    };
    let mut request = site.base.request("p=1");
    request.posts[0].post_type = "attachment".into();
    let candidates = vec!["single".to_string()];
    assert_eq!(with_page_template(&site, &request, &candidates), candidates);
    request.posts[0].post_type = "post".into();
    request.singular = false;
    assert_eq!(with_page_template(&site, &request, &candidates), candidates);
}

/// Templates and parts saved for themes, newest first, the theme's own
/// with an area term or none.
struct Saved(Vec<(Post, &'static str, Option<&'static str>)>);

fn saved(id: u64, post_type: &str, name: &str, content: &str) -> Post {
    Post {
        id,
        post_type: post_type.into(),
        name: name.into(),
        content: content.into(),
        status: "publish".into(),
        ..Post::default()
    }
}

impl Site for Saved {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn home_url(&self) -> Vec<u8> {
        Vec::new()
    }
    fn post(&self, _: u64) -> Option<Post> {
        None
    }
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<Term> {
        let (_, theme, area) = self.0.iter().find(|(post, ..)| post.id == post_id).unwrap();
        let name = match taxonomy {
            "wp_theme" => Some(*theme),
            _ => *area,
        };
        let term = |name: &str| Term {
            name: name.into(),
            ..Term::default()
        };
        name.map(term).into_iter().collect()
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        assert_eq!(query.limit, u32::MAX);
        let posts = self.0.iter().map(|(post, ..)| post.clone());
        posts
            .filter(|post| query.post_types == [post.post_type.clone()])
            .filter(|post| query.names.iter().any(|name| name.as_bytes() == post.name))
            .collect()
    }
}

fn candidates(slugs: &[&str]) -> Vec<String> {
    slugs.iter().map(|slug| slug.to_string()).collect()
}

/// `resolve_block_template()`: the theme's posts by the candidates'
/// priority over its files, as many as there are slugs, a slug not asked
/// for first; an emptied template shows its file, if there is one.
#[test]
fn saved_templates_take_the_place_of_files() {
    let theme = twentytwentyfive();
    let file = |slug: &str| theme.template(&[slug.into()]).unwrap().1.to_vec();
    let found = |site: &Saved, slugs: &[&str]| block_template(&theme, site, &candidates(slugs));
    let site = Saved(vec![
        (
            saved(5, "wp_template", "archive", ""),
            "twentytwentyfive",
            None,
        ),
        (
            saved(6, "wp_template", "blank", "0"),
            "twentytwentyfive",
            None,
        ),
        (
            saved(1, "wp_template", "index", "new index"),
            "twentytwentyfive",
            None,
        ),
        (
            saved(2, "wp_template", "index", "old index"),
            "TwentyTwentyFive",
            None,
        ),
        (
            saved(3, "wp_template", "single", "single"),
            "twentytwentyfive",
            None,
        ),
        (
            saved(4, "wp_template", "home", "other"),
            "twentytwentyfour",
            None,
        ),
    ]);
    assert_eq!(found(&site, &["index"]), Some(b"new index".to_vec()));
    // Two slugs find the two newest posts, both index templates, which
    // leave the single template to its file.
    let file_single = Some(file("single"));
    assert_eq!(found(&site, &["single", "index", "index"]), file_single);
    // Another theme's template is not this one's.
    assert_eq!(found(&site, &["home", "index"]), Some(file("home")));
    // The slug asked for as `Single` finds `single`, which takes priority.
    assert_eq!(found(&site, &["Single", "home"]), Some(b"single".to_vec()));
    assert_eq!(found(&site, &["archive", "index"]), Some(file("archive")));
    assert_eq!(found(&site, &["blank", "index"]), Some(b"0".to_vec()));
    assert_eq!(found(&site, &["page", "home"]), Some(file("page")));
    assert_eq!(found(&site, &["nothing"]), None);
}

/// `render_block_core_template_part()`: the theme's newest published part,
/// its area `uncategorized` without a term, else the theme's file.
#[test]
fn saved_parts_take_the_place_of_files() {
    let theme = twentytwentyfive();
    let site = Saved(vec![
        (
            saved(1, "wp_template_part", "header", "new"),
            "twentytwentyfive",
            None,
        ),
        (
            saved(2, "wp_template_part", "header", "old"),
            "twentytwentyfive",
            Some("header"),
        ),
        (
            saved(3, "wp_template_part", "footer", "other"),
            "twentytwentyfour",
            Some("footer"),
        ),
        (
            saved(4, "wp_template_part", "mine", "mine"),
            "twentytwentyfive",
            Some("footer"),
        ),
    ]);
    let part = |slug: &[u8]| template_part(&theme, &site, slug);
    assert_eq!(
        part(b"header"),
        Some((b"new".to_vec(), b"uncategorized".to_vec()))
    );
    assert_eq!(part(b"mine"), Some((b"mine".to_vec(), b"footer".to_vec())));
    let (file, area) = theme.part(b"footer").unwrap();
    assert_eq!(part(b"footer"), Some((file.to_vec(), area.to_vec())));
    assert_eq!(part(b"nothing"), None);
}
