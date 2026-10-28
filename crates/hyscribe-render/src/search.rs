// SPDX-License-Identifier: AGPL-3.0-or-later

//! The search block, `render_block_core_search()`, with its class and
//! inline style helpers.

use hyscribe_html::TagProcessor;
use hyscribe_kses::{esc_attr, esc_url, safecss_filter_attr, wp_strip_all_tags};
use hyscribe_phpser::{Key, Value, is_empty_string, json_encode};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::formatting::contains;
use crate::kses::wp_kses_post;
use crate::style::preset_value_slug;
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;

/// The magnifying glass of an icon button, as the block prints it.
const ICON: &str = "<svg class=\"search-icon\" viewBox=\"0 0 24 24\" width=\"24\" height=\"24\">
\t\t\t\t\t<path d=\"M13 5c-3.3 0-6 2.7-6 6 0 1.4.5 2.7 1.3 3.7l-3.8 3.8 1.1 1.1 3.8-3.8c1 .8 2.3 1.3 3.7 1.3 3.3 0 6-2.7 6-6S16.3 5 13 5zm0 10.5c-2.5 0-4.5-2-4.5-4.5s2-4.5 4.5-4.5 4.5 2 4.5 4.5-2 4.5-4.5 4.5z\"></path>
\t\t\t\t</svg>";

/// The inline `style` attributes of the block's parts.
#[derive(Default)]
struct Styles {
    input: Vec<String>,
    button: Vec<String>,
    wrapper: Vec<String>,
    label: Vec<String>,
}

impl Renderer<'_> {
    /// `render_block_core_search()`.
    pub(crate) fn search(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let mut attributes = prepared("core/search", parsed);
        // wp_parse_args() with the default label and button text.
        for name in ["label", "buttonText"] {
            if attributes.get_str(name).is_none()
                && let Value::Array(entries) = &mut attributes
            {
                entries.push((
                    Key::String(name.as_bytes().to_vec()),
                    Value::String(b"Search".to_vec()),
                ));
            }
        }
        let a = Attributes(&attributes);
        let input_id = self.unique_id("wp-block-search__input-");
        let classnames = classnames(&a);
        let show_label = !a.empty(&["showLabel"]);
        let use_icon_button = !a.empty(&["buttonUseIcon"]);
        let position = a.text(&["buttonPosition"]);
        let show_button = a.empty(&["buttonPosition"]) || position != b"no-button";
        let is_button_inside = position == b"button-inside";
        let styles = styles(self, &a, is_button_inside);
        let color_classes = color_classes(&a);
        let typography_classes = typography_classes(&a);
        let border_color_classes = border_color_classes(&a);
        let upload_host = self.context.upload_host();

        let label_html = if a.empty(&["label"]) {
            b"Search".to_vec()
        } else {
            wp_kses_post(&a.text(&["label"]), &upload_host)
        };
        let mut label = TagProcessor::new(
            &[
                format!("<label {}>", style_attribute(&styles.label)).as_bytes(),
                &label_html,
                b"</label>",
            ]
            .concat(),
        );
        // Each processor's markup is built here, so it always has its tag.
        label.next_tag(None, None);
        label.set_attribute(b"for", input_id.as_slice());
        label.add_class(b"wp-block-search__label");
        if show_label && !a.empty(&["label"]) {
            if !typography_classes.is_empty() {
                label.add_class(typography_classes.as_bytes());
            }
        } else {
            label.add_class(b"screen-reader-text");
        }

        let mut input = TagProcessor::new(
            format!(
                "<input type=\"search\" name=\"s\" required {}/>",
                style_attribute(&styles.input)
            )
            .as_bytes(),
        );
        let mut input_classes = vec!["wp-block-search__input"];
        if !is_button_inside && !border_color_classes.is_empty() {
            input_classes.push(&border_color_classes);
        }
        if !typography_classes.is_empty() {
            input_classes.push(&typography_classes);
        }
        if !show_button && !color_classes.is_empty() {
            input_classes.push(&color_classes);
        }
        let expandable = show_button && position == b"button-only";
        input.next_tag(None, None);
        input.add_class(input_classes.join(" ").as_bytes());
        input.set_attribute(b"id", &input_id);
        input.set_attribute(
            b"value",
            self.context.request.search.as_deref().unwrap_or_default(),
        );
        input.set_attribute(b"placeholder", &a.text(&["placeholder"]));
        if expandable {
            self.assets
                .enqueue_module("@wordpress/block-library/search/view");
            input.set_attribute(
                b"data-wp-bind--aria-hidden",
                b"!context.isSearchInputVisible",
            );
            input.set_attribute(b"data-wp-bind--tabindex", b"state.tabindex");
            input.set_attribute(b"aria-hidden", b"true");
            input.set_attribute(b"tabindex", b"-1");
        }

        let mut hidden = Vec::new();
        if let Some(Value::Array(params)) = a.get(&["query"]).filter(|v| !v.is_empty()) {
            for (param, value) in params {
                hidden.extend(
                    format!(
                        "<input type=\"hidden\" name=\"{}\" value=\"{}\" />",
                        esc_attr(&param.to_php_string()),
                        esc_attr(&value.to_php_string())
                    )
                    .into_bytes(),
                );
            }
        }

        let mut button = Vec::new();
        if show_button {
            let mut button_classes = vec!["wp-block-search__button"];
            if !color_classes.is_empty() {
                button_classes.push(&color_classes);
            }
            if !typography_classes.is_empty() {
                button_classes.push(&typography_classes);
            }
            if !is_button_inside && !border_color_classes.is_empty() {
                button_classes.push(&border_color_classes);
            }
            let inner = if use_icon_button {
                button_classes.push("has-icon");
                ICON.as_bytes().to_vec()
            } else if a.empty(&["buttonText"]) {
                Vec::new()
            } else {
                wp_kses_post(&a.text(&["buttonText"]), &upload_host)
            };
            button_classes.push("wp-element-button");
            let mut tags = TagProcessor::new(
                &[
                    format!(
                        "<button type=\"submit\" {}>",
                        style_attribute(&styles.button)
                    )
                    .as_bytes(),
                    &inner,
                    b"</button>",
                ]
                .concat(),
            );
            tags.next_tag(None, None);
            tags.add_class(button_classes.join(" ").as_bytes());
            if expandable {
                tags.set_attribute(b"data-wp-bind--aria-label", b"state.ariaLabel");
                tags.set_attribute(b"data-wp-bind--aria-controls", b"state.ariaControls");
                tags.set_attribute(
                    b"data-wp-bind--aria-expanded",
                    b"context.isSearchInputVisible",
                );
                tags.set_attribute(b"data-wp-bind--type", b"state.type");
                tags.set_attribute(b"data-wp-on--click", b"actions.openSearchInput");
                tags.set_attribute(b"aria-label", b"Expand search field");
                // The prefix is there twice, as in PHP.
                tags.set_attribute(
                    b"aria-controls",
                    &[&b"wp-block-search__input-"[..], &input_id].concat(),
                );
                tags.set_attribute(b"aria-expanded", b"false");
                tags.set_attribute(b"type", b"button");
            } else {
                tags.set_attribute(
                    b"aria-label",
                    &wp_strip_all_tags(&a.text(&["buttonText"]), false),
                );
            }

            button = tags.get_updated_html().to_vec();
        }

        let mut field_classes = vec!["wp-block-search__inside-wrapper"];
        if is_button_inside && !border_color_classes.is_empty() {
            field_classes.push(&border_color_classes);
        }
        let field = [
            format!(
                "<div class=\"{}\" {}>",
                esc_attr(field_classes.join(" ").as_bytes()),
                style_attribute(&styles.wrapper)
            )
            .as_bytes(),
            input.get_updated_html(),
            &hidden,
            &button,
            b"</div>",
        ]
        .concat();
        let extra = [("class", Value::String(classnames.into_bytes()))];
        let wrapper =
            get_block_wrapper_attributes("core/search", parsed, &extra, &self.context.theme.global);
        let directives = if expandable {
            let context = Value::Array(vec![
                (
                    Key::String(b"isSearchInputVisible".to_vec()),
                    Value::Bool(false),
                ),
                (
                    Key::String(b"inputId".to_vec()),
                    Value::String(input_id.clone()),
                ),
                (
                    Key::String(b"ariaLabelExpanded".to_vec()),
                    Value::String(b"Submit Search".to_vec()),
                ),
                (
                    Key::String(b"ariaLabelCollapsed".to_vec()),
                    Value::String(b"Expand search field".to_vec()),
                ),
            ]);
            format!(
                "\n\t\t data-wp-interactive=\"core/search\"\n\t\t data-wp-context='{}'\n\t\t data-wp-class--wp-block-search__searchfield-hidden=\"!context.isSearchInputVisible\"\n\t\t data-wp-on--keydown=\"actions.handleSearchKeydown\"\n\t\t data-wp-on--focusout=\"actions.handleSearchFocusout\"\n\t\t",
                String::from_utf8_lossy(&json_encode(&context).unwrap_or_default())
            )
        } else {
            String::new()
        };
        let action = String::from_utf8_lossy(&esc_url(
            &[&self.context.site.home_url()[..], b"/"].concat(),
        ))
        .into_owned();
        // Block themes have no `html5` support for `search-element`.
        let (open, close) = if a.text(&["tagName"]) == b"search" {
            (
                format!("<search {wrapper} {directives}><form method=\"get\" action=\"{action}\">"),
                "</form></search>",
            )
        } else {
            (
                format!(
                    "<form role=\"search\" method=\"get\" action=\"{action}\" {wrapper} {directives}>"
                ),
                "</form>",
            )
        };
        [
            open.as_bytes(),
            label.get_updated_html(),
            &field,
            close.as_bytes(),
        ]
        .concat()
    }
}

/// The block's attributes, read the way the PHP reads them.
struct Attributes<'a>(&'a Value);

impl Attributes<'_> {
    fn get(&self, path: &[&str]) -> Option<&Value> {
        path.iter()
            .try_fold(self.0, |value, key| value.get_str(key))
    }

    /// `empty( $attributes[...] )`.
    fn empty(&self, path: &[&str]) -> bool {
        self.get(path).is_none_or(Value::is_empty)
    }

    fn text(&self, path: &[&str]) -> Vec<u8> {
        self.get(path).map(Value::to_php_string).unwrap_or_default()
    }
}

/// `classnames_for_block_core_search()`.
fn classnames(a: &Attributes<'_>) -> String {
    let mut classes = Vec::new();
    let position = a.text(&["buttonPosition"]);
    if !a.empty(&["buttonPosition"]) {
        match &position[..] {
            b"button-inside" => classes.push("wp-block-search__button-inside"),
            b"button-outside" => classes.push("wp-block-search__button-outside"),
            b"no-button" => classes.push("wp-block-search__no-button"),
            b"button-only" => {
                classes.push("wp-block-search__button-only wp-block-search__searchfield-hidden")
            }
            _ => {}
        }
        if a.get(&["buttonUseIcon"]).is_some_and(|v| *v != Value::Null) && position != b"no-button"
        {
            classes.push(if a.empty(&["buttonUseIcon"]) {
                "wp-block-search__text-button"
            } else {
                "wp-block-search__icon-button"
            });
        }
    }
    classes.join(" ")
}

/// `styles_for_block_core_search()`: each part's declarations.
fn styles(renderer: &Renderer<'_>, a: &Attributes<'_>, is_button_inside: bool) -> Styles {
    let mut styles = Styles::default();
    let show_label = a
        .get(&["showLabel"])
        .is_some_and(|v| *v != Value::Null && *v != Value::Bool(false));
    if !a.empty(&["width"]) && !a.empty(&["widthUnit"]) {
        // sprintf( 'width: %d%s;' ) of the escaped width.
        let width = Value::String(esc_attr(&a.text(&["width"])).into_bytes()).to_int();
        styles.wrapper.push(format!(
            "width: {width}{};",
            esc_attr(&a.text(&["widthUnit"]))
        ));
    }
    for property in ["width", "color", "style"] {
        for side in [
            None,
            Some("top"),
            Some("right"),
            Some("bottom"),
            Some("left"),
        ] {
            border_style(a, property, side, is_button_inside, &mut styles);
        }
    }
    if !a.empty(&["style", "border", "radius"]) {
        border_radius(a, is_button_inside, &mut styles);
    }
    let use_input = a.text(&["buttonPosition"]) == b"no-button";
    for (key, property) in [
        ("text", "color"),
        ("background", "background-color"),
        ("gradient", "background"),
    ] {
        if !a.empty(&["style", "color", key]) {
            let style = format!(
                "{property}: {};",
                String::from_utf8_lossy(&a.text(&["style", "color", key]))
            );
            if use_input {
                styles.input.push(style);
            } else {
                styles.button.push(style);
            }
        }
    }
    let typography = esc_attr(typography_styles(renderer, a).as_bytes());
    if !is_empty_string(typography.as_bytes()) {
        styles.label.push(typography.clone());
        styles.button.push(typography.clone());
        styles.input.push(typography);
    }
    if !a.empty(&["style", "typography", "textDecoration"]) {
        let decoration = format!(
            "text-decoration: {};",
            esc_attr(&a.text(&["style", "typography", "textDecoration"]))
        );
        styles.button.push(decoration.clone());
        if show_label {
            styles.label.push(decoration);
        }
    }
    styles
}

/// A part's ` style="..."` attribute, or empty.
fn style_attribute(parts: &[String]) -> String {
    if parts.is_empty() {
        String::new()
    } else {
        format!(
            " style=\"{}\"",
            esc_attr(&safecss_filter_attr(parts.join(" ").as_bytes()))
        )
    }
}

/// `apply_block_core_search_border_style()`.
fn border_style(
    a: &Attributes<'_>,
    property: &str,
    side: Option<&str>,
    is_button_inside: bool,
    styles: &mut Styles,
) {
    let path: Vec<&str> = match side {
        Some(side) => vec!["style", "border", side, property],
        None => vec!["style", "border", property],
    };
    if a.empty(&path) {
        return;
    }
    let mut value = a.text(&path);
    if property == "color" && side.is_some() && contains(&value, b"var:preset|color|") {
        let name = value.rsplit(|&b| b == b'|').next().unwrap_or_default();
        value = format!(
            "var(--wp--preset--color--{})",
            String::from_utf8_lossy(name)
        )
        .into_bytes();
    }
    let suffix = side.map_or(property.to_owned(), |side| format!("{side}-{property}"));
    let style = format!("border-{suffix}: {};", esc_attr(&value));
    if is_button_inside {
        styles.wrapper.push(style);
    } else {
        styles.button.push(style.clone());
        styles.input.push(style);
    }
}

/// The border radius part of `styles_for_block_core_search()`, with the
/// wrapper's radius 4px wider around an inside button.
fn border_radius(a: &Attributes<'_>, is_button_inside: bool, styles: &mut Styles) {
    // A preset as its CSS variable, the slug kebab-cased.
    let preset = |value: &Value| {
        preset_value_slug(value, "border-radius")
            .map(|slug| format!("var(--wp--preset--border-radius--{slug})").into_bytes())
    };
    let radius = a
        .get(&["style", "border", "radius"])
        .cloned()
        .unwrap_or_default();
    if let Value::Array(corners) = radius {
        for (key, value) in corners {
            if value == Value::Null {
                continue;
            }
            let text = preset(&value).unwrap_or_else(|| value.to_php_string());
            let key = String::from_utf8_lossy(&key.to_php_string()).into_owned();
            // preg_replace( '/(?<!^)[A-Z]/', '-$0', $key ), lowercased.
            let name: String = key
                .chars()
                .enumerate()
                .flat_map(|(i, c)| {
                    let dash = (i > 0 && c.is_ascii_uppercase()).then_some('-');
                    dash.into_iter()
                        .chain(std::iter::once(c.to_ascii_lowercase()))
                })
                .collect();
            let style = format!(
                "border-{}-radius: {};",
                esc_attr(name.as_bytes()),
                esc_attr(&text)
            );
            styles.input.push(style.clone());
            styles.button.push(style);
            let rounded = Value::String(text.clone()).to_int() != 0
                || contains(&text, b"var(--wp--preset--border-radius--");
            if is_button_inside && rounded {
                styles.wrapper.push(format!(
                    "border-{}-radius: calc({} + 4px);",
                    esc_attr(name.as_bytes()),
                    esc_attr(&text)
                ));
            }
        }
    } else {
        let text = preset(&radius).unwrap_or_else(|| {
            let mut text = radius.to_php_string();
            if radius.is_numeric() {
                text.extend_from_slice(b"px");
            }
            text
        });
        let style = format!("border-radius: {};", esc_attr(&text));
        styles.input.push(style.clone());
        styles.button.push(style);
        if is_button_inside && Value::String(text.clone()).to_int() != 0 {
            styles
                .wrapper
                .push(format!("border-radius: calc({} + 4px);", esc_attr(&text)));
        }
    }
}

/// `get_typography_classes_for_block_core_search()`.
fn typography_classes(a: &Attributes<'_>) -> String {
    let mut classes = Vec::new();
    if !a.empty(&["fontSize"]) {
        classes.push(format!(
            "has-{}-font-size",
            esc_attr(&a.text(&["fontSize"]))
        ));
    }
    if !a.empty(&["fontFamily"]) {
        classes.push(format!(
            "has-{}-font-family",
            esc_attr(&a.text(&["fontFamily"]))
        ));
    }
    classes.join(" ")
}

/// `get_typography_styles_for_block_core_search()`.
fn typography_styles(renderer: &Renderer<'_>, a: &Attributes<'_>) -> String {
    let mut styles = String::new();
    if let Some(size) = a
        .get(&["style", "typography", "fontSize"])
        .filter(|v| !v.is_empty())
    {
        let preset = Value::Array(vec![(Key::String(b"size".to_vec()), size.clone())]);
        let value = renderer
            .context
            .theme
            .global
            .font_size_value(&preset)
            .map(|v| v.to_php_string())
            .unwrap_or_default();
        styles.push_str(&format!("font-size: {};", String::from_utf8_lossy(&value)));
    }
    for (key, property) in [
        ("fontFamily", "font-family"),
        ("letterSpacing", "letter-spacing"),
        ("fontWeight", "font-weight"),
        ("fontStyle", "font-style"),
        ("lineHeight", "line-height"),
        ("textTransform", "text-transform"),
    ] {
        if !a.empty(&["style", "typography", key]) {
            styles.push_str(&format!(
                "{property}: {};",
                String::from_utf8_lossy(&a.text(&["style", "typography", key]))
            ));
        }
    }
    styles
}

/// `get_border_color_classes_for_block_core_search()`.
fn border_color_classes(a: &Attributes<'_>) -> String {
    let mut classes = Vec::new();
    if !a.empty(&["style", "border", "color"]) || !a.empty(&["borderColor"]) {
        classes.push("has-border-color".to_owned());
    }
    if !a.empty(&["borderColor"]) {
        classes.push(format!(
            "has-{}-border-color",
            esc_attr(&a.text(&["borderColor"]))
        ));
    }
    classes.join(" ")
}

/// `get_color_classes_for_block_core_search()`.
fn color_classes(a: &Attributes<'_>) -> String {
    let text = |path: &[&str]| String::from_utf8_lossy(&a.text(path)).into_owned();
    let mut classes = Vec::new();
    if !a.empty(&["textColor"]) {
        classes.push(format!("has-text-color has-{}-color", text(&["textColor"])));
    } else if !a.empty(&["style", "color", "text"]) {
        classes.push("has-text-color".to_owned());
    }
    let background = ["backgroundColor", "gradient"]
        .iter()
        .any(|name| !a.empty(&[name]))
        || ["background", "gradient"]
            .iter()
            .any(|key| !a.empty(&["style", "color", key]));
    if background {
        classes.push("has-background".to_owned());
    }
    if !a.empty(&["backgroundColor"]) {
        classes.push(format!(
            "has-{}-background-color",
            text(&["backgroundColor"])
        ));
    }
    if !a.empty(&["gradient"]) {
        classes.push(format!("has-{}-gradient-background", text(&["gradient"])));
    }
    classes.join(" ")
}

#[cfg(test)]
mod tests;
