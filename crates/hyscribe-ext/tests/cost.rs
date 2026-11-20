// SPDX-License-Identifier: AGPL-3.0-or-later

//! The cost of crossing into a component (ADR 0022, risk R7): run with
//! `cargo test --release -p hyscribe-ext --test cost -- --ignored --nocapture`.

use std::hint::black_box;
use std::sync::Arc;
use std::time::{Duration, Instant};

use hyscribe_ext::{
    Asset, AssetKind, Extensions, Fault, Found, Grants, Host, Limits, Message, ObjectKind, Package,
    Post, Request, Site,
};
use hyscribe_phpser::Value;

mod guest;

const CALLS: u32 = 10_000;

fn each(total: Duration, n: u32) -> String {
    format!("{:.2} us", total.as_secs_f64() * 1e6 / f64::from(n))
}

/// A site the measured filter never calls.
struct Quiet;

impl Site for Quiet {
    fn now(&self) -> u64 {
        0
    }
    fn get_option(&self, _: &str) -> Result<Option<Value>, String> {
        Ok(None)
    }
    fn update_option(&self, _: &str, _: Value, _: Option<bool>) -> Result<bool, String> {
        Ok(false)
    }
    fn add_option(&self, _: &str, _: Value, _: Option<bool>) -> Result<bool, String> {
        Ok(false)
    }
    fn delete_option(&self, _: &str) -> Result<bool, String> {
        Ok(false)
    }
    fn get_meta(&self, _: ObjectKind, _: u64, _: &str) -> Result<Vec<Value>, String> {
        Ok(Vec::new())
    }
    fn add_meta(
        &self,
        _: ObjectKind,
        _: u64,
        _: &str,
        _: Value,
        _: bool,
    ) -> Result<Option<u64>, String> {
        Ok(None)
    }
    fn update_meta(
        &self,
        _: ObjectKind,
        _: u64,
        _: &str,
        _: Value,
        _: Option<Value>,
    ) -> Result<bool, String> {
        Ok(false)
    }
    fn delete_meta(
        &self,
        _: ObjectKind,
        _: u64,
        _: &str,
        _: Option<Value>,
    ) -> Result<bool, String> {
        Ok(false)
    }
    fn public_post_type(&self, _: &str) -> bool {
        false
    }
    fn query(&self, _: &Value) -> Result<Found, String> {
        Ok(Found::default())
    }
    fn post(&self, _: u64) -> Result<Option<Post>, String> {
        Ok(None)
    }
    fn schedule(&self, _: i64, _: Option<&str>, _: &str, _: &[Value]) -> Result<(), String> {
        Ok(())
    }
    fn unschedule(&self, _: &str, _: &[Value]) -> Result<u32, String> {
        Ok(0)
    }
    fn next_scheduled(&self, _: &str, _: &[Value]) -> Option<i64> {
        None
    }
    fn mail(&self, _: &Message) -> Result<bool, String> {
        Ok(false)
    }
    fn current_user(&self) -> u64 {
        0
    }
    fn user_can(&self, _: &str, _: &[Value]) -> bool {
        false
    }
    fn create_nonce(&self, _: &str) -> String {
        String::new()
    }
    fn verify_nonce(&self, _: &str, _: &str) -> u8 {
        0
    }
    fn translate(&self, _: &str, text: &str, _: Option<&str>) -> String {
        text.into()
    }
    fn translate_plural(
        &self,
        _: &str,
        (single, _): (&str, &str),
        _: u64,
        _: Option<&str>,
    ) -> String {
        single.into()
    }
    fn register_asset(&self, _: &str, _: &Asset) -> Result<(), String> {
        Ok(())
    }
    fn enqueue(&self, _: AssetKind, _: &str) -> Result<(), String> {
        Ok(())
    }
    fn add_inline(&self, _: AssetKind, _: &str, _: &str, _: bool) -> Result<(), String> {
        Ok(())
    }
}

fn grants() -> Grants {
    let names = |names: &str| names.split(' ').map(String::from).collect();
    Grants {
        filters: names(
            "the_content ext_trap ext_loop ext_grow ext_late ext_order ext_options ext_cron ext_echo \
             ext_meta ext_query ext_mail ext_auth ext_assets ext_fire ext_chain ext_remove ext_current \
             ext_bad_json ext_error ext_stack ext_count ext_gone ext_nest",
        ),
        actions: names("ext_action ext_event"),
        sensitive: names("map_meta_cap"),
        shortcodes: names("ext ext_gone"),
        options: names("ext"),
        meta: names("_ext"),
        post_types: names("ext_item ext_item_named_too_long"),
        taxonomies: names("ext_kind ext_kind_named_far_too_long_for_wp"),
        rest: names("ext/v1"),
        ..Grants::default()
    }
}

#[test]
#[ignore = "a measurement, run by hand in release"]
fn the_content_10000_times() -> Result<(), Fault> {
    let host = Host::new()?;
    let folder = tempfile::tempdir().map_err(|e| Fault::Refused(e.to_string()))?;
    let extension = host.load(Package {
        slug: "ext",
        wasm: guest::component(),
        grants: grants(),
        limits: Limits::default(),
        folder: folder.path(),
    })?;
    let extensions = Arc::new(Extensions::new(vec![extension]));
    let site: Arc<dyn Site> = Arc::new(Quiet);

    let started = Instant::now();
    for _ in 0..1_000 {
        let request = Request::new(&host, &extensions, site.clone());
        black_box(request.filter("ext_echo", Value::Null, &[]));
    }
    println!(
        "instance per request: {} each (1,000 requests, one call each)",
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
