// SPDX-License-Identifier: Apache-2.0 OR MIT

use super::*;

#[test]
fn strings_escape_what_json_requires() {
    assert_eq!(string(""), "\"\"");
    assert_eq!(string("a\"b\\c/é"), "\"a\\\"b\\\\c/é\"");
    assert_eq!(string("\n\u{1f}\u{20}"), "\"\\u000a\\u001f \"");
}

#[test]
fn objects_of_entries() {
    assert_eq!(object(&[]), "{}");
    assert_eq!(object(&[("post_type", "\"x\"")]), "{\"post_type\":\"x\"}");
    assert_eq!(
        object(&[("a", "1"), ("b\"", "[2]")]),
        "{\"a\":1,\"b\\\"\":[2]}"
    );
}
