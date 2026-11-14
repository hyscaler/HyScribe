// SPDX-License-Identifier: AGPL-3.0-or-later

//! `/wp/v2/search` answered as PHP answers it for a visitor and each role,
//! over the default install and, by hash, the Theme Test Data fixture.

use hyscribe_testkit::{RestCase, rest_cases};

/// A term search over more than one taxonomy: any, or none left; a
/// subtype not of the enum is refused first.
fn unported(case: &RestCase) -> bool {
    let one = [
        "subtype=category",
        "subtype=post_tag",
        "subtype=post_format",
    ];
    case.target.contains("type=term") && !one.iter().any(|one| case.target.contains(one))
}

#[test]
fn search_answers_as_php() {
    let goldens = [
        (
            super::content::default_install(),
            include_str!("../../tests/golden/search.tsv"),
            false,
        ),
        (
            super::content::theme_test_data(),
            include_str!("../../tests/golden/search-ttd.tsv"),
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
