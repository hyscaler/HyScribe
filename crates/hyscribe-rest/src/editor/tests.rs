// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's answers on the editors' routes, `tests/golden/editor.tsv`
//! from `reference/rest/editor.list`, replayed for every identity.

use hyscribe_testkit::{RestCase, rest_cases, rest_headers};

use crate::tests::serve_case;

/// The identities PHP lets read the editors' registries.
const EDITORS: [&str; 4] = ["administrator", "editor", "author", "contributor"];

/// `case` as `who`.
fn as_who(case: &RestCase, who: &str) -> RestCase {
    RestCase {
        who: who.into(),
        method: case.method.clone(),
        target: case.target.clone(),
        flags: case.flags.clone(),
        status: case.status,
        headers: Vec::new(),
        body: Vec::new(),
        sent: Vec::new(),
    }
}

fn not_ported(case: &RestCase) {
    let served = serve_case(case);
    let what = format!("{} {} {}", case.who, case.method, case.target);
    assert_eq!(served.status, 501, "{what}");
    assert!(
        served.headers.is_empty() && served.body.is_empty(),
        "{what}"
    );
}

/// Every recorded answer is PHP's; a request PHP answers by writing, or
/// was not sent as an identity because it would, is answered 501, as is
/// an inactive theme, which HyScribe does not have.
#[test]
fn answers_match_php() {
    let cases = rest_cases(include_str!("../../tests/golden/editor.tsv"));
    assert_eq!(cases.len(), 319);
    for case in &cases {
        if case.status == 0 {
            EDITORS
                .iter()
                .for_each(|who| not_ported(&as_who(case, who)));
            continue;
        }
        if let Some((_, skipped)) = case.flags.split_once("skip=") {
            skipped
                .split('+')
                .for_each(|who| not_ported(&as_who(case, who)));
        }
        if case.who == "administrator" && case.target == "/?rest_route=/wp/v2/themes/nope" {
            not_ported(case);
            continue;
        }
        let served = serve_case(case);
        let what = format!(
            "{} {} {} {}",
            case.who, case.method, case.target, case.flags
        );
        assert_eq!(served.status, case.status, "{what}");
        let mut php = case.headers.clone();
        php.sort();
        assert_eq!(rest_headers(&served.headers), php, "{what}");
        assert_eq!(
            String::from_utf8_lossy(&served.body),
            String::from_utf8_lossy(&case.body),
            "{what}"
        );
    }
}
