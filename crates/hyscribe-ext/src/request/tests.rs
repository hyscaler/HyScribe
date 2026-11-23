// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use std::time::Instant;

use crate::Limits;
use crate::tests::{TestSite, load};

fn spend(request: &Request, spent: Duration) {
    if let Some(ledger) = request.0.ledgers.first() {
        lock(ledger).spent = spent;
    }
}

fn faults(request: &Request) -> Vec<Fault> {
    request
        .reports()
        .into_iter()
        .map(|report| report.fault)
        .collect()
}

#[test]
fn a_call_gets_what_is_left_of_the_budget() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    assert_eq!(
        request.filter("ext_echo", Value::Int(1), &[]),
        Value::Int(1)
    );
    spend(&request, Duration::from_millis(240));
    let started = Instant::now();
    assert_eq!(
        request.filter("ext_loop", Value::Int(2), &[]),
        Value::Int(2)
    );
    let took = started.elapsed();
    assert!(
        took >= Duration::from_millis(9) && took < Duration::from_millis(45),
        "{took:?}"
    );
    assert_eq!(faults(&request), [Fault::Timeout]);
    Ok(())
}

#[test]
fn a_spent_budget_ends_the_extension_s_request() -> Result<(), Fault> {
    let loaded = load(&["ext"], Limits::default())?;
    let request = loaded.request(&TestSite::new());
    spend(&request, Duration::from_millis(250));
    assert_eq!(
        request.filter("ext_echo", Value::Int(1), &[]),
        Value::Int(1)
    );
    assert_eq!(
        request.filter("ext_echo", Value::Int(2), &[]),
        Value::Int(2)
    );
    assert_eq!(faults(&request), [Fault::Timeout]);
    Ok(())
}

#[test]
fn the_subsystems_hook_points_reach_the_extensions() -> Result<(), Fault> {
    use hyscribe_model::hooks::{Dispatch, apply_filters, do_action, scoped};
    let loaded = load(&["ext"], Limits::default())?;
    let site = TestSite::new();
    let dispatch: Arc<dyn Dispatch> = Arc::new(loaded.request(&site));
    assert!(dispatch.attached("the_content"));
    assert!(!dispatch.attached("the_title"));
    let (content, order, title) = scoped(&dispatch, || {
        do_action("ext_action", &[&"w".to_string(), &2_i64]);
        (
            apply_filters("the_content", String::from("<p>x</p>"), &[]),
            apply_filters("ext_order", b"".to_vec(), &[]),
            apply_filters("the_title", String::from("Hi"), &[]),
        )
    });
    assert_eq!(content, "<p>x</p><!-- ext -->");
    assert_eq!(order, b"ab");
    assert_eq!(title, "Hi");
    assert_eq!(site.logged(), ["ext:seen"]);
    assert_eq!(apply_filters("the_content", String::from("x"), &[]), "x");
    Ok(())
}
