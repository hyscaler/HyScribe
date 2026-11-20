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
    assert_eq!(NoHooks.wp_read_audio_metadata(meta.clone(), None), meta);
    assert_eq!(
        NoHooks.wp_read_video_metadata(meta.clone(), Some("mp4")),
        meta
    );
}

#[test]
fn the_filters_whose_values_cross_reach_the_extensions() {
    use hyscribe_phpser::Key;
    use hyscribe_testkit::hooks::recorded;
    let text = |s: &str| Value::String(s.as_bytes().to_vec());
    let answers = [
        ("wp_editor_set_quality", Value::Int(70)),
        ("jpeg_quality", Value::Int(80)),
        ("image_make_intermediate_size", text("b.jpg")),
        ("wp_generate_attachment_metadata", Value::Int(5)),
    ];
    let (got, fired) = recorded(&answers, || {
        (
            NoHooks.wp_editor_set_quality(55, "image/png", (1, 2)),
            NoHooks.jpeg_quality(56, "image_resize"),
            NoHooks.image_make_intermediate_size("a.jpg".into()),
            NoHooks.wp_generate_attachment_metadata(Value::Int(4), 7, "create"),
        )
    });
    assert_eq!(got, (70, 80, "b.jpg".to_string(), Value::Int(5)));
    let size = Value::Array(vec![
        (Key::String(b"width".to_vec()), Value::Int(1)),
        (Key::String(b"height".to_vec()), Value::Int(2)),
    ]);
    let fire = |hook: &str, value: Value, args: Vec<Value>| (hook.to_string(), value, args);
    let expected = vec![
        fire(
            "wp_editor_set_quality",
            Value::Int(55),
            vec![text("image/png"), size],
        ),
        fire("jpeg_quality", Value::Int(56), vec![text("image_resize")]),
        fire("image_make_intermediate_size", text("a.jpg"), vec![]),
        fire(
            "wp_generate_attachment_metadata",
            Value::Int(4),
            vec![Value::Int(7), text("create")],
        ),
    ];
    assert_eq!(fired, expected);
}
