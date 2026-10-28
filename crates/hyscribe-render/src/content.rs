// SPDX-License-Identifier: AGPL-3.0-or-later

//! `core/post-content`: the post's content through `get_the_content()` and
//! the `the_content` filters.
//!
//! Not ported yet: `wpautop()` for content without blocks, shortcodes,
//! `wp_link_pages()`, `force_balance_tags()` on a teaser, attachments and
//! `wp_filter_content_tags()`.

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::Post;
use hyscribe_phpser::{Value, is_empty_string};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::formatting::{CORE_SHORTCODE_TAGS, capital_p_dangit, wptexturize};
use crate::link::{front_page, permalink};
use crate::post::the_title;
use crate::supports::get_block_wrapper_attributes;
use crate::template::{Renderer, replace, site_smilies};

impl Renderer<'_> {
    /// `render_block_core_post_content()`.
    pub(crate) fn post_content(&mut self, block: &Block) -> Vec<u8> {
        let Some(id) = self.context.post_id else {
            return Vec::new();
        };
        if self.open_posts.contains(&id) {
            return Vec::new();
        }
        let Some(post) = self.context.site.post(id) else {
            return Vec::new();
        };
        self.open_posts.push(id);
        let content = self.the_content(&post);
        self.open_posts.pop();
        if is_empty_string(&content) {
            return Vec::new();
        }
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/post-content", parsed);
        let tag = attributes
            .get_str("tagName")
            .and_then(Value::as_bytes)
            .map(<[u8]>::to_ascii_lowercase)
            .filter(|tag| matches!(tag.as_slice(), b"div" | b"main" | b"section" | b"article"))
            .unwrap_or_else(|| b"div".to_vec());
        let extra = [("class", Value::String(b"entry-content".to_vec()))];
        let wrapper = get_block_wrapper_attributes(
            "core/post-content",
            parsed,
            &extra,
            &self.context.theme.global,
        );
        [
            b"<",
            &tag[..],
            b" ",
            wrapper.as_bytes(),
            b">",
            &content,
            b"</",
            &tag,
            b">",
        ]
        .concat()
    }

    /// `apply_filters( 'the_content', get_the_content() )`, with the `]]>`
    /// escape between them.
    fn the_content(&mut self, post: &Post) -> Vec<u8> {
        let site = self.context.site;
        let content = replace(&get_the_content(self, post), b"]]>", b"]]&gt;");
        let outer = self.context.loading.in_the_content;
        self.context.loading.in_the_content = true;
        let rendered = self.render_top_level(&content);
        self.context.loading.in_the_content = outer;
        let content = wptexturize(&rendered, &CORE_SHORTCODE_TAGS);
        site_smilies(
            site,
            &capital_p_dangit(&replace_insecure_home_url(&site.home_url(), &content)),
        )
    }
}

/// `get_the_content()` for the main loop's post: the password form for a
/// protected post, else the requested page of the content, whole on a
/// singular request and up to its more tag elsewhere.
fn get_the_content(renderer: &Renderer<'_>, post: &Post) -> Vec<u8> {
    let context = &renderer.context;
    if !post.password.is_empty() {
        return password_form(renderer, post);
    }
    let pages: Vec<&[u8]> = split_pages(&post.content);
    let page = usize::try_from(context.request.page)
        .unwrap_or(1)
        .clamp(1, pages.len());
    let content = pages.get(page - 1).copied().unwrap_or_default();
    let Some((start, end)) = more_tag(content) else {
        return content.to_vec();
    };
    // The more tag's own text, as `<!--more Read on-->` gives it.
    let label = hyscribe_kses::strip_tags(&hyscribe_kses::wp_kses_no_null(
        hyscribe_phpser::php_trim(content.get(start + 8..end - 3).unwrap_or_default()),
    ));
    let (teaser, rest) = (
        content.get(..start).unwrap_or_default(),
        content.get(end..).unwrap_or_default(),
    );
    let without_delimiters = |part: &[u8]| remove_more_delimiters(content, part);
    let (teaser, rest) = (without_delimiters(teaser), without_delimiters(rest));
    if context.request.singular {
        let teaser = if contains(&post.content, b"<!--noteaser-->") && page == 1 {
            Vec::new()
        } else {
            teaser
        };
        return [
            &teaser[..],
            format!("<span id=\"more-{}\"></span>", post.id).as_bytes(),
            &rest,
        ]
        .concat();
    }
    let link_text = if label.is_empty() {
        // `the_title_attribute()`.
        let title = esc_attr(&hyscribe_kses::strip_tags(&the_title(post)));
        format!("<span aria-label=\"Continue reading {title}\">(more&hellip;)</span>").into_bytes()
    } else {
        label
    };
    let link = renderer_permalink(renderer, post);
    [
        &teaser[..],
        b" <a href=\"",
        &link,
        format!("#more-{}\" class=\"more-link\">", post.id).as_bytes(),
        &link_text,
        b"</a>",
    ]
    .concat()
}

fn contains(text: &[u8], needle: &[u8]) -> bool {
    text.windows(needle.len()).any(|w| w == needle)
}

/// A part of `content` without `core/more` block delimiters, which
/// `get_the_content()` removes when the content has that block.
fn remove_more_delimiters(content: &[u8], part: &[u8]) -> Vec<u8> {
    if !contains(content, b"<!-- wp:more") {
        return part.to_vec();
    }
    let mut out = Vec::with_capacity(part.len());
    let mut rest = part;
    for _ in 0..=part.len() {
        // `/<!-- \/?wp:more(.*?) -->/`: the earliest delimiter of either kind.
        let open = ["<!-- wp:more", "<!-- /wp:more"]
            .iter()
            .filter_map(|d| rest.windows(d.len()).position(|w| w == d.as_bytes()))
            .min();
        let Some(at) = open else {
            break;
        };
        let after = rest.get(at..).unwrap_or_default();
        let Some(close) = after.windows(4).position(|w| w == b" -->") else {
            break;
        };
        out.extend_from_slice(rest.get(..at).unwrap_or_default());
        rest = after.get(close + 4..).unwrap_or_default();
    }
    out.extend_from_slice(rest);
    out
}

fn renderer_permalink(renderer: &Renderer<'_>, post: &Post) -> Vec<u8> {
    let site = renderer.context.site;
    let option = |name| site.option(name).unwrap_or_default();
    permalink(
        &site.home_url(),
        front_page(&option("show_on_front"), &option("page_on_front")),
        post,
    )
}

/// The pages of a post's content, split at `<!--nextpage-->` as
/// `generate_postdata()` splits them.
fn split_pages(content: &[u8]) -> Vec<&[u8]> {
    const BREAK: &[u8] = b"<!--nextpage-->";
    let mut pages = Vec::new();
    let mut rest = content;
    for _ in 0..=content.len() {
        match rest.windows(BREAK.len()).position(|w| w == BREAK) {
            Some(at) => {
                pages.push(rest.get(..at).unwrap_or_default());
                rest = rest.get(at + BREAK.len()..).unwrap_or_default();
            }
            None => break,
        }
    }
    pages.push(rest);
    pages
}

/// Where the first `<!--more ...-->` starts and ends.
fn more_tag(content: &[u8]) -> Option<(usize, usize)> {
    let start = content.windows(8).position(|w| w == b"<!--more")?;
    let end = content
        .get(start..)
        .unwrap_or_default()
        .windows(3)
        .position(|w| w == b"-->")?
        + start
        + 3;
    Some((start, end))
}

/// `get_the_password_form()` in a block theme, for a visitor without the
/// password cookie.
fn password_form(renderer: &Renderer<'_>, post: &Post) -> Vec<u8> {
    let site = renderer.context.site;
    let action = esc_url(
        &[
            &site.option("siteurl").unwrap_or_default()[..],
            b"/wp-login.php?action=postpass",
        ]
        .concat(),
    );
    let redirect = esc_attr(&renderer_permalink(renderer, post));
    let id = post.id;
    format!(
        "<form action=\"{}\" class=\"post-password-form\" method=\"post\"><input type=\"hidden\" name=\"redirect_to\" value=\"{redirect}\" />\n\
         \t<p>This content is password-protected. To view it, please enter the password below.</p>\n\
         \t<p><label for=\"pwbox-{id}\">Password: <input name=\"post_password\" id=\"pwbox-{id}\" type=\"password\" spellcheck=\"false\" required size=\"20\" /></label> \
         <span class=\"wp-block-button\"><input type=\"submit\" name=\"Submit\" class=\"wp-block-button__link wp-element-button\" value=\"Enter\" /></span></p></form>\n\t",
        String::from_utf8_lossy(&action)
    )
    .into_bytes()
}

/// `wp_replace_insecure_home_url()`: on an HTTPS site, links to its HTTP
/// address become HTTPS, escaped slashes included.
fn replace_insecure_home_url(home: &[u8], content: &[u8]) -> Vec<u8> {
    let Some(rest) = home.strip_prefix(b"https://") else {
        return content.to_vec();
    };
    let https = home.to_vec();
    let http = [b"http://", rest].concat();
    let escaped = |url: &[u8]| replace(url, b"/", b"\\/");
    replace(
        &replace(content, &http, &https),
        &escaped(&http),
        &escaped(&https),
    )
}

#[cfg(test)]
mod tests;
