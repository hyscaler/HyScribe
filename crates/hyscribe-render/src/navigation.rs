// SPDX-License-Identifier: AGPL-3.0-or-later

//! The navigation block (`WP_Navigation_Block_Renderer`) and the navigation
//! link and submenu blocks.
//!
//! Ported: menus from inner blocks or a `wp_navigation` post (`ref`), the
//! fallback (the latest published `wp_navigation` post, else a page list),
//! the page list block and submenus with their directives, the responsive
//! overlay and unique `aria-label`s. Not yet: creating the fallback's post,
//! custom overlay template parts and the viewport layout classes.

use hyscribe_html::TagProcessor;
use hyscribe_kses::{esc_attr, esc_url, safecss_filter_attr, wp_strip_all_tags};
use hyscribe_model::{OrderBy, Post, PostQuery};
use hyscribe_phpser::{Key, Value, is_empty_string, isset, php_trim, wp_array_set};
use hyscribe_theme_json::kebab_case;

use crate::bindings::computed_attributes;
use crate::blocks::{Block, parse_blocks, serialize_blocks};
use crate::core_blocks::{merged, prepared};
use crate::formatting::find;
use crate::kses::wp_kses_post;
use crate::pages::add_child;
use crate::site::{Archive, Site};
use crate::supports::forced_colors;
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
        let block_attributes = prepared("core/navigation", &parsed);
        let mut attributes = block_attributes.clone();
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
        // `navigationMenuId`, the older name, wins over `ref`.
        let menu = ["navigationMenuId", "ref"]
            .into_iter()
            .find_map(|key| attributes.get_str(key))
            .map(|id| self.ref_blocks(id));
        let found;
        let (inner_blocks, context): (&[Block], &Value) = match menu {
            Some(Some(blocks)) => {
                found = blocks;
                (&found, &attributes)
            }
            None if !block.inner_blocks.is_empty() => (&block.inner_blocks, &block_attributes),
            _ => {
                found = self.fallback_blocks();
                (&found, &attributes)
            }
        };
        if has_block_type(inner_blocks, "core/navigation") {
            return Vec::new();
        }
        if self.is_interactive(&attributes, inner_blocks) {
            self.assets
                .enqueue_module("@wordpress/block-library/navigation/view");
        }
        let within_overlay = attributes
            .get_str("_isWithinOverlayTemplatePart")
            .is_some_and(|v| !v.is_empty());
        let tag: &[u8] = if within_overlay { b"div" } else { b"nav" };
        let nav_attributes = self.nav_attributes(&attributes, &parsed, within_overlay);
        let outer = self.navigation.replace(context.clone());
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
        // get_navigation_name()
        let label = attributes
            .get_str("ariaLabel")
            .map(Value::to_php_string)
            .unwrap_or_default();
        let menu = attributes.get_str("ref").and_then(|id| self.menu(id));
        let name = match menu {
            Some(menu) if is_empty_string(&label) => menu.title,
            _ => label,
        };
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
        let has_submenus = self.has_submenus(inner_blocks);
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
        if has_submenus && self.is_interactive(attributes, inner_blocks) {
            html = add_directives_to_submenu(&html, &submenu_visibility(attributes));
        }
        html
    }

    /// `is_interactive()`: submenus that open on click or show an icon, or
    /// a responsive menu.
    fn is_interactive(&mut self, attributes: &Value, inner_blocks: &[Block]) -> bool {
        let open_on_click = submenu_visibility(attributes) == b"click";
        let icon = attributes
            .get_str("showSubmenuIcon")
            .is_some_and(|v| !v.is_empty());
        (self.has_submenus(inner_blocks) && (open_on_click || icon)) || is_responsive(attributes)
    }

    /// `has_submenus()`: a submenu block, or a page list with child pages.
    fn has_submenus(&mut self, inner_blocks: &[Block]) -> bool {
        if self.has_submenus {
            return true;
        }
        for inner in inner_blocks {
            match inner.name.as_deref() {
                Some("core/page-list") => {
                    self.has_submenus = self.pages().iter().any(|page| page.parent != 0);
                }
                Some("core/navigation-submenu") => {
                    self.has_submenus = true;
                    break;
                }
                _ => {}
            }
        }
        self.has_submenus
    }

    /// `get_pages()` sorted by menu order and title.
    fn pages(&self) -> Vec<Post> {
        get_page_children(self.context.site.query(&PostQuery {
            post_types: vec!["page".to_string()],
            order: vec![(OrderBy::MenuOrder, true), (OrderBy::Title, true)],
            limit: u32::MAX,
            ..PostQuery::default()
        }))
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

    /// The prepared attributes with those block bindings computed.
    fn bound(&self, name: &str, block: &Block) -> Value {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let computed = computed_attributes(&self.context, block, self.pattern_overrides.as_ref());
        merged(prepared(name, parsed), &computed)
    }

    /// `render_block_core_navigation_link()`.
    pub(crate) fn navigation_link(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = self.bound("core/navigation-link", block);
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
        let url = attribute("url").map(|url| maybe_urldecode(&url.to_php_string()));
        let label = wp_kses_post(&label.to_php_string(), &self.context.upload_host());
        html.extend(self.anchor_rest(&attributes, url, &label, is_active));
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

    /// A navigation item's link from its `href` on: `aria-current`, a new
    /// tab, `rel` and `title`, then the label and description and the closer.
    fn anchor_rest(
        &self,
        attributes: &Value,
        url: Option<Vec<u8>>,
        label: &[u8],
        is_active: bool,
    ) -> Vec<u8> {
        let attribute = |name| attributes.get_str(name).filter(|v| isset(Some(v)));
        let mut html = Vec::new();
        if let Some(url) = url {
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
        html.extend_from_slice(b"><span class=\"wp-block-navigation-item__label\">");
        html.extend_from_slice(label);
        html.extend_from_slice(b"</span>");
        html.extend(self.description(attributes));
        html.extend_from_slice(b"</a>");
        html
    }

    /// A navigation item's description in its span, empty for none.
    fn description(&self, attributes: &Value) -> Vec<u8> {
        attributes
            .get_str("description")
            .filter(|v| !v.is_empty())
            .map(|d| {
                [
                    b"<span class=\"wp-block-navigation-item__description\">".as_slice(),
                    &wp_kses_post(&d.to_php_string(), &self.context.upload_host()),
                    b"</span>",
                ]
                .concat()
            })
            .unwrap_or_default()
    }

    /// `render_block_core_navigation_submenu()`.
    pub(crate) fn navigation_submenu(&mut self, block: &Block) -> Vec<u8> {
        let name = "core/navigation-submenu";
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let mut attributes = self.bound(name, block);
        if !self.item_should_render(&attributes) {
            return Vec::new();
        }
        let attribute = |name| attributes.get_str(name).filter(|v| isset(Some(v))).cloned();
        let Some(label) = attribute("label").filter(|v| !v.is_empty()) else {
            return Vec::new();
        };
        let mut inner_html = Vec::new();
        for inner in &block.inner_blocks {
            inner_html.extend(self.block(inner));
        }
        let has_submenu = !is_empty_string(php_trim(&inner_html));
        let is_active = self.is_active(&attributes);
        let none = Value::Null;
        let context = self.navigation.as_ref().unwrap_or(&none);
        let show_icon = context
            .get_str("showSubmenuIcon")
            .is_some_and(|v| !v.is_empty());
        let visibility = submenu_visibility(context);
        let open_on_click = visibility == b"click";
        let mut classes = b"wp-block-navigation-item".to_vec();
        for (on, class) in [
            (has_submenu, &b" has-child"[..]),
            (open_on_click, b" open-on-click"),
            (visibility == b"hover" && show_icon, b" open-on-hover-click"),
            (visibility == b"always", b" open-always"),
            (is_active, b" current-menu-item"),
        ] {
            if on {
                classes.extend_from_slice(class);
            }
        }
        let global = &self.context.theme.global;
        let wrapper = get_block_wrapper_attributes(
            name,
            parsed,
            &[("class", Value::String(classes))],
            global,
        );
        let upload_host = self.context.upload_host();
        let label = wp_kses_post(&label.to_php_string(), &upload_host);
        let aria_label = esc_attr(&[&wp_strip_all_tags(&label, false)[..], b" submenu"].concat());
        let mut html = format!("<li {wrapper}>").into_bytes();
        if open_on_click {
            html.extend_from_slice(format!("<button aria-label=\"{aria_label}\" class=\"wp-block-navigation-item__content wp-block-navigation-submenu__toggle\" aria-expanded=\"false\"><span class=\"wp-block-navigation-item__label\">").as_bytes());
            html.extend_from_slice(&label);
            html.extend_from_slice(b"</span>");
            html.extend(self.description(&attributes));
            html.extend_from_slice(b"</button>");
            if has_submenu {
                html.extend_from_slice(b"<span class=\"wp-block-navigation__submenu-icon\">");
                html.extend_from_slice(SUBMENU_ICON);
                html.extend_from_slice(b"</span>");
            }
        } else {
            html.extend_from_slice(b"<a class=\"wp-block-navigation-item__content\"");
            let url = attribute("url")
                .filter(|v| !v.is_empty())
                .map(|url| url.to_php_string());
            html.extend(self.anchor_rest(&attributes, url, &label, is_active));
            if show_icon && has_submenu {
                html.extend_from_slice(format!("<button aria-label=\"{aria_label}\" class=\"wp-block-navigation__submenu-icon wp-block-navigation-submenu__toggle\" aria-expanded=\"false\">").as_bytes());
                html.extend_from_slice(SUBMENU_ICON);
                html.extend_from_slice(b"</button>");
            }
        }
        if has_submenu {
            // The overlay's colors, copied from the navigation block.
            for (key, path) in [
                ("overlayTextColor", &["textColor"][..]),
                ("overlayBackgroundColor", &["backgroundColor"]),
                ("customOverlayTextColor", &["style", "color", "text"]),
                (
                    "customOverlayBackgroundColor",
                    &["style", "color", "background"],
                ),
            ] {
                if let Some(color) = context.get_str(key) {
                    wp_array_set(&mut attributes, path, color.clone());
                }
            }
            let (class, style) = forced_colors(&attributes);
            let mut container = b"wp-block-navigation__submenu-container".to_vec();
            if !class.is_empty() {
                container.push(b' ');
                container.extend_from_slice(&class);
            }
            // strpos(): a match at 0 is falsy.
            if find(&inner_html, 1, b"current-menu-item").is_some() {
                let mut tags = TagProcessor::new(&html);
                while tags.next_tag(None, Some(b"wp-block-navigation-item")) {
                    tags.add_class(b"current-menu-ancestor");
                }
                html = tags.get_updated_html().to_vec();
            }
            let extra = [
                ("class", Value::String(container)),
                ("style", Value::String(style)),
            ];
            let wrapper = get_block_wrapper_attributes(name, parsed, &extra, global);
            html.extend_from_slice(format!("<ul {wrapper}>").as_bytes());
            html.extend(inner_html);
            html.extend_from_slice(b"</ul>");
        }
        html.extend_from_slice(b"</li>");
        html
    }

    /// `render_block_core_home_link()`: a link to the home page, current on
    /// the front page or on a home that is not the page for posts.
    pub(crate) fn home_link(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/home-link", parsed);
        let text = |name| {
            attributes
                .get_str(name)
                .filter(|v| !v.is_empty())
                .map(Value::to_php_string)
        };
        let site = self.context.site;
        let current = self.context.request.is_site_home(site);
        let (mut classes, style) = home_link_colors(self.navigation.as_ref());
        classes.push(b"wp-block-navigation-item".to_vec());
        if current {
            classes.push(b"current-menu-item".to_vec());
        }
        let extra = [
            ("class", Value::String(classes.join(&b' '))),
            ("style", Value::String(style)),
        ];
        let wrapper = get_block_wrapper_attributes(
            "core/home-link",
            parsed,
            &extra,
            &self.context.theme.global,
        );
        let upload_host = self.context.upload_host();
        let mut html = format!(
            "<li {wrapper}><a class=\"wp-block-home-link__content wp-block-navigation-item__content\" href=\""
        )
        .into_bytes();
        html.extend(esc_url(&site.home_url()));
        html.extend_from_slice(b"\" rel=\"home\" ");
        if attributes.get_str("opensInNewTab") == Some(&Value::Bool(true)) {
            html.extend_from_slice(b" target=\"_blank\"");
        }
        if current {
            html.extend_from_slice(b" aria-current=\"page\"");
        }
        html.push(b'>');
        let label = text("label").unwrap_or_else(|| b"Home".to_vec());
        html.extend(wp_kses_post(&label, &upload_host));
        if let Some(description) = text("description") {
            html.extend_from_slice(b"<span class=\"wp-block-navigation-item__description\">");
            html.extend(wp_kses_post(&description, &upload_host));
            html.extend_from_slice(b"</span>");
        }
        html.extend_from_slice(b"</a></li>");
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
        let queried_is_term = matches!(request.archive, Some(Archive::Term(_, Some(_))));
        let kind_matches = match kind.as_slice() {
            b"post-type" | b"post_type" => !queried_is_term,
            b"taxonomy" => queried_is_term,
            _ => false,
        };
        request.queried_object_id != 0
            && i64::try_from(request.queried_object_id) == Ok(id.to_int())
            && kind_matches
    }

    /// The post a `ref` names; 0, the global post in PHP, finds none.
    fn menu_post(&self, id: &Value) -> Option<Post> {
        let id = u64::try_from(id.to_int()).ok()?;
        self.context.site.post(id)
    }

    /// The menu `id` names, if it is published.
    fn menu(&self, id: &Value) -> Option<Post> {
        self.menu_post(id).filter(|post| post.status == "publish")
    }

    /// `get_inner_blocks_from_navigation_post()`: none for a missing post,
    /// `None` (the fallback) for one not published.
    fn ref_blocks(&self, id: &Value) -> Option<Vec<Block>> {
        match self.menu_post(id) {
            None => Some(Vec::new()),
            Some(post) => (post.status == "publish").then(|| menu_blocks(&post.content)),
        }
    }

    /// `block_core_navigation_get_fallback_blocks()`: the blocks of the most
    /// recently published menu, or a page list.
    fn fallback_blocks(&self) -> Vec<Block> {
        let page_list = vec![Block {
            name: Some("core/page-list".to_string()),
            attrs: Some(Vec::new()),
            inner_blocks: Vec::new(),
            inner_html: Vec::new(),
            inner_content: Vec::new(),
        }];
        let latest = self.context.site.query(&PostQuery {
            post_types: vec!["wp_navigation".to_string()],
            limit: 1,
            ..PostQuery::default()
        });
        let Some(menu) = latest.first() else {
            return page_list;
        };
        let blocks = menu_blocks(&menu.content);
        if blocks.is_empty() {
            return page_list;
        }
        blocks
    }

    /// `render_block_core_page_list()`.
    pub(crate) fn page_list(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/page-list", parsed);
        let site = self.context.site;
        let all_pages = self.pages();
        if all_pages.is_empty() {
            return Vec::new();
        }
        let queried = self.context.request.queried_object_id;
        let mut active_ancestors = Vec::new();
        let mut top_level = Vec::new();
        let mut children: Vec<(u64, Vec<ListedPage>)> = Vec::new();
        for page in &all_pages {
            let is_active = page.id != 0 && page.id == queried;
            if is_active {
                active_ancestors = post_ancestors(self.context.site, page);
            }
            let listed = ListedPage {
                id: page.id,
                title: page.title.clone(),
                link: self.context.permalink(page),
                is_active,
            };
            if page.parent == 0 {
                top_level.push(listed);
            } else {
                add_child(&mut children, page.parent, listed);
            }
        }
        let none = Value::Null;
        let context = self.navigation.as_ref().unwrap_or(&none);
        let colors = build_colors(context, true);
        let parent_id = attributes.get_str("parentPageID").map_or(0, Value::to_int);
        let pages = if parent_id == 0 {
            top_level
        } else {
            match children
                .iter()
                .find(|(parent, _)| i64::try_from(*parent) == Ok(parent_id))
            {
                Some((_, pages)) => pages.clone(),
                None => return Vec::new(),
            }
        };
        let showing = context.get_str("showSubmenuIcon");
        let list = PageList {
            visibility: match showing {
                Some(_) => submenu_visibility(context),
                None => b"hover".to_vec(),
            },
            show_icons: showing.is_some_and(|v| !v.is_empty()),
            in_navigation: showing.is_some(),
            children: &children,
            active_ancestors: &active_ancestors,
            overlay_classes: colors.overlay_classes.join(&b' '),
            overlay_styles: colors.overlay_styles,
            front_page: site
                .option("page_on_front")
                .map_or(0, |id| Value::String(id).to_int()),
            upload_host: self.context.upload_host(),
        };
        let items = list.render(&pages, 0);
        let extra = [
            (
                "class",
                Value::String(php_trim(&colors.classes.join(&b' ')).to_vec()),
            ),
            ("style", Value::String(colors.styles)),
        ];
        let wrapper = get_block_wrapper_attributes(
            "core/page-list",
            parsed,
            &extra,
            &self.context.theme.global,
        );
        [b"<ul ", wrapper.as_bytes(), b">", &items, b"</ul>"].concat()
    }
}

/// A page as `render_block_core_page_list()` lists it.
#[derive(Clone)]
struct ListedPage {
    id: u64,
    title: Vec<u8>,
    link: Vec<u8>,
    is_active: bool,
}

/// What `block_core_page_list_render_nested_page_list()` renders with.
struct PageList<'a> {
    visibility: Vec<u8>,
    show_icons: bool,
    in_navigation: bool,
    children: &'a [(u64, Vec<ListedPage>)],
    active_ancestors: &'a [u64],
    overlay_classes: Vec<u8>,
    overlay_styles: Vec<u8>,
    front_page: i64,
    upload_host: Vec<u8>,
}

impl PageList<'_> {
    /// `block_core_page_list_render_nested_page_list()`.
    fn render(&self, pages: &[ListedPage], depth: usize) -> Vec<u8> {
        let on_click = self.visibility == b"click";
        let mut markup = Vec::new();
        for page in pages {
            let children = self
                .children
                .iter()
                .find(|(parent, _)| *parent == page.id)
                .map(|(_, pages)| pages);
            let mut class = Vec::new();
            if page.is_active {
                class.extend_from_slice(b" current-menu-item");
            }
            if self.active_ancestors.contains(&page.id) {
                class.extend_from_slice(b" current-menu-ancestor");
            }
            if children.is_some() {
                class.extend_from_slice(b" has-child");
            }
            if self.in_navigation {
                class.extend_from_slice(b" wp-block-navigation-item");
                if on_click {
                    class.extend_from_slice(b" open-on-click");
                } else if self.visibility == b"hover" && self.show_icons {
                    class.extend_from_slice(b" open-on-hover-click");
                } else if self.visibility == b"always" {
                    class.extend_from_slice(b" open-always");
                }
            }
            let content_class: &[u8] = if self.in_navigation {
                b" wp-block-navigation-item__content"
            } else {
                b""
            };
            let mut style = Vec::new();
            if depth > 0 {
                class.push(b' ');
                class.extend_from_slice(php_trim(&self.overlay_classes));
                if !self.overlay_styles.is_empty() {
                    style = format!(" style=\"{}\"", esc_attr(&self.overlay_styles)).into_bytes();
                }
            }
            if i64::try_from(page.id) == Ok(self.front_page) {
                class.extend_from_slice(b" menu-item-home");
            }
            let title: &[u8] = if is_empty_string(&page.title) {
                b"(no title)"
            } else {
                &page.title
            };
            let aria_label = [&wp_strip_all_tags(title, false)[..], b" submenu"].concat();
            let title = wp_kses_post(title, &self.upload_host);
            markup.extend_from_slice(b"<li class=\"wp-block-pages-list__item");
            markup.extend(esc_attr(&class).into_bytes());
            markup.push(b'"');
            markup.extend(style);
            markup.push(b'>');
            if children.is_some() && self.in_navigation && on_click {
                markup.extend(format!("<button aria-label=\"{}\" class=\"{} wp-block-navigation-submenu__toggle\" aria-expanded=\"false\">", esc_attr(&aria_label), esc_attr(content_class)).into_bytes());
                markup.extend(title);
                markup.extend_from_slice(b"</button><span class=\"wp-block-page-list__submenu-icon wp-block-navigation__submenu-icon\">");
                markup.extend_from_slice(SUBMENU_ICON);
                markup.extend_from_slice(b"</span>");
            } else {
                markup.extend(
                    format!(
                        "<a class=\"wp-block-pages-list__item__link{}\" href=\"",
                        esc_attr(content_class)
                    )
                    .into_bytes(),
                );
                markup.extend(esc_url(&page.link));
                markup.push(b'"');
                if page.is_active {
                    markup.extend_from_slice(b" aria-current=\"page\"");
                }
                markup.push(b'>');
                markup.extend(title);
                markup.extend_from_slice(b"</a>");
            }
            if let Some(children) = children {
                if self.in_navigation && self.show_icons && !on_click {
                    markup.extend(format!("<button aria-label=\"{}\" class=\"wp-block-navigation__submenu-icon wp-block-navigation-submenu__toggle\" aria-expanded=\"false\">", esc_attr(&aria_label)).into_bytes());
                    markup.extend_from_slice(SUBMENU_ICON);
                    markup.extend_from_slice(b"</button>");
                }
                markup.extend_from_slice(b"<ul class=\"wp-block-navigation__submenu-container\">");
                markup.extend(self.render(children, depth + 1));
                markup.extend_from_slice(b"</ul>");
            }
            markup.extend_from_slice(b"</li>");
        }
        markup
    }
}

/// `block_core_navigation_add_directives_to_submenu()`. PHP recurses with
/// one tag processor, so every later item is handled in document order.
fn add_directives_to_submenu(html: &[u8], visibility: &[u8]) -> Vec<u8> {
    let mut tags = TagProcessor::new(html);
    while tags.next_tag(Some(b"LI"), Some(b"has-child")) {
        tags.set_attribute(b"data-wp-interactive", b"core/navigation");
        tags.set_attribute(
            b"data-wp-context",
            br#"{ "submenuOpenedBy": { "click": false, "hover": false, "focus": false }, "type": "submenu", "modal": null, "previousFocus": null }"#,
        );
        tags.set_attribute(b"data-wp-watch", b"callbacks.initMenu");
        tags.set_attribute(b"data-wp-on--focusout", b"actions.handleMenuFocusout");
        tags.set_attribute(b"data-wp-on--keydown", b"actions.handleMenuKeydown");
        tags.set_attribute(b"tabindex", b"-1");
        if visibility == b"hover" {
            tags.set_attribute(b"data-wp-on--pointerenter", b"actions.openMenuOnHover");
            tags.set_attribute(b"data-wp-on--pointerleave", b"actions.closeMenuOnHover");
        }
        if tags.next_tag(
            Some(b"BUTTON"),
            Some(b"wp-block-navigation-submenu__toggle"),
        ) {
            tags.set_attribute(b"data-wp-on--click", b"actions.toggleMenuOnClick");
            tags.set_attribute(b"data-wp-bind--aria-expanded", b"state.isSubmenuOpen");
        }
        if tags.next_tag(Some(b"UL"), Some(b"wp-block-navigation__submenu-container")) {
            tags.set_attribute(b"data-wp-on--focus", b"actions.openMenuOnFocus");
        }
    }
    tags.get_updated_html().to_vec()
}

/// `get_page_children( 0, $pages )`: the pages that reach the top level
/// through their parents, each followed by its descendants.
fn get_page_children(pages: Vec<Post>) -> Vec<Post> {
    let mut found = Vec::with_capacity(pages.len());
    let mut to_look: Vec<&Post> = pages.iter().filter(|page| page.parent == 0).rev().collect();
    while let Some(page) = to_look.pop() {
        found.push(page.clone());
        to_look.extend(pages.iter().filter(|child| child.parent == page.id).rev());
    }
    found
}

/// `block_core_page_list_get_submenu_visibility()`.
fn submenu_visibility(context: &Value) -> Vec<u8> {
    match context
        .get_str("openSubmenusOnClick")
        .filter(|v| **v != Value::Null)
    {
        Some(on_click) if !on_click.is_empty() => b"click".to_vec(),
        Some(_) => b"hover".to_vec(),
        None => context
            .get_str("submenuVisibility")
            .filter(|v| **v != Value::Null)
            .map_or(b"hover".to_vec(), Value::to_php_string),
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

/// Color classes and inline styles, for the block and for its overlay.
#[derive(Default)]
struct Colors {
    classes: Vec<Vec<u8>>,
    styles: Vec<u8>,
    overlay_classes: Vec<Vec<u8>>,
    overlay_styles: Vec<u8>,
}

/// `block_core_navigation_build_css_colors()`, or with `page_list` its page
/// list twin, which also reads `style.color` and kebab-cases named colors.
fn build_colors(source: &Value, page_list: bool) -> Colors {
    let mut colors = Colors::default();
    let value = |name| source.get_str(name).map(Value::to_php_string);
    for (named, picked, custom, flag, property, overlay) in [
        (
            "textColor",
            "customTextColor",
            "text",
            "has-text-color",
            "color",
            false,
        ),
        (
            "backgroundColor",
            "customBackgroundColor",
            "background",
            "has-background",
            "background-color",
            false,
        ),
        (
            "overlayTextColor",
            "customOverlayTextColor",
            "",
            "has-text-color",
            "color",
            true,
        ),
        (
            "overlayBackgroundColor",
            "customOverlayBackgroundColor",
            "",
            "has-background",
            "background-color",
            true,
        ),
    ] {
        let (classes, styles) = if overlay {
            (&mut colors.overlay_classes, &mut colors.overlay_styles)
        } else {
            (&mut colors.classes, &mut colors.styles)
        };
        let custom = source
            .get_path(&["style", "color", custom])
            .filter(|v| page_list && !overlay && isset(Some(v)))
            .map(Value::to_php_string);
        let (named, picked) = (value(named), value(picked));
        if named.is_some() || picked.is_some() || custom.is_some() {
            classes.push(flag.as_bytes().to_vec());
        }
        if let Some(named) = named {
            let named = if page_list {
                kebab_case(&String::from_utf8_lossy(&named)).into_bytes()
            } else {
                named
            };
            classes.push([&b"has-"[..], &named, b"-", property.as_bytes()].concat());
        } else if let Some(color) = picked.or(custom) {
            styles.extend_from_slice(format!("{property}: ").as_bytes());
            styles.extend(color);
            styles.push(b';');
        }
    }
    colors
}

/// `block_core_home_link_build_css_colors()` of the navigation block's
/// context: its named colors, else those of `style.color`.
fn home_link_colors(context: Option<&Value>) -> (Vec<Vec<u8>>, Vec<u8>) {
    let (mut classes, mut style) = (Vec::new(), Vec::new());
    for (named, custom, flag, property) in [
        ("textColor", "text", "has-text-color", "color"),
        (
            "backgroundColor",
            "background",
            "has-background",
            "background-color",
        ),
    ] {
        let named = context
            .and_then(|c| c.get_str(named))
            .map(Value::to_php_string);
        let custom = context
            .and_then(|c| c.get_path(&["style", "color", custom]))
            .filter(|v| isset(Some(v)))
            .map(Value::to_php_string);
        if named.is_some() || custom.is_some() {
            classes.push(flag.as_bytes().to_vec());
        }
        if let Some(named) = named {
            classes.push([&b"has-"[..], &named, b"-", property.as_bytes()].concat());
        } else if let Some(color) = custom {
            style.extend([format!("{property}: ").as_bytes(), &color, b";"].concat());
        }
    }
    (classes, style)
}

/// The navigation block's colors, then its font size class or style.
fn colors(attributes: &Value) -> Colors {
    let mut colors = build_colors(attributes, false);
    let value = |name| attributes.get_str(name).map(Value::to_php_string);
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

fn string(text: &str) -> Value {
    Value::String(text.as_bytes().to_vec())
}

/// `get_post_ancestors()`: the IDs above `post`, nearest first, a missing
/// parent's included, stopping at a loop back to `post` or to one listed.
pub(crate) fn post_ancestors(site: &dyn Site, post: &Post) -> Vec<u64> {
    let mut ancestors = Vec::new();
    let mut parent = post.parent;
    while parent != 0 && parent != post.id && !ancestors.contains(&parent) {
        ancestors.push(parent);
        parent = site.post(parent).map_or(0, |post| post.parent);
    }
    ancestors
}

/// A navigation post's blocks: `block_core_navigation_filter_out_empty_blocks()`
/// of its parsed content, serialized and parsed again.
fn menu_blocks(content: &[u8]) -> Vec<Block> {
    let blocks: Vec<Block> = parse_blocks(content)
        .into_iter()
        .filter(|block| block.name.is_some())
        .collect();
    parse_blocks(&serialize_blocks(&blocks))
}

#[cfg(test)]
mod tests;
