// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress core's `wp-includes` files from the pinned release, every one
//! but its PHP, built in so they are served at their usual URLs.

/// The files by path under `wp-includes/`, with their content types.
const FILES: &[(&str, &str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/wp_includes.rs"));

/// The files by path under `wp-admin/`, its CSS, scripts and images.
const ADMIN_FILES: &[(&str, &str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/wp_admin.rs"));

/// The files under `wp-includes/` that PHP reads but that are not served.
const READ: &[(&str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/read.rs"));

/// A file under `wp-includes/` that PHP reads though it is not served.
pub fn wp_includes_read(path: &str) -> Option<&'static [u8]> {
    READ.iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}

/// Every file served from the release, by path under `wordpress/`, with
/// its modification time and SHA-256 in hex.
const RELEASE: &[(&str, u64, &str)] = include!(concat!(env!("OUT_DIR"), "/release.rs"));

/// The modification time (Unix seconds) and SHA-256 (hex) that a file
/// served from the release has there, by its path under `wordpress/`.
pub fn release_file(path: &str) -> Option<(u64, &'static str)> {
    RELEASE
        .binary_search_by(|(name, _, _)| name.cmp(&path))
        .ok()
        .and_then(|i| RELEASE.get(i))
        .map(|(_, modified, sha)| (*modified, *sha))
}

/// One of the files under `wp-admin/`, as the web server sends it.
pub fn wp_admin(path: &str) -> Option<(&'static str, &'static [u8])> {
    lookup(ADMIN_FILES, path)
}

/// HyScribe's own files under `wp-admin/` (ADR 0008), with their SHA-256.
const OWN: &[(&str, &str, &[u8], &str)] = include!(concat!(env!("OUT_DIR"), "/own.rs"));

/// One of HyScribe's own files under `wp-admin/`, the brand's marks and the
/// HyScribe colour scheme: its content type, bytes and SHA-256 in hex.
pub fn hyscribe_file(path: &str) -> Option<(&'static str, &'static [u8], &'static str)> {
    OWN.iter()
        .find(|(name, _, _, _)| *name == path)
        .map(|(_, content_type, bytes, sha)| (*content_type, *bytes, *sha))
}

/// One of the files under `wp-includes/`, as the web server sends it: its
/// content type (empty for none) and bytes.
pub fn wp_includes(path: &str) -> Option<(&'static str, &'static [u8])> {
    lookup(FILES, path)
}

fn lookup(
    files: &'static [(&str, &str, &[u8])],
    path: &str,
) -> Option<(&'static str, &'static [u8])> {
    files
        .binary_search_by(|(name, _, _)| name.cmp(&path))
        .ok()
        .and_then(|i| files.get(i))
        .map(|(_, content_type, bytes)| (*content_type, *bytes))
}

#[cfg(test)]
mod tests {
    use super::{hyscribe_file, release_file, wp_admin, wp_includes, wp_includes_read};
    use sha2::{Digest, Sha256};

    /// The brand's marks as kept in `brand/`, and the scheme's files core
    /// would serve of its own, none of them in the release.
    #[test]
    fn hyscribe_files() {
        let mark = include_bytes!("../../../brand/hyscribe-mark.svg");
        let (content_type, bytes, sha) =
            hyscribe_file("images/hyscribe-mark.svg").unwrap_or_default();
        assert_eq!((content_type, bytes), ("image/svg+xml", &mark[..]));
        let digest: String = Sha256::digest(mark)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(sha, digest);
        let types = [
            ("images/hyscribe-mark-small.svg", "image/svg+xml"),
            ("css/colors/hyscribe/colors.min.css", "text/css"),
            ("css/colors/hyscribe/colors-rtl.min.css", "text/css"),
            ("css/colors/hyscribe/colors.scss", ""),
        ];
        for (path, expected) in types {
            assert_eq!(hyscribe_file(path).map(|f| f.0), Some(expected), "{path}");
            assert!(wp_admin(path).is_none(), "{path}");
        }
        for missing in [
            "css/colors/hyscribe/colors.css",
            "images/wordpress-logo.svg",
            "",
        ] {
            assert!(hyscribe_file(missing).is_none(), "{missing}");
        }
    }

    #[test]
    fn release_dates_and_digests() {
        let (modified, sha) =
            release_file("wp-includes/js/jquery/jquery.min.js").unwrap_or_default();
        // Mon, 28 Aug 2023 17:14:23 GMT, as the oracle's web server sends it.
        assert_eq!(modified, 1_693_242_863);
        assert_eq!(sha.len(), 64);
        assert!(release_file("wp-content/themes/twentytwentyfive/style.css").is_some());
        assert!(release_file("wp-content/themes/twentytwentyfive/functions.php").is_none());
        assert!(
            release_file("wp-content/themes/twentytwentyfour/assets/css/button-outline.css")
                .is_some()
        );
        assert!(release_file("wp-content/themes/twentytwentythree/theme.json").is_some());
        // From the pinned archive, not the release.
        assert!(release_file("wp-content/themes/twentytwentytwo/theme.json").is_some());
        assert!(release_file("wp-includes/version.php").is_none());
    }

    #[test]
    fn files_by_path() {
        let (content_type, bytes) = wp_includes("js/jquery/jquery.min.js").unwrap_or_default();
        assert_eq!((content_type, bytes.len()), ("text/javascript", 87553));
        assert_eq!(wp_includes("sodium_compat/LICENSE").map(|f| f.0), Some(""));
        assert_eq!(
            wp_includes("images/wpicons.png").map(|f| f.0),
            Some("image/png")
        );
        for missing in ["js/jquery/jquery.js", "version.php", "nope.js", ""] {
            assert!(wp_includes(missing).is_none(), "{missing}");
        }
    }

    /// The default editor styles, which `get_default_block_editor_settings()`
    /// reads though no URL serves them.
    #[test]
    fn files_php_reads() {
        let css = wp_includes_read("css/dist/block-editor/default-editor-styles.css");
        let css = css.unwrap_or_default();
        assert_eq!(css.len(), 1310);
        assert!(css.starts_with(b"/**\n * SCSS Variables."));
        assert!(wp_includes("css/dist/block-editor/default-editor-styles.css").is_none());
        assert!(wp_includes_read("css/dist/block-editor/content.css").is_none());
    }
}
