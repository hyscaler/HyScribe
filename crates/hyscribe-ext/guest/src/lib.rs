// SPDX-License-Identifier: Apache-2.0 OR MIT

//! The spike's guest: a `the_content` filter and callbacks that misbehave,
//! for the host's tests and its cost measurement (ADR 0020). Without `std`,
//! so the component imports no WASI interface, only the SDK's.

#![no_std]

extern crate alloc;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

#[global_allocator]
static ALLOCATOR: dlmalloc::GlobalDlmalloc = dlmalloc::GlobalDlmalloc;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

// The canonical ABI's allocator, which `std` would otherwise export.
#[unsafe(no_mangle)]
unsafe extern "C" fn cabi_realloc(old: *mut u8, old_len: usize, align: usize, new_len: usize) -> *mut u8 {
    use alloc::alloc::{Layout, alloc, realloc};
    if new_len == 0 {
        return align as *mut u8;
    }
    let ptr = unsafe {
        if old_len == 0 {
            alloc(Layout::from_size_align_unchecked(new_len, align))
        } else {
            realloc(old, Layout::from_size_align_unchecked(old_len, align), new_len)
        }
    };
    if ptr.is_null() {
        core::arch::wasm32::unreachable()
    }
    ptr
}

wit_bindgen::generate!({ world: "spike", path: "../wit" });

use alloc::format;
use exports::hyscribe::sdk::guest::Guest;
use hyscribe::sdk::hooks::{HookKind, add};
use hyscribe::sdk::types::{SdkError, Value, ValueKind};
use hyscribe::sdk::{cron, options};

struct Spike;

const HOOKS: [(&str, HookKind, u32, i32); 10] = [
    ("the_content", HookKind::Filter, 0, 10),
    ("spike_trap", HookKind::Filter, 1, 10),
    ("spike_loop", HookKind::Filter, 2, 10),
    ("spike_grow", HookKind::Filter, 3, 10),
    ("spike_late", HookKind::Filter, 4, 10),
    ("spike_order", HookKind::Filter, 5, 10),
    ("spike_order", HookKind::Filter, 6, 9),
    ("spike_options", HookKind::Filter, 7, 10),
    ("spike_cron", HookKind::Filter, 8, 10),
    ("spike_echo", HookKind::Filter, 9, 10),
];

fn int(n: i64) -> Value {
    (ValueKind::Int, n, 0.0, String::new())
}

fn text(s: String) -> Value {
    (ValueKind::Text, 0, 0.0, s)
}

fn why(e: SdkError) -> String {
    match e {
        SdkError::Denied(why) | SdkError::Invalid(why) | SdkError::Failed(why) => why,
    }
}

/// Every cron call on a granted hook, then one on a hook not granted.
fn cron_round() -> Value {
    let args = [int(1)];
    let steps = (
        cron::schedule(100, None, "spike_event", &args),
        cron::schedule(50, Some("hourly"), "spike_event", &args),
        cron::schedule(70, None, "spike_event", &[]),
        cron::next("spike_event", &args),
        cron::next("spike_event", &[]),
        cron::unschedule("spike_event", &args),
        cron::next("spike_event", &args),
        cron::next("spike_event", &[]),
        cron::schedule(1, None, "wp_version_check", &args).map_err(why),
    );
    text(format!("{steps:?}"))
}

/// Every options call on a granted name, then one on a name not granted.
fn options_round() -> Value {
    let steps = [
        options::add("spike", &int(1), None),
        options::add("spike", &int(1), Some(true)),
        options::update("spike", &int(1), None),
        options::update("spike", &int(2), None),
    ];
    let got = options::get("spike").map(|value| value.map(|(_, n, _, _)| n));
    let deleted = [options::delete("spike"), options::delete("spike")];
    let gone = options::get("spike").map(|value| value.is_some());
    let denied = options::update("siteurl", &int(3), None);
    text(format!("{steps:?} {got:?} {deleted:?} {gone:?} {:?}", denied.map_err(why)))
}

fn register(hook: &str, kind: HookKind, callback: u32, priority: i32) -> Result<(), String> {
    add(hook, kind, callback, priority, 1).map_err(why)
}

impl Guest for Spike {
    fn init() -> Result<(), String> {
        HOOKS.iter().try_for_each(|&(hook, kind, callback, priority)| register(hook, kind, callback, priority))
    }

    fn activate() -> Result<(), String> {
        Ok(())
    }

    fn deactivate() -> Result<(), String> {
        Ok(())
    }

    fn invoke(callback: u32, args: Vec<Value>) -> Result<Value, String> {
        match (callback, args.into_iter().next()) {
            (0, Some((ValueKind::Text, _, _, content))) => Ok(text(content + "<!-- spike -->")),
            (1, _) => core::arch::wasm32::unreachable(),
            (2, _) => {
                let mut n = 0u64;
                loop {
                    n = core::hint::black_box(n.wrapping_add(1));
                }
            }
            (3, _) => Ok(int(core::hint::black_box(vec![1u8; 64 << 20]).len() as i64)),
            (4, _) => Ok(text(register("the_content", HookKind::Filter, 0, 10).err().unwrap_or_default())),
            (5, Some((ValueKind::Text, _, _, s))) => Ok(text(s + "b")),
            (6, Some((ValueKind::Text, _, _, s))) => Ok(text(s + "a")),
            (7, _) => Ok(options_round()),
            (8, _) => Ok(cron_round()),
            (9, Some(value)) => Ok(value),
            _ => Err("no such callback, or not its argument".into()),
        }
    }
}

export!(Spike);
