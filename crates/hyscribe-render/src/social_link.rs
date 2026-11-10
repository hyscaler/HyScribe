// SPDX-License-Identifier: AGPL-3.0-or-later

//! The social links block's context and the social link block,
//! `render_block_core_social_link()`, with the services WordPress names and
//! draws (`reference/social-link/export.sh`).

use std::sync::OnceLock;

use hyscribe_html::TagProcessor;
use hyscribe_kses::{esc_html, esc_url, url_scheme};
use hyscribe_phpser::{Value, json_decode, php_trim};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::formatting::{antispambot, is_email};
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

/// `block_core_social_link_services()`: each service's name and icon.
fn services() -> &'static Value {
    static SERVICES: OnceLock<Value> = OnceLock::new();
    SERVICES.get_or_init(|| {
        json_decode(include_bytes!("social_link/services.json")).unwrap_or_default()
    })
}

/// A field of `service`, or of `share` for a service WordPress lacks.
fn service_field(service: &[u8], field: &str) -> Vec<u8> {
    let services = services();
    let key = String::from_utf8_lossy(service);
    services
        .get_str(&key)
        .and_then(|s| s.get_str(field))
        .or_else(|| services.get_path(&["share", field]))
        .map(Value::to_php_string)
        .unwrap_or_default()
}

impl Renderer<'_> {
    /// The social links block, whose attributes its links get as context.
    pub(crate) fn social_links(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let outer = self
            .social_links
            .replace(prepared("core/social-links", parsed));
        let content = self.saved_markup(block);
        self.social_links = outer;
        content
    }

    /// `render_block_core_social_link()`.
    pub(crate) fn social_link(&self, block: &Block) -> Vec<u8> {
        let name = "core/social-link";
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared(name, parsed);
        let get = |key: &str| attributes.get_str(key).filter(|v| **v != Value::Null);
        let context = self.social_links.as_ref();
        let from_context = |key: &str| {
            context
                .and_then(|c| c.get_str(key))
                .filter(|v| **v != Value::Null)
        };
        let url = get("url")
            .map(Value::to_php_string)
            .filter(|u| !u.is_empty() && u != b"0");
        let Some(mut url) = url else {
            return Vec::new();
        };
        let service = get("service").map_or_else(|| b"Icon".to_vec(), Value::to_php_string);
        let label = get("label")
            .filter(|l| !l.is_empty())
            .map(|l| php_trim(&l.to_php_string()).to_vec());
        let text = label
            .filter(|l| !l.is_empty() && l != b"0")
            .unwrap_or_else(|| service_field(&service, "name"));
        if is_email(&url) {
            url = [b"mailto:".as_slice(), &antispambot(&url)].concat();
        }
        if url_scheme(&url).is_none() && !url.starts_with(b"//") && !url.starts_with(b"#") {
            url = [b"https://".as_slice(), &url].concat();
        }
        let mut class = [b"wp-social-link wp-social-link-".as_slice(), &service, b" "].concat();
        let mut colors = Vec::new();
        if let Some(color) = from_context("iconColor") {
            colors.push([b"has-".as_slice(), &color.to_php_string(), b"-color"].concat());
        }
        if let Some(color) = from_context("iconBackgroundColor") {
            colors.push(
                [
                    b"has-".as_slice(),
                    &color.to_php_string(),
                    b"-background-color",
                ]
                .concat(),
            );
        }
        class.extend_from_slice(&colors.join(&b' '));
        let mut style = Vec::new();
        if let Some(color) = from_context("iconColorValue") {
            style.extend_from_slice(&[b"color:".as_slice(), &color.to_php_string(), b";"].concat());
        }
        if let Some(color) = from_context("iconBackgroundColorValue") {
            style.extend_from_slice(
                &[
                    b"background-color:".as_slice(),
                    &color.to_php_string(),
                    b";",
                ]
                .concat(),
            );
        }
        let extra = [
            ("class", Value::String(class)),
            ("style", Value::String(style)),
        ];
        let wrapper =
            get_block_wrapper_attributes(name, parsed, &extra, &self.context.theme.global);
        let shown = from_context("showLabels").is_some_and(|v| !v.is_empty());
        let link = [
            format!("<li {wrapper}><a href=\"").as_bytes(),
            &esc_url(&url),
            b"\" class=\"wp-block-social-link-anchor\">",
            &service_field(&service, "icon"),
            b"<span class=\"wp-block-social-link-label",
            if shown { b"" } else { b" screen-reader-text" },
            b"\">",
            esc_html(&text).as_bytes(),
            b"</span></a></li>",
        ]
        .concat();
        let rel = get("rel").map(Value::to_php_string).unwrap_or_default();
        let mut tags = TagProcessor::new(&link);
        tags.next_tag(Some(b"a"), None);
        if from_context("openInNewTab").is_some_and(|v| !v.is_empty()) {
            tags.set_attribute(
                b"rel",
                php_trim(&[&rel[..], b" noopener nofollow"].concat()),
            );
            tags.set_attribute(b"target", b"_blank");
        } else if !rel.is_empty() {
            tags.set_attribute(b"rel", php_trim(&rel));
        }
        tags.get_updated_html().to_vec()
    }
}

#[cfg(test)]
mod tests;
