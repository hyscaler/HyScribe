// SPDX-License-Identifier: AGPL-3.0-or-later

//! `_embed` answered as PHP answers it for a visitor and each role, over
//! the default install and, by hash, the Theme Test Data fixture.

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
    let (failures, not_served) = super::mismatches(&goldens, true, &|_| false);
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!(not_served, 0);
}
