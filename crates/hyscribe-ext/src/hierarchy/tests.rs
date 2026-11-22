// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

fn file(name: &str) -> TemplateFile {
    TemplateFile {
        file: name.into(),
        name: None,
        post_types: Vec::new(),
    }
}

#[test]
fn the_first_candidate_the_theme_has() {
    let files = ["index.php", "single.php", "templates/wide.php", "page"].map(file);
    let asked = |names: &[&str]| {
        let names: Vec<String> = names.iter().map(|name| String::from(*name)).collect();
        locate(&names, &files).map(String::from)
    };
    let single = asked(&["single-post", "single", "index"]);
    assert_eq!(single.as_deref(), Some("single.php"));
    let wide = asked(&["templates/wide", "page", "index"]);
    assert_eq!(wide.as_deref(), Some("templates/wide.php"));
    let page = asked(&["page", "singular", "index"]);
    assert_eq!(page.as_deref(), Some("index.php"));
    assert_eq!(asked(&["archive"]), None);
}
