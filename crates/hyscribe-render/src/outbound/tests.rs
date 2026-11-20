// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::hooks::recorded;

use super::*;

fn text(s: &str) -> Value {
    Value::String(s.as_bytes().to_vec())
}

#[test]
fn the_filters_whose_values_cross_reach_the_extensions() {
    let hooks = ();
    let size = Size {
        width: 500,
        height: 750,
    };
    let sized = Value::Array(vec![
        (Key::String(b"width".to_vec()), Value::Int(500)),
        (Key::String(b"height".to_vec()), Value::Int(750)),
    ]);
    let attr = [(Key::String(b"id".to_vec()), Value::Int(3))];
    let atts = [("width".to_string(), Value::Int(4))];
    let named = Value::Array(vec![(Key::String(b"width".to_vec()), Value::Int(4))]);
    let list = |all: &[&str]| {
        Value::Array(
            all.iter()
                .zip(0..)
                .map(|(s, n)| (Key::Int(n), text(s)))
                .collect(),
        )
    };
    let answers = [
        ("oembed_fetch_url", text("P")),
        ("embed_oembed_html", text("H")),
        ("wp_feed_cache_transient_lifetime", Value::Int(60)),
        ("rss_enclosure", text("E")),
        ("atom_enclosure", text("A")),
        ("img_caption_shortcode", text("C")),
        ("img_caption_shortcode_width", Value::Int(20)),
        ("post_gallery", text("G")),
        ("use_default_gallery_style", Value::Bool(false)),
        ("gallery_style", text("S")),
        ("wp_audio_shortcode_override", text("O")),
        ("wp_video_extensions", list(&["webm"])),
        ("wp_audio_shortcode_library", text("none")),
        ("wp_video_shortcode_class", text("k")),
        ("wp_mediaelement_fallback", text("F")),
        ("wp_navigation_should_create_fallback", Value::Bool(false)),
        ("nav_menu_attr_title", text("T")),
        ("nav_menu_description", text("D")),
    ];
    let (got, fired) = recorded(&answers, || {
        [
            hooks.oembed_fetch_url(b"p".to_vec(), b"u", size),
            hooks.embed_oembed_html(b"h".to_vec(), b"u", size, 9),
            hooks
                .wp_feed_cache_transient_lifetime(43_200, b"feed")
                .to_string()
                .into_bytes(),
            hooks.rss_enclosure(b"e".to_vec()),
            hooks.atom_enclosure(b"a".to_vec()),
            hooks.img_caption_shortcode(&attr, b"c"),
            hooks
                .img_caption_shortcode_width(10, &atts, b"c")
                .to_string()
                .into_bytes(),
            hooks.post_gallery(&attr, 2),
            hooks
                .use_default_gallery_style(true)
                .to_string()
                .into_bytes(),
            hooks.gallery_style(b"s".to_vec()),
            hooks.media_shortcode_override("audio", &attr, b"c", 1),
            hooks
                .media_extensions("video", vec!["mp4".into()])
                .concat()
                .into_bytes(),
            hooks
                .media_shortcode_library("audio", "mediaelement".into())
                .into_bytes(),
            hooks.media_shortcode_class("video", b"wp-video-shortcode".to_vec(), &atts),
            hooks.wp_mediaelement_fallback(b"f".to_vec(), b"u"),
            hooks
                .wp_navigation_should_create_fallback(true)
                .to_string()
                .into_bytes(),
            hooks.nav_menu_attr_title(b"t".to_vec()),
            hooks.nav_menu_description(b"d".to_vec()),
        ]
    });
    let got: Vec<String> = got
        .iter()
        .map(|got| String::from_utf8_lossy(got).into_owned())
        .collect();
    let answered = [
        "P", "H", "60", "E", "A", "C", "20", "G", "false", "S", "O", "webm", "none", "k", "F",
        "false", "T", "D",
    ];
    assert_eq!(got, answered);
    let fire = |hook: &str, value: Value, args: Vec<Value>| (hook.to_string(), value, args);
    let expected = vec![
        fire(
            "oembed_fetch_url",
            text("p"),
            vec![text("u"), sized.clone()],
        ),
        fire(
            "embed_oembed_html",
            text("h"),
            vec![text("u"), sized, Value::Int(9)],
        ),
        fire(
            "wp_feed_cache_transient_lifetime",
            Value::Int(43_200),
            vec![text("feed")],
        ),
        fire("rss_enclosure", text("e"), vec![]),
        fire("atom_enclosure", text("a"), vec![]),
        fire(
            "img_caption_shortcode",
            text(""),
            vec![Value::Array(attr.to_vec()), text("c")],
        ),
        fire(
            "img_caption_shortcode_width",
            Value::Int(10),
            vec![named.clone(), text("c")],
        ),
        fire(
            "post_gallery",
            text(""),
            vec![Value::Array(attr.to_vec()), Value::Int(2)],
        ),
        fire("use_default_gallery_style", Value::Bool(true), vec![]),
        fire("gallery_style", text("s"), vec![]),
        fire(
            "wp_audio_shortcode_override",
            text(""),
            vec![Value::Array(attr.to_vec()), text("c"), Value::Int(1)],
        ),
        fire("wp_video_extensions", list(&["mp4"]), vec![]),
        fire("wp_audio_shortcode_library", text("mediaelement"), vec![]),
        fire(
            "wp_video_shortcode_class",
            text("wp-video-shortcode"),
            vec![named],
        ),
        fire("wp_mediaelement_fallback", text("f"), vec![text("u")]),
        fire(
            "wp_navigation_should_create_fallback",
            Value::Bool(true),
            vec![],
        ),
        fire("nav_menu_attr_title", text("t"), vec![]),
        fire("nav_menu_description", text("d"), vec![]),
    ];
    assert_eq!(fired, expected);
}
