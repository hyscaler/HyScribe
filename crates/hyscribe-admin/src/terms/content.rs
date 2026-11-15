// SPDX-License-Identifier: AGPL-3.0-or-later

//! `reference/terms-list/content.sql` over the default install, as the
//! terms list's test site reads it.

use hyscribe_model::{Term, TermOrder, TermQuery};

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
            term(416, "omega", "omega", "", 413, 0),
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

/// `query` over `terms`, which are by name: a search on names and slugs
/// as MySQL compares them, in ASCII, then the order, offset and limit.
pub(crate) fn query(terms: Vec<Term>, query: &TermQuery) -> Vec<Term> {
    let lower = |bytes: &[u8]| String::from_utf8_lossy(bytes).to_lowercase();
    let mut found: Vec<Term> = terms
        .into_iter()
        .filter(|term| {
            query.search.as_deref().is_none_or(|search| {
                let search = search.to_lowercase();
                lower(&term.name).contains(&search) || lower(&term.slug).contains(&search)
            })
        })
        .collect();
    if let Some((order, descending)) = query.order {
        found.sort_by(|a, b| {
            let order = match order {
                TermOrder::Slug => lower(&a.slug).cmp(&lower(&b.slug)),
                TermOrder::TermId => a.id.cmp(&b.id),
                _ => std::cmp::Ordering::Equal,
            };
            if descending { order.reverse() } else { order }
        });
        if order == TermOrder::Name && descending {
            found.reverse();
        }
    }
    let skip = usize::try_from(query.offset).unwrap();
    let take = query
        .limit
        .map_or(usize::MAX, |n| usize::try_from(n).unwrap());
    found.into_iter().skip(skip).take(take).collect()
}

/// Every term of `taxonomy` once `reference/admin-saves/golden.sh` has
/// renamed tag 801 and added tag 802 and category 803, by name.
pub(crate) fn saved(taxonomy: &str) -> Vec<Term> {
    if taxonomy == "post_tag" {
        return vec![
            term(802, "Fresh tag", "fresh-tag", "About it", 0, 0),
            term(
                801,
                "Quick &amp; named",
                "quick-named",
                "Now <em>described</em>",
                0,
                0,
            ),
        ];
    }
    vec![
        term(803, "Child category", "child-category", "", 1, 0),
        term(1, "Uncategorized", "uncategorized", "", 0, 1),
    ]
}
