// SPDX-License-Identifier: AGPL-3.0-or-later

//! The cost of crossing into a component (ADR 0022, risk R7): run with
//! `cargo test --release -p hyscribe-ext --test cost -- --ignored --nocapture`.

use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyscribe_ext::{Extensions, Fault, Host, Limits, Package, Request, Runtime, Site, installed};
use hyscribe_phpser::Value;

mod guest;

const CALLS: u32 = 10_000;

const MANIFEST: &[u8] = include_bytes!("../guest/hyscribe.ext.toml");

fn each(total: Duration, n: u32) -> String {
    format!("{:.2} us", total.as_secs_f64() * 1e6 / f64::from(n))
}

#[test]
#[ignore = "a measurement, run by hand in release"]
fn the_content_10000_times() -> Result<(), Fault> {
    let host = Host::new()?;
    let folder = tempfile::tempdir().map_err(|e| Fault::Refused(e.to_string()))?;
    let extension = host.load(Package {
        kind: hyscribe_ext::manifest::Kind::Plugin,
        sdk: "0.3",
        slug: "ext",
        wasm: guest::component(),
        grants: guest::grants(),
        limits: Limits::default(),
        folder: folder.path(),
    })?;
    let extensions = Arc::new(Extensions::new(vec![Arc::new(extension)]));
    let site: Arc<dyn Site> = Arc::new(guest::Quiet);

    let started = Instant::now();
    for _ in 0..1_000 {
        let request = Request::new(&host, &extensions, site.clone());
        black_box(request.filter("ext_echo", Value::Null, &[]));
    }
    println!(
        "instance per request: {} each (1,000 requests, one call each)",
        each(started.elapsed(), 1_000)
    );

    // What the embedder does for each request: the active set looked up in
    // the runtime's cache (its files checked, nothing compiled), and a request.
    let plugins = tempfile::tempdir().map_err(|e| Fault::Refused(e.to_string()))?;
    let installed_in = plugins.path().join("hyscribe-test");
    let written = std::fs::create_dir(&installed_in)
        .and_then(|()| std::fs::write(installed_in.join("hyscribe.ext.toml"), MANIFEST))
        .and_then(|()| std::fs::write(installed_in.join("extension.wasm"), guest::component()));
    written.map_err(|e| Fault::Refused(e.to_string()))?;
    let file = "hyscribe-test/hyscribe-test.php";
    let found = installed::find(plugins.path(), file).ok_or(Fault::Disabled)?;
    installed::approve(&found).map_err(|e| Fault::Refused(e.to_string()))?;
    let runtime = Runtime::new(plugins.path(), plugins.path());
    let files = [file.to_string()];
    let started = Instant::now();
    let (first, _) = runtime.request(&files, None, site.clone());
    let loaded = started.elapsed();
    assert!(first.is_some());
    let started = Instant::now();
    for _ in 0..1_000 {
        black_box(runtime.request(&files, None, site.clone()));
    }
    println!(
        "the runtime's active set: loaded once in {:.1} ms, then {} a request (1,000 requests)",
        loaded.as_secs_f64() * 1e3,
        each(started.elapsed(), 1_000)
    );

    for size in [0, 1 << 10, 10 << 10, 100 << 10] {
        let content = "x".repeat(size);
        let started = Instant::now();
        for _ in 0..CALLS {
            black_box(black_box(content.clone()) + "<!-- ext -->");
        }
        let native = started.elapsed();
        let request = Request::new(&host, &extensions, site.clone());
        let started = Instant::now();
        for _ in 0..CALLS {
            let value = Value::String(black_box(content.clone()).into_bytes());
            black_box(request.filter("the_content", value, &[]));
        }
        let wasm = started.elapsed();
        assert_eq!(request.reports(), Vec::new());
        println!(
            "the_content, {size:>6} bytes: native {}, component {}, 10,000 calls {:.1} ms",
            each(native, CALLS),
            each(wasm, CALLS),
            wasm.as_secs_f64() * 1e3
        );
    }

    let request = Request::new(&host, &extensions, site);
    let started = Instant::now();
    for _ in 0..CALLS {
        black_box(request.filter("the_title", Value::String(b"Hello".to_vec()), &[]));
    }
    println!(
        "a hook it has no callback on: {}",
        each(started.elapsed(), CALLS)
    );
    Ok(())
}
