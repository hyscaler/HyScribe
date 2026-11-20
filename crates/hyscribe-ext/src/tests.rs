// SPDX-License-Identifier: AGPL-3.0-or-later

use std::time::Instant;

use super::*;

#[path = "../tests/guest/mod.rs"]
mod guest;

use guest::spike;

fn grants(hooks: &[&str]) -> Grants {
    Grants {
        hooks: hooks.iter().map(|hook| (*hook).into()).collect(),
        options: ["spike".into()].into(),
    }
}

fn all() -> Grants {
    grants(&[
        "the_content",
        "spike_trap",
        "spike_loop",
        "spike_grow",
        "spike_late",
        "spike_order",
        "spike_options",
        "spike_cron",
        "spike_event",
        "spike_echo",
    ])
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

#[test]
fn filters_by_priority_in_a_fresh_instance() -> Result<(), Fault> {
    let host = Host::new(Limits::default()).map_err(refused)?;
    let extension = host.load(spike(), all())?;
    let mut running = extension.start(&host)?;
    assert_eq!(
        running.filter("the_content", text("<p>Hi</p>")),
        text("<p>Hi</p><!-- spike -->")
    );
    assert_eq!(running.filter("spike_order", text("")), text("ab"));
    assert_eq!(running.filter("the_title", text("Hi")), text("Hi"));
    assert_eq!(running.fault(), None);
    Ok(())
}

#[test]
fn options_are_checked_against_their_grant() -> Result<(), Fault> {
    let host = Host::new(Limits::default()).map_err(refused)?;
    let extension = host.load(spike(), all())?;
    let round = extension.start(&host)?.filter("spike_options", Value::Null);
    assert_eq!(
        round,
        text(concat!(
            "[Ok(true), Ok(false), Ok(false), Ok(true)] Ok(Some(2)) ",
            "[Ok(true), Ok(false)] Ok(false) Err(\"option siteurl is not in the manifest\")"
        ))
    );
    Ok(())
}

#[test]
fn cron_hooks_are_checked_against_their_grant() -> Result<(), Fault> {
    let host = Host::new(Limits::default()).map_err(refused)?;
    let extension = host.load(spike(), all())?;
    let round = extension.start(&host)?.filter("spike_cron", Value::Null);
    assert_eq!(
        round,
        text(concat!(
            "(Ok(()), Ok(()), Ok(()), Some(50), Some(70), Ok(2), None, Some(70), ",
            "Err(\"wp_version_check is not in the manifest\"))"
        ))
    );
    Ok(())
}

#[test]
fn every_kind_of_value_crosses_both_ways() -> Result<(), Fault> {
    let host = Host::new(Limits::default()).map_err(refused)?;
    let extension = host.load(spike(), all())?;
    let mut running = extension.start(&host)?;
    let values = [
        Value::Null,
        Value::Boolean(true),
        Value::Boolean(false),
        Value::Int(-3),
        Value::Float(1.5),
        text("é"),
        Value::Json("[1]".into()),
    ];
    for value in values {
        assert_eq!(running.filter("spike_echo", value.clone()), value);
    }
    Ok(())
}

#[test]
fn grants_nothing_by_default() -> Result<(), Fault> {
    let host = Host::new(Limits::default()).map_err(refused)?;
    let refused = host.load(spike(), Grants::default()).err();
    assert_eq!(
        refused,
        Some(Fault::Refused("the_content is not in the manifest".into()))
    );
    let refused = host
        .load(spike(), grants(&["the_content", "spike_trap"]))
        .err();
    assert_eq!(
        refused,
        Some(Fault::Refused("spike_loop is not in the manifest".into()))
    );
    Ok(())
}

#[test]
fn adds_hooks_in_init_only() -> Result<(), Fault> {
    let host = Host::new(Limits::default()).map_err(refused)?;
    let extension = host.load(spike(), all())?;
    let mut running = extension.start(&host)?;
    assert_eq!(
        running.filter("spike_late", Value::Null),
        text("hooks are added in init only")
    );
    Ok(())
}

#[test]
fn keeps_the_value_when_the_guest_answers_an_error() -> Result<(), Fault> {
    let host = Host::new(Limits {
        faults: 1,
        ..Limits::default()
    })
    .map_err(refused)?;
    let extension = host.load(spike(), all())?;
    let mut running = extension.start(&host)?;
    assert_eq!(running.filter("the_content", Value::Int(7)), Value::Int(7));
    assert_eq!(
        running.fault(),
        Some(&Fault::Guest(
            "no such callback, or not its argument".into()
        ))
    );
    assert_eq!(
        running.filter("the_content", text("x")),
        text("x<!-- spike -->")
    );
    assert!(!extension.disabled());
    Ok(())
}

#[test]
fn a_trap_ends_the_request_and_enough_disable() -> Result<(), Fault> {
    let host = Host::new(Limits {
        faults: 2,
        ..Limits::default()
    })
    .map_err(refused)?;
    let extension = host.load(spike(), all())?;
    for _ in 0..2 {
        assert!(!extension.disabled());
        let mut running = extension.start(&host)?;
        assert_eq!(running.filter("spike_trap", text("kept")), text("kept"));
        assert!(matches!(running.fault(), Some(Fault::Trap(why)) if why.contains("unreachable")));
        assert_eq!(running.filter("the_content", text("kept")), text("kept"));
    }
    assert!(extension.disabled());
    assert_eq!(extension.start(&host).err(), Some(Fault::Disabled));
    Ok(())
}

#[test]
fn a_loop_is_stopped_at_its_deadline() -> Result<(), Fault> {
    let host = Host::new(Limits {
        ticks: 20,
        ..Limits::default()
    })
    .map_err(refused)?;
    let extension = host.load(spike(), all())?;
    let mut running = extension.start(&host)?;
    let started = Instant::now();
    assert_eq!(running.filter("spike_loop", text("kept")), text("kept"));
    assert_eq!(running.fault(), Some(&Fault::Timeout));
    assert!(started.elapsed() < Duration::from_secs(2));
    Ok(())
}

#[test]
fn memory_stops_at_its_limit() -> Result<(), Fault> {
    let host = Host::new(Limits::default()).map_err(refused)?;
    let extension = host.load(spike(), all())?;
    let mut running = extension.start(&host)?;
    assert_eq!(running.filter("spike_grow", text("kept")), text("kept"));
    assert_eq!(running.fault(), Some(&Fault::Memory));
    let roomy = Host::new(Limits {
        memory: 128 << 20,
        ..Limits::default()
    })
    .map_err(refused)?;
    let extension = roomy.load(spike(), all())?;
    assert_eq!(
        extension.start(&roomy)?.filter("spike_grow", text("")),
        Value::Int(64 << 20)
    );
    Ok(())
}

#[test]
fn refuses_what_is_not_an_extension() -> Result<(), Fault> {
    let stackless = Limits {
        stack: 0,
        ..Limits::default()
    };
    assert!(Host::new(stackless).is_err());
    let host = Host::new(Limits::default()).map_err(refused)?;
    assert!(matches!(
        host.load(b"not wasm", all()),
        Err(Fault::Refused(_))
    ));
    let empty = [0, b'a', b's', b'm', 0x0d, 0, 1, 0];
    assert_eq!(
        host.load(&empty, all()).err(),
        Some(Fault::Refused("no hyscribe:sdk/guest@0.1.0".into()))
    );
    let starved = Host::new(Limits {
        memory: 64 << 10,
        ..Limits::default()
    })
    .map_err(refused)?;
    assert_eq!(starved.load(spike(), all()).err(), Some(Fault::Memory));
    Ok(())
}

#[test]
fn a_guest_that_traps_while_loading_is_refused() -> Result<(), Fault> {
    // A deadline of no ticks interrupts the guest at its first check.
    let hurried = Host::new(Limits {
        ticks: 0,
        ..Limits::default()
    })
    .map_err(refused)?;
    assert_eq!(hurried.load(spike(), all()).err(), Some(Fault::Timeout));
    Ok(())
}

#[test]
fn bounds_tables_and_memory_at_their_limits() {
    let mut state = State {
        grants: Arc::new(Grants::default()),
        initialising: false,
        added: Vec::new(),
        options: BTreeMap::new(),
        events: Vec::new(),
        memory: 1 << 16,
        over: false,
    };
    assert_eq!(state.table_growing(0, 10_000, None).ok(), Some(true));
    assert_eq!(state.table_growing(0, 10_001, None).ok(), Some(false));
    assert_eq!(state.memory_growing(0, 1 << 16, None).ok(), Some(true));
    assert!(!state.over);
    assert_eq!(
        state.memory_growing(0, (1 << 16) + 1, None).ok(),
        Some(false)
    );
    assert!(state.over);
}

#[test]
fn dropping_the_host_stops_its_ticker() -> Result<(), Fault> {
    let host = Host::new(Limits::default()).map_err(refused)?;
    let stop = host.stop.clone();
    drop(host);
    assert_eq!(Arc::strong_count(&stop), 1);
    Ok(())
}
