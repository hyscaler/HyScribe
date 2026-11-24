// SPDX-License-Identifier: AGPL-3.0-or-later

//! The test extension, built from source by `guest/build.sh` once per test
//! binary (ADR 0022): no compiled component is committed.

#![cfg(test)]

use std::process::Command;
use std::sync::OnceLock;

use hyscribe_ext::{
    Asset, AssetKind, Caps, Found, Grants, Incoming, Message, ObjectKind, Outgoing, Post, Site,
    ThemeCall,
};
use hyscribe_phpser::Value;

/// The component `build.sh` builds with `arg`, read once per test binary.
fn built(arg: &str, once: &'static OnceLock<Vec<u8>>) -> &'static [u8] {
    once.get_or_init(|| {
        // From either crate that includes this module.
        let script = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../hyscribe-ext/guest/build.sh"
        );
        let built = Command::new("sh")
            .arg(script)
            .arg(arg)
            .output()
            .expect("run guest/build.sh");
        assert!(
            built.status.success(),
            "guest/build.sh failed (is the wasm32-wasip2 target installed?): {}",
            String::from_utf8_lossy(&built.stderr)
        );
        let path = String::from_utf8_lossy(&built.stdout);
        std::fs::read(path.trim()).expect("read the guest component")
    })
}

/// The test extension's component bytes.
pub fn component() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    built("", &WASM)
}

/// The test theme's component bytes (ADR 0023).
#[allow(dead_code, reason = "the cost harness loads no theme")]
pub fn theme() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    built("theme", &WASM)
}

/// A site that does nothing and holds nothing, for a test that never
/// reaches it.
#[allow(dead_code, reason = "only the cost harness builds one")]
pub struct Quiet;

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
    fn get_transient(&self, _: &str, _: bool) -> Result<Option<Value>, String> {
        Ok(None)
    }
    fn set_transient(&self, _: &str, _: Value, _: i64, _: bool) -> Result<bool, String> {
        Ok(false)
    }
    fn delete_transient(&self, _: &str, _: bool) -> Result<bool, String> {
        Ok(false)
    }
    fn fetch(&self, _: &Outgoing, _: &(dyn Fn(&str) -> bool + Sync)) -> Result<Incoming, String> {
        Err(String::new())
    }
    fn role(&self, _: &str) -> Result<Option<Caps>, String> {
        Ok(None)
    }
    fn add_role(&self, _: &str, _: &str, _: &[(String, bool)]) -> Result<bool, String> {
        Ok(false)
    }
    fn remove_role(&self, _: &str) -> Result<bool, String> {
        Ok(false)
    }
    fn role_cap(&self, _: &str, _: &str, _: Option<bool>) -> Result<bool, String> {
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
    fn theme(&self, _: &str, _: &ThemeCall) -> Result<Value, String> {
        Ok(Value::Null)
    }
    fn settings_fields(&self, _: &str) -> Result<Vec<u8>, String> {
        Ok(Vec::new())
    }
}

fn names(names: &str) -> std::collections::BTreeSet<String> {
    names
        .split(' ')
        .filter(|name| !name.is_empty())
        .map(String::from)
        .collect()
}

/// Everything the test extension asks for.
pub fn grants() -> Grants {
    Grants {
        filters: names(
            "the_content ext_trap ext_loop ext_grow ext_late ext_order ext_options ext_cron ext_echo \
             ext_meta ext_query ext_mail ext_auth ext_assets ext_fire ext_chain ext_remove ext_current \
             ext_bad_json ext_error ext_stack ext_count ext_gone ext_nest body_class \
             admin_footer_text ext_transients ext_roles ext_http ext_slow",
        ),
        actions: names("ext_action ext_event"),
        sensitive: names("map_meta_cap"),
        shortcodes: names("ext ext_gone"),
        options: names("ext ext_fail ext_bytes ext_active ext_hooks ext_title"),
        meta: names("_ext _ext_*"),
        post_types: names("ext_item ext_item_named_too_long"),
        taxonomies: names("ext_kind ext_kind_named_far_too_long_for_wp"),
        read_taxonomies: names("category"),
        rest: names("ext/v1"),
        mail: true,
        filesystem: names("uploads/ext"),
        transients: names("ext_cache ext_tmp_*"),
        site_transients: names("ext_site*"),
        roles: names("ext_manager ext_fail"),
        capabilities: names("ext_cap read"),
        hosts: names("api.test redirect.test"),
    }
}
