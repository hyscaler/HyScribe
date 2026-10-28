// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_get_loading_optimization_attributes()` for images rendered outside
//! post content, with the request's content media count and high priority
//! flag, which every image of the request shares.

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
    shared: Rc<Shared>,
}

#[derive(Default)]
struct Shared {
    /// `wp_increase_content_media_count( 0 )`.
    media_count: Cell<u64>,
    /// `! wp_high_priority_element_flag()`.
    high_priority_taken: Cell<bool>,
}

/// The attributes `wp_get_loading_optimization_attributes()` adds.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Optimization {
    pub(crate) loading: Option<&'static str>,
    pub(crate) fetchpriority: Option<&'static str>,
    pub(crate) decoding: Option<&'static str>,
}

impl Loading {
    pub(crate) fn in_the_loop(in_the_loop: bool) -> Loading {
        Loading {
            in_the_loop,
            ..Loading::default()
        }
    }

    /// `wp_get_loading_optimization_attributes( 'img', $attr, $context )`
    /// for an image with a width and height and none of `loading`,
    /// `fetchpriority` or `decoding` given, in a context that is neither
    /// the header nor post content.
    pub(crate) fn img(&self, width: i64, height: i64) -> Optimization {
        if self.in_the_content {
            return Optimization::default();
        }
        let mut attributes = Optimization {
            decoding: Some("async"),
            ..Optimization::default()
        };
        // wp_omit_loading_attr_threshold()
        let in_viewport = self.in_the_loop && self.shared.media_count.get() < 3;
        if !in_viewport {
            attributes.loading = Some("lazy");
        } else if !self.shared.high_priority_taken.get() && width.saturating_mul(height) >= 50_000 {
            // wp_maybe_add_fetchpriority_high_attr()
            attributes.fetchpriority = Some("high");
            self.shared.high_priority_taken.set(true);
        }
        if self.in_the_loop {
            self.shared
                .media_count
                .set(self.shared.media_count.get() + 1);
        }
        attributes
    }
}

#[cfg(test)]
mod tests;
