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
    let siteurl = siteurl(site);
    let mut out = footer_actions(site, &siteurl, assets);
    out.extend(footer_scripts(&siteurl, assets));
    out
}

pub(crate) fn siteurl(site: &dyn Site) -> String {
    String::from_utf8_lossy(&site.option("siteurl").unwrap_or_default()).into_owned()
}

/// What `wp_footer()`'s callbacks at priority 10 print: the speculation
/// rules, the blocks' footer markup and the script modules.
pub(crate) fn footer_actions(site: &dyn Site, siteurl: &str, assets: &Assets) -> Vec<u8> {
    let registry = registered_modules(siteurl);
    let mut out = speculation_rules(site, siteurl);
    out.extend_from_slice(&assets.footer);
    let modules = Modules {
        registry: &registry,
        queue: &assets.modules,
    };
    out.extend(modules.footer());
    out.extend(modules.data(&assets.state));
    out
}

/// `wp_print_footer_scripts()`, at priority 20: the scripts in the order
/// they were enqueued, then the emoji detection script.
pub(crate) fn footer_scripts(siteurl: &str, assets: &Assets) -> Vec<u8> {
    let siteurl = siteurl.to_owned();
    let mut out = Vec::new();
    let at = |handles: &[&str]| {
        let wanted = |h: &String| handles.contains(&h.as_str());
        assets.scripts.iter().position(wanted)
    };
    let reply = at(&["comment-reply"]).map(|at| (at, comment_reply(&siteurl)));
    let media = at(&["wp-mediaelement", "mediaelement-vimeo"])
        .map(|at| (at, mediaelement_scripts(&siteurl, &assets.scripts)));
    let mut printed: Vec<(usize, Vec<u8>)> = reply.into_iter().chain(media).collect();
    printed.sort_by_key(|(at, _)| *at);
    out.extend(printed.into_iter().flat_map(|(_, script)| script));
    out.extend(emoji_detection_script(&siteurl));
    out
}

/// The `comment-reply` script's tag.
fn comment_reply(siteurl: &str) -> Vec<u8> {
    let src = esc_url_raw(format!("{siteurl}{COMMENT_REPLY}?ver={}", WP_VERSION.trim()).as_bytes());
    tag(
        b"script",
        &[
            (b"src", Some(&src)),
            (b"id", Some(b"comment-reply-js")),
            (b"async", None),
            (b"data-wp-strategy", Some(b"async")),
            (b"fetchpriority", Some(b"low")),
        ],
        None,
    )
}

/// MediaElement's `mejsL10n` for locale en_US, as `wp_default_scripts()`
/// builds it.
const MEJS_L10N: &str = include_str!("mejs-l10n.js");

/// What MediaElement's scripts depend on and `wp_print_head_scripts()`
/// prints: jQuery, which a script enqueued while the template rendered, as
/// a block theme's are, finds the head still to print.
pub(crate) fn head_scripts(siteurl: &str, scripts: &[String]) -> Vec<u8> {
    if !scripts
        .iter()
        .any(|h| h == "wp-mediaelement" || h == "mediaelement-vimeo")
    {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (id, path, version) in [
        ("jquery-core", "jquery.min.js", "3.7.1"),
        ("jquery-migrate", "jquery-migrate.min.js", "3.4.1"),
    ] {
        let url = format!("{siteurl}/wp-includes/js/jquery/{path}?ver={version}");
        let src = esc_url_raw(url.as_bytes());
        let id = format!("{id}-js");
        out.extend(tag(
            b"script",
            &[(b"id", Some(id.as_bytes())), (b"src", Some(&src))],
            None,
        ));
    }
    out
}

/// The MediaElement scripts `wp_audio_shortcode()` and `wp_video_shortcode()`
/// enqueue, in the order they were enqueued, after what they depend on.
pub(crate) fn mediaelement_scripts(siteurl: &str, scripts: &[String]) -> Vec<u8> {
    let script = |id: &str, path: &str, version: &str| {
        let url = format!("{siteurl}/wp-includes/js/mediaelement/{path}?ver={version}");
        let src = esc_url_raw(url.as_bytes());
        let id = format!("{id}-js");
        tag(
            b"script",
            &[(b"id", Some(id.as_bytes())), (b"src", Some(&src))],
            None,
        )
    };
    let inline = |id: &str, data: &str| {
        let text = inline_script(format!("{data}\n//# sourceURL={id}").as_bytes());
        tag(b"script", &[(b"id", Some(id.as_bytes()))], Some(&text))
    };
    let version = WP_VERSION.trim();
    let mut out = Vec::new();
    for handle in scripts {
        let own = match handle.as_str() {
            "wp-mediaelement" => script("wp-mediaelement", "wp-mediaelement.min.js", version),
            "mediaelement-vimeo" => {
                script("mediaelement-vimeo", "renderers/vimeo.min.js", "4.2.17")
            }
            _ => continue,
        };
        if out.is_empty() {
            out.extend(inline("mediaelement-core-js-before", MEJS_L10N));
            out.extend(script(
                "mediaelement-core",
                "mediaelement-and-player.min.js",
                "4.2.17",
            ));
            out.extend(script(
                "mediaelement-migrate",
                "mediaelement-migrate.min.js",
                version,
            ));
            let path = hyscribe_kses::wp_parse_url(siteurl.as_bytes())
                .and_then(|parts| parts.path)
                .unwrap_or_default();
            let path = String::from_utf8_lossy(&path)
                .trim_end_matches('/')
                .to_owned();
            let settings = format!(
                "var _wpmejsSettings = {{\"pluginPath\":\"{path}/wp-includes/js/mediaelement/\",\"classPrefix\":\"mejs-\",\"stretching\":\"responsive\",\"audioShortcodeLibrary\":\"mediaelement\",\"videoShortcodeLibrary\":\"mediaelement\"}};"
            );
            out.extend(inline("mediaelement-js-extra", &settings));
        }
        out.extend(own);
    }
    out
}

/// `wp_print_speculation_rules()`: conservative prefetching of the site's
/// own pages, which WordPress turns on only under pretty permalinks and
/// for a visitor.
fn speculation_rules(site: &dyn Site, siteurl: &str) -> Vec<u8> {
    if !crate::permalinks::Permalinks::of(site).pretty() || site.viewer().user != 0 {
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
        // WordPress names its `template` context after the stylesheet's
        // directory and its `stylesheet` context after the template's.
        themes("stylesheet"),
        themes("template"),
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
pub fn emoji_detection_script(siteurl: &str) -> Vec<u8> {
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
            r#""/wp/wp-content/themes/child\\(1\\)/*","/wp/wp-content/themes/parent/*","/a\\+b\u003Cc\u003E/*\\?(.+)"]"#
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
