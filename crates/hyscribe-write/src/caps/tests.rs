// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

/// The capabilities read only the options, user, post and constants they load.
#[test]
fn caps_read_what_they_load() {
    let mut caps = Caps {
        prefix: "wp_".into(),
        options: Vec::new(),
        user: 1,
        exists: true,
        capabilities: None,
        post: None,
        constants: Constants::default(),
    };
    assert_eq!(caps.comment_post(1), None);
    assert_eq!(caps.term_taxonomy(1), None);
    assert!(!caps.constant("MULTISITE") && caps.constant("DISALLOW_UNFILTERED_HTML"));
    caps.constants.disallow_unfiltered_html = false;
    assert!(!caps.constant("DISALLOW_UNFILTERED_HTML"));
}
