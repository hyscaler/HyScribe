// SPDX-License-Identifier: AGPL-3.0-or-later

//! `_embed` answered as PHP answers it for a visitor and each role, over
//! the default install and, by hash, the Theme Test Data fixture.

use hyscribe_testkit::{RestCase, rest_cases};

/// A post with a featured image, whose `wp:featuredmedia` link is to the
/// media route, not ported, and term searches over every taxonomy.
fn unported(case: &RestCase) -> bool {
    case.target == "/?rest_route=/wp/v2/posts/1011&_embed" || case.target.contains("type=term")
}

#[test]
fn embedding_answers_as_php() {
    let goldens = [
        (
            super::content::default_install(),
            include_str!("../../tests/golden/embed.tsv"),
            false,
        ),
        (
            super::content::theme_test_data(),
            include_str!("../../tests/golden/embed-ttd.tsv"),
            true,
        ),
    ];
    let (failures, not_served) = super::mismatches(&goldens, true, &unported);
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
    let cases = goldens.iter().flat_map(|(_, golden, _)| rest_cases(golden));
    assert_eq!(not_served, cases.filter(unported).count());
}
