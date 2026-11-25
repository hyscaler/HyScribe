// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin's wording of the WordPress strings that `__()` does not reach
//! (ADR 0008): block descriptions translated with their context, and strings
//! of WordPress's script bundles, given to `wp.i18n` as their translations
//! so the bundles stay as released; and the brand's marks on its own pages.
//! `reference/admin-look/hyscribe-look.php` gives PHP the same.

use hyscribe_kses::esc_url;
use hyscribe_phpser::{Key, Value, json_encode, rawurlencode};

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

/// The post editor's publish panel shows the WordPress logo as an `svg`
/// when the site has no Site Icon (an `img` of the icon otherwise): the
/// mark is painted over it and the logo's path hidden, the bundle unchanged.
pub(crate) fn editor_icon_style(admin_url: &dyn Fn(&str) -> String) -> String {
    let url = mark_url(admin_url, MARK);
    format!(
        "svg.components-site-icon{{background:url({url}) center/36px no-repeat}}svg.components-site-icon path{{display:none}}"
    )
}

/// The address people porting themes or plugins write to.
const CONTACT: &str = "contact@hyscribe.com";

/// The release notes of a HyScribe version on GitHub.
pub(crate) fn release_url(version: &str) -> String {
    format!("https://github.com/hyscaler/HyScribe/releases/tag/v{version}")
}

/// The host and port of a URL, as `wp_parse_url()` finds them.
fn domain(url: &str) -> &str {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    authority.rsplit('@').next().unwrap_or_default()
}

/// The `mailto:` link asking for porting help, as an escaped attribute: the
/// subject names the site's domain, the body its home URL and HyScribe's
/// version (RFC 6068, line breaks as `%0D%0A`).
pub(crate) fn porting_mailto(home_url: &str, version: &str) -> String {
    let encode = |text: &str| String::from_utf8_lossy(&rawurlencode(text.as_bytes())).into_owned();
    let body = format!(
        "Site: {home_url}\r\nHyScribe version: {version}\r\n\r\nWhat I need help with:\r\n"
    );
    format!(
        "mailto:{CONTACT}?subject={}&amp;body={}",
        encode(&format!("Porting help for {}", domain(home_url))),
        encode(&body)
    )
}

/// The dashboard's welcome banner in the brand's colours (the scheme's
/// ink, blue and sky): core's artwork hidden, the mark beside the text.
pub(crate) fn welcome_style(admin_url: &dyn Fn(&str) -> String) -> String {
    let url = mark_url(admin_url, MARK);
    format!(
        ".welcome-panel-header-wrap{{background-color:#131b26;border-bottom:4px solid #1a5fd0}}.welcome-panel-header-image{{display:none}}.welcome-panel-header{{background:url({url}) no-repeat right 48px center/96px 96px;padding-right:192px}}.welcome-panel-header p a{{color:#7db2ff;text-decoration:underline}}.welcome-panel-header p.hyscribe-porting{{font-size:16px}}.welcome-panel-column>svg rect{{fill:#1a5fd0}}@media screen and (max-width:600px){{.welcome-panel-header{{background-image:none;padding-right:32px}}}}"
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

    #[test]
    fn editor_icon_covers_only_the_fallback_svg() {
        let style = editor_icon_style(&|path| format!("/hs-admin/{path}"));
        assert_eq!(
            style,
            "svg.components-site-icon{background:url(/hs-admin/images/hyscribe-mark.svg) center/36px no-repeat}svg.components-site-icon path{display:none}"
        );
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

    #[test]
    fn the_release_notes_are_the_versions_tag() {
        assert_eq!(
            release_url("0.1.0"),
            "https://github.com/hyscaler/HyScribe/releases/tag/v0.1.0"
        );
    }

    /// Subject and body as `rawurlencode()` makes them: spaces `%20`, line
    /// breaks `%0D%0A`, and the attribute's `&` escaped.
    #[test]
    fn the_porting_link_names_the_domain_and_the_site() {
        assert_eq!(
            porting_mailto("https://example.com/", "0.1.0"),
            "mailto:contact@hyscribe.com?subject=Porting%20help%20for%20example.com&amp;body=Site%3A%20https%3A%2F%2Fexample.com%2F%0D%0AHyScribe%20version%3A%200.1.0%0D%0A%0D%0AWhat%20I%20need%20help%20with%3A%0D%0A"
        );
    }

    #[test]
    fn the_domain_keeps_its_port_and_drops_the_folder() {
        let link = porting_mailto("http://localhost:8081/blog/", "0.1.0");
        assert!(link.contains("subject=Porting%20help%20for%20localhost%3A8081&amp;body="));
        assert!(link.contains("Site%3A%20http%3A%2F%2Flocalhost%3A8081%2Fblog%2F%0D%0A"));
        assert!(!link.contains('+'));
    }

    #[test]
    fn the_domain_is_the_host_alone() {
        for (url, domain) in [
            ("https://user:pw@example.com/a?b#c", "example.com"),
            ("https://example.com?x=1", "example.com"),
            ("example.com/x", "example.com"),
            ("https://b\u{fc}cher.example/", "b\u{fc}cher.example"),
            ("", ""),
        ] {
            assert_eq!(super::domain(url), domain, "{url}");
        }
        let link = porting_mailto("https://b\u{fc}cher.example/a&b c", "0.1.0");
        assert!(link.contains("for%20b%C3%BCcher.example&amp;body="));
        assert!(link.contains("a%26b%20c%0D%0A"));
    }

    #[test]
    fn the_banner_style_uses_the_brand_mark() {
        let style = welcome_style(&|path| format!("/hs-admin/{path}"));
        assert!(style.contains("url(/hs-admin/images/hyscribe-mark.svg) no-repeat"));
        assert!(style.contains("background-color:#131b26"));
    }
}
