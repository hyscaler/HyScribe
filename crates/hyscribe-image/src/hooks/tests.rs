// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

#[test]
fn returns_each_value_unchanged_with_nothing_attached() {
    assert_eq!(NoHooks.big_image_size_threshold(7, (1, 2), "f", 3), 7);
    assert_eq!(NoHooks.wp_editor_set_quality(55, "image/png", (1, 2)), 55);
    assert_eq!(NoHooks.jpeg_quality(56, "image_resize"), 56);
    assert_eq!(
        NoHooks.image_make_intermediate_size("a/b.jpg".into()),
        "a/b.jpg"
    );
    let meta = Value::Int(4);
    assert_eq!(
        NoHooks.wp_generate_attachment_metadata(meta.clone(), 1, "create"),
        meta
    );
    assert!(
        NoHooks
            .intermediate_image_sizes_advanced(Vec::new(), &meta, 1)
            .is_empty()
    );
}
