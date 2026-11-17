// SPDX-License-Identifier: AGPL-3.0-or-later

//! Batch requests: `batch.tsv`'s checks and answers that write nothing,
//! the batches that write answered 501 by a site without a writer, and
//! what answers a batch 501 before anything is written.

use super::*;

#[test]
fn batches_are_checked_as_php_checks_them() {
    let golden = include_str!("../../tests/golden/batch.tsv");
    let refused = |case: &RestCase| case.flag("refused");
    let goldens = [(content::default_install(), golden, false)];
    let (failures, not_served) = mismatches(&goldens, true, &refused);
    assert!(
        failures.is_empty(),
        "{} failures\n{}",
        failures.len(),
        failures.join("\n")
    );
    assert_eq!((rest_cases(golden).len(), not_served), (6 * 28, 46));
}

/// `who`'s batch of `requests`, JSON, over `content`.
fn batch(content: &'static Content, who: &str, requests: &str) -> Served {
    let case = RestCase {
        who: who.into(),
        method: "POST".into(),
        target: "/?rest_route=/batch/v1".into(),
        flags: "json".into(),
        status: 0,
        headers: Vec::new(),
        body: Vec::new(),
        sent: format!(r#"{{"requests":[{requests}]}}"#).into_bytes(),
    };
    serve_over(content, &case)
}

/// A request without a path, to a route not ported, or to the widgets PHP
/// would store answers the batch 501; so does a write not ported that its
/// permission check lets through, before anything is written.
#[test]
fn requests_not_ported_answer_the_batch_501() {
    let default = content::default_install();
    for request in [
        r#"{"path":"?x=1"}"#,
        r#"{"path":"/wp/v2/plugins"}"#,
        r#"{"method":"DELETE","path":"/wp/v2/widgets/block-2"}"#,
    ] {
        assert_eq!(batch(default, "visitor", request).status, 501, "{request}");
    }
    static RETRIEVED: std::sync::OnceLock<Content> = std::sync::OnceLock::new();
    let retrieved = RETRIEVED.get_or_init(|| {
        content::default_install_with(include_str!("../../../../reference/rest/widgets.sql"))
    });
    let delete = r#"{"path":"/wp/v2/posts","body":{"title":"x"}},{"method":"DELETE","path":"/wp/v2/widgets/block-2"}"#;
    assert_eq!(batch(retrieved, "administrator", delete).status, 501);
    let refused = batch(retrieved, "visitor", delete);
    assert_eq!(refused.status, 207);
    let body = String::from_utf8_lossy(&refused.body).into_owned();
    assert!(body.contains("rest_cannot_create") && body.contains("rest_cannot_manage_widgets"));
}
