// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin's wording of the WordPress strings that `__()` does not reach
//! (ADR 0008): block descriptions translated with their context, and strings
//! of WordPress's script bundles, given to `wp.i18n` as their translations
//! so the bundles stay as released; and the brand's marks on its own pages.
//! `reference/admin-look/hyscribe-look.php` gives PHP the same.

use hyscribe_kses::esc_url;
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

/// The brand's mark and small mark, as hs-admin serves them from `brand/`.
const MARK: &str = "images/hyscribe-mark.svg";
const SMALL_MARK: &str = "images/hyscribe-mark-small.svg";

/// An escaped URL of one of the marks, by `admin_url( $path )`.
fn mark_url(admin_url: &dyn Fn(&str) -> String, mark: &str) -> String {
    String::from_utf8_lossy(&esc_url(admin_url(mark).as_bytes())).into_owned()
}

/// The small mark as the page's icon, printed where the site has no Site
/// Icon.
pub(crate) fn mark_icon(admin_url: &dyn Fn(&str) -> String) -> String {
    let url = mark_url(admin_url, SMALL_MARK);
    format!("<link rel=\"icon\" href=\"{url}\" type=\"image/svg+xml\" />\n")
}

/// `/hs-login`'s mark in place of the WordPress logo, and the HyScribe
/// scheme's theme colour, as the login page loads no scheme.
pub(crate) fn login_style(admin_url: &dyn Fn(&str) -> String) -> String {
    let url = mark_url(admin_url, MARK);
    format!(
        "<style>.login h1 a{{background-image:url({url});}}body.admin-color-hyscribe{{--wp-admin-theme-color:#1a5fd0;--wp-admin-theme-color--rgb:26,95,208;--wp-admin-theme-color-darker-10:#1755b9;--wp-admin-theme-color-darker-10--rgb:23,85,185;--wp-admin-theme-color-darker-20:#144aa3;--wp-admin-theme-color-darker-20--rgb:20,74,163;}}</style>\n"
    )
}

/// The admin bar's HyScribe item: the small mark as its icon's `:before`, as
/// the bar's stylesheet takes icons' own background images away, hidden on
/// the narrowest screens as the WordPress logo it replaces is.
pub(crate) fn bar_style(admin_url: &dyn Fn(&str) -> String) -> String {
    let url = mark_url(admin_url, SMALL_MARK);
    format!(
        "#wpadminbar #wp-admin-bar-hyscribe>.ab-item{{padding:0 1px 0 7px}}#wpadminbar #wp-admin-bar-hyscribe .ab-icon:before{{content:\"\";display:block;width:20px;height:20px;background:url({url}) center/20px no-repeat}}@media screen and (max-width:782px){{#wpadminbar li#wp-admin-bar-hyscribe{{display:block}}#wpadminbar #wp-admin-bar-hyscribe>.ab-item{{padding:0 13px}}#wpadminbar #wp-admin-bar-hyscribe .ab-icon:before{{width:26px;height:26px;margin-top:10px;background-size:26px}}}}@media screen and (max-width:600px){{#wpadminbar li#wp-admin-bar-hyscribe{{display:none}}}}"
    )
}

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
