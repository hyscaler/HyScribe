// SPDX-License-Identifier: AGPL-3.0-or-later

//! The spike's guest, built from source by `guest/build.sh` once per test
//! binary (ADR 0022): no compiled component is committed.

#![cfg(test)]

use std::process::Command;
use std::sync::OnceLock;

/// The guest component's bytes.
pub fn spike() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(|| {
        let script = concat!(env!("CARGO_MANIFEST_DIR"), "/guest/build.sh");
        let built = Command::new("sh")
            .arg(script)
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
