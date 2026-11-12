// SPDX-License-Identifier: AGPL-3.0-or-later

//! The in template and table insertion modes, which bail on foster parenting.
//! Clearing back to a table context never pops here, so it is left out.

use std::rc::Rc;

use super::{HtmlProcessor, Mode, Step, Token};
use crate::tag_processor::TextClass;

impl HtmlProcessor {
    /// The token as `+NAME` or `-NAME` from `get_tag()`: `+` alone for a
    /// token that is no tag, as the modes that read it this way write it.
    fn tag_op(&self) -> String {
        let sigil = if self.tags.is_tag_closer() { "-" } else { "+" };
        format!("{sigil}{}", self.get_tag().unwrap_or_default())
    }

    /// `step_in_template()`.
    pub(super) fn step_in_template(&mut self) -> Step {
        let op = self.op();
        let switch_to = match op.as_str() {
            "#text"
            | "#comment"
            | "#funky-comment"
            | "#presumptuous-tag"
            | "#processing-instruction"
            | "html" => return self.step_in_body(),
            "+BASE" | "+BASEFONT" | "+BGSOUND" | "+LINK" | "+META" | "+NOFRAMES" | "+SCRIPT"
            | "+STYLE" | "+TEMPLATE" | "+TITLE" | "-TEMPLATE" => return self.step_in_head(),
            "+CAPTION" | "+COLGROUP" | "+TBODY" | "+TFOOT" | "+THEAD" => Mode::Table,
            "+COL" => Mode::ColumnGroup,
            "+TR" => Mode::TableBody,
            "+TD" | "+TH" => Mode::Row,
            _ if self.tags.is_tag_closer() => return self.next(),
            _ => Mode::Body,
        };
        self.template_modes.pop();
        self.template_modes.push(switch_to);
        self.mode = switch_to;
        self.reprocess()
    }

    /// `step_in_table()`.
    pub(super) fn step_in_table(&mut self) -> Step {
        let op = self.op();
        match op.as_str() {
            // The current node is always a table, section, row or template
            // here, so text goes to the in table text rules.
            "#text" => match self.tags.text_class() {
                TextClass::NullSequence => return self.next(),
                TextClass::Whitespace => {
                    self.insert_html_element(self.current());
                    return Ok(true);
                }
                _ => {}
            },
            "#comment" | "#funky-comment" | "#presumptuous-tag" | "#processing-instruction" => {
                self.insert_html_element(self.current());
                return Ok(true);
            }
            "html" => return self.next(),
            "+CAPTION" => {
                self.formatting.insert_marker();
                self.insert_html_element(self.current());
                self.mode = Mode::Caption;
                return Ok(true);
            }
            "+COLGROUP" => {
                self.insert_html_element(self.current());
                self.mode = Mode::ColumnGroup;
                return Ok(true);
            }
            "+COL" => {
                self.insert_virtual_node("COLGROUP")?;
                self.mode = Mode::ColumnGroup;
                return self.reprocess();
            }
            "+TBODY" | "+TFOOT" | "+THEAD" => {
                self.insert_html_element(self.current());
                self.mode = Mode::TableBody;
                return Ok(true);
            }
            "+TD" | "+TH" | "+TR" => {
                self.insert_virtual_node("TBODY")?;
                self.mode = Mode::TableBody;
                return self.reprocess();
            }
            "+TABLE" | "-TABLE" => {
                if !self.open.has_element_in_table_scope("TABLE") {
                    return self.next();
                }
                self.pop_until("TABLE");
                self.reset_insertion_mode_appropriately();
                return if op == "+TABLE" {
                    self.reprocess()
                } else {
                    Ok(true)
                };
            }
            "-BODY" | "-CAPTION" | "-COL" | "-COLGROUP" | "-HTML" | "-TBODY" | "-TD" | "-TFOOT"
            | "-TH" | "-THEAD" | "-TR" => return self.next(),
            "+STYLE" | "+SCRIPT" | "+TEMPLATE" | "-TEMPLATE" => return self.step_in_head(),
            "+INPUT" => {
                let hidden = matches!(
                    self.tags.get_attribute(b"type"),
                    Some(crate::Attribute::Value(t)) if t.eq_ignore_ascii_case(b"hidden")
                );
                if hidden {
                    self.insert_html_element(self.current());
                    return Ok(true);
                }
            }
            "+FORM" => {
                if self.open.has_element_in_scope("TEMPLATE") || self.form_element.is_some() {
                    return self.next();
                }
                self.insert_html_element(self.current());
                self.form_element = Some(self.current());
                self.pop();
                return Ok(true);
            }
            _ => {}
        }
        self.bail("Foster parenting is not supported.")
    }

    /// `step_in_caption()`.
    pub(super) fn step_in_caption(&mut self) -> Step {
        let op = self.tag_op();
        match op.as_str() {
            "-CAPTION" | "+CAPTION" | "+COL" | "+COLGROUP" | "+TBODY" | "+TD" | "+TFOOT"
            | "+TH" | "+THEAD" | "+TR" | "-TABLE" => {
                // Only a template could hide the caption, and closing one
                // resets the mode, so the caption is in table scope.
                self.generate_implied_end_tags(None);
                self.pop_until("CAPTION");
                self.formatting.clear_up_to_last_marker();
                self.mode = Mode::Table;
                if op == "-CAPTION" {
                    Ok(true)
                } else {
                    self.reprocess()
                }
            }
            _ => self.step_in_body(),
        }
    }

    /// `step_in_column_group()`.
    pub(super) fn step_in_column_group(&mut self) -> Step {
        match self.op().as_str() {
            "#text" if self.tags.text_class() == TextClass::Whitespace => {
                self.insert_html_element(self.current());
                return Ok(true);
            }
            "#comment" | "#funky-comment" | "#presumptuous-tag" | "#processing-instruction" => {
                self.insert_html_element(self.current());
                return Ok(true);
            }
            "html" | "-COL" => return self.next(),
            "+HTML" => return self.step_in_body(),
            "+COL" => {
                self.insert_html_element(self.current());
                self.pop();
                return Ok(true);
            }
            "-COLGROUP" => {
                if !self.open.current_node_is("COLGROUP") {
                    return self.next();
                }
                self.pop();
                self.mode = Mode::Table;
                return Ok(true);
            }
            "+TEMPLATE" | "-TEMPLATE" => return self.step_in_head(),
            _ => {}
        }
        if !self.open.current_node_is("COLGROUP") {
            return self.next();
        }
        self.pop();
        self.mode = Mode::Table;
        self.reprocess()
    }

    /// `step_in_table_body()`.
    pub(super) fn step_in_table_body(&mut self) -> Step {
        let op = self.tag_op();
        let name = self.get_tag().unwrap_or_default();
        match op.as_str() {
            "+TR" => {
                self.insert_html_element(self.current());
                self.mode = Mode::Row;
                Ok(true)
            }
            "+TH" | "+TD" => {
                self.insert_virtual_node("TR")?;
                self.mode = Mode::Row;
                self.reprocess()
            }
            "-TBODY" | "-TFOOT" | "-THEAD" => {
                if !self.open.has_element_in_table_scope(&name) {
                    return self.next();
                }
                self.pop();
                self.mode = Mode::Table;
                Ok(true)
            }
            "+CAPTION" | "+COL" | "+COLGROUP" | "+TBODY" | "+TFOOT" | "+THEAD" | "-TABLE" => {
                let open_section = ["TBODY", "THEAD", "TFOOT"]
                    .iter()
                    .any(|section| self.open.has_element_in_table_scope(section));
                if !open_section {
                    return self.next();
                }
                self.pop();
                self.mode = Mode::Table;
                self.reprocess()
            }
            _ => self.step_in_table(),
        }
    }

    /// `step_in_row()`.
    pub(super) fn step_in_row(&mut self) -> Step {
        let op = self.tag_op();
        let name = self.get_tag().unwrap_or_default();
        match op.as_str() {
            "+TH" | "+TD" => {
                self.insert_html_element(self.current());
                self.mode = Mode::Cell;
                self.formatting.insert_marker();
                Ok(true)
            }
            "-TR" | "+CAPTION" | "+COL" | "+COLGROUP" | "+TBODY" | "+TFOOT" | "+THEAD" | "+TR"
            | "-TABLE" | "-TBODY" | "-TFOOT" | "-THEAD" => {
                let section = matches!(op.as_str(), "-TBODY" | "-TFOOT" | "-THEAD");
                if section && !self.open.has_element_in_table_scope(&name) {
                    return self.next();
                }
                if !self.open.has_element_in_table_scope("TR") {
                    return self.next();
                }
                self.pop();
                self.mode = Mode::TableBody;
                if op == "-TR" {
                    Ok(true)
                } else {
                    self.reprocess()
                }
            }
            _ => self.step_in_table(),
        }
    }

    /// `step_in_cell()`.
    pub(super) fn step_in_cell(&mut self) -> Step {
        let op = self.tag_op();
        let name = self.get_tag().unwrap_or_default();
        match op.as_str() {
            "-TD" | "-TH" => {
                if !self.open.has_element_in_table_scope(&name) {
                    return self.next();
                }
                self.generate_implied_end_tags(None);
                self.pop_until(&name);
                self.formatting.clear_up_to_last_marker();
                self.mode = Mode::Row;
                Ok(true)
            }
            "+CAPTION" | "+COL" | "+COLGROUP" | "+TBODY" | "+TD" | "+TFOOT" | "+TH" | "+THEAD"
            | "+TR" => {
                self.close_cell();
                self.reprocess()
            }
            "-TABLE" | "-TBODY" | "-TFOOT" | "-THEAD" | "-TR" => {
                if !self.open.has_element_in_table_scope(&name) {
                    return self.next();
                }
                self.close_cell();
                self.reprocess()
            }
            _ => self.step_in_body(),
        }
    }

    /// `close_cell()`.
    fn close_cell(&mut self) {
        self.generate_implied_end_tags(None);
        while let Some(element) = self.open.current_node() {
            self.pop();
            if matches!(element.node_name.as_str(), "TD" | "TH") {
                break;
            }
        }
        self.formatting.clear_up_to_last_marker();
        self.mode = Mode::Row;
    }

    /// `insert_virtual_node()`: an element HTML implies. Its bookmark only
    /// counts against the limit, so where it points is never read.
    fn insert_virtual_node(&mut self, name: &str) -> Result<(), super::Bail> {
        let bookmark = self.bookmark_token()?;
        self.tags.put_bookmark(&bookmark, 0, 0);
        self.insert_html_element(Rc::new(Token::new(Some(bookmark), name, false)));
        Ok(())
    }
}
