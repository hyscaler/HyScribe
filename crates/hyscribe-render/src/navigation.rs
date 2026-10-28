// SPDX-License-Identifier: AGPL-3.0-or-later

//! The navigation block (`WP_Navigation_Block_Renderer`) and the navigation
//! link block.
//!
//! Ported: menus from inner blocks, the responsive overlay with its
//! Interactivity API directives, and unique `aria-label`s. Not yet: menus
//! from a `wp_navigation` post (`ref`), the fallback menu, submenus and
//! their directives, custom overlay template parts and the viewport layout
//! classes; a navigation block that needs a menu post or the fallback
//! renders with no items.

use hyscribe_html::TagProcessor;
use hyscribe_kses::{esc_attr, esc_url, safecss_filter_attr};
use hyscribe_phpser::{Key, Value, is_empty_string, isset, php_trim, wp_array_set};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::kses::wp_kses_post;
use crate::site::Archive;
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

/// Blocks that `get_markup_for_inner_block()` wraps in a list item.
const LISTABLE: [&str; 3] = ["core/site-title", "core/site-logo", "core/social-links"];

const TOGGLE_ICON: &[u8] = br#"<svg width="24" height="24" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" aria-hidden="true" focusable="false"><path d="M4 7.5h16v1.5H4z"></path><path d="M4 15h16v1.5H4z"></path></svg>"#;
const MENU_ICON: &[u8] = br#"<svg width="24" height="24" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path d="M5 5v1.5h14V5H5z"></path><path d="M5 12.8h14v-1.5H5v1.5z"></path><path d="M5 19h14v-1.5H5V19z"></path></svg>"#;
const CLOSE_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true" focusable="false"><path d="m13.06 12 6.47-6.47-1.06-1.06L12 10.94 5.53 4.47 4.47 5.53 10.94 12l-6.47 6.47 1.06 1.06L12 13.06l6.47 6.47 1.06-1.06L13.06 12Z"></path></svg>"#;
const SUBMENU_ICON: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="12" height="12" viewBox="0 0 12 12" fill="none" aria-hidden="true" focusable="false"><path d="M1.50002 4L6.00002 8L10.5 4" stroke-width="1.5"></path></svg>"#;

const OPEN_DIRECTIVES: &[u8] = b"\n\t\t\t\tdata-wp-on--click=\"actions.openMenuOnClick\"\n\t\t\t\tdata-wp-on--keydown=\"actions.handleMenuKeydown\"\n\t\t\t";
const CONTAINER_DIRECTIVES: &[u8] = b"\n\t\t\t\tdata-wp-class--has-modal-open=\"state.isMenuOpen\"\n\t\t\t\tdata-wp-class--is-menu-open=\"state.isMenuOpen\"\n\t\t\t\tdata-wp-watch=\"callbacks.initMenu\"\n\t\t\t\tdata-wp-on--keydown=\"actions.handleMenuKeydown\"\n\t\t\t\tdata-wp-on--focusout=\"actions.handleMenuFocusout\"\n\t\t\t\ttabindex=\"-1\"\n\t\t\t";
const DIALOG_DIRECTIVES: &[u8] = b"\n\t\t\t\tdata-wp-bind--aria-modal=\"state.ariaModal\"\n\t\t\t\tdata-wp-bind--aria-label=\"state.ariaLabel\"\n\t\t\t\tdata-wp-bind--role=\"state.roleAttribute\"\n\t\t\t";
const CLOSE_DIRECTIVES: &[u8] = b"\n\t\t\t\tdata-wp-on--click=\"actions.closeMenuOnClick\"\n\t\t\t";
const CONTENT_DIRECTIVES: &[u8] =
    b"\n\t\t\t\tdata-wp-watch=\"callbacks.focusFirstElement\"\n\t\t\t";
/// `get_nav_element_directives()`: its context never varies.
const NAV_DIRECTIVES: &[u8] = b"\n\t\t data-wp-interactive=\"core/navigation\" data-wp-context='{\"overlayOpenedBy\":{\"click\":false,\"hover\":false,\"focus\":false},\"type\":\"overlay\",\"roleAttribute\":\"\",\"ariaLabel\":\"Menu\"}'";

impl Renderer<'_> {
    /// `WP_Navigation_Block_Renderer::render()`.
    pub(crate) fn navigation(&mut self, block: &Block) -> Vec<u8> {
        let parsed =
            typographic_presets_backcompatibility(block.attrs.as_deref().unwrap_or_default());
        let mut attributes = prepared("core/navigation", &parsed);
        for (rgb, named, custom) in [
            ("rgbTextColor", "textColor", "customTextColor"),
            (
                "rgbBackgroundColor",
                "backgroundColor",
                "customBackgroundColor",
            ),
        ] {
            if let Some(rgb_value) = attributes.get_str(rgb).filter(|v| isset(Some(v))).cloned()
                && attributes.get_str(named).is_none_or(Value::is_empty)
            {
                wp_array_set(&mut attributes, &[custom], rgb_value);
            }
            let key = Key::String(rgb.as_bytes().to_vec());
            let kept = attributes
                .as_array()
                .unwrap_or_default()
                .iter()
                .filter(|(k, _)| *k != key);
            attributes = Value::Array(kept.cloned().collect());
        }
        // The `ref` menu post and the fallback are not ported: both give
        // no items here.
        let inner_blocks: &[Block] = if attributes.get_str("ref").is_some() {
            &[]
        } else {
            &block.inner_blocks
        };
        if has_block_type(inner_blocks, "core/navigation") {
            return Vec::new();
        }
        let within_overlay = attributes
            .get_str("_isWithinOverlayTemplatePart")
            .is_some_and(|v| !v.is_empty());
        let tag: &[u8] = if within_overlay { b"div" } else { b"nav" };
        let nav_attributes = self.nav_attributes(&attributes, &parsed, within_overlay);
        let outer = self.navigation.replace(attributes.clone());
        let mut inner = self.inner_blocks_html(&attributes, &parsed, inner_blocks);
        self.navigation = outer;
        if is_responsive(&attributes) {
            inner = self.responsive_container(&attributes, &inner);
        }
        [
            &b"<"[..],
            tag,
            b" ",
            &nav_attributes,
            b">",
            &inner,
            b"</",
            tag,
            b">",
        ]
        .concat()
    }

    /// `get_nav_attributes()`.
    fn nav_attributes(
        &mut self,
        attributes: &Value,
        parsed: &[(Key, Value)],
        within_overlay: bool,
    ) -> Vec<u8> {
        let name = attributes
            .get_str("ariaLabel")
            .map(Value::to_php_string)
            .unwrap_or_default();
        let name = if within_overlay {
            name
        } else {
            // get_unique_navigation_name()
            let seen = self.navigation_names.entry(name.clone()).or_insert(0);
            *seen += 1;
            match *seen {
                1 => name,
                count => [&name[..], format!(" {count}").as_bytes()].concat(),
            }
        };
        let mut extra = vec![
            ("class", Value::String(classes(attributes))),
            ("style", Value::String(styles(attributes))),
        ];
        if !is_empty_string(&name) {
            extra.push(("aria-label", Value::String(name)));
        }
        let mut wrapper = get_block_wrapper_attributes(
            "core/navigation",
            parsed,
            &extra,
            &self.context.theme.global,
        )
        .into_bytes();
        if is_responsive(attributes) {
            wrapper.push(b' ');
            wrapper.extend_from_slice(NAV_DIRECTIVES);
        }
        wrapper
    }

    /// `get_inner_blocks_html()`, with the items in a list where they are
    /// list items.
    fn inner_blocks_html(
        &mut self,
        attributes: &Value,
        parsed: &[(Key, Value)],
        inner_blocks: &[Block],
    ) -> Vec<u8> {
        let container_class = [
            &b"wp-block-navigation__container "[..],
            &classes(attributes),
        ]
        .concat();
        let extra = [
            ("class", Value::String(container_class)),
            ("style", Value::String(styles(attributes))),
        ];
        let container = get_block_wrapper_attributes(
            "core/navigation",
            parsed,
            &extra,
            &self.context.theme.global,
        );
        let mut html = Vec::new();
        let mut list_open = false;
        for inner in inner_blocks {
            let mut markup = self.block(inner);
            if !is_empty_string(&markup)
                && LISTABLE.contains(&inner.name.as_deref().unwrap_or_default())
            {
                markup = [
                    &b"<li class=\"wp-block-navigation-item\">"[..],
                    &markup,
                    b"</li>",
                ]
                .concat();
            }
            let is_list_item = TagProcessor::new(&markup).next_tag(Some(b"LI"), None);
            if is_list_item && !list_open {
                html.extend_from_slice(b"<ul ");
                html.extend_from_slice(container.as_bytes());
                html.push(b'>');
            } else if !is_list_item && list_open {
                html.extend_from_slice(b"</ul>");
            }
            list_open = is_list_item;
            html.extend(markup);
        }
        if list_open {
            html.extend_from_slice(b"</ul>");
        }
        html
    }

    /// `get_responsive_container_markup()`, without custom overlays.
    fn responsive_container(&mut self, attributes: &Value, inner_html: &[u8]) -> Vec<u8> {
        let colors = colors(attributes);
        let modal_id = esc_attr(&self.unique_id("modal-")).into_bytes();
        let hidden_by_default = attributes.get_str("overlayMenu") == Some(&string("always"));
        let mut container_classes = vec![b"wp-block-navigation__responsive-container".to_vec()];
        if hidden_by_default {
            container_classes.push(b"hidden-by-default".to_vec());
        }
        container_classes.push(colors.overlay_classes.join(&b' '));
        let open_classes = [
            b"wp-block-navigation__responsive-container-open".to_vec(),
            if hidden_by_default {
                b"always-shown".to_vec()
            } else {
                Vec::new()
            },
        ];
        let with_icons = attributes.get_str("hasIcon") == Some(&Value::Bool(true));
        let icon = match attributes.get_str("icon") {
            Some(icon) if *icon == string("menu") => MENU_ICON,
            _ => TOGGLE_ICON,
        };
        let (open_content, close_content, open_label, close_label): (&[u8], &[u8], &[u8], &[u8]) =
            if with_icons {
                (
                    icon,
                    CLOSE_ICON,
                    br#"aria-label="Open menu""#,
                    br#"aria-label="Close menu""#,
                )
            } else {
                (b"Menu", b"Close", b"", b"")
            };
        let overlay_styles = esc_attr(&safecss_filter_attr(&colors.overlay_styles));
        let overlay_styles = if is_empty_string(overlay_styles.as_bytes()) {
            Vec::new()
        } else {
            format!("style=\"{overlay_styles}\"").into_bytes()
        };
        let joined = |classes: &[Vec<u8>]| esc_attr(php_trim(&classes.join(&b' '))).into_bytes();
        [
            &b"<button aria-haspopup=\"dialog\" "[..],
            open_label,
            b" class=\"",
            &joined(&open_classes),
            b"\" ",
            OPEN_DIRECTIVES,
            b">",
            open_content,
            b"</button>\n\t\t\t\t<div class=\"",
            &joined(&container_classes),
            b"\" ",
            &overlay_styles,
            b" id=\"",
            &modal_id,
            b"\" ",
            CONTAINER_DIRECTIVES,
            b">\n\t\t\t\t\t<div class=\"wp-block-navigation__responsive-close\" tabindex=\"-1\">\n\t\t\t\t\t\t<div class=\"wp-block-navigation__responsive-dialog\" ",
            DIALOG_DIRECTIVES,
            b">\n\t\t\t\t\t\t\t<button ",
            close_label,
            b" class=\"wp-block-navigation__responsive-container-close\" ",
            CLOSE_DIRECTIVES,
            b">",
            close_content,
            b"</button>\n\t\t\t\t\t\t\t<div class=\"wp-block-navigation__responsive-container-content\" ",
            CONTENT_DIRECTIVES,
            b" id=\"",
            &modal_id,
            b"-content\">\n\t\t\t\t\t\t\t\t",
            inner_html,
            b"\n\t\t\t\t\t\t\t\t\n\t\t\t\t\t\t\t</div>\n\t\t\t\t\t\t</div>\n\t\t\t\t\t</div>\n\t\t\t\t</div>",
        ]
        .concat()
    }

    /// `render_block_core_navigation_link()`.
    pub(crate) fn navigation_link(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/navigation-link", parsed);
        let attribute = |name| attributes.get_str(name).filter(|v| isset(Some(v)));
        if !self.item_should_render(&attributes) {
            return Vec::new();
        }
        let Some(label) = attribute("label").filter(|v| !v.is_empty()) else {
            return Vec::new();
        };
        let mut inner_html = Vec::new();
        for inner in &block.inner_blocks {
            inner_html.extend(self.block(inner));
        }
        let has_submenu = !is_empty_string(php_trim(&inner_html));
        let is_active = self.is_active(&attributes);
        let mut class = b" wp-block-navigation-item".to_vec();
        if has_submenu {
            class.extend_from_slice(b" has-child");
        }
        if is_active {
            class.extend_from_slice(b" current-menu-item");
        }
        let extra = [("class", Value::String(class))];
        let wrapper = get_block_wrapper_attributes(
            "core/navigation-link",
            parsed,
            &extra,
            &self.context.theme.global,
        );
        let mut html =
            format!("<li {wrapper}><a class=\"wp-block-navigation-item__content\" ").into_bytes();
        if let Some(url) = attribute("url") {
            let url = maybe_urldecode(&url.to_php_string());
            html.extend_from_slice(b" href=\"");
            html.extend(esc_url(&url));
            html.push(b'"');
        }
        if is_active {
            html.extend_from_slice(b" aria-current=\"page\"");
        }
        if attribute("opensInNewTab") == Some(&Value::Bool(true)) {
            html.extend_from_slice(b" target=\"_blank\"  ");
        }
        if let Some(rel) = attribute("rel") {
            html.extend_from_slice(
                format!(" rel=\"{}\"", esc_attr(&rel.to_php_string())).as_bytes(),
            );
        } else if attribute("nofollow").is_some_and(|v| !v.is_empty()) {
            html.extend_from_slice(b" rel=\"nofollow\"");
        }
        if let Some(title) = attribute("title") {
            html.extend_from_slice(
                format!(" title=\"{}\"", esc_attr(&title.to_php_string())).as_bytes(),
            );
        }
        let upload_host = upload_host(&self.context.site.option("siteurl").unwrap_or_default());
        html.extend_from_slice(b"><span class=\"wp-block-navigation-item__label\">");
        html.extend(wp_kses_post(&label.to_php_string(), &upload_host));
        html.extend_from_slice(b"</span>");
        if let Some(description) = attribute("description").filter(|v| !v.is_empty()) {
            html.extend_from_slice(b"<span class=\"wp-block-navigation-item__description\">");
            html.extend(wp_kses_post(&description.to_php_string(), &upload_host));
            html.extend_from_slice(b"</span>");
        }
        html.extend_from_slice(b"</a>");
        let show_icon = self
            .navigation
            .as_ref()
            .and_then(|context| context.get_str("showSubmenuIcon"))
            .is_some_and(|v| !v.is_empty());
        if show_icon && has_submenu {
            html.extend_from_slice(b"<span class=\"wp-block-navigation__submenu-icon\">");
            html.extend_from_slice(SUBMENU_ICON);
            html.extend_from_slice(b"</span>");
        }
        if has_submenu {
            html.extend_from_slice(b"<ul class=\"wp-block-navigation__submenu-container\">");
            html.extend(inner_html);
            html.extend_from_slice(b"</ul>");
        }
        html.extend_from_slice(b"</li>");
        html
    }

    /// `block_core_shared_navigation_item_should_render()`: a link to a
    /// post or page needs that post, published.
    fn item_should_render(&self, attributes: &Value) -> bool {
        let id = attributes.get_str("id").filter(|v| v.is_numeric());
        let is_post_type = attributes.get_str("kind") == Some(&string("post-type"))
            || matches!(attributes.get_str("type"), Some(t) if *t == string("post") || *t == string("page"));
        match id {
            Some(id) if is_post_type => u64::try_from(id.to_int())
                .ok()
                .and_then(|id| self.context.site.post(id))
                .is_some_and(|post| post.status == "publish"),
            _ => true,
        }
    }

    /// Whether the link points at the queried object: its ID, and an object
    /// of the link's kind (`! empty( get_queried_object()->$kind )`).
    fn is_active(&self, attributes: &Value) -> bool {
        let request = self.context.request;
        let Some(id) = attributes.get_str("id").filter(|v| !v.is_empty()) else {
            return false;
        };
        let kind = match attributes.get_str("kind").filter(|v| !v.is_empty()) {
            Some(kind) => kind.to_php_string(),
            None => b"post_type".to_vec(),
        };
        let queried_is_term = matches!(request.archive, Some(Archive::Category(_)));
        let kind_matches = match kind.as_slice() {
            b"post-type" | b"post_type" => !queried_is_term,
            b"taxonomy" => queried_is_term,
            _ => false,
        };
        request.queried_object_id != 0
            && i64::try_from(request.queried_object_id) == Ok(id.to_int())
            && kind_matches
    }
}

/// `block_core_navigation_typographic_presets_backcompatibility()`, the
/// `render_block_data` filter: preset references in typography styles
/// become plain values.
fn typographic_presets_backcompatibility(parsed: &[(Key, Value)]) -> Vec<(Key, Value)> {
    let mut parsed = Value::Array(parsed.to_vec());
    for (attribute, prefix) in [
        ("fontStyle", "var:preset|font-style|"),
        ("fontWeight", "var:preset|font-weight|"),
        ("textDecoration", "var:preset|text-decoration|"),
        ("textTransform", "var:preset|text-transform|"),
    ] {
        let path = ["style", "typography", attribute];
        let Some(value) = parsed
            .get_path(&path)
            .filter(|v| !v.is_empty())
            .map(Value::to_php_string)
        else {
            continue;
        };
        let mut text = value
            .strip_prefix(prefix.as_bytes())
            .unwrap_or(&value)
            .to_vec();
        if attribute == "textDecoration" && text == b"strikethrough" {
            text = b"line-through".to_vec();
        }
        wp_array_set(&mut parsed, &path, Value::String(text));
    }
    parsed.as_array().unwrap_or_default().to_vec()
}

/// `is_responsive()`.
fn is_responsive(attributes: &Value) -> bool {
    let overlay = attributes
        .get_str("overlayMenu")
        .is_some_and(|v| isset(Some(v)) && *v != string("never"));
    overlay
        || attributes
            .get_str("isResponsive")
            .is_some_and(|v| !v.is_empty())
}

/// `block_core_navigation_block_tree_has_block_type()`.
fn has_block_type(blocks: &[Block], name: &str) -> bool {
    blocks.iter().any(|block| {
        block.name.as_deref() == Some(name) || has_block_type(&block.inner_blocks, name)
    })
}

/// `block_core_navigation_build_css_colors()` and its font size twin.
struct Colors {
    classes: Vec<Vec<u8>>,
    styles: Vec<u8>,
    overlay_classes: Vec<Vec<u8>>,
    overlay_styles: Vec<u8>,
}

fn colors(attributes: &Value) -> Colors {
    let mut colors = Colors {
        classes: Vec::new(),
        styles: Vec::new(),
        overlay_classes: Vec::new(),
        overlay_styles: Vec::new(),
    };
    let value = |name| attributes.get_str(name).map(Value::to_php_string);
    for (named, custom, flag, class, property, overlay) in [
        (
            "textColor",
            "customTextColor",
            "has-text-color",
            "color",
            "color",
            false,
        ),
        (
            "backgroundColor",
            "customBackgroundColor",
            "has-background",
            "background-color",
            "background-color",
            false,
        ),
        (
            "overlayTextColor",
            "customOverlayTextColor",
            "has-text-color",
            "color",
            "color",
            true,
        ),
        (
            "overlayBackgroundColor",
            "customOverlayBackgroundColor",
            "has-background",
            "background-color",
            "background-color",
            true,
        ),
    ] {
        let (classes, styles) = if overlay {
            (&mut colors.overlay_classes, &mut colors.overlay_styles)
        } else {
            (&mut colors.classes, &mut colors.styles)
        };
        let (named, custom) = (value(named), value(custom));
        if named.is_some() || custom.is_some() {
            classes.push(flag.as_bytes().to_vec());
        }
        if let Some(named) = named {
            classes.push([&b"has-"[..], &named, b"-", class.as_bytes()].concat());
        } else if let Some(custom) = custom {
            styles.extend_from_slice(format!("{property}: ").as_bytes());
            styles.extend(custom);
            styles.push(b';');
        }
    }
    if let Some(size) = value("fontSize") {
        colors
            .classes
            .push([&b"has-"[..], &size, b"-font-size"].concat());
    } else if let Some(size) = value("customFontSize") {
        colors.styles = [&colors.styles[..], b"font-size: ", &size, b"px;"].concat();
    }
    colors
}

/// `get_classes()`: colors, font size, `is-responsive`, the layout's legacy
/// classes and the text decoration, space separated.
fn classes(attributes: &Value) -> Vec<u8> {
    let mut classes = colors(attributes).classes;
    if is_responsive(attributes) {
        classes.push(b"is-responsive".to_vec());
    }
    let layout = |name| {
        attributes
            .get_path(&["layout", name])
            .filter(|v| isset(Some(v)))
    };
    let mut layout_class = Vec::new();
    if let Some(justify) = layout("justifyContent").and_then(Value::as_bytes)
        && matches!(justify, b"left" | b"right" | b"center" | b"space-between")
    {
        layout_class.extend_from_slice(b"items-justified-");
        layout_class.extend_from_slice(justify);
    }
    if layout("orientation") == Some(&string("vertical")) {
        layout_class.extend_from_slice(b" is-vertical");
    }
    if layout("flexWrap") == Some(&string("nowrap")) {
        layout_class.extend_from_slice(b" no-wrap");
    }
    if !layout_class.is_empty() {
        classes.push(layout_class);
    }
    if let Some(decoration) = attributes
        .get_path(&["style", "typography", "textDecoration"])
        .filter(|v| !v.is_empty())
    {
        classes.push([&b"has-text-decoration-"[..], &decoration.to_php_string()].concat());
    }
    classes.join(&b' ')
}

/// `get_styles()`.
fn styles(attributes: &Value) -> Vec<u8> {
    let own = attributes
        .get_str("styles")
        .map(Value::to_php_string)
        .unwrap_or_default();
    [own, colors(attributes).styles].concat()
}

/// `block_core_navigation_link_maybe_urldecode()`: a URL whose query
/// values are still encoded after `parse_str()` is decoded once.
fn maybe_urldecode(url: &[u8]) -> Vec<u8> {
    let query = url.split(|&b| b == b'#').next().and_then(|before| {
        before
            .iter()
            .position(|&b| b == b'?')
            .and_then(|at| before.get(at + 1..))
    });
    let encoded = query.is_some_and(|query| {
        parse_str_values(query)
            .iter()
            .any(|value| !is_empty_string(value) && rawurldecode(value) != *value)
    });
    if encoded {
        rawurldecode(url)
    } else {
        url.to_vec()
    }
}

/// The string values `parse_str()` gives for a query, the last of each
/// name; names with a `[...]` index give arrays, which have none.
fn parse_str_values(query: &[u8]) -> Vec<Vec<u8>> {
    let mut values: Vec<(Vec<u8>, Option<Vec<u8>>)> = Vec::new();
    for pair in query.split(|&b| b == b'&').filter(|pair| !pair.is_empty()) {
        let mut parts = pair.splitn(2, |&b| b == b'=');
        let name = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        let name = urldecode(name);
        let name = name
            .get(name.iter().take_while(|&&b| b == b' ').count()..)
            .unwrap_or_default();
        let base_len = name
            .iter()
            .position(|&b| b == b'[')
            .filter(|&at| name.get(at..).is_some_and(|rest| rest.contains(&b']')));
        let (base, value) = match base_len {
            Some(at) => (name.get(..at).unwrap_or_default(), None),
            None => (name, Some(urldecode(value))),
        };
        if base.is_empty() {
            continue;
        }
        let base: Vec<u8> = base
            .iter()
            .map(|&b| {
                if matches!(b, b' ' | b'.' | b'[') {
                    b'_'
                } else {
                    b
                }
            })
            .collect();
        values.retain(|(name, _)| *name != base);
        values.push((base, value));
    }
    values.into_iter().filter_map(|(_, value)| value).collect()
}

/// `rawurldecode()`: `%XX` sequences, in either case, become bytes.
fn rawurldecode(text: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len());
    let mut at = 0;
    while let Some(&b) = text.get(at) {
        let digit = |offset| {
            text.get(at + offset)
                .and_then(|&d| char::from(d).to_digit(16))
        };
        match (b, digit(1), digit(2)) {
            (b'%', Some(high), Some(low)) => {
                out.extend(u8::try_from(high * 16 + low));
                at += 3;
            }
            _ => {
                out.push(b);
                at += 1;
            }
        }
    }
    out
}

/// `urldecode()`: `rawurldecode()` with `+` as a space.
fn urldecode(text: &[u8]) -> Vec<u8> {
    let spaced: Vec<u8> = text
        .iter()
        .map(|&b| if b == b'+' { b' ' } else { b })
        .collect();
    rawurldecode(&spaced)
}

/// The host, with `:port`, of `wp_upload_dir()`'s URL, which lives under
/// `siteurl`.
fn upload_host(siteurl: &[u8]) -> Vec<u8> {
    let after_scheme = siteurl
        .windows(3)
        .position(|w| w == b"://")
        .and_then(|at| siteurl.get(at + 3..))
        .unwrap_or_default();
    let authority = after_scheme
        .split(|&b| matches!(b, b'/' | b'?' | b'#'))
        .next()
        .unwrap_or_default();
    let host = authority.rsplit(|&b| b == b'@').next().unwrap_or_default();
    host.to_vec()
}

fn string(text: &str) -> Value {
    Value::String(text.as_bytes().to_vec())
}

#[cfg(test)]
mod tests;
