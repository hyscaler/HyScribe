// SPDX-License-Identifier: AGPL-3.0-or-later

//! M1 exit check on the first test target: every serialized option and meta
//! value of a default WordPress install round trips byte for byte.

// Tests fail by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice
)]

use hyscribe_phpser::{Maybe, Value, encode, maybe_unserialize};
use hyscribe_testkit::{default_install, default_install_versions};

/// (table, column index of the value)
const COLUMNS: [(&str, usize); 4] = [
    ("wp_options", 2),
    ("wp_postmeta", 3),
    ("wp_usermeta", 3),
    ("wp_commentmeta", 3),
];

#[test]
fn serialized_values_round_trip() {
    let versions = default_install_versions();
    assert!(
        !versions.is_empty(),
        "run reference/default-install/build.sh"
    );
    for version in versions {
        let dump = default_install(&version).unwrap();
        let mut serialized = 0;
        for (table, column) in COLUMNS {
            for row in dump.table(table) {
                let raw = row[column].as_text().unwrap();
                match maybe_unserialize(raw) {
                    Maybe::Plain(_) => {}
                    Maybe::Value(value) => {
                        assert_eq!(encode(&value), raw, "{version} {table} {row:?}");
                        serialized += 1;
                    }
                    Maybe::Invalid(err) => panic!("{version} {table} {row:?}: {err}"),
                }
            }
        }
        assert!(
            serialized >= 25,
            "{version}: only {serialized} serialized values"
        );
    }
}

#[test]
fn reads_known_default_options() {
    for version in default_install_versions() {
        let dump = default_install(&version).unwrap();
        let option = |name: &str| {
            dump.table("wp_options")
                .find(|row| row[1].as_text() == Some(name.as_bytes()))
                .and_then(|row| row[2].as_text())
                .map(<[u8]>::to_vec)
                .unwrap()
        };
        assert_eq!(option("active_plugins"), b"a:0:{}");
        let Maybe::Value(roles) = maybe_unserialize(&option("wp_user_roles")) else {
            panic!("wp_user_roles is not serialized");
        };
        let admin_caps = roles
            .get_str("administrator")
            .and_then(|r| r.get_str("capabilities"))
            .unwrap();
        assert_eq!(
            admin_caps.get_str("manage_options"),
            Some(&Value::Bool(true))
        );
    }
}
