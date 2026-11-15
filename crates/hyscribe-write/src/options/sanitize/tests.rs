// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_phpser::{Value, decode};
use hyscribe_testkit::unhex;

use super::*;
use crate::tests::{fails_at_each_step, rolled_back};

/// Every value of `reference/settings-saves/sanitize-golden.php`, as PHP
/// sanitizes it over the fixture: what PHP stops on is not ported, and nor
/// is invalid UTF-8 beside other text in the site title.
#[tokio::test]
async fn sanitizes_as_php() {
    let golden = include_str!("../../../tests/golden/sanitize-option.tsv");
    rolled_back(async |write| {
        let mut failures = Vec::new();
        for line in golden.lines() {
            let [option, value, sanitized, error] = line.split('\t').collect::<Vec<_>>()[..] else {
                panic!("{line}");
            };
            let value = decode(&unhex(value)).unwrap();
            let ours = sanitize_option(write, option, value.clone()).await;
            let unported = option == "blogname" && value.as_bytes() == Some(b"caf\xc3\xa9\xff");
            let same = match (sanitized, ours) {
                ("fatal", Err(Error::Unported(_))) => true,
                (_, Err(Error::Unported(_))) => unported,
                (sanitized, Ok(ours)) => {
                    let php = decode(&unhex(sanitized)).unwrap();
                    ours.value == php && ours.error.unwrap_or_default() == error
                }
                (_, Err(err)) => panic!("{option}: {err:?}"),
            };
            if !same {
                failures.push(format!("{option} {value:?}"));
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
        assert_eq!(golden.lines().count(), 195);
    })
    .await;
}

/// The reads on the way: the stored value of a refused one, the language
/// and the roles.
#[tokio::test]
async fn fails_at_each_read() {
    let text = |text: &str| Value::String(text.as_bytes().to_vec());
    for (option, value) in [("home", "x"), ("WPLANG", "de_DE"), ("default_role", "king")] {
        fails_at_each_step(
            async |_| (),
            async |write, ()| sanitize_option(write, option, text(value)).await.map(drop),
        )
        .await;
    }
}

/// A refused address keeps the one the configuration sets, as PHP's
/// `get_option()` gives it through `WP_HOME`.
#[tokio::test]
async fn configured_addresses_are_kept() {
    let config = hyscribe_testkit::fixture_database();
    let writer = hyscribe_db::Writer::open(&config, hyscribe_config::Mode::Primary).await;
    let urls = vec![("home", "http://configured.test".to_string())];
    let writer = writer.unwrap().unwrap().with_urls(urls);
    let mut write = writer.begin().await.unwrap();
    let refused = Value::String(b"x".to_vec());
    let kept = sanitize_option(&mut write, "home", refused).await.unwrap();
    assert_eq!(
        kept.value,
        Value::String(b"http://configured.test".to_vec())
    );
}
