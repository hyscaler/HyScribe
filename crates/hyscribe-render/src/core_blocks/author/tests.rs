// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_model::{Post, PostQuery, Term, User};
use hyscribe_testkit::unhex;

use crate::blocks::parse_blocks;
use crate::core_blocks::render;
use crate::fixture::DefaultInstall;
use crate::site::{Request, Site};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// The default install, its admin with a biography.
struct Described {
    site: DefaultInstall,
    description: Vec<u8>,
}

impl Site for Described {
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

    fn user(&self, id: u64) -> Option<User> {
        let user = self.site.user(id)?;
        Some(User {
            description: self.description.clone(),
            ..user
        })
    }
}

/// Every case of `reference/post-author/golden.sh`, through the render
/// callbacks alone, as the oracle leaves out the elements support.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../../tests/golden/post-author.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (markup, post, description, php) = (
            unhex(fields[0]),
            fields[1].parse().unwrap(),
            unhex(fields[2]),
            unhex(fields[3]),
        );
        let site = Described {
            site: DefaultInstall {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            description,
        };
        let request = Request {
            post: Some(post),
            ..Request::default()
        };
        let context = Renderer::new(&theme, &site, &request).context;
        let block = &parse_blocks(&markup)[0];
        let name = block.name.as_deref().unwrap();
        let ours = render(
            &context,
            name,
            block.attrs.as_deref().unwrap_or_default(),
            b"",
        )
        .unwrap();
        if ours != php {
            failures.push(format!(
                "{} {post}\n  php:  {}\n  ours: {}",
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
    assert_eq!(golden.lines().count(), 96);
}

/// Posts by user 1 of other types than the default install has.
struct Typed;

impl Site for Typed {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        None
    }

    fn home_url(&self) -> Vec<u8> {
        Vec::new()
    }

    fn post(&self, id: u64) -> Option<Post> {
        let post_type = match id {
            1 => "attachment",
            2 => "revision",
            _ => "wp_block",
        };
        Some(Post {
            id,
            author: 1,
            post_type: post_type.into(),
            ..Post::default()
        })
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

    fn user(&self, id: u64) -> Option<User> {
        Some(User {
            id,
            display_name: b"admin".to_vec(),
            ..User::default()
        })
    }
}

/// `post_type_supports( $type, 'author' )`: attachments and revisions
/// have authors, reusable blocks do not; with no post, nothing shows.
#[test]
fn post_types_and_no_post() {
    let theme = twentytwentyfive();
    let shows = |post| {
        let request = Request {
            post,
            ..Request::default()
        };
        let context = Renderer::new(&theme, &Typed, &request).context;
        let author = render(&context, "core/post-author", &[], b"").unwrap();
        let biography = render(&context, "core/post-author-biography", &[], b"").unwrap();
        assert!(biography.is_empty());
        !author.is_empty()
    };
    assert_eq!(
        [shows(Some(1)), shows(Some(2)), shows(Some(3)), shows(None)],
        [true, true, false, false]
    );
}

/// A post author block that hides its avatar still gets it, as PHP does,
/// which counts as one of the three page images that are not lazy loaded:
/// after three such blocks the next avatar loads lazily.
#[test]
fn a_hidden_avatar_counts_as_a_page_image() {
    let theme = twentytwentyfive();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let request = Request {
        post: Some(1),
        singular: true,
        ..Request::default()
    };
    let hidden = r#"<!-- wp:post-author {"showAvatar":false} /-->"#;
    let shown = r#"<!-- wp:post-author {"avatarSize":24} /-->"#;
    let page = |hidden_authors: usize| {
        let markup = format!("{}{shown}", hidden.repeat(hidden_authors));
        let ours = Renderer::new(&theme, &site, &request).render(markup.as_bytes());
        String::from_utf8(ours).unwrap()
    };
    assert!(page(0).contains("<img"));
    assert!(!page(0).contains("loading='lazy'"));
    assert!(!page(2).contains("loading='lazy'"));
    assert!(page(3).contains("loading='lazy'"));
}
