// SPDX-License-Identifier: AGPL-3.0-or-later

//! Property tests: encode then decode is the identity, and decode never panics.

// Tests fail by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use hyscribe_phpser::{Float, Key, Value, decode, encode};
use proptest::prelude::*;
use proptest::test_runner::RngSeed;

fn key() -> impl Strategy<Value = Key> {
    prop_oneof![
        any::<i64>().prop_map(Key::Int),
        proptest::collection::vec(any::<u8>(), 0..12).prop_map(Key::String),
    ]
}

fn class() -> impl Strategy<Value = Vec<u8>> {
    "[A-Za-z_][A-Za-z0-9_]{0,10}(\\\\[A-Za-z_][A-Za-z0-9_]{0,6}){0,2}".prop_map(String::into_bytes)
}

fn value() -> impl Strategy<Value = Value> {
    let leaf = prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::Int),
        any::<f64>().prop_map(|f| Value::Float(Float::from_f64(f))),
        proptest::collection::vec(any::<u8>(), 0..24).prop_map(Value::String),
        (class(), proptest::collection::vec(any::<u8>(), 0..16))
            .prop_map(|(class, data)| Value::Custom { class, data }),
        (class(), "[A-Za-z_][A-Za-z0-9_]{0,8}")
            .prop_map(|(c, case)| Value::Enum([c, b":".to_vec(), case.into_bytes()].concat())),
        any::<u32>().prop_map(|n| Value::ObjectRef(n.into())),
        any::<u32>().prop_map(|n| Value::Ref(n.into())),
    ];
    leaf.prop_recursive(6, 64, 8, |inner| {
        prop_oneof![
            proptest::collection::vec((key(), inner.clone()), 0..8).prop_map(Value::Array),
            (class(), proptest::collection::vec((key(), inner), 0..8))
                .prop_map(|(class, properties)| Value::Object { class, properties }),
        ]
    })
}

proptest! {
    // Fixed seed: tests and coverage are the same on every run.
    #![proptest_config(ProptestConfig {
        cases: 1024,
        rng_seed: RngSeed::Fixed(20_261_027),
        ..ProptestConfig::default()
    })]

    #[test]
    fn encode_then_decode_is_identity(v in value()) {
        let bytes = encode(&v);
        prop_assert_eq!(decode(&bytes), Ok(v));
    }

    #[test]
    fn decode_then_encode_is_identity(v in value()) {
        let bytes = encode(&v);
        let decoded = decode(&bytes).unwrap();
        prop_assert_eq!(encode(&decoded), bytes);
    }

    #[test]
    fn decode_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..256)) {
        let _ = decode(&bytes);
    }

    #[test]
    fn truncation_is_always_an_error(v in value(), cut in any::<prop::sample::Index>()) {
        let bytes = encode(&v);
        let cut = cut.index(bytes.len());
        prop_assert!(decode(&bytes[..cut]).is_err());
    }
}
