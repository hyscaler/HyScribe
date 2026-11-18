// SPDX-License-Identifier: AGPL-3.0-or-later

//! The cost of crossing into a component (ADR 0020, risk R7): run with
//! `cargo test --release -p hyscribe-ext --test cost -- --ignored --nocapture`.

use std::hint::black_box;
use std::time::{Duration, Instant};

use hyscribe_ext::{Fault, Grants, Host, Limits, Value};

const SPIKE: &[u8] = include_bytes!("spike.wasm");
const CALLS: u32 = 10_000;

fn each(total: Duration, n: u32) -> String {
    format!("{:.2} us", total.as_secs_f64() * 1e6 / f64::from(n))
}

#[test]
#[ignore = "a measurement, run by hand in release"]
fn the_content_10000_times() -> Result<(), Fault> {
    let host = Host::new(Limits::default()).map_err(|e| Fault::Refused(e.to_string()))?;
    let grants = [
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
    ];
    let extension = host.load(
        SPIKE,
        Grants {
            hooks: grants.iter().map(|hook| (*hook).into()).collect(),
            ..Grants::default()
        },
    )?;

    let started = Instant::now();
    for _ in 0..1_000 {
        black_box(extension.start(&host)?);
    }
    println!(
        "instance per request: {} each (1,000 starts)",
        each(started.elapsed(), 1_000)
    );

    for size in [0, 1 << 10, 10 << 10, 100 << 10] {
        let content = "x".repeat(size);
        let started = Instant::now();
        for _ in 0..CALLS {
            black_box(black_box(content.clone()) + "<!-- spike -->");
        }
        let native = started.elapsed();
        let mut running = extension.start(&host)?;
        let started = Instant::now();
        for _ in 0..CALLS {
            black_box(running.filter("the_content", Value::Text(black_box(content.clone()))));
        }
        let wasm = started.elapsed();
        assert_eq!(running.fault(), None);
        println!(
            "the_content, {size:>6} bytes: native {}, component {}, 10,000 calls {:.1} ms",
            each(native, CALLS),
            each(wasm, CALLS),
            wasm.as_secs_f64() * 1e3
        );
    }

    let mut running = extension.start(&host)?;
    let started = Instant::now();
    for _ in 0..CALLS {
        black_box(running.filter("the_title", Value::Text("Hello".into())));
    }
    println!(
        "a hook it has no callback on: {}",
        each(started.elapsed(), CALLS)
    );
    Ok(())
}
