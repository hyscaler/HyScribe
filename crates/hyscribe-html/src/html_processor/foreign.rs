// SPDX-License-Identifier: AGPL-3.0-or-later

//! The rules for parsing tokens in foreign content: SVG and MathML, and
//! their integration points back into HTML.

use std::rc::Rc;

use super::{HtmlProcessor, Mode, Step, Token};
use crate::tag_processor::{Attribute, Namespace};

impl HtmlProcessor {
    /// `step_in_foreign_content()`.
    pub(super) fn step_in_foreign_content(&mut self) -> Step {
        let mut op = self.op();
        let font_with_attributes = op == "+FONT"
            && [&b"color"[..], b"face", b"size"]
                .iter()
                .any(|name| self.tags.get_attribute(name).is_some());
        if font_with_attributes {
            op = "+FONT with attributes".into();
        }
        match op.as_str() {
            "html" => return self.next(),
            "+B"
            | "+BIG"
            | "+BLOCKQUOTE"
            | "+BODY"
            | "+BR"
            | "+CENTER"
            | "+CODE"
            | "+DD"
            | "+DIV"
            | "+DL"
            | "+DT"
            | "+EM"
            | "+EMBED"
            | "+H1"
            | "+H2"
            | "+H3"
            | "+H4"
            | "+H5"
            | "+H6"
            | "+HEAD"
            | "+HR"
            | "+I"
            | "+IMG"
            | "+LI"
            | "+LISTING"
            | "+MENU"
            | "+META"
            | "+NOBR"
            | "+OL"
            | "+P"
            | "+PRE"
            | "+RUBY"
            | "+S"
            | "+SMALL"
            | "+SPAN"
            | "+STRONG"
            | "+STRIKE"
            | "+SUB"
            | "+SUP"
            | "+TABLE"
            | "+TT"
            | "+U"
            | "+UL"
            | "+VAR"
            | "+FONT with attributes"
            | "-BR"
            | "-P" => {
                // Out of foreign content, to the nearest integration point or
                // HTML element.
                while let Some(node) = self.open.current_node() {
                    if node.integration.get().is_some() || node.namespace.get() == Namespace::Html {
                        break;
                    }
                    self.pop();
                }
                return self.step_in_current_mode();
            }
            _ => {}
        }
        if !self.tags.is_tag_closer() {
            let token = self.current();
            self.insert_foreign_element(Rc::clone(&token));
            if token.self_closing {
                self.pop();
            }
            return Ok(true);
        }
        // PHP's case for an SVG `</script>` reads the closer's own namespace,
        // which is always HTML, so it never applies.
        self.foreign_end_tag(&self.current().node_name)
    }

    /// Any other end tag in foreign content: closes the nearest element of its
    /// name, any case, unless an HTML element is nearer.
    fn foreign_end_tag(&mut self, name: &str) -> Step {
        // The current node is foreign, so an HTML element is never first.
        let found = self
            .open
            .walk_up()
            .find(|node| {
                node.namespace.get() == Namespace::Html || node.node_name.eq_ignore_ascii_case(name)
            })
            .cloned();
        match found {
            Some(node) if node.namespace.get() != Namespace::Html => {
                self.pop_through(&node);
                Ok(true)
            }
            _ => self.step_in_current_mode(),
        }
    }

    /// The current insertion mode's rules.
    pub(super) fn step_in_current_mode(&mut self) -> Step {
        match self.mode {
            Mode::Body => self.step_in_body(),
            Mode::Template => self.step_in_template(),
            Mode::Table => self.step_in_table(),
            Mode::Caption => self.step_in_caption(),
            Mode::ColumnGroup => self.step_in_column_group(),
            Mode::TableBody => self.step_in_table_body(),
            Mode::Row => self.step_in_row(),
            Mode::Cell => self.step_in_cell(),
        }
    }

    /// `insert_foreign_element()`: in the adjusted current node's
    /// namespace, which is SVG or MathML here, marking integration points.
    fn insert_foreign_element(&mut self, token: Rc<Token>) {
        let namespace = self.adjusted_current_node().namespace.get();
        token.namespace.set(namespace);
        let name = token.node_name.as_str();
        let integration = if namespace == Namespace::Svg {
            matches!(name, "DESC" | "FOREIGNOBJECT" | "TITLE").then_some(Namespace::Html)
        } else if matches!(name, "MI" | "MO" | "MN" | "MS" | "MTEXT") {
            Some(Namespace::Math)
        } else {
            let holds_html = name == "ANNOTATION-XML"
                && matches!(
                    self.tags.get_attribute(b"encoding"),
                    Some(Attribute::Value(encoding))
                        if encoding.eq_ignore_ascii_case(b"application/xhtml+xml")
                            || encoding.eq_ignore_ascii_case(b"text/html")
                );
            holds_html.then_some(Namespace::Html)
        };
        token.integration.set(integration);
        self.insert_html_element(token);
    }
}
