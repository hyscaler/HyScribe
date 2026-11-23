// SPDX-License-Identifier: AGPL-3.0-or-later

//! A classic theme's `wp_head()` and `wp_footer()` (ADR 0023), for a theme
//! with a `theme.json`: core's callbacks at their priorities among the
//! extensions', which print what they answer, and WordPress 6.9's template
//! output buffer, which loads block assets on demand and moves the global
//! styles and the styles enqueued after the head into it once the page is
//! done (`wp_hoist_late_printed_styles()`). Not ported yet: the late core
//! block styles, which no template tag served enqueues yet, font faces, a
//! site icon, a `custom_css` post's styles and the admin bar.

use std::sync::{Mutex, MutexGuard, PoisonError};

use hyscribe_model::hooks::{Core, do_action_printing};
use hyscribe_theme_json::GlobalStyles;

use super::footer::{footer_actions, footer_scripts, head_scripts, siteurl};
use super::{
    AUTO_SIZES_CSS, Assets, COMMON_CSS, COMMON_SRC, EMOJI_CSS, Head, Style,
    normalize_relative_css_links, style_text,
};
use crate::site::{Request, Site};
use crate::template::Renderer;
use crate::theme::Theme;

/// A stylesheet of an extension's package, enqueued for every medium.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    /// `$handle`.
    pub handle: String,
    /// The file's address.
    pub href: String,
    /// `$ver`, `None` for WordPress's version.
    pub version: Option<String>,
}

/// A style in the queue: core's, by handle, or an extension's file.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Queued {
    Core(&'static str),
    Link(Link),
}

impl Queued {
    fn handle(&self) -> &str {
        match self {
            Queued::Core(handle) => handle,
            Queued::Link(link) => &link.handle,
        }
    }
}

/// What a classic page has enqueued, core and the extensions alike, how
/// far its head printed the styles, and what its footer kept to hoist.
#[derive(Debug, Default)]
pub struct Enqueued {
    styles: Vec<Queued>,
    scripts: Vec<String>,
    printed: usize,
    global: Vec<u8>,
    late: Vec<u8>,
}

impl Enqueued {
    /// `wp_enqueue_style()` of an extension's file, once for each handle.
    pub fn enqueue_style(&mut self, link: Link) {
        self.style(Queued::Link(link));
    }

    /// `wp_enqueue_script()` of a script core registers.
    pub fn enqueue_script(&mut self, handle: &str) {
        super::enqueue(&mut self.scripts, handle);
    }

    fn style(&mut self, queued: Queued) {
        if !self.styles.iter().any(|q| q.handle() == queued.handle()) {
            self.styles.push(queued);
        }
    }

    fn assets(&self) -> Assets {
        Assets {
            scripts: self.scripts.clone(),
            ..Assets::default()
        }
    }
}

fn held(enqueued: &Mutex<Enqueued>) -> MutexGuard<'_, Enqueued> {
    enqueued.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A classic theme's page as its head and footer print it.
pub struct ClassicPage<'a> {
    /// The block theme loaded, whose context the posts' links take.
    pub theme: &'a Theme,
    /// The global styles of the theme's `theme.json`.
    pub global: &'a GlobalStyles,
    /// The site, read for the page.
    pub site: &'a dyn Site,
    /// The page's request.
    pub request: &'a Request,
    /// `wp_get_document_title()`.
    pub title: &'a [u8],
    /// `current_theme_supports()`.
    pub supports: &'a dyn Fn(&str) -> bool,
    /// The active theme's `stylesheet`.
    pub stylesheet: &'a str,
    /// What the page enqueued, shared with the extensions' calls.
    pub enqueued: &'a Mutex<Enqueued>,
    /// Where the head leaves the global styles to hoist: unique to the page.
    pub hole: &'a str,
}

impl ClassicPage<'_> {
    /// What `wp_head()` prints.
    pub fn wp_head(&self) -> Vec<u8> {
        let renderer = Renderer::new(self.theme, self.site, self.request);
        let head = Head {
            theme: self.theme,
            site: self.site,
            request: self.request,
            context: &renderer.context,
        };
        let enqueue = |handle: &'static str| held(self.enqueued).style(Queued::Core(handle));
        let feeds = (self.supports)("automatic-feed-links");
        let mut core: Vec<Core<'_>> = vec![
            (
                0,
                Box::new(move || {
                    enqueue("wp-img-auto-sizes-contain");
                    Vec::new()
                }),
            ),
            (1, Box::new(|| self.title_tag())),
            (1, Box::new(|| self.enqueue_scripts())),
            (1, Box::new(|| head.robots())),
        ];
        if let Some(post) = self.request.posts.first().filter(|_| self.request.preview) {
            core.push((1, Box::new(move || super::post_preview_js(post.id))));
        }
        if feeds {
            core.push((2, Box::new(|| head.feed_links())));
            core.push((3, Box::new(|| head.feed_links_extra())));
        }
        core.push((4, Box::new(|| head.oembed_links())));
        core.push((8, Box::new(|| self.print_styles())));
        let scripts = || head_scripts(&siteurl(self.site), &held(self.enqueued).scripts);
        core.push((9, Box::new(scripts)));
        core.push((10, Box::new(|| head.rest_links())));
        core.push((10, Box::new(super::generator)));
        core.push((10, Box::new(|| head.canonical(true, None))));
        // `wp_custom_css_cb()`: a `custom_css` post's styles are not ported.
        let custom_css = || {
            self.site.custom_css_post_lookup(self.stylesheet);
            Vec::new()
        };
        core.push((101, Box::new(custom_css)));
        do_action_printing("wp_head", &[], core)
    }

    /// `_wp_render_title_tag()`, for a theme that supports `title-tag`.
    fn title_tag(&self) -> Vec<u8> {
        if !(self.supports)("title-tag") {
            return Vec::new();
        }
        [&b"<title>"[..], self.title, b"</title>\n"].concat()
    }

    /// `wp_enqueue_scripts`: core's styles for a classic theme that loads
    /// block assets on demand, with the placeholders the buffer replaces.
    fn enqueue_scripts(&self) -> Vec<u8> {
        let enqueue = |handles: &'static [&'static str]| -> Box<dyn FnOnce() -> Vec<u8>> {
            Box::new(move || {
                let mut enqueued = held(self.enqueued);
                for handle in handles {
                    enqueued.style(Queued::Core(handle));
                }
                Vec::new()
            })
        };
        let core = vec![
            (10, enqueue(&["wp-emoji-styles"])),
            (
                10,
                enqueue(&["wp-block-library", "wp-block-styles-placeholder"]),
            ),
            (10, enqueue(&["wp-global-styles-placeholder"])),
        ];
        do_action_printing("wp_enqueue_scripts", &[], core)
    }

    /// `wp_print_styles()`: the queue so far, `common.css` inlined.
    fn print_styles(&self) -> Vec<u8> {
        let mut enqueued = held(self.enqueued);
        let out = enqueued
            .styles
            .iter()
            .flat_map(|q| self.print_style(q))
            .collect();
        enqueued.printed = enqueued.styles.len();
        out
    }

    /// `WP_Styles::do_item()` of `queued`, as the buffer leaves it.
    fn print_style(&self, queued: &Queued) -> Vec<u8> {
        match queued {
            Queued::Core("wp-img-auto-sizes-contain") => {
                Style::inline("wp-img-auto-sizes-contain", AUTO_SIZES_CSS).print("")
            }
            Queued::Core("wp-emoji-styles") => {
                Style::inline("wp-emoji-styles", EMOJI_CSS).print("")
            }
            Queued::Core("wp-block-library") => block_library(),
            // Taken out by the buffer, its line left.
            Queued::Core("wp-block-styles-placeholder") => b"\n".to_vec(),
            // `wp-global-styles-placeholder`, which the buffer replaces.
            Queued::Core(_) => [self.hole.as_bytes(), b"\n"].concat(),
            Queued::Link(link) => {
                Style::link(&link.handle, &link.href, link.version.as_deref()).print("")
            }
        }
    }

    /// What `wp_footer()` prints, keeping the global styles and the styles
    /// enqueued after the head for [`hoist`].
    pub fn wp_footer(&self) -> Vec<u8> {
        let siteurl = siteurl(self.site);
        let assets = held(self.enqueued).assets();
        let core: Vec<Core<'_>> = vec![
            (
                10,
                Box::new(|| footer_actions(self.site, &siteurl, &assets)),
            ),
            (
                20,
                Box::new(|| {
                    self.capture();
                    footer_scripts(&siteurl, &held(self.enqueued).assets())
                }),
            ),
        ];
        do_action_printing("wp_footer", &[], core)
    }

    /// The capture of `wp_hoist_late_printed_styles()`: the global styles,
    /// then the styles enqueued since the head.
    fn capture(&self) {
        let mut enqueued = held(self.enqueued);
        let on_page = |block: &str| {
            let handle = format!("wp-block-{}", block.trim_start_matches("core/"));
            enqueued
                .styles
                .iter()
                .any(|queued| queued.handle() == handle)
        };
        let css = self.global.inline_css(&on_page);
        let global = Style::inline("global-styles", &css).print("");
        let late = enqueued.styles.get(enqueued.printed..).unwrap_or_default();
        let late = late.iter().flat_map(|q| self.print_style(q)).collect();
        (enqueued.global, enqueued.late) = (global, late);
    }
}

/// `wp-block-library` with `common.css` inlined, as the buffer leaves it
/// once it takes out the placeholder.
fn block_library() -> Vec<u8> {
    let css = normalize_relative_css_links(COMMON_CSS, COMMON_SRC);
    let text = format!("\n{css}\n\n/*# sourceURL={COMMON_SRC} */\n");
    [
        &b"<style id=\"wp-block-library-inline-css\">"[..],
        &style_text(text.as_bytes()),
        b"</style>\n",
    ]
    .concat()
}

/// The template output buffer's filter: the global styles the footer kept
/// in the head's `hole`, and the late styles before the first `</head>`.
pub fn hoist(page: &[u8], enqueued: &Mutex<Enqueued>, hole: &str) -> Vec<u8> {
    let enqueued = held(enqueued);
    let Some(at) = crate::formatting::find(page, 0, hole.as_bytes()) else {
        return page.to_vec();
    };
    let mut out = page.get(..at).unwrap_or_default().to_vec();
    out.extend_from_slice(&enqueued.global);
    let rest = page.get(at + hole.len()..).unwrap_or_default();
    let head_end = (!enqueued.late.is_empty())
        .then(|| head_end(rest))
        .flatten();
    match head_end {
        Some(end) => {
            out.extend_from_slice(rest.get(..end).unwrap_or_default());
            out.extend_from_slice(&enqueued.late);
            out.push(b'\n');
            out.extend_from_slice(rest.get(end..).unwrap_or_default());
        }
        None => out.extend_from_slice(rest),
    }
    out
}

/// Where the first `</head>` closer starts, its name in any case.
fn head_end(html: &[u8]) -> Option<usize> {
    (0..html.len()).find(|&at| {
        let tag = html.get(at..at + 6).unwrap_or_default();
        let after = html.get(at + 6).copied();
        tag.eq_ignore_ascii_case(b"</head")
            && matches!(
                after,
                Some(b'>' | b'/' | b' ' | b'\t' | b'\n' | b'\x0c' | b'\r')
            )
    })
}

#[cfg(test)]
mod tests;
