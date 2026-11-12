// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::replace_selected_rich_text;

/// Every case of `reference/bindings/rich-text-golden.php`.
#[test]
fn rich_text_matches_php() {
    let golden = include_str!("../../tests/golden/bindings-rich-text.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        let [content, selectors, text, offsets, php] = fields[..] else {
            panic!("malformed line: {line}");
        };
        let offsets: Vec<usize> = offsets
            .split(',')
            .filter(|o| !o.is_empty())
            .map(|o| o.parse().unwrap())
            .collect();
        let (content, selectors, text, php) =
            (unhex(content), unhex(selectors), unhex(text), unhex(php));
        let selectors = String::from_utf8(selectors).unwrap();
        let ours = replace_selected_rich_text(&content, &selectors, &text, &offsets);
        if ours != php {
            failures.push(format!(
                "{} / {selectors} / {offsets:?}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&content),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(golden.lines().count(), 747);
}

/// The default install with the posts, meta and terms
/// `reference/bindings/golden.sh` inserted and rolled back.
struct Inserted {
    install: crate::fixture::DefaultInstall,
    posts: Vec<hyscribe_model::Post>,
    meta: Vec<(u64, String, Vec<u8>)>,
    terms: Vec<(String, hyscribe_model::Term)>,
}

impl crate::site::Site for Inserted {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.install.option(name)
    }
    fn home_url(&self) -> Vec<u8> {
        self.install.home_url()
    }
    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        let inserted = self.posts.iter().find(|post| post.id == id).cloned();
        inserted.or_else(|| self.install.post(id))
    }
    fn post_terms(&self, post_id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        self.install.post_terms(post_id, taxonomy)
    }
    fn post_meta(&self, post_id: u64, key: &str) -> Option<Vec<u8>> {
        self.meta
            .iter()
            .find(|(id, k, _)| *id == post_id && k == key)
            .map(|(_, _, value)| value.clone())
    }
    fn query(&self, query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
        self.install.query(query)
    }
    fn term(&self, id: u64, taxonomy: &str) -> Option<hyscribe_model::Term> {
        self.terms
            .iter()
            .find(|(t, term)| term.id == id && t == taxonomy)
            .map(|(_, term)| term.clone())
    }
}

fn text(hex: &str) -> String {
    String::from_utf8(unhex(hex)).unwrap()
}

/// Every case of `reference/bindings/golden.sh`.
#[test]
fn bindings_match_php() {
    use hyscribe_phpser::{Value, json_decode, json_encode};

    use crate::site::Request;
    use crate::template::Renderer;
    use crate::theme::twentytwentyfive;

    let golden = include_str!("../../tests/golden/bindings.tsv");
    let mut site = Inserted {
        install: crate::fixture::DefaultInstall {
            home: b"http://127.0.0.1:8081".to_vec(),
        },
        posts: Vec::new(),
        meta: Vec::new(),
        terms: Vec::new(),
    };
    let lines: Vec<Vec<&str>> = golden.lines().map(|l| l.split('\t').collect()).collect();
    for f in &lines {
        match f[0] {
            "post" => site.posts.push(hyscribe_model::Post {
                id: f[1].parse().unwrap(),
                post_type: f[2].into(),
                status: f[3].into(),
                password: unhex(f[4]),
                content: unhex(f[5]),
                date: f[6].into(),
                modified: f[7].into(),
                name: unhex(f[8]),
                ..Default::default()
            }),
            "meta" => site
                .meta
                .push((f[1].parse().unwrap(), text(f[2]), unhex(f[3]))),
            "term" => site.terms.push((
                f[2].into(),
                hyscribe_model::Term {
                    id: f[1].parse().unwrap(),
                    name: unhex(f[3]),
                    slug: unhex(f[4]),
                    description: unhex(f[5]),
                    parent: f[6].parse().unwrap(),
                    count: f[7].parse().unwrap(),
                },
            )),
            _ => {}
        }
    }
    let theme = twentytwentyfive();
    let mut failures = Vec::new();
    for f in &lines {
        let (case, php, ours) = match f[0] {
            "render" => {
                let id: u64 = f[1].parse().unwrap();
                let request = Request {
                    post: (id > 0).then_some(id),
                    ..Default::default()
                };
                let ours = Renderer::new(&theme, &site, &request).render(&unhex(f[2]));
                (text(f[2]), unhex(f[3]), ours)
            }
            "source" => {
                let request = Request::default();
                let mut renderer = Renderer::new(&theme, &site, &request);
                let instance = json_decode(&unhex(f[2])).unwrap();
                let name =
                    String::from_utf8(instance.get_str("name").unwrap().to_php_string()).unwrap();
                let attributes = instance
                    .get_str("attributes")
                    .cloned()
                    .unwrap_or(Value::Null);
                renderer.context.terms.term = instance.get_str("context").and_then(|c| {
                    let taxonomy = c.get_str("taxonomy")?.to_php_string();
                    let id = u64::try_from(c.get_str("termId")?.to_int()).ok()?;
                    Some((String::from_utf8(taxonomy).unwrap(), id))
                });
                let args = json_decode(&unhex(f[3])).unwrap();
                let source = unhex(f[1]);
                let value = super::source_value(
                    &renderer.context,
                    &source,
                    &name,
                    &attributes,
                    None,
                    &args,
                    "content",
                );
                let ours = json_encode(&value.unwrap_or(Value::Null)).unwrap_or_default();
                (
                    format!("{} {} {}", text(f[1]), text(f[2]), text(f[3])),
                    unhex(f[4]),
                    ours,
                )
            }
            _ => continue,
        };
        if ours != php {
            failures.push(format!(
                "{case}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(lines.len(), 125);
}
