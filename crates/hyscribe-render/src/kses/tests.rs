// SPDX-License-Identifier: AGPL-3.0-or-later

use hyscribe_testkit::unhex;

use super::*;

/// Every case of `reference/kses-post/golden.sh`.
#[test]
fn matches_php() {
    let golden = include_str!("../../tests/golden/kses-post.tsv");
    let mut host = Vec::new();
    let mut failures = Vec::new();
    for line in golden.lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let input = unhex(fields.get(1).unwrap());
        let (kind, ours) = match fields[0] {
            "host" => {
                host = input;
                continue;
            }
            "kses" => ("kses", wp_kses_post(&input, &host)),
            _ => ("serialize", serialize_blocks(&parse_blocks(&input))),
        };
        let php = unhex(fields[2]);
        if ours != php {
            failures.push(format!(
                "{kind} {:?}\n  php:  {:?}\n  ours: {:?}",
                String::from_utf8_lossy(&input),
                String::from_utf8_lossy(&php),
                String::from_utf8_lossy(&ours)
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures[..failures.len().min(8)].join("\n")
    );
    assert!(golden.lines().count() > 6000);
}

#[test]
fn refiltering_stops_at_the_depth_limit() {
    let mut kses = Kses {
        allowed: &POST,
        upload_host: b"",
        budget: budget(0),
    };
    assert_eq!(
        kses.refilter(b"x", true, MAX_DEPTH - 1),
        Some(b"x".to_vec())
    );
    assert_eq!(kses.refilter(b"x", true, MAX_DEPTH), None);
    // The comment inside is one level deeper, past the limit.
    let comment = b"<!-- x -->";
    assert_eq!(
        kses.refilter(comment, true, MAX_DEPTH - 1),
        Some(Vec::new())
    );
}

#[test]
fn an_exhausted_budget_drops_the_token() {
    assert_eq!(budget(10), 4256);
    // Each pass costs the content's length and one.
    for (token, budget) in [(&b"<!-- x -->"[..], 0), (b"</ x>", 0), (b"</ >", 1)] {
        let mut kses = Kses {
            allowed: &POST,
            upload_host: b"",
            budget,
        };
        assert_eq!(kses.kses(token, true, 0), b"", "{token:?}");
    }
}

#[test]
fn limits_no_core_list_uses() {
    let allowed = AllowedHtml::from_json(
        br#"{"p":{"title":{"maxlen":5},"lang":{"REQUIRED":true,"Required":true},"cite":{"value_callback":"other"}},"1":true}"#,
    );
    // PHP would keep the short title; unported limits fail closed.
    assert_eq!(
        wp_kses(
            br#"<p title="x" lang="en" cite="http://h/a.pdf">a</p><1 x="y">"#,
            &allowed,
            b"h"
        ),
        br#"<p lang="en">a</p><1>"#
    );
    let nothing = AllowedHtml::from_json(b"null");
    assert_eq!(wp_kses(b"<p>x</p>", &nothing, b""), b"x");
}

/// HTTP requests render on 2 MiB worker stacks.
#[test]
fn the_deepest_blocks_fit_a_worker_stack() {
    let depth = crate::blocks::MAX_BLOCK_DEPTH;
    let document = [
        br#"<!-- wp:group {"a":{"b":"\u003ci\u003e"}} -->"#.repeat(depth),
        b"<!-- /wp:group -->".repeat(depth),
    ]
    .concat();
    let clean = std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || wp_kses_post(&document, b""))
        .unwrap()
        .join()
        .unwrap();
    assert!(clean.starts_with(br#"<!-- wp:group {"a":{"b":"\u003ci\u003e"}} -->"#));
}

/// Block attributes with many keys filter in linear time, each key found
/// through an index. A key the filter changes moves to the end, as PHP
/// unsets it and sets the filtered one, so here every key but the first
/// moves, in order.
#[test]
fn many_attribute_keys_filter_in_linear_time() {
    let keys = |suffix: &str| {
        (0..20_000)
            .map(|n| format!(r#""k{n}{suffix}":{n}"#))
            .collect::<Vec<_>>()
            .join(",")
    };
    let document = format!(r#"<!-- wp:x {{"a":1,{}}} /-->"#, keys(r"\u0026"));
    let clean = String::from_utf8(wp_kses_post(document.as_bytes(), b"")).unwrap();
    let expected = format!(r#"<!-- wp:x {{"a":1,{}}} /-->"#, keys(r"\u0026amp;"));
    assert!(
        clean == expected,
        "{}",
        clean.get(..200).unwrap_or_default()
    );
}

/// `wp_kses_data()` keeps `$allowedtags` only, with their attributes.
#[test]
fn data_keeps_the_allowed_tags() {
    assert_eq!(
        wp_kses_data(
            b"<b>x</b><script>y</script><a href=\"https://h.test/\" onclick=\"z\">l</a><p>p</p>"
        ),
        b"<b>x</b>y<a href=\"https://h.test/\">l</a>p"
    );
}

/// A biography keeps links' `rel` and `target` too, as the write oracle's
/// `user-insert-every-field` records.
#[test]
fn user_descriptions_keep_link_relations() {
    assert_eq!(
        wp_kses_user_description(
            b"<a href=\"https://example.test\" rel=\"me\" target=\"_blank\" onclick=\"x()\">Me</a> \
              & <b>bold</b> <script>x</script>"
        ),
        b"<a href=\"https://example.test\" rel=\"me\" target=\"_blank\">Me</a> &amp; <b>bold</b> x"
    );
    assert_eq!(
        wp_kses_data(b"<a rel=\"me\" target=\"_blank\">l</a>"),
        b"<a>l</a>"
    );
}

/// What PHP saved for comments' content in the write oracle's
/// `new-comment-unfiltered` and `new-comment-anonymous` (no kses) and
/// `new-comment-by-admin` (kses, then `wp_rel_ugc()`).
#[test]
fn comments_get_ugc_links() {
    let home = b"http://127.0.0.1:8081";
    for (content, saved) in [
        (
            "<a href=\"https://example.test\" class=\"x\" rel=\"me\">out</a> \
             <A HREF='http://127.0.0.1:8081/about'>in</A> <a\nhref=\"https://split.test\">split</a>",
            "<a href=\"https://example.test\" class=\"x\" rel=\"me nofollow ugc\">out</a> \
             <a HREF='http://127.0.0.1:8081/about' rel=\"ugc\">in</A> \
             <a\nhref=\"https://split.test\">split</a>",
        ),
        (
            "<a download rel=\"me  me\" href=\"/about\">x</a> \
             <a href=\"mailto:x@example.test\">m</a>",
            "<a download href=\"/about\" rel=\"me  nofollow ugc\">x</a> \
             <a href=\"mailto:x@example.test\" rel=\"nofollow ugc\">m</a>",
        ),
    ] {
        let filtered = wp_rel_ugc(content.as_bytes(), home);
        assert_eq!(String::from_utf8(filtered).unwrap(), saved);
    }
    // `|<a (.+?)>|i` matches no newline and needs a byte and a `>`.
    for kept in ["<a \nhref=x>", "<a >", "<a x", "<a x\n>", "<a <a x\n>"] {
        assert_eq!(wp_rel_ugc(kept.as_bytes(), home), kept.as_bytes());
    }
    // Unclosed links stay linear.
    let started = std::time::Instant::now();
    for unit in [&b"<a x"[..], b"<a x\n", b"<a <a "] {
        wp_rel_ugc(&unit.repeat(500_000), home);
    }
    assert!(started.elapsed().as_secs() < 20, "{:?}", started.elapsed());
    // `wp_is_internal_link()` wants a protocol.
    let relative = wp_rel_ugc(b"<a href=\"//127.0.0.1:8081/x\">", home);
    let expected = b"<a href=\"//127.0.0.1:8081/x\" rel=\"nofollow ugc\">";
    assert_eq!(relative, expected);
    let content = "<a href=\"https://example.test\" class=\"x\" rel=\"me\">out</a> \
        <A HREF='http://127.0.0.1:8081/about'>in</A> <a\nhref=\"https://split.test\">split</a> \
        <span class=\"wp-note-mention user-2\">@me</span> <script>alert(1)</script> <b>bold";
    let saved = "<a href=\"https://example.test\" rel=\"nofollow ugc\">out</a> \
        <a href=\"http://127.0.0.1:8081/about\" rel=\"ugc\">in</A> \
        <a href=\"https://split.test\" rel=\"nofollow ugc\">split</a> \
        <span class=\"wp-note-mention user-2\">@me</span> alert(1) <b>bold";
    let filtered = wp_rel_ugc(&wp_kses_comment(content.as_bytes()), home);
    assert_eq!(String::from_utf8(filtered).unwrap(), saved);
}

/// A plugin's headers keep emphasis and code, and links but in its name.
#[test]
fn plugin_headers_keep_what_php_keeps() {
    let header = b"<a href=\"https://x.test/\" class=\"c\">A</a> <em>B</em> <script>c</script>'s";
    assert_eq!(
        wp_kses_plugin_data(header, true),
        b"<a href=\"https://x.test/\">A</a> <em>B</em> c's"
    );
    assert_eq!(wp_kses_plugin_data(header, false), b"A <em>B</em> c's");
}
