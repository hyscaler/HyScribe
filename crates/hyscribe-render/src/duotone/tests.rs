// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;
use crate::fixture::DefaultInstall;
use crate::layout::stylesheet;
use crate::site::Request;
use crate::theme::{twentytwentyfive, twentytwentyfive_with};

/// Every case of `reference/duotone/golden.sh`, in the order PHP ran them,
/// the ID counter running on.
#[test]
fn matches_php() {
    let site = DefaultInstall {
        home: b"http://127.0.0.1:8081".to_vec(),
    };
    let request = Request::default();
    let mut theme = twentytwentyfive();
    let (mut counter, mut cases) = (0, 0);
    let mut failures = Vec::new();
    for line in include_str!("../../tests/golden/duotone.tsv").lines() {
        if let Some(user) = line.strip_prefix("user\t") {
            theme = twentytwentyfive_with(&unhex(user), &[]);
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let markup = unhex(fields[1]);
        let mut renderer = Renderer::new(&theme, &site, &request);
        renderer.unique_id = counter;
        let html = renderer.render(&markup);
        let rules = renderer.duotone.rules();
        renderer.assets.rules.extend(rules);
        let ours = [
            html,
            stylesheet(&renderer.assets.rules).into_bytes(),
            renderer.duotone.footer(),
            renderer.duotone.global_styles().into_bytes(),
        ];
        let php = [2, 3, 4, 5].map(|at| unhex(fields[at]));
        counter = renderer.unique_id;
        cases += 1;
        if ours != php {
            let names = ["html", "css", "footer", "presets"];
            let parts: Vec<String> = (0..4)
                .filter(|&at| ours[at] != php[at])
                .map(|at| {
                    format!(
                        "{}\n    php:  {}\n    ours: {}",
                        names[at],
                        String::from_utf8_lossy(&php[at]),
                        String::from_utf8_lossy(&ours[at])
                    )
                })
                .collect();
            failures.push(format!(
                "phase {} {}\n  {}",
                fields[0],
                String::from_utf8_lossy(&markup),
                parts.join("\n  ")
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {cases} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(10)].join("\n")
    );
    assert_eq!(cases, 138);
}

/// Channels as colord reads them, the ones the golden's tables show: hex of
/// three to eight digits, `rgb()` and `hsl()`, and what is no color.
#[test]
fn parses_colors() {
    assert_eq!(parse_color(b"#8c00b7"), Some([140.0, 0.0, 183.0, 1.0]));
    assert_eq!(parse_color(b"#fff8"), Some([255.0, 255.0, 255.0, 0.53]));
    assert_eq!(parse_color(b"#00000080"), Some([0.0, 0.0, 0.0, 0.5]));
    assert_eq!(
        parse_color(b"rgb(10%, 20%, 30%)"),
        Some([25.5, 51.0, 76.5, 1.0])
    );
    assert_eq!(
        parse_color(b"rgb(1 2 3 / 25%)"),
        Some([1.0, 2.0, 3.0, 0.25])
    );
    assert_eq!(
        parse_color(b"hsl(120, 100%, 25%)"),
        Some([0.0, 127.5, 0.0, 1.0])
    );
    // A number may start with its dot, and a dot with no digit after is no number.
    assert_eq!(parse_color(b"rgb(.5, +2, -3)"), Some([0.5, 2.0, 0.0, 1.0]));
    // A newline may end the pattern, nothing else may follow it.
    assert_eq!(parse_color(b"#fff\n"), Some([255.0, 255.0, 255.0, 1.0]));
    assert_eq!(parse_color(b"rgb(1,2,3)\n"), Some([1.0, 2.0, 3.0, 1.0]));
    // An angle too large to be one is none.
    let huge = format!("hsl({}, 50%, 50%)", "9".repeat(400));
    assert_eq!(
        parse_color(huge.as_bytes()),
        parse_color(b"hsl(0, 50%, 50%)")
    );
    for no_color in [
        "red",
        "rgb(1,2,3)\n\n",
        "rgb(1,2,3)x",
        "rgb(, 2, 3)",
        "rgb(1, , 3)",
        "rgb(1 2,3)",
        "rgb(1 2-3)",
        "rgb(1.,2,3)",
        "rgbx(1,2,3)",
        "rgb 1,2,3",
        "rgb(1,2,3",
        "rgb(1,2,3,)",
        "rgb(1,2,3, 0.5",
        "hsl(10, 50, 50%)",
        "hsl(10, 50%, 50)",
        "hsl(10 50% 50%, 1)",
        "#12",
        "#123456789",
        "#12g456",
        "#12345",
        "#",
        "rgb(1,2)",
        "rgb(1%, 2, 3)",
        "",
    ] {
        assert_eq!(parse_color(no_color.as_bytes()), None, "{no_color}");
    }
}
