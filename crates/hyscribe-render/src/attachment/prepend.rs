// SPDX-License-Identifier: AGPL-3.0-or-later

//! `prepend_attachment()`: what `the_content` puts before the content of
//! an attachment that is the global post, a link to its file, or the
//! audio player of a song. A video player is not ported: callers answer
//! 501 for one ([`plays_video`]).

use std::cell::Cell;

use hyscribe_kses::{esc_html, esc_url};
use hyscribe_model::Post;
use hyscribe_phpser::{add_query_arg, is_empty_string, php_trim, wp_basename};

use super::{ImageOptions, attachment_url, wp_get_attachment_image};
use crate::media::Loading;
use crate::site::Site;

/// The extensions `wp_get_audio_extensions()` gives, with their types.
const AUDIO: [(&str, &str); 5] = [
    ("mp3", "audio/mpeg"),
    ("ogg", "audio/ogg"),
    ("flac", "audio/flac"),
    ("m4a", "audio/mpeg"),
    ("wav", "audio/wav"),
];

/// `wp_get_video_extensions()`.
const VIDEO: [&str; 5] = ["mp4", "m4v", "webm", "ogv", "flv"];

/// `wp_attachment_is( $kind, $post )` for `audio` or `video`: an attached
/// file of that type, or of one of `extensions` for an imported one.
fn attachment_is(site: &dyn Site, post: &Post, kind: &str, extensions: &[&str]) -> bool {
    let file = site
        .post_meta(post.id, "_wp_attached_file")
        .unwrap_or_default();
    file_is(&file, &post.mime_type, kind, extensions)
}

/// [`attachment_is`] of a file `file` of type `mime`.
fn file_is(file: &[u8], mime: &str, kind: &str, extensions: &[&str]) -> bool {
    !is_empty_string(file)
        && (mime.starts_with(&format!("{kind}/"))
            || mime == "import" && extensions.contains(&extension(file).as_str()))
}

/// The extension of a file or URL, in lower case, as
/// `wp_check_filetype()` finds it and its caller lowers it.
fn extension(file: &[u8]) -> String {
    let (_, extension) = split_name(file);
    String::from_utf8_lossy(&extension).to_ascii_lowercase()
}

/// A file's name without its directory, split at its last dot: the stem
/// and the extension, empty without a dot.
fn split_name(file: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let name = wp_basename(file);
    let end = name.iter().rposition(|b| *b == b'.').unwrap_or(name.len());
    let extension = name.get(end + 1..).unwrap_or_default().to_vec();
    (name.get(..end).unwrap_or_default().to_vec(), extension)
}

/// Whether `post` shows a video player, which is not ported: a video
/// whose file a player plays.
pub fn plays_video(site: &dyn Site, post: &Post) -> bool {
    attachment_is(site, post, "video", &VIDEO)
        && attachment_url(site, post.id)
            .is_some_and(|url| VIDEO.contains(&extension(&url).as_str()))
}

/// The HTML `prepend_attachment()` puts before `post`'s content, and a
/// newline; `audio` counts `wp_audio_shortcode()`'s instances.
pub(crate) fn prepend_attachment(
    site: &dyn Site,
    loading: &Loading,
    post: &Post,
    audio: &Cell<u64>,
) -> Vec<u8> {
    let url = attachment_url(site, post.id).unwrap_or_default();
    let html = if attachment_is(site, post, "video", &VIDEO) {
        // `wp_video_shortcode()` of a file no player plays.
        embedded("video", &url)
    } else if attachment_is(site, post, "audio", &AUDIO.map(|(ext, _)| ext)) {
        audio_shortcode(post.id, &url, audio)
    } else {
        format!(
            "<p class=\"attachment\">{}</p>",
            String::from_utf8_lossy(&attachment_link(site, loading, post, &url))
        )
        .into_bytes()
    };
    [html, b"\n".to_vec()].concat()
}

/// The link the shortcodes give to a file their player does not play.
fn embedded(kind: &str, url: &[u8]) -> Vec<u8> {
    let (href, text) = (esc_url(url), esc_html(url));
    format!(
        "<a class=\"wp-embedded-{kind}\" href=\"{}\">{text}</a>",
        String::from_utf8_lossy(&href),
    )
    .into_bytes()
}

/// `wp_audio_shortcode( array( 'src' => $url ) )` of attachment `id`.
fn audio_shortcode(id: u64, url: &[u8], instances: &Cell<u64>) -> Vec<u8> {
    let instance = instances.get() + 1;
    instances.set(instance);
    let extension = extension(url);
    let Some((_, mime)) = AUDIO.iter().find(|(ext, _)| *ext == extension) else {
        return embedded("audio", url);
    };
    let source = add_query_arg(&[(b"_", Some(instance.to_string().as_bytes()))], url);
    let url = String::from_utf8_lossy(&esc_url(url)).into_owned();
    format!(
        "<audio class=\"wp-audio-shortcode\" id=\"audio-{id}-{instance}\" preload=\"none\" style=\"width: 100%;\" controls=\"controls\"><source type=\"{mime}\" src=\"{}\" /><a href=\"{url}\">{url}</a></audio>",
        String::from_utf8_lossy(&esc_url(&source)),
    )
    .into_bytes()
}

/// `wp_get_attachment_link( 0, 'medium' )`: the medium image, or the
/// title, or the file's name, linked to the file.
fn attachment_link(site: &dyn Site, loading: &Loading, post: &Post, url: &[u8]) -> Vec<u8> {
    if url.is_empty() {
        return b"Missing Attachment".to_vec();
    }
    // Inside `the_content`, which adds the loading attributes after.
    let mut loading = loading.clone();
    loading.in_the_content = true;
    let image = wp_get_attachment_image(
        site,
        &loading,
        post.id,
        "medium",
        &[],
        &ImageOptions::default(),
    );
    let text = if !php_trim(&image).is_empty() {
        image
    } else if !php_trim(&post.title).is_empty() {
        post.title.clone()
    } else {
        let file = site
            .post_meta(post.id, "_wp_attached_file")
            .unwrap_or_default();
        esc_html(&split_name(&file).0).into_bytes()
    };
    [b"<a href='", &esc_url(url)[..], b"'>", &text, b"</a>"].concat()
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use hyscribe_model::{Post, PostQuery, Term};

    use super::{VIDEO, file_is, prepend_attachment};
    use crate::media::Loading;
    use crate::site::Site;

    /// A site whose attachments' files are named by their IDs.
    struct Files;

    const FILES: [(u64, &str); 6] = [
        (1, "2026/10/clip.mov"),
        (2, "2026/10/song.mp3"),
        (3, "2026/10/voice.aac"),
        (4, "2026/10/notes.txt"),
        (6, "2026/10/dot.png"),
        (7, "2026/10/clip.mp4"),
    ];

    impl Site for Files {
        fn option(&self, _: &str) -> Option<Vec<u8>> {
            None
        }
        fn home_url(&self) -> Vec<u8> {
            b"http://example.test".to_vec()
        }
        fn post(&self, id: u64) -> Option<Post> {
            // Only the dot is an image.
            let mime = if id == 6 { "image/png" } else { "" };
            Some(attachment(id, mime, "Dot"))
        }
        fn post_terms(&self, _: u64, _: &str) -> Vec<Term> {
            Vec::new()
        }
        fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
            if key == "_wp_attachment_metadata" {
                return Some(b"a:2:{s:5:\"width\";i:10;s:6:\"height\";i:10;}".to_vec());
            }
            let file = FILES.iter().find(|(known, _)| *known == id);
            file.filter(|_| key == "_wp_attached_file")
                .map(|(_, file)| file.as_bytes().to_vec())
        }
        fn query(&self, _: &PostQuery) -> Vec<Post> {
            Vec::new()
        }
    }

    fn attachment(id: u64, mime: &str, title: &str) -> Post {
        Post {
            id,
            post_type: "attachment".into(),
            mime_type: mime.into(),
            title: title.as_bytes().to_vec(),
            ..Post::default()
        }
    }

    /// What `prepend_attachment()` puts before each kind of attachment:
    /// a link to a video no player plays, a player for a song and a link
    /// to one it does not play, the title of a file that is no image, its
    /// file's name without one, and without a file a missing one.
    #[test]
    fn attachments_are_prepended() {
        let audio = Cell::new(0);
        let prepend = |post: &Post| {
            let html = prepend_attachment(&Files, &Loading::default(), post, &audio);
            String::from_utf8(html).unwrap()
        };
        // No `siteurl`: the uploads sit at the root.
        let uploads = "/wp-content/uploads/2026/10";
        let cases = [
            (
                attachment(1, "video/quicktime", "Clip"),
                format!(
                    "<a class=\"wp-embedded-video\" href=\"{uploads}/clip.mov\">{uploads}/clip.mov</a>\n"
                ),
            ),
            (
                attachment(2, "audio/mpeg", "Song"),
                format!(
                    "<audio class=\"wp-audio-shortcode\" id=\"audio-2-1\" preload=\"none\" style=\"width: 100%;\" controls=\"controls\"><source type=\"audio/mpeg\" src=\"{uploads}/song.mp3?_=1\" /><a href=\"{uploads}/song.mp3\">{uploads}/song.mp3</a></audio>\n"
                ),
            ),
            (
                attachment(3, "audio/aac", "Voice"),
                format!(
                    "<a class=\"wp-embedded-audio\" href=\"{uploads}/voice.aac\">{uploads}/voice.aac</a>\n"
                ),
            ),
            (
                attachment(4, "text/plain", "Notes"),
                format!("<p class=\"attachment\"><a href='{uploads}/notes.txt'>Notes</a></p>\n"),
            ),
            (
                attachment(4, "text/plain", ""),
                format!("<p class=\"attachment\"><a href='{uploads}/notes.txt'>notes</a></p>\n"),
            ),
            (
                attachment(5, "image/png", ""),
                "<p class=\"attachment\">Missing Attachment</p>\n".to_string(),
            ),
            (
                attachment(6, "image/png", "Dot"),
                format!(
                    "<p class=\"attachment\"><a href='{uploads}/dot.png'><img width=\"10\" height=\"10\" src=\"{uploads}/dot.png\" class=\"attachment-medium size-medium\" alt=\"\" /></a></p>\n"
                ),
            ),
        ];
        for (post, html) in cases {
            assert_eq!(prepend(&post), html, "{}", post.id);
        }
        assert_eq!(audio.get(), 2);
        // Only a file a player plays is a video player, which is not ported.
        assert!(super::plays_video(&Files, &attachment(7, "video/mp4", "")));
        assert!(!super::plays_video(
            &Files,
            &attachment(1, "video/quicktime", "")
        ));
        // The site's other reads, which prepending makes none of.
        assert_eq!(Files.home_url(), b"http://example.test");
        assert!(Files.post_terms(1, "category").is_empty());
        assert!(Files.query(&PostQuery::default()).is_empty());
    }

    /// An imported file is a video by its extension, any other by its type.
    #[test]
    fn imported_files_are_known_by_extension() {
        assert!(file_is(b"a/clip.MP4", "import", "video", &VIDEO));
        assert!(!file_is(b"a/notes.txt", "import", "video", &VIDEO));
        assert!(!file_is(b"a/clip.mp4", "video", "video", &VIDEO));
        assert!(file_is(b"a/clip.mov", "video/quicktime", "video", &VIDEO));
        assert!(!file_is(b"", "video/mp4", "video", &VIDEO));
    }
}
