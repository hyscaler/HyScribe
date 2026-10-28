// SPDX-License-Identifier: AGPL-3.0-or-later

//! The dynamic core blocks: the `render_block_core_*()` callbacks from
//! `wp-includes/blocks/`, run on the attributes as
//! `prepare_attributes_for_render()` leaves them.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_phpser::{Key, Value, php_trim};

use crate::registry::block_type;
use crate::site::{Request, Site};
use crate::supports::get_block_wrapper_attributes;
use crate::theme::Theme;

/// What a render callback reads besides its block.
pub(crate) struct Context<'a> {
    pub(crate) theme: &'a Theme,
    pub(crate) site: &'a dyn Site,
    pub(crate) request: Request,
}

/// One block being rendered: its name, parsed and prepared attributes.
struct Block<'a> {
    name: &'a str,
    parsed: &'a [(Key, Value)],
    attributes: Value,
}

/// The output of `name`'s render callback, `None` for a block that renders
/// its saved markup.
pub(crate) fn render(
    context: &Context<'_>,
    name: &str,
    parsed: &[(Key, Value)],
) -> Option<Vec<u8>> {
    let callback = match name {
        "core/site-tagline" => site_tagline,
        "core/site-title" => site_title,
        _ => return None,
    };
    let block = Block {
        name,
        parsed,
        attributes: Value::Array(
            block_type(name)
                .map(|t| t.prepare(parsed))
                .unwrap_or_default(),
        ),
    };
    Some(callback(context, &block))
}

impl Block<'_> {
    fn attribute(&self, name: &str) -> Option<&Value> {
        self.attributes.get_str(name).filter(|v| **v != Value::Null)
    }

    /// `get_block_wrapper_attributes( array( 'class' => $class ) )`.
    fn wrapper(&self, context: &Context<'_>, class: &[u8]) -> String {
        let extra = [("class", Value::String(class.to_vec()))];
        get_block_wrapper_attributes(self.name, self.parsed, &extra, &context.theme.global)
    }

    /// `has-text-align-<textAlign>`, or empty without an alignment.
    fn text_align_class(&self) -> Vec<u8> {
        match self.attribute("textAlign").filter(|v| !v.is_empty()) {
            Some(align) => [b"has-text-align-", &align.to_php_string()[..]].concat(),
            None => Vec::new(),
        }
    }
}

/// `'h' . (int) $level`.
fn heading(level: &Value) -> Vec<u8> {
    format!("h{}", level.to_float() as i64).into_bytes()
}

/// `sprintf( '<%1$s %2$s>%3$s</%1$s>', $tag, $wrapper, $content )`.
fn element(tag: &[u8], wrapper: &str, content: &[u8]) -> Vec<u8> {
    [
        b"<",
        tag,
        b" ",
        wrapper.as_bytes(),
        b">",
        content,
        b"</",
        tag,
        b">",
    ]
    .concat()
}

/// `render_block_core_site_title()`.
fn site_title(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let title = context.site.option("blogname").unwrap_or_default();
    if Value::String(php_trim(&title).to_vec()).is_empty() {
        return Vec::new();
    }
    let mut classes = block.text_align_class();
    if block
        .attributes
        .get_path(&["style", "elements", "link", "color", "text"])
        .is_some_and(|v| *v != Value::Null)
    {
        classes.extend_from_slice(b" has-link-color");
    }
    let level = block.attribute("level");
    let tag = match level {
        Some(Value::Int(0)) => b"p".to_vec(),
        _ => level.map_or(b"h1".to_vec(), heading),
    };
    let content = if block.attribute("isLink").is_some_and(|v| !v.is_empty()) {
        let request = context.request;
        let page_for_posts = context
            .site
            .option("page_for_posts")
            .map_or(0, |v| Value::String(v).to_float() as u64);
        let current = !request.paged
            && (request.front_page || request.home && page_for_posts != request.queried_object_id);
        let target = block
            .attribute("linkTarget")
            .filter(|v| !v.is_empty())
            .map_or(b"_self".to_vec(), Value::to_php_string);
        [
            b"<a href=\"".as_slice(),
            &esc_url(&context.site.home_url()),
            b"\" target=\"",
            esc_attr(&target).as_bytes(),
            b"\" rel=\"home\"",
            if current {
                b" aria-current=\"page\""
            } else {
                b""
            },
            b">",
            esc_html(&title).as_bytes(),
            b"</a>",
        ]
        .concat()
    } else {
        esc_html(&title).into_bytes()
    };
    element(&tag, &block.wrapper(context, php_trim(&classes)), &content)
}

/// `render_block_core_site_tagline()`. The tagline prints as stored.
fn site_tagline(context: &Context<'_>, block: &Block<'_>) -> Vec<u8> {
    let tagline = context.site.option("blogdescription").unwrap_or_default();
    if Value::String(tagline.clone()).is_empty() {
        return Vec::new();
    }
    let wrapper = block.wrapper(context, &block.text_align_class());
    let tag = match block.attribute("level") {
        Some(level) if *level != Value::Int(0) => heading(level),
        _ => b"p".to_vec(),
    };
    element(&tag, &wrapper, &tagline)
}

#[cfg(test)]
mod tests;
