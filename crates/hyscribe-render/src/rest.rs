// SPDX-License-Identifier: AGPL-3.0-or-later

//! Posts' content and excerpts as the REST API renders them: outside the
//! loop and the main query, each post the global one in turn, with the
//! unique IDs and lazy loading counted on from post to post as PHP's
//! globals count them through a request.

use hyscribe_model::Post;

use crate::autop::wpautop;
use crate::content::replace_insecure_home_url;
use crate::content_tags::wp_filter_content_tags;
use crate::excerpt::EXCERPT_MORE;
use crate::formatting::{CORE_SHORTCODE_TAGS, contains, convert_chars, wptexturize};
use crate::site::{Request, Site};
use crate::template::{Renderer, site_smilies};
use crate::theme::Theme;

/// The rendering of one REST request.
pub struct Rendering<'a> {
    renderer: Renderer<'a>,
}

impl<'a> Rendering<'a> {
    /// A request's rendering; `request` gives its URI and query.
    pub fn new(theme: &'a Theme, site: &'a dyn Site, request: &'a Request) -> Rendering<'a> {
        Rendering {
            renderer: Renderer::new(theme, site, request),
        }
    }

    /// `apply_filters( 'the_content', $post->post_content )`.
    pub fn the_content(&mut self, post: &Post) -> Vec<u8> {
        let renderer = &mut self.renderer;
        renderer.context.post_id = Some(post.id);
        renderer.attachment = attachment(post);
        renderer.context.loading.in_the_content = true;
        let rendered = renderer.render_top_level(&post.content);
        renderer.context.loading.in_the_content = false;
        // do_blocks() takes wpautop() off for content with blocks, and
        // puts it back last for the content after.
        let blocks = contains(&post.content, b"<!-- wp:");
        let rendered = renderer.content_filters(&rendered, !blocks, true);
        renderer.autop_restored |= blocks;
        rendered
    }

    /// The `get_the_excerpt` filters of `$post->post_excerpt`, with
    /// `excerpt_length` as `length`, then the `the_excerpt` filters.
    pub fn the_excerpt(&mut self, post: &Post, length: i64) -> Vec<u8> {
        let renderer = &mut self.renderer;
        renderer.context.post_id = Some(post.id);
        renderer.attachment = attachment(post);
        let excerpt = renderer.excerpt(post, length, EXCERPT_MORE);
        let site = renderer.context.site;
        let excerpt = wptexturize(&excerpt, &CORE_SHORTCODE_TAGS);
        let excerpt = convert_chars(&site_smilies(site, &excerpt));
        let excerpt = replace_insecure_home_url(site, &wpautop(&excerpt, true));
        wp_filter_content_tags(site, &renderer.context.loading, &excerpt)
    }
}

/// The global post `prepend_attachment()` reads, if an attachment.
fn attachment(post: &Post) -> Option<Post> {
    (post.post_type == "attachment").then(|| post.clone())
}
