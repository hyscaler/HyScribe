// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::HashMap;

use hyscribe_phpser::json_decode;
use hyscribe_testkit::unhex;

use super::*;
use crate::blocks::parse_blocks;
use crate::theme::twentytwentyfive;

/// The options and home URL one golden case rendered with.
struct Fixture {
    options: HashMap<String, Vec<u8>>,
    home: Vec<u8>,
}

impl Site for Fixture {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.options.get(name).cloned()
    }

    fn home_url(&self) -> Vec<u8> {
        self.home.clone()
    }
}

fn object(json: &[u8]) -> Value {
    json_decode(json).unwrap()
}

/// Every case of `reference/core-blocks/golden.sh`.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let golden = include_str!("../../tests/golden/core-blocks.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let mut fields = line.split('\t').map(unhex);
        let mut field = || fields.next().unwrap();
        let (markup, state, used, php) = (field(), object(&field()), object(&field()), field());
        let flag = |name| state.get_str(name) == Some(&Value::Bool(true));
        let mut options: HashMap<String, Vec<u8>> = ["blogname", "blogdescription"]
            .into_iter()
            .map(|name| {
                (
                    name.to_string(),
                    used.get_str(name).unwrap().to_php_string(),
                )
            })
            .collect();
        let page_for_posts = state.get_str("page_for_posts").unwrap().to_php_string();
        options.insert("page_for_posts".into(), page_for_posts);
        let site = Fixture {
            options,
            home: used.get_str("home").unwrap().to_php_string(),
        };
        let context = Context {
            theme: &theme,
            site: &site,
            request: Request {
                front_page: flag("front_page"),
                home: flag("home"),
                paged: flag("paged"),
                queried_object_id: state.get_str("queried_object_id").unwrap().to_float() as u64,
            },
        };
        let block = &parse_blocks(&markup)[0];
        let ours = render(
            &context,
            block.name.as_deref().unwrap(),
            block.attrs.as_deref().unwrap(),
        )
        .unwrap();
        if ours != php {
            failures.push(format!(
                "{}\n  {}\n  php:  {}\n  ours: {}",
                String::from_utf8_lossy(&markup),
                String::from_utf8_lossy(&unhex(line.split('\t').nth(1).unwrap())),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(6)].join("\n")
    );
    assert!(golden.lines().count() > 300);
}

#[test]
fn blocks_without_a_callback_render_their_saved_markup() {
    let theme = twentytwentyfive();
    let site = Fixture {
        options: HashMap::new(),
        home: Vec::new(),
    };
    let context = Context {
        theme: &theme,
        site: &site,
        request: Request::default(),
    };
    assert_eq!(render(&context, "core/paragraph", &[]), None);
    assert_eq!(render(&context, "core/site-title", &[]), Some(Vec::new()));
}
