// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin's wording of the WordPress strings that `__()` does not reach
//! (ADR 0008): block descriptions translated with their context, and strings
//! of WordPress's script bundles, given to `wp.i18n` as their translations
//! so the bundles stay as released. `reference/admin-look/hyscribe-look.php`
//! gives PHP the same.

use hyscribe_phpser::{Key, Value, json_encode};

/// `_x( $text, 'block description' )`'s changed strings.
const BLOCK_TEXT: [(&str, &str); 3] = [
    (
        "Use the classic WordPress editor.",
        "Use the classic editor.",
    ),
    (
        "Insert additional custom elements with a WordPress shortcode.",
        "Insert additional custom elements with a shortcode.",
    ),
    (
        "Displays a link to edit the comment in the WordPress Dashboard. This link is only visible to users with the edit comment capability.",
        "Displays a link to edit the comment in the Dashboard. This link is only visible to users with the edit comment capability.",
    ),
];

/// The changed strings of each script, by handle.
const SCRIPT_TEXT: [(&str, &[(&str, &str)]); 4] = [
    (
        "wp-block-editor",
        &[(
            "Patterns are available from the <Link>WordPress.org Pattern Directory</Link>, bundled in the active theme, or created by users on this site. Only patterns created on this site can be synced.",
            "Patterns are available from the <Link>Pattern Directory</Link>, bundled in the active theme, or created by users on this site. Only patterns created on this site can be synced.",
        )],
    ),
    (
        "wp-block-library",
        &[
            ("A WordPress Commenter", "A Commenter"),
            ("Reply to A WordPress Commenter", "Reply to A Commenter"),
            (
                "Site Icons are what you see in browser tabs, bookmark bars, and within the WordPress mobile apps. To use a custom icon that is different from your site logo, use the <a>Site Icon settings</a>.",
                "Site Icons are what you see in browser tabs, bookmark bars, and within mobile apps. To use a custom icon that is different from your site logo, use the <a>Site Icon settings</a>.",
            ),
        ],
    ),
    (
        "wp-edit-post",
        &[(
            "In the WordPress editor, each paragraph, image, or video is presented as a distinct \u{201c}block\u{201d} of content.",
            "In the editor, each paragraph, image, or video is presented as a distinct \u{201c}block\u{201d} of content.",
        )],
    ),
    (
        "wp-edit-site",
        &[(
            "%1$s \u{2039} %2$s \u{2039} Editor \u{2014} WordPress",
            "%1$s \u{2039} %2$s \u{2039} Editor \u{2014} HyScribe",
        )],
    ),
];

/// A block's description as hs-admin's editor shows it.
pub(crate) fn block_description(text: &[u8]) -> Option<&'static str> {
    let found = BLOCK_TEXT.iter().find(|(from, _)| from.as_bytes() == text);
    found.map(|(_, to)| *to)
}

/// `load_script_textdomain( $handle )`: the JSON locale data of the
/// `default` text domain hs-admin gives the script, if any.
pub(crate) fn script_translations(handle: &str) -> Option<String> {
    let (_, strings) = SCRIPT_TEXT.iter().find(|(name, _)| *name == handle)?;
    let text = |s: &str| Value::String(s.as_bytes().to_vec());
    let key = |s: &str| Key::String(s.as_bytes().to_vec());
    let header = Value::Array(vec![(key("domain"), text("messages"))]);
    let mut messages = vec![(key(""), header)];
    for (from, to) in strings.iter() {
        messages.push((key(from), Value::Array(vec![(Key::Int(0), text(to))])));
    }
    let messages = Value::Array(vec![(key("messages"), Value::Array(messages))]);
    let data = Value::Array(vec![(key("locale_data"), messages)]);
    json_encode(&data).map(|json| String::from_utf8_lossy(&json).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn block_descriptions_name_no_other_software() {
        assert_eq!(
            block_description(b"Use the classic WordPress editor."),
            Some("Use the classic editor.")
        );
        assert_eq!(block_description(b"Add an image."), None);
    }

    /// The locale data is what PHP's `wp_json_encode()` gives the look
    /// plugin's array.
    #[test]
    fn script_translations_are_jed_locale_data() {
        assert_eq!(
            script_translations("wp-edit-site").as_deref(),
            Some(
                "{\"locale_data\":{\"messages\":{\"\":{\"domain\":\"messages\"},\"%1$s \\u2039 %2$s \\u2039 Editor \\u2014 WordPress\":[\"%1$s \\u2039 %2$s \\u2039 Editor \\u2014 HyScribe\"]}}}"
            )
        );
        assert_eq!(script_translations("wp-blocks"), None);
        for (handle, strings) in SCRIPT_TEXT {
            let json = script_translations(handle).unwrap_or_default();
            assert_eq!(json.matches("\":[\"").count(), strings.len(), "{handle}");
        }
    }
}
