// SPDX-License-Identifier: AGPL-3.0-or-later

//! `reference/terms-list/content.sql` over the default install, as the
//! terms list's test site reads it.

use hyscribe_model::Term;

fn term(id: u64, name: &str, slug: &str, description: &str, parent: u64, count: u64) -> Term {
    Term {
        id,
        name: name.as_bytes().to_vec(),
        slug: slug.as_bytes().to_vec(),
        description: description.as_bytes().to_vec(),
        parent,
        count,
    }
}

/// Every term of `taxonomy`, by name as MySQL compares them.
pub(crate) fn terms(taxonomy: &str) -> Vec<Term> {
    if taxonomy == "post_tag" {
        return vec![
            term(411, "alpha", "alpha", "", 0, 1),
            term(412, "beta", "beta", "Second", 0, 0),
            term(414, "delta", "delta", "", 0, 0),
            term(415, "epsilon", "epsilon", "", 0, 0),
            term(413, "gamma", "gamma", "", 0, 3),
        ];
    }
    vec![
        term(
            405,
            "Arts &amp; \"Culture\"",
            "arts-culture",
            "Line one\nLine two",
            0,
            0,
        ),
        term(403, "City", "city", "", 402, 1234),
        term(402, "Local", "local", "", 401, 0),
        term(
            401,
            "News",
            "news",
            "Daily <b>news</b> &amp; views<script>x</script>",
            0,
            2,
        ),
        term(1, "Uncategorized", "uncategorized", "", 0, 1),
        term(407, "\u{dc}nicode", "%c3%bcnicode", "", 0, 0),
        term(404, "World", "world", "", 401, 1),
        term(406, "Zeta", "zeta", "", 499, 0),
    ]
}
