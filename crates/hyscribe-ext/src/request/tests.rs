// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;
use crate::Limits;
use crate::tests::{TestSite, load};

fn spend(request: &Request, spent: Duration) {
    if let Some(slot) = request.0.slots.first() {
        lock(slot).spent = spent;
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
