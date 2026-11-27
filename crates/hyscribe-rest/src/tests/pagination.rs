// SPDX-License-Identifier: AGPL-3.0-or-later

//! The collections' pagination as PHP answers it, `pagination.tsv`: the
//! totals an `_envelope` holds (numbers, but strings for comments) and the
//! `Link` header of an offset past the largest integer, over the default
//! install with `reference/rest/pagination.sql`.

#[test]
fn pagination_answers_as_php() {
    let goldens = [(
        super::content::default_install_paged(),
        include_str!("../../tests/golden/pagination.tsv"),
        false,
    )];
    let (failures, not_served) = super::mismatches(&goldens, true, &|_| false);
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(not_served, 0);
}
