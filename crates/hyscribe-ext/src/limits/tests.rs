// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn defaults_and_their_units() {
    let limits = Limits::default();
    assert_eq!(limits, DEFAULT);
    assert_eq!(limits.call(), Duration::from_millis(50));
    assert_eq!(limits.request(), Duration::from_millis(250));
    assert_eq!(limits.memory(), 16 << 20);
}

#[test]
fn asks_within_the_ceiling() {
    assert_eq!(Limits::asked(&[]), Ok(DEFAULT));
    let asked = Limits::asked(&[("call_ms", 200), ("request_ms", 1_000), ("memory_mib", 32)]);
    assert_eq!(
        asked,
        Ok(Limits {
            call_ms: 200,
            request_ms: 1_000,
            memory_mib: 32,
        })
    );
    let edges = [("call_ms", 500), ("request_ms", 250), ("memory_mib", 64)];
    assert_eq!(
        Limits::asked(&edges),
        Ok(Limits {
            call_ms: 500,
            request_ms: 250,
            memory_mib: 64,
        })
    );
    assert_eq!(
        Limits::asked(&[("call_ms", 50), ("request_ms", 2_000), ("memory_mib", 16)]),
        Ok(Limits {
            request_ms: 2_000,
            ..DEFAULT
        })
    );
}

#[test]
fn refuses_rather_than_clamps() {
    let refused = |key: &str, value: i64| Limits::asked(&[(key, value)]).err();
    let call = Some("[limits] call_ms must be from 50 to 500".to_string());
    assert_eq!(refused("call_ms", 49), call);
    assert_eq!(refused("call_ms", 501), call);
    assert_eq!(refused("call_ms", -1), call);
    let request = Some("[limits] request_ms must be from 250 to 2000".to_string());
    assert_eq!(refused("request_ms", 249), request);
    assert_eq!(refused("request_ms", 2_001), request);
    let memory = Some("[limits] memory_mib must be from 16 to 64".to_string());
    assert_eq!(refused("memory_mib", 15), memory);
    assert_eq!(refused("memory_mib", 65), memory);
    assert_eq!(
        refused("fuel", 1),
        Some("[limits] has no key fuel".to_string())
    );
}
