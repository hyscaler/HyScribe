// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use crate::fixture::DefaultInstall;
use crate::site::Request;
use crate::template::Renderer;
use crate::theme::twentytwentyfive;

/// Every case of `reference/loginout/golden.sh`. The oracle's `siteurl` is
/// its `WP_SITEURL`, the fixture's the stored option.
#[test]
fn matches_php() {
    let theme = twentytwentyfive();
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let golden = include_str!("../../../tests/golden/loginout.tsv");
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<Vec<u8>> = line.split('\t').map(unhex).collect();
        let request = Request {
            uri: fields[0].clone(),
            ..Request::default()
        };
        let php = String::from_utf8(fields[2].clone()).unwrap().replace(
            "http://127.0.0.1:8081/wp-login.php",
            "http://default-install.test/wp-login.php",
        );
        let ours = Renderer::new(&theme, &site, &request).render(&fields[1]);
        let ours = String::from_utf8(ours).unwrap();
        if ours != php {
            failures.push(format!(
                "{}\n  php:  {php}\n  ours: {ours}",
                String::from_utf8_lossy(&fields[1])
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(golden.lines().count(), 31);
}

/// The current URL takes the home URL's scheme and host, not the path of a
/// home in a folder, which the request URI already holds.
#[test]
fn current_url_uses_the_home_origin() {
    let theme = twentytwentyfive();
    let request = Request {
        uri: b"/blog/?p=1".to_vec(),
        ..Request::default()
    };
    for home in ["https://example.test/blog", "http://ab/blog", "example"] {
        let site = DefaultInstall {
            home: home.as_bytes().to_vec(),
        };
        let html = Renderer::new(&theme, &site, &request).render(b"<!-- wp:loginout /-->");
        let html = String::from_utf8(html).unwrap();
        let origin = home.trim_end_matches("/blog");
        let expected = format!(
            "redirect_to={}%2Fblog%2F%3Fp%3D1",
            origin.replace(':', "%3A").replace('/', "%2F")
        );
        assert!(html.contains(&expected), "{html}");
    }
}
