// SPDX-License-Identifier: AGPL-3.0-or-later

//! WordPress core's `wp-includes` files from the pinned release, every one
//! but its PHP, built in so they are served at their usual URLs.

/// The files by path under `wp-includes/`, with their content types.
const FILES: &[(&str, &str, &[u8])] = include!(concat!(env!("OUT_DIR"), "/wp_includes.rs"));

/// One of the files under `wp-includes/`, as the web server sends it: its
/// content type (empty for none) and bytes.
pub fn wp_includes(path: &str) -> Option<(&'static str, &'static [u8])> {
    FILES
        .binary_search_by(|(name, _, _)| name.cmp(&path))
        .ok()
        .and_then(|i| FILES.get(i))
        .map(|(_, content_type, bytes)| (*content_type, *bytes))
}

#[cfg(test)]
mod tests {
    use super::wp_includes;

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
}
