// SPDX-License-Identifier: AGPL-3.0-or-later

//! The playlist blocks: the tracks with their image and artist shown as the
//! playlist sets, and the playlist's store, context and waveform player.
//! Not checked yet: the tracks' data in the footer's Interactivity API
//! state.

use hyscribe_html::TagProcessor;
use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_phpser::{Key, Value, is_empty_string, json_encode};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::formatting::find;
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

impl Renderer<'_> {
    /// `render_block_core_playlist()` around its tracks, rendered with its
    /// `showImages` as context.
    pub(crate) fn playlist(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/playlist", parsed);
        let outer = self.playlist.replace(attributes.clone());
        let content = self.saved_markup(block);
        self.playlist = outer;
        let playlist_id = self.unique_id("playlist-");
        let tracks: Vec<Value> = block
            .inner_blocks
            .iter()
            .filter(|track| track.name.as_deref() == Some("core/playlist-track"))
            .filter(|track| {
                let parsed = track.attrs.as_deref().unwrap_or_default();
                prepared("core/playlist-track", parsed)
                    .get_str("id")
                    .is_some_and(|id| !id.is_empty())
            })
            .enumerate()
            .map(|(index, _)| Value::String(format!("track-{index}").into_bytes()))
            .collect();
        let Some(first) = tracks.first().cloned() else {
            return Vec::new();
        };
        self.assets
            .enqueue_module("@wordpress/block-library/playlist/view");
        let mut waveform = String::new();
        for (key, name) in [
            ("waveformColor", "color"),
            ("waveformGradient", "gradient"),
            ("waveformBackgroundColor", "background-color"),
            ("waveformBackgroundGradient", "background-gradient"),
        ] {
            if let Some(value) = attributes.get_str(key).filter(|v| !v.is_empty()) {
                let value = esc_attr(&value.to_php_string());
                waveform.push_str(&format!(" data-waveform-player-{name}=\"{value}\""));
            }
        }
        let player = format!(
            "<div class=\"wp-block-playlist__waveform-player\"{waveform}\n\t\tdata-wp-watch=\"callbacks.initWaveformPlayer\"\n\t\tdata-label-play=\"Play\"\n\t\tdata-label-pause=\"Pause\"\n\t\tdata-label-seek=\"Seek\"\n\t\tdata-label-seek-value=\"%1$s of %2$s\"\n\t></div>"
        );
        // `preg_replace( '/(<figure[^>]*>)/', '$1' . $html, $content, 1 )`.
        let figure = find(&content, 0, b"<figure")
            .and_then(|start| Some((start, find(&content, start, b">")? + 1)));
        let content = match figure {
            Some((start, end)) => {
                let tag = content.get(start..end).unwrap_or_default();
                let replacement = [b"$1", player.as_bytes()].concat();
                [
                    content.get(..start).unwrap_or_default(),
                    &expand_replacement(&replacement, &[tag, tag]),
                    content.get(end..).unwrap_or_default(),
                ]
                .concat()
            }
            None => content,
        };
        let entry = |key: &str, value: Value| (Key::String(key.as_bytes().to_vec()), value);
        let show_artwork = attributes
            .get_str("showPlayButtonArtwork")
            .is_some_and(|v| !v.is_empty());
        let context = Value::Array(vec![
            entry("playlistId", Value::String(playlist_id)),
            entry("currentId", first),
            entry("isPlaying", Value::Bool(false)),
            entry("tracks", Value::Array(list(tracks.clone()))),
            entry(
                "waveformStyle",
                attributes
                    .get_str("waveformStyle")
                    .cloned()
                    .unwrap_or(Value::Null),
            ),
            entry("showPlayButtonArtwork", Value::Bool(show_artwork)),
            entry("labelPauseTrack", Value::String(b"Pause".to_vec())),
            entry("labelSelectTrack", Value::String(b"Play".to_vec())),
        ]);
        let mut tags = TagProcessor::new(&content);
        tags.next_tag(Some(b"FIGURE"), None);
        tags.set_attribute(b"data-wp-interactive", b"core/playlist");
        tags.set_attribute(
            b"data-wp-context",
            &json_encode(&context).unwrap_or_default(),
        );
        for track in &tracks {
            if !tags.next_tag(None, Some(b"wp-block-playlist-track__button")) {
                break;
            }
            let context = Value::Array(vec![entry("trackId", track.clone())]);
            tags.set_attribute(
                b"data-wp-context",
                &json_encode(&context).unwrap_or_default(),
            );
        }
        tags.get_updated_html().to_vec()
    }

    /// `render_block_core_playlist_track()`: the track's button, with its
    /// image unless the playlist hides images.
    pub(crate) fn playlist_track(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/playlist-track", parsed);
        if attributes.get_str("id").is_none_or(Value::is_empty) {
            return Vec::new();
        }
        let wrapper = get_block_wrapper_attributes(
            "core/playlist-track",
            parsed,
            &[],
            &self.context.theme.global,
        );
        let show_images = self
            .playlist
            .as_ref()
            .and_then(|playlist| playlist.get_str("showImages"))
            .is_none_or(|show| !show.is_empty());
        let text = |key| {
            attributes
                .get_str(key)
                .map(Value::to_php_string)
                .unwrap_or_default()
        };
        let (artist, image, alt, length) = (
            text("artist"),
            text("image"),
            text("imageAlt"),
            text("length"),
        );
        let title = Some(text("title"))
            .filter(|title| !is_empty_string(title))
            .unwrap_or(b"Unknown title".to_vec());
        let mut html = format!(
            "<li {wrapper}><button data-wp-on--click=\"actions.changeTrack\" data-wp-bind--aria-current=\"state.isCurrentTrack\" class=\"wp-block-playlist-track__button\">"
        );
        if show_images && !is_empty_string(&image) {
            html.push_str(&format!(
                "<img class=\"wp-block-playlist-track__image\" src=\"{}\" alt=\"{}\" />",
                String::from_utf8_lossy(&esc_url(&image)),
                esc_attr(&alt)
            ));
        }
        html.push_str("<span class=\"wp-block-playlist-track__content\">");
        html.push_str(&format!(
            "<span class=\"wp-block-playlist-track__title\">{}</span>",
            esc_html(&title)
        ));
        if !is_empty_string(&artist) {
            html.push_str(&format!(
                "<span class=\"wp-block-playlist-track__artist\">{}</span>",
                esc_html(&artist)
            ));
        }
        html.push_str("</span>");
        if !is_empty_string(&length) {
            html.push_str(&format!(
                "<span class=\"wp-block-playlist-track__length\"><span class=\"screen-reader-text\">Duration: </span>{}</span>",
                esc_html(&length)
            ));
        }
        html.push_str("<span class=\"screen-reader-text\" data-wp-text=\"state.trackButtonActionLabel\">Play</span></button></li>");
        html.into_bytes()
    }
}

/// The values as a list, keyed from 0.
fn list(values: Vec<Value>) -> Vec<(Key, Value)> {
    (0..).map(Key::Int).zip(values).collect()
}

/// A `preg_replace()` replacement with its references to `groups` filled
/// in: `\n`, `$n` and `${n}` up to two digits, a backslash before either
/// sign standing for the sign.
fn expand_replacement(replacement: &[u8], groups: &[&[u8]]) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut at = 0;
    let mut last = 0u8;
    while let Some(&byte) = replacement.get(at) {
        if byte == b'\\' || byte == b'$' {
            if last == b'\\' {
                out.pop();
                out.push(byte);
                at += 1;
                last = 0;
                continue;
            }
            if let Some((group, next)) = backref(replacement, at) {
                out.extend_from_slice(groups.get(group).copied().unwrap_or_default());
                at = next;
                continue;
            }
        }
        out.push(byte);
        at += 1;
        last = byte;
    }
    out
}

/// `preg_get_backref()`: the group a reference at `at` names, and where it
/// ends.
fn backref(replacement: &[u8], at: usize) -> Option<(usize, usize)> {
    let braced = replacement.get(at) == Some(&b'$') && replacement.get(at + 1) == Some(&b'{');
    let mut walk = at + 1 + usize::from(braced);
    let digit = |walk: usize| {
        replacement
            .get(walk)
            .filter(|b| b.is_ascii_digit())
            .map(|b| usize::from(b - b'0'))
    };
    let mut group = digit(walk)?;
    walk += 1;
    if let Some(second) = digit(walk) {
        group = group * 10 + second;
        walk += 1;
    }
    if braced {
        (replacement.get(walk) == Some(&b'}')).then_some(())?;
        walk += 1;
    }
    Some((group, walk))
}

#[cfg(test)]
mod tests;
