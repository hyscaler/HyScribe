// SPDX-License-Identifier: AGPL-3.0-or-later

//! Loading hostile bytes as an extension (ADR 0022) never panics: anything
//! that is not a component of the SDK's world is refused, and one that
//! loads runs its filters within its deadline and memory limit.

#![no_main]

use hyscribe_ext::{Fault, Grants, Host, Limits, Value};
use libfuzzer_sys::fuzz_target;

thread_local! {
    static HOST: Option<Host> = Host::new(Limits::default()).ok();
}

fuzz_target!(|wasm: &[u8]| {
    HOST.with(|host| {
        let Some(host) = host else { return };
        let grants = Grants {
            hooks: ["the_content".into()].into(),
            options: ["fuzz".into()].into(),
        };
        match host.load(wasm, grants) {
            Ok(extension) => {
                if let Ok(mut running) = extension.start(host) {
                    running.filter("the_content", Value::Text("<p>x</p>".into()));
                }
            }
            Err(fault) => assert!(!matches!(fault, Fault::Guest(_) | Fault::Disabled)),
        }
    });
});
