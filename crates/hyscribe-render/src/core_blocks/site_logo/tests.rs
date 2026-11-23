// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;
use sha2::{Digest, Sha256};

use crate::fixture::ThemeTestData;
use crate::site::{Request, Site};
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// The Theme Test Data fixture with the `site_logo` option given, as the
/// golden's filter sets it.
struct WithLogo {
    base: ThemeTestData,
    logo: Option<Vec<u8>>,
    theme_mods: Option<Vec<u8>>,
    no_alt: bool,
}

impl Site for WithLogo {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        match name {
            "site_logo" => self.logo.clone(),
            "theme_mods_twentytwentyfive" => self.theme_mods.clone(),
            "blogname" if self.no_alt => Some(b"Tom's \"Logo\" <b>&</b> co -- 1".to_vec()),
            _ => self.base.option(name),
        }
    }
    fn home_url(&self) -> Vec<u8> {
        self.base.home_url()
    }
    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        self.base.post(id)
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        match key {
            "_wp_attachment_image_alt" if self.no_alt => None,
            _ => self.base.post_meta(id, key),
        }
    }
    fn post_terms(&self, id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        self.base.post_terms(id, taxonomy)
    }
    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        self.base.query(query)
    }
}

/// Every case of `reference/site-logo/golden.sh`, by hash: the fixture's
/// content is not ours to commit.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../../tests/golden/site-logo.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let (kind, id) = fields[1].split_once(':').unwrap_or(("", fields[1]));
        let site = WithLogo {
            base: ThemeTestData {
                home: b"http://127.0.0.1:8081".to_vec(),
            },
            logo: (id != "none" && kind != "mod").then(|| id.as_bytes().to_vec()),
            theme_mods: (kind == "mod")
                .then(|| format!("a:1:{{s:11:\"custom_logo\";i:{id};}}").into_bytes()),
            no_alt: kind == "noalt",
        };
        let request = Request {
            front_page: kind != "post",
            home: kind != "post",
            ..Request::default()
        };
        let markup = unhex(fields[2]);
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.unique_id = fields[0].parse().unwrap();
        let ours = renderer.render(&markup);
        let hash: String = Sha256::digest(&ours)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if hash != fields[3] {
            // Only names and hashes: the fixture's content must not reach logs.
            failures.push(format!(
                "{} {}: ours {hash}",
                fields[1],
                String::from_utf8_lossy(&markup)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(12)].join("\n")
    );
    assert_eq!(golden.lines().count(), 95);
}

/// `get_custom_logo()` prints no link when the image function gives
/// nothing, as for an image file on a post that is no attachment.
#[test]
fn no_image_is_no_logo() {
    let mut post = crate::fixture::attachment(7, "image/png", "");
    post.post_type = "page".into();
    let site = crate::fixture::Fake {
        options: vec![("site_logo", "7")],
        posts: vec![post],
        meta: vec![(7, "_wp_attached_file", b"a.png".to_vec())],
    };
    let theme = twentytwentyfive();
    let request = Request::default();
    let html = Renderer::new(&theme, &site, &request).render(b"<!-- wp:site-logo /-->");
    assert_eq!(html, b"");
}

/// The unlinking expression keeps text with no whole link as it is.
#[test]
fn unlink_needs_a_whole_link() {
    for (logo, unlinked) in [
        (&b"<A HREF=x>i</a> <a"[..], &b"i <a"[..]),
        (b"<a href=x>i", b"<a href=x>i"),
        (b"<a><a>i</a></a>", b"<a>i</a>"),
        (b"xxxxx<a>i</a>", b"xxxxxi"),
    ] {
        assert_eq!(super::unlink(logo), unlinked);
    }
}
