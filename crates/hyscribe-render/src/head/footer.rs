// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_footer()` for a block theme: the script modules left for the footer,
//! the footer scripts, and the emoji detection script.

use hyscribe_kses::esc_url_raw;
use hyscribe_phpser::{Key, Value};

use super::{Assets, Modules, WP_VERSION, json_hex_tag, registered_modules, tag};
use crate::site::Site;

const EMOJI_LOADER: &str =
    include_str!("../../../../wordpress/wp-includes/js/wp-emoji-loader.min.js");

/// The classic scripts blocks enqueue, as `wp_default_scripts()` registers
/// them: handle, path, loading strategy and fetch priority.
const SCRIPTS: [(&str, &str, &str, &str); 1] = [(
    "comment-reply",
    "/wp-includes/js/comment-reply.min.js",
    "async",
    "low",
)];

/// What `wp_footer()` prints after the template.
pub fn wp_footer(site: &dyn Site, assets: &Assets) -> Vec<u8> {
    let siteurl = String::from_utf8_lossy(&site.option("siteurl").unwrap_or_default()).into_owned();
    let registry = registered_modules(&siteurl);
    let mut out = Modules {
        registry: &registry,
        queue: &assets.modules,
    }
    .footer();
    // wp_print_footer_scripts()
    for handle in &assets.scripts {
        for (name, path, strategy, fetchpriority) in SCRIPTS.iter().filter(|s| s.0 == handle) {
            let src = esc_url_raw(format!("{siteurl}{path}?ver={}", WP_VERSION.trim()).as_bytes());
            let id = format!("{name}-js");
            out.extend(tag(
                b"script",
                &[
                    (b"src", Some(&src)),
                    (b"id", Some(id.as_bytes())),
                    (strategy.as_bytes(), None),
                    (b"data-wp-strategy", Some(strategy.as_bytes())),
                    (b"fetchpriority", Some(fetchpriority.as_bytes())),
                ],
                None,
            ));
        }
    }
    out.extend(emoji_detection_script(&siteurl));
    out
}

/// `_print_emoji_detection_script()`.
fn emoji_detection_script(siteurl: &str) -> Vec<u8> {
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
    let loader_url =
        esc_url_raw(format!("{siteurl}/wp-includes/js/wp-emoji-loader.min.js").as_bytes());
    let loader = [
        EMOJI_LOADER
            .trim_end_matches([' ', '\t', '\n', '\r', '\0', '\x0b'])
            .as_bytes(),
        b"\n//# sourceURL=",
        &loader_url,
    ]
    .concat();
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

/// The text `wp_get_inline_script_tag()` gives a script: between
/// newlines, after trimming newlines and spaces, which these scripts have
/// none of at their ends.
fn inline_script(data: &[u8]) -> Vec<u8> {
    [&b"\n"[..], data, b"\n"].concat()
}
