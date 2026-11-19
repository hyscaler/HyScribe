// SPDX-License-Identifier: AGPL-3.0-or-later

use std::cmp::Ordering;

use hyscribe_model::{OrderBy, Post, PostQuery};
use hyscribe_phpser::{Key, Value};
use hyscribe_testkit::unhex;
use sha2::{Digest, Sha256};

use super::*;
use crate::fixture::ThemeTestData;
use crate::head::{head_scripts, mediaelement_scripts};
use crate::outbound::Hooks;
use crate::shortcode::Shortcode;
use crate::site::{Request, Site};
use crate::theme::twentytwentyfive;

const GOLDEN: &str = include_str!("../../tests/golden/shortcodes.tsv");

/// The Theme Test Data answering the attachment queries of galleries and
/// players, with filters a plugin attaches.
struct Library<'a> {
    data: ThemeTestData,
    hooks: &'a dyn Hooks,
}

fn library(hooks: &dyn Hooks) -> Library<'_> {
    Library {
        data: ThemeTestData {
            home: b"http://127.0.0.1:8081".to_vec(),
        },
        hooks,
    }
}

impl Site for Library<'_> {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        self.data.option(name)
    }
    fn home_url(&self) -> Vec<u8> {
        self.data.home_url()
    }
    fn post(&self, id: u64) -> Option<Post> {
        self.data.post(id)
    }
    fn post_terms(&self, id: u64, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        self.data.post_terms(id, taxonomy)
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.data.post_meta(id, key)
    }
    fn hooks(&self) -> &dyn Hooks {
        self.hooks
    }
    /// The attachments asked for, among those of the fixture, in the
    /// orders the golden uses; MariaDB's ties are broken by ID.
    fn query(&self, query: &PostQuery) -> Vec<Post> {
        assert_eq!(query.post_types, ["attachment"]);
        let prefix = query.mime_types[0].trim_end_matches('%').to_string();
        let mut found: Vec<Post> = self
            .data
            .posts()
            .into_iter()
            .filter(|post| post.post_type == "attachment" && post.status == "inherit")
            .filter(|post| post.mime_type.starts_with(&prefix))
            .filter(|post| query.parents.is_empty() || query.parents.contains(&post.parent))
            .filter(|post| {
                query
                    .include
                    .as_ref()
                    .is_none_or(|ids| ids.contains(&post.id))
            })
            .filter(|post| !query.exclude.contains(&post.id))
            .collect();
        found.sort_by(|a, b| {
            let mut order = Ordering::Equal;
            for &(by, ascending) in &query.order {
                let by = match by {
                    OrderBy::MenuOrder => a.menu_order.cmp(&b.menu_order),
                    OrderBy::Id => a.id.cmp(&b.id),
                    OrderBy::Title => a.title.cmp(&b.title),
                    OrderBy::Date => a.date.cmp(&b.date),
                    OrderBy::Included => {
                        let include = query.include.as_ref().unwrap();
                        let at = |post: &Post| include.iter().position(|id| *id == post.id);
                        at(a).cmp(&at(b))
                    }
                    other => panic!("no {other:?} order in the golden"),
                };
                order = order.then(if ascending { by } else { by.reverse() });
            }
            order.then(a.id.cmp(&b.id))
        });
        found.truncate(query.limit as usize);
        found
    }
}

/// The 64 hex digits of the SHA-256 of `data`.
fn sha256(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Every case of `reference/shortcodes/golden.sh` through the content
/// filters, in order, as the counters of galleries, players and unique IDs
/// run through the whole list; and the scripts the players enqueued.
#[test]
fn shortcodes_match_php() {
    let theme = twentytwentyfive();
    let site = library(&());
    let (page, feed) = (
        Request::default(),
        Request {
            feed: true,
            ..Request::default()
        },
    );
    let mut renderer = Renderer::new(&theme, &site, &page);
    let mut feeding = false;
    let (mut failures, mut cases) = (Vec::new(), 0);
    for line in GOLDEN.lines() {
        let fields: Vec<&str> = line.split('\t').collect();
        if !matches!(fields[0], "0" | "1") {
            continue;
        }
        if fields[0] == "1" && !feeding {
            // `is_feed()` turns true: the counters go on.
            let mut next = Renderer::new(&theme, &site, &feed);
            next.gallery_instances = renderer.gallery_instances;
            next.audio_instances = renderer.audio_instances.clone();
            next.video_instances = renderer.video_instances;
            next.prefixed_ids = renderer.prefixed_ids.clone();
            next.assets = renderer.assets;
            renderer = next;
            feeding = true;
        }
        let id: u64 = fields[1].parse().unwrap();
        renderer.context.post_id = (id != 0).then_some(id);
        let content = unhex(fields[2]);
        let ours = renderer.the_content_filters(&content);
        cases += 1;
        if sha256(&ours) != fields[3] {
            failures.push(format!("{id} {}", String::from_utf8_lossy(&content)));
        }
    }
    assert!(
        failures.is_empty(),
        "{} differ:\n{}",
        failures.len(),
        failures.join("\n")
    );
    let assets = GOLDEN
        .lines()
        .find_map(|l| l.strip_prefix("assets\t"))
        .unwrap();
    let scripts = &renderer.assets.scripts;
    let printed = [
        head_scripts("http://127.0.0.1:8081", scripts),
        mediaelement_scripts("http://127.0.0.1:8081", scripts),
    ]
    .concat();
    assert_eq!(sha256(&printed), assets);
    assert_eq!(cases, 118);
}

/// A plugin that changes everything it may, with `library` for the players'.
struct Plugin {
    library: &'static str,
}

impl Hooks for Plugin {
    fn pre_do_shortcode_tag(
        &self,
        tag: &[u8],
        _: &[(Key, Value)],
        _: &Shortcode<'_>,
    ) -> Option<Vec<u8>> {
        (tag == b"embed").then(|| b"pre".to_vec())
    }
    fn do_shortcode_tag(
        &self,
        output: Vec<u8>,
        tag: &[u8],
        _: &[(Key, Value)],
        _: &Shortcode<'_>,
    ) -> Vec<u8> {
        if tag == b"wp_caption" {
            return b"after".to_vec();
        }
        output
    }
    fn shortcode_atts(
        &self,
        shortcode: &str,
        mut out: Vec<(String, Value)>,
        _: &[(&str, Value)],
        _: &[(Key, Value)],
    ) -> Vec<(String, Value)> {
        if shortcode == "caption" {
            for (name, value) in &mut out {
                if name == "align" {
                    *value = Value::String(b"alignplugin".to_vec());
                }
            }
        }
        out
    }
    fn img_caption_shortcode(&self, attr: &[(Key, Value)], _: &[u8]) -> Vec<u8> {
        match given(attr, "id") {
            Some(_) => b"<figure>plugin</figure>".to_vec(),
            None => Vec::new(),
        }
    }
    fn img_caption_shortcode_width(&self, width: i64, _: &[(String, Value)], _: &[u8]) -> i64 {
        if width == 77 { 0 } else { width }
    }
    fn post_gallery(&self, attr: &[(Key, Value)], _: u64) -> Vec<u8> {
        match given(attr, "columns") {
            Some(columns) if columns.to_int() == 9 => b"<div>plugin gallery</div>".to_vec(),
            _ => Vec::new(),
        }
    }
    fn use_default_gallery_style(&self, _: bool) -> bool {
        true
    }
    fn gallery_style(&self, html: Vec<u8>) -> Vec<u8> {
        [&b"<!--styled-->"[..], &html].concat()
    }
    fn media_shortcode_override(
        &self,
        kind: &str,
        attr: &[(Key, Value)],
        _: &[u8],
        _: u64,
    ) -> Vec<u8> {
        match given(attr, "override") {
            Some(_) => format!("override {kind}").into_bytes(),
            None => Vec::new(),
        }
    }
    fn media_extensions(&self, kind: &str, mut extensions: Vec<String>) -> Vec<String> {
        extensions.push(format!("x{kind}"));
        extensions
    }
    fn media_shortcode_library(&self, _: &str, _: String) -> String {
        self.library.into()
    }
    fn media_shortcode_class(&self, _: &str, class: Vec<u8>, _: &[(String, Value)]) -> Vec<u8> {
        [&class[..], b" plugin"].concat()
    }
    fn wp_mediaelement_fallback(&self, output: Vec<u8>, _: &[u8]) -> Vec<u8> {
        [&output[..], b"fallback"].concat()
    }
    fn media_shortcode(
        &self,
        kind: &str,
        html: Vec<u8>,
        _: &[(String, Value)],
        attachment: Option<&Post>,
        post_id: u64,
        library: &str,
    ) -> Vec<u8> {
        let attached = attachment.map_or(0, |post| post.id);
        let tail = format!("<!--{kind} {post_id} {attached} {library}-->");
        [&html[..], tail.as_bytes()].concat()
    }
}

/// What the content filters make of `content` for post `id`, with `hooks`
/// attached.
fn filtered(hooks: &dyn Hooks, id: u64, content: &str) -> String {
    let theme = twentytwentyfive();
    let site = library(hooks);
    let request = Request::default();
    let mut renderer = Renderer::new(&theme, &site, &request);
    renderer.context.post_id = Some(id);
    let out = renderer.the_content_filters(content.as_bytes());
    String::from_utf8(out).unwrap().trim().to_string()
}

#[test]
fn caption_and_gallery_hooks_change_what_they_may() {
    let plugin = Plugin { library: "plain" };
    let out = |content| filtered(&plugin, 555, content);
    // `pre_do_shortcode_tag` replaces a shortcode, `do_shortcode_tag` its output.
    assert_eq!(out("[embed]x[/embed]"), "pre");
    assert_eq!(
        out("[wp_caption width=\"1\" caption=\"c\"]y[/wp_caption]"),
        "after"
    );
    // `img_caption_shortcode` replaces the caption; `shortcode_atts_caption`
    // and `img_caption_shortcode_width` change its parts.
    assert_eq!(
        out("[caption id=\"i\" width=\"5\" caption=\"c\"]x[/caption]"),
        "<figure>plugin</figure>"
    );
    let caption = out("[caption width=\"5\" caption=\"c\"]<img src=\"i.png\" />[/caption]");
    assert!(
        caption.starts_with("<figure style=\"width: 5px\" class=\"wp-caption alignplugin\"><img "),
        "{caption}"
    );
    assert!(caption.ends_with("<figcaption class=\"wp-caption-text\">c</figcaption></figure>"));
    assert!(out("[caption width=\"77\" caption=\"c\"]x[/caption]").starts_with("<figure class="));
    // `post_gallery` replaces the gallery; `use_default_gallery_style` and
    // `gallery_style` change its opening.
    assert_eq!(out("[gallery columns=9]"), "<div>plugin gallery</div>");
    let gallery = out("[gallery ids=\"754\" columns=3]");
    assert!(
        gallery.starts_with("<!--styled-->\n\t\t<style>\n\t\t\t#gallery-"),
        "{gallery}"
    );
    assert!(gallery.contains("width: 33%;"), "{gallery}");
    assert!(out("[gallery ids=\"754\" columns=0]").contains("width: 100%;"));
}

#[test]
fn player_hooks_change_what_they_may() {
    let plain = Plugin { library: "plain" };
    let out = |hooks, id, content| filtered(hooks, id, content);
    // The override, the extensions, the library, the class and the final filter.
    assert_eq!(out(&plain, 555, "[audio override=1]"), "override audio");
    assert_eq!(out(&plain, 555, "[video override=1]"), "override video");
    let audio = out(&plain, 587, "[audio]");
    assert!(
        audio.contains("class=\"wp-audio-shortcode plugin\""),
        "{audio}"
    );
    assert!(audio.ends_with("<!--audio 587 821 plain-->"), "{audio}");
    assert!(!audio.contains("fallback"));
    let video = out(&plain, 555, "[video xvideo=\"http://x.test/a.xvideo\"]");
    assert!(
        video.contains("<source type=\"\" src=\"http://x.test/a.xvideo?_=1\" />"),
        "{video}"
    );
    assert!(video.ends_with("<!--video 555 0 plain-->"), "{video}");
    // MediaElement's fallback, as a filter changes it.
    let media = Plugin {
        library: "mediaelement",
    };
    let audio = out(&media, 587, "[audio]");
    assert!(audio.contains("</a>fallback</audio>"), "{audio}");
}

#[test]
fn shortcodes_too_deep_stay_as_written() {
    let site = library(&());
    let theme = twentytwentyfive();
    let request = Request::default();
    let mut renderer = Renderer::new(&theme, &site, &request);
    let text = b"[caption width=1 caption=c]x[/caption]";
    assert!(renderer.do_shortcode(text).starts_with(b"<figure "));
    renderer.shortcode_depth = crate::shortcode::MAX_DEPTH;
    assert_eq!(renderer.do_shortcode(text), text);
}

/// A site with a post and attachments of every kind that have no file.
struct Bare;

impl Site for Bare {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        (name == "siteurl").then(|| b"http://s.test".to_vec())
    }
    fn home_url(&self) -> Vec<u8> {
        b"http://s.test".to_vec()
    }
    fn post(&self, id: u64) -> Option<Post> {
        Some(Post {
            id,
            post_type: if id == 1 { "post" } else { "attachment" }.into(),
            ..Post::default()
        })
    }
    fn post_terms(&self, _: u64, _: &str) -> Vec<hyscribe_model::Term> {
        Vec::new()
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn query(&self, _: &PostQuery) -> Vec<Post> {
        self.post(2).into_iter().collect()
    }
}

#[test]
fn an_attachment_without_a_file_makes_no_player() {
    let theme = twentytwentyfive();
    let request = Request::default();
    let mut renderer = Renderer::new(&theme, &Bare, &request);
    renderer.context.post_id = Some(1);
    for tag in ["audio", "video"] {
        let out = renderer.the_content_filters(format!("[{tag}]").as_bytes());
        assert_eq!(out.trim_ascii(), b"");
    }
}

#[test]
fn shortcodes_without_a_callback_stay_as_written() {
    let text = "[playlist ids=\"1\"] and [embed]http://x.test/y[/embed]";
    let out = filtered(&(), 555, text);
    assert_eq!(out, format!("<p>{text}</p>"));
    // Escaped, they lose a bracket on each side as any shortcode does.
    let out = filtered(&(), 555, "[[playlist]]");
    assert_eq!(out, "<p>[playlist]</p>");
}
