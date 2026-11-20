// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_get_loading_optimization_attributes()`, with the request's content
//! media count and high priority flag, which every image of the request
//! shares.

use std::cell::Cell;
use std::rc::Rc;

/// Where in the request a block renders, and the counts so far.
#[derive(Clone, Default)]
pub(crate) struct Loading {
    /// `in_the_loop() && is_main_query()`: true through a singular
    /// template, which runs in the main loop, and in a post template that
    /// inherits the main query.
    pub(crate) in_the_loop: bool,
    /// `doing_filter( 'the_content' )`.
    pub(crate) in_the_content: bool,
    /// `doing_filter( 'widget_block_content' )`: a block widget's content.
    pub(crate) in_widget_block_content: bool,
    /// The context `template_part_header`, whose elements count as in the
    /// viewport.
    pub(crate) in_header: bool,
    shared: Rc<Shared>,
}

#[derive(Default)]
struct Shared {
    /// `wp_increase_content_media_count( 0 )`.
    media_count: Cell<u64>,
    /// `! wp_high_priority_element_flag()`.
    high_priority_taken: Cell<bool>,
}

/// The filter a `$context` of `wp_get_loading_optimization_attributes()`
/// names, if any. While a filter runs, only its own context gets loading
/// attributes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Filter {
    /// A context of no filter, such as `wp_get_attachment_image`.
    None,
    /// `the_content`.
    Content,
    /// `widget_block_content`.
    WidgetBlock,
    /// `widget_text_content`, which renders no blocks or shortcodes, so no
    /// image of another context is made inside it and [`Loading`] needs no
    /// flag for it.
    WidgetText,
}

/// The attributes `wp_get_loading_optimization_attributes()` adds.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Optimization {
    pub(crate) loading: Option<&'static str>,
    pub(crate) fetchpriority: Option<&'static str>,
    pub(crate) decoding: Option<&'static str>,
}

/// The attributes an element has already, as `wp_get_loading_optimization_attributes()`
/// reads them: a width or height only counts when both are known.
#[derive(Default)]
pub(crate) struct Given<'a> {
    pub(crate) width: Option<i64>,
    pub(crate) height: Option<i64>,
    pub(crate) loading: Option<&'a [u8]>,
    pub(crate) fetchpriority: Option<&'a [u8]>,
}

impl Loading {
    pub(crate) fn in_the_loop(in_the_loop: bool) -> Loading {
        Loading {
            in_the_loop,
            ..Loading::default()
        }
    }

    /// The request's counts in the context `template_part_<area>`.
    pub(crate) fn in_area(&self, area: &[u8]) -> Loading {
        Loading {
            in_header: area == b"header",
            ..self.clone()
        }
    }

    /// `wp_get_loading_optimization_attributes( 'img', $attr, $context )`
    /// for an image with a width and height and none of `loading`,
    /// `fetchpriority` or `decoding` given, in a context that is neither
    /// the header nor a filter's.
    pub(crate) fn img(&self, width: i64, height: i64) -> Optimization {
        let given = Given {
            width: Some(width),
            height: Some(height),
            ..Given::default()
        };
        self.optimization(true, &given, Filter::None)
    }

    /// `wp_get_loading_optimization_attributes()` for an `img` or, when
    /// `img` is false, an `iframe`, in the context of `filter`: inside
    /// `the_content` or `widget_block_content`, an element of another
    /// context gets nothing.
    pub(crate) fn optimization(
        &self,
        img: bool,
        given: &Given<'_>,
        filter: Filter,
    ) -> Optimization {
        let mut attributes = Optimization::default();
        if (self.in_the_content && filter != Filter::Content)
            || (self.in_widget_block_content && filter != Filter::WidgetBlock)
        {
            return attributes;
        }
        if img {
            attributes.decoding = Some("async");
        }
        let (Some(width), Some(height)) = (given.width, given.height) else {
            return attributes;
        };
        let mut in_viewport = given.loading.map(|loading| loading != b"lazy");
        let low = given.fetchpriority == Some(b"low");
        // A given priority stays as given; only its effect is ported.
        match given.fetchpriority {
            Some(b"high") if in_viewport != Some(false) => in_viewport = Some(true),
            Some(b"low") => in_viewport = Some(false),
            _ => {}
        }
        let mut counted = false;
        let mut counted_when_large = false;
        if in_viewport.is_none() && self.in_header {
            in_viewport = Some(true);
            counted_when_large = true;
        } else if in_viewport.is_none() && self.in_the_loop {
            // wp_omit_loading_attr_threshold()
            in_viewport = Some(self.shared.media_count.get() < 3);
            counted = true;
        }
        if in_viewport == Some(true) {
            // wp_maybe_add_fetchpriority_high_attr()
            // Iframes, sized 100 by 100 here, never reach the threshold.
            match given.fetchpriority {
                Some(b"auto") | None => {
                    let flag = &self.shared.high_priority_taken;
                    if !flag.get() && width.saturating_mul(height) >= 50_000 {
                        if given.fetchpriority.is_none() {
                            attributes.fetchpriority = Some("high");
                        }
                        flag.set(true);
                    }
                }
                Some(b"high") => self.shared.high_priority_taken.set(true),
                _ => {}
            }
        } else if !low {
            attributes.loading = Some("lazy");
        }
        // wp_min_priority_img_pixels
        let large = width.saturating_mul(height) >= 50_000;
        if (counted || counted_when_large && large) && given.fetchpriority != Some(b"auto") {
            let count = &self.shared.media_count;
            count.set(count.get() + 1);
        }
        attributes
    }
}

#[cfg(test)]
mod tests;
