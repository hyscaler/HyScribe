// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_footer()` for a block theme: the script modules left for the footer,
//! the footer scripts, and the emoji detection script.

use hyscribe_kses::esc_url_raw;
use hyscribe_phpser::{Key, Value};

use super::{
    Assets, Modules, WP_VERSION, inline_script, json_hex_tag, registered_modules, script_source,
    tag,
};
use crate::site::Site;

const EMOJI_LOADER: &str =
    include_str!("../../../../wordpress/wp-includes/js/wp-emoji-loader.min.js");

/// `comment-reply`, the one classic script blocks enqueue, as
/// `wp_default_scripts()` registers it: async, at low priority.
const COMMENT_REPLY: &str = "/wp-includes/js/comment-reply.min.js";

/// What `wp_footer()` prints after the template.
pub fn wp_footer(site: &dyn Site, assets: &Assets) -> Vec<u8> {
    let siteurl = String::from_utf8_lossy(&site.option("siteurl").unwrap_or_default()).into_owned();
    let registry = registered_modules(&siteurl);
    let mut out = speculation_rules(site, &siteurl);
    out.extend(
        Modules {
            registry: &registry,
            queue: &assets.modules,
        }
        .footer(),
    );
    // wp_print_footer_scripts()
    if assets.scripts.iter().any(|h| h == "comment-reply") {
        let src =
            esc_url_raw(format!("{siteurl}{COMMENT_REPLY}?ver={}", WP_VERSION.trim()).as_bytes());
        out.extend(tag(
            b"script",
            &[
                (b"src", Some(&src)),
                (b"id", Some(b"comment-reply-js")),
                (b"async", None),
                (b"data-wp-strategy", Some(b"async")),
                (b"fetchpriority", Some(b"low")),
            ],
            None,
        ));
    }
    out.extend(emoji_detection_script(&siteurl));
    out
}

/// `wp_print_speculation_rules()`: conservative prefetching of the site's
/// own pages, which WordPress turns on only under pretty permalinks.
fn speculation_rules(site: &dyn Site, siteurl: &str) -> Vec<u8> {
    if !crate::permalinks::Permalinks::of(site).pretty() {
        return Vec::new();
    }
    let path = |url: &str| escape_pattern(crate::permalinks::home_path(url));
    let home = path(&String::from_utf8_lossy(&site.home_url()));
    let base = path(siteurl);
    let option =
        |name| String::from_utf8_lossy(&site.option(name).unwrap_or_default()).into_owned();
    let themes = |name| {
        format!(
            "{base}/wp-content/themes/{}/*",
            escape_pattern(&option(name))
        )
    };
    let mut excluded = vec![
        format!("{base}/wp-*.php"),
        format!("{base}/wp-admin/*"),
        format!("{base}/wp-content/uploads/*"),
        format!("{base}/wp-content/*"),
        format!("{base}/wp-content/plugins/*"),
        themes("template"),
        themes("stylesheet"),
    ];
    // `array_unique()`: a theme that is not a child theme once.
    excluded.dedup();
    excluded.push(format!("{home}/*\\?(.+)"));
    let excluded: Vec<String> = excluded.iter().map(|p| json_string(p)).collect();
    format!(
        "<script type=\"speculationrules\">\n{{\"prefetch\":[{{\"source\":\"document\",\"where\":{{\"and\":[{{\"href_matches\":{}}},{{\"not\":{{\"href_matches\":[{}]}}}},{{\"not\":{{\"selector_matches\":\"a[rel~=\\\"nofollow\\\"]\"}}}},{{\"not\":{{\"selector_matches\":\".no-prefetch, .no-prefetch a\"}}}}]}},\"eagerness\":\"conservative\"}}]}}\n</script>\n",
        json_string(&format!("{home}/*")),
        excluded.join(",")
    )
    .into_bytes()
}

/// `WP_URL_Pattern_Prefixer::escape_pattern_string()`.
fn escape_pattern(text: &str) -> String {
    text.chars()
        .flat_map(|c| {
            "+*?:{}()\\"
                .contains(c)
                .then_some('\\')
                .into_iter()
                .chain([c])
        })
        .collect()
}

/// A JSON string as `wp_json_encode()` gives it with `JSON_HEX_TAG` and
/// unescaped slashes.
fn json_string(text: &str) -> String {
    let escaped = text
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('<', "\\u003C")
        .replace('>', "\\u003E");
    format!("\"{escaped}\"")
}

/// `_print_emoji_detection_script()`.
pub(super) fn emoji_detection_script(siteurl: &str) -> Vec<u8> {
    let text = |s: &str| Value::String(s.as_bytes().to_vec());
    let entry = |k: &str, v: Value| (Key::String(k.as_bytes().to_vec()), v);
    let concatemoji = format!(
        "{siteurl}/wp-includes/js/wp-emoji-release.min.js?ver={}",
        WP_VERSION.trim()
    );
    let settings = Value::Array(vec![
        entry(
            "baseUrl",
            text("https://s.w.org/images/core/emoji/17.0.2/72x72/"),
        ),
        entry("ext", text(".png")),
        entry(
            "svgUrl",
            text("https://s.w.org/images/core/emoji/17.0.2/svg/"),
        ),
        entry("svgExt", text(".svg")),
        entry(
            "source",
            Value::Array(vec![entry("concatemoji", text(&concatemoji))]),
        ),
    ]);
    let loader = script_source(siteurl, "wp-emoji-loader", EMOJI_LOADER);
    let mut out = tag(
        b"script",
        &[
            (b"id", Some(b"wp-emoji-settings")),
            (b"type", Some(b"application/json")),
        ],
        Some(&inline_script(&json_hex_tag(&settings))),
    );
    out.extend(tag(
        b"script",
        &[(b"type", Some(b"module"))],
        Some(&inline_script(&loader)),
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A child theme, and paths with characters URL patterns reserve.
    struct Child;

    impl Site for Child {
        fn option(&self, name: &str) -> Option<Vec<u8>> {
            let value: &[u8] = match name {
                "permalink_structure" => b"/%postname%/",
                "template" => b"parent",
                "stylesheet" => b"child(1)",
                _ => return None,
            };
            Some(value.to_vec())
        }

        fn home_url(&self) -> Vec<u8> {
            b"http://example.test/a+b<c>".to_vec()
        }

        fn post(&self, _id: u64) -> Option<hyscribe_model::Post> {
            None
        }

        fn post_terms(&self, _post_id: u64, _taxonomy: &str) -> Vec<hyscribe_model::Term> {
            Vec::new()
        }

        fn post_meta(&self, _post_id: u64, _key: &str) -> Option<Vec<u8>> {
            None
        }

        fn query(&self, _query: &hyscribe_model::PostQuery) -> Vec<hyscribe_model::Post> {
            Vec::new()
        }
    }

    #[test]
    fn speculation_rules_of_a_child_theme() {
        let rules = String::from_utf8(speculation_rules(&Child, "http://example.test/wp")).unwrap();
        assert!(
            rules.contains(r#"{"href_matches":"/a\\+b\u003Cc\u003E/*"}"#),
            "{rules}"
        );
        assert!(rules.contains(
            r#""/wp/wp-content/themes/parent/*","/wp/wp-content/themes/child\\(1\\)/*","/a\\+b\u003Cc\u003E/*\\?(.+)"]"#
        ), "{rules}");
        assert_eq!(json_string("a\"b"), r#""a\"b""#);
        // The rest of the test site is empty.
        assert!(Child.post(1).is_none() && Child.post_meta(1, "k").is_none());
        assert!(Child.post_terms(1, "category").is_empty() && Child.term(1, "category").is_none());
        assert!(
            Child
                .query(&hyscribe_model::PostQuery::default())
                .is_empty()
        );
    }
}
