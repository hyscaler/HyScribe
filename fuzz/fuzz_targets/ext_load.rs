// SPDX-License-Identifier: AGPL-3.0-or-later

//! Loading hostile bytes as an extension (ADR 0022) never panics: anything
//! that is not a component of the SDK's world is refused, and one that
//! loads runs its filters within its deadline, budget and memory limit.

#![no_main]

use std::path::PathBuf;
use std::sync::Arc;

use hyscribe_ext::{
    Asset, AssetKind, Caps, Extensions, Fault, Found, Grants, Host, Incoming, Limits, Message, ObjectKind,
    Outgoing, Package, Post, Request, Site,
};
use hyscribe_phpser::Value;
use libfuzzer_sys::fuzz_target;

/// A site that has nothing and refuses every write.
struct Empty;

impl Site for Empty {
    fn now(&self) -> u64 {
        0
    }
    fn get_option(&self, _: &str) -> Result<Option<Value>, String> {
        Ok(None)
    }
    fn update_option(&self, _: &str, _: Value, _: Option<bool>) -> Result<bool, String> {
        Err("read only".into())
    }
    fn add_option(&self, _: &str, _: Value, _: Option<bool>) -> Result<bool, String> {
        Err("read only".into())
    }
    fn delete_option(&self, _: &str) -> Result<bool, String> {
        Err("read only".into())
    }
    fn get_meta(&self, _: ObjectKind, _: u64, _: &str) -> Result<Vec<Value>, String> {
        Ok(Vec::new())
    }
    fn add_meta(&self, _: ObjectKind, _: u64, _: &str, _: Value, _: bool) -> Result<Option<u64>, String> {
        Err("read only".into())
    }
    fn update_meta(&self, _: ObjectKind, _: u64, _: &str, _: Value, _: Option<Value>) -> Result<bool, String> {
        Err("read only".into())
    }
    fn delete_meta(&self, _: ObjectKind, _: u64, _: &str, _: Option<Value>) -> Result<bool, String> {
        Err("read only".into())
    }
    fn public_post_type(&self, post_type: &str) -> bool {
        post_type == "post"
    }
    fn query(&self, _: &Value) -> Result<Found, String> {
        Ok(Found::default())
    }
    fn post(&self, _: u64) -> Result<Option<Post>, String> {
        Ok(None)
    }
    fn schedule(&self, _: i64, _: Option<&str>, _: &str, _: &[Value]) -> Result<(), String> {
        Err("read only".into())
    }
    fn unschedule(&self, _: &str, _: &[Value]) -> Result<u32, String> {
        Err("read only".into())
    }
    fn next_scheduled(&self, _: &str, _: &[Value]) -> Option<i64> {
        None
    }
    fn mail(&self, _: &Message) -> Result<bool, String> {
        Err("read only".into())
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
    fn translate_plural(&self, _: &str, (single, _): (&str, &str), _: u64, _: Option<&str>) -> String {
        single.into()
    }
    fn register_asset(&self, _: &str, _: &Asset) -> Result<(), String> {
        Err("read only".into())
    }
    fn enqueue(&self, _: AssetKind, _: &str) -> Result<(), String> {
        Err("read only".into())
    }
    fn add_inline(&self, _: AssetKind, _: &str, _: &str, _: bool) -> Result<(), String> {
        Err("read only".into())
    }
    fn get_transient(&self, _: &str, _: bool) -> Result<Option<Value>, String> {
        Ok(None)
    }
    fn set_transient(&self, _: &str, _: Value, _: i64, _: bool) -> Result<bool, String> {
        Err("read only".into())
    }
    fn delete_transient(&self, _: &str, _: bool) -> Result<bool, String> {
        Err("read only".into())
    }
    fn fetch(&self, _: &Outgoing, _: &(dyn Fn(&str) -> bool + Sync)) -> Result<Incoming, String> {
        Err("no network".into())
    }
    fn role(&self, _: &str) -> Result<Option<Caps>, String> {
        Ok(None)
    }
    fn add_role(&self, _: &str, _: &str, _: &[(String, bool)]) -> Result<bool, String> {
        Err("read only".into())
    }
    fn remove_role(&self, _: &str) -> Result<bool, String> {
        Err("read only".into())
    }
    fn role_cap(&self, _: &str, _: &str, _: Option<bool>) -> Result<bool, String> {
        Err("read only".into())
    }
}

thread_local! {
    static HOST: Option<Host> = Host::new().ok();
    // A folder with no fault record, so every load starts enabled.
    static FOLDER: PathBuf = std::env::temp_dir().join(format!("hyscribe-ext-load-{}", std::process::id()));
}

fuzz_target!(|wasm: &[u8]| {
    HOST.with(|host| {
        let Some(host) = host else { return };
        let folder = FOLDER.with(PathBuf::clone);
        let _ = std::fs::create_dir_all(&folder);
        let _ = hyscribe_ext::faults::clear(&folder);
        let grants = Grants {
            filters: ["the_content".into()].into(),
            options: ["fuzz".into()].into(),
            ..Grants::default()
        };
        let package = Package {
            sdk: "0.2",
            slug: "fuzz",
            wasm,
            grants,
            limits: Limits::default(),
            folder: &folder,
        };
        match host.load(package) {
            Ok(extension) => {
                let extensions = Arc::new(Extensions::new(vec![Arc::new(extension)]));
                let request = Request::new(host, &extensions, Arc::new(Empty));
                request.filter("the_content", Value::String(b"<p>x</p>".to_vec()), &[]);
                let _ = request.activate(0);
            }
            Err(fault) => assert!(!matches!(fault, Fault::Guest(_) | Fault::Disabled | Fault::Value(_))),
        }
    });
});
