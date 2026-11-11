// SPDX-License-Identifier: AGPL-3.0-or-later

//! `core/post-content`: the post's content through `get_the_content()` and
//! the `the_content` filters.
//!
//! Not ported yet: shortcodes, `wp_link_pages()`, `force_balance_tags()` on
//! a teaser, attachments and `wp_filter_content_tags()`.

use hyscribe_kses::{esc_attr, esc_url};
use hyscribe_model::Post;
use hyscribe_phpser::{Value, is_empty_string};

use crate::autop::wpautop;
use crate::blocks::Block;
use crate::content_tags::wp_filter_content_tags;
use crate::core_blocks::prepared;
use crate::formatting::{CORE_SHORTCODE_TAGS, capital_p_dangit, contains, wptexturize};
use crate::post::the_title;
use crate::site::Request;
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
        let mut content = get_the_content(self, &post, true);
        // has_block( 'core/nextpage' ): page links for a post in pages.
        if contains(&post.content, b"<!-- wp:nextpage ") {
            content.extend(wp_link_pages(self, &post));
        }
        let content = self.content_filtered(&content);
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
    pub(crate) fn the_content(&mut self, post: &Post) -> Vec<u8> {
        let content = get_the_content(self, post, true);
        self.content_filtered(&content)
    }

    /// The `the_content` filters on `content`, `]]>` escaped first.
    fn content_filtered(&mut self, content: &[u8]) -> Vec<u8> {
        let content = replace(content, b"]]>", b"]]&gt;");
        let outer = self.context.loading.in_the_content;
        self.context.loading.in_the_content = true;
        let rendered = self.render_top_level(&content);
        self.context.loading.in_the_content = outer;
        // do_blocks() takes wpautop() off for content with blocks.
        self.content_filters(&rendered, !contains(&content, b"<!-- wp:"), true)
    }

    /// The `the_content` filters after `do_blocks()`; `wpautop()` only with
    /// `autop`, `wp_filter_content_tags()` only with `tags` (excerpts unhook
    /// it).
    pub(crate) fn content_filters(&self, content: &[u8], autop: bool, tags: bool) -> Vec<u8> {
        let site = self.context.site;
        let content = wptexturize(content, &CORE_SHORTCODE_TAGS);
        let content = if autop {
            wpautop(&content, true)
        } else {
            content
        };
        let content = capital_p_dangit(&replace_insecure_home_url(&site.home_url(), &content));
        let content = if tags {
            wp_filter_content_tags(site, &self.context.loading, &content)
        } else {
            content
        };
        site_smilies(site, &content)
    }
}

/// `get_the_content()` for the main loop's post: the password form for a
/// protected post, else the requested page of the content, whole on a
/// singular request or in a feed and up to its more tag elsewhere, with a
/// more link unless `more_link` is false (an empty `$more_link_text`).
pub(crate) fn get_the_content(renderer: &Renderer<'_>, post: &Post, more_link: bool) -> Vec<u8> {
    let context = &renderer.context;
    if !post.password.is_empty() {
        return password_form(renderer, post);
    }
    let pages = split_pages(&post.content);
    let page = usize::try_from(context.request.page)
        .unwrap_or(1)
        .clamp(1, pages.len());
    let content = pages.get(page - 1).map_or(&[][..], Vec::as_slice);
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
    if shows_more(context.request, post) {
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
    if !more_link {
        return teaser;
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

/// generate_postdata()'s `$more`: the whole post when it is the one asked
/// for, and in feeds.
fn shows_more(request: &Request, post: &Post) -> bool {
    request.singular && request.queried_object_id == post.id || request.feed
}

/// `wp_link_pages()` for `post`: a link to each of its pages, the one shown
/// marked current unless only its teaser shows.
fn wp_link_pages(renderer: &Renderer<'_>, post: &Post) -> Vec<u8> {
    let count = split_pages(&post.content).len();
    if count < 2 {
        return Vec::new();
    }
    let request = renderer.context.request;
    // The global `$page`, which only get_the_content() clamps.
    let page = usize::try_from(request.page).unwrap_or(1).max(1);
    let more = shows_more(request, post);
    let permalink = renderer_permalink(renderer, post);
    let mut out = b"<p class=\"post-nav-links\">Pages:".to_vec();
    for i in 1..=count {
        let number = i.to_string();
        out.push(b' ');
        if i != page || !more && page == 1 {
            let url = if i == 1 {
                permalink.clone()
            } else {
                crate::permalinks::post_page(renderer.context.site, post, &permalink, i)
            };
            out.extend_from_slice(
                &[
                    b"<a href=\"",
                    &esc_url(&url)[..],
                    b"\" class=\"post-page-numbers\">",
                    number.as_bytes(),
                    b"</a>",
                ]
                .concat(),
            );
        } else {
            out.extend_from_slice(format!("<span class=\"post-page-numbers current\" aria-current=\"page\">{number}</span>").as_bytes());
        }
    }
    out.extend_from_slice(b"</p>");
    out
}

fn renderer_permalink(renderer: &Renderer<'_>, post: &Post) -> Vec<u8> {
    crate::permalinks::permalink(renderer.context.site, post)
}

/// The pages of a post's content, split at `<!--nextpage-->` as
/// `generate_postdata()` splits them.
fn split_pages(content: &[u8]) -> Vec<Vec<u8>> {
    const BREAK: &[u8] = b"<!--nextpage-->";
    if !contains(content, BREAK) {
        return vec![content.to_vec()];
    }
    // generate_postdata() takes the newlines and block delimiters around
    // each break away first.
    let mut content = content.to_vec();
    for (from, to) in [
        (&b"\n<!--nextpage-->\n"[..], BREAK),
        (b"\n<!--nextpage-->", BREAK),
        (b"<!--nextpage-->\n", BREAK),
        (b"<!-- wp:nextpage -->", b""),
        (b"<!-- /wp:nextpage -->", b""),
    ] {
        content = replace(&content, from, to);
    }
    let content = content.strip_prefix(BREAK).unwrap_or(&content);
    let mut pages = Vec::new();
    let mut rest = content;
    for _ in 0..=content.len() {
        match rest.windows(BREAK.len()).position(|w| w == BREAK) {
            Some(at) => {
                pages.push(rest.get(..at).unwrap_or_default().to_vec());
                rest = rest.get(at + BREAK.len()..).unwrap_or_default();
            }
            None => break,
        }
    }
    pages.push(rest.to_vec());
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
pub(crate) fn password_form(renderer: &Renderer<'_>, post: &Post) -> Vec<u8> {
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
