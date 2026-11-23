// SPDX-License-Identifier: AGPL-3.0-or-later

//! The in head and in body insertion modes, and the algorithms they share.

use std::rc::Rc;

use super::stacks::{ANY_HEADING, is_heading};
use super::{HtmlProcessor, Mode, Step};
use crate::tag_processor::{Namespace, TextClass};

/// The elements `generate_implied_end_tags()` closes.
const IMPLIED_END_TAGS: [&str; 10] = [
    "DD", "DT", "LI", "OPTGROUP", "OPTION", "P", "RB", "RP", "RT", "RTC",
];

impl HtmlProcessor {
    /// The token as `+NAME` for an opener, `-NAME` for a closer, or its
    /// name for any other token.
    pub(super) fn op(&self) -> String {
        let name = self.token_name();
        match self.tags.get_token_type().as_deref() {
            Some(b"#tag") if self.tags.is_tag_closer() => format!("-{name}"),
            Some(b"#tag") => format!("+{name}"),
            _ => name,
        }
    }

    /// `step_in_head()` for the tags the other modes hand it: a fragment
    /// never has a head of its own.
    pub(super) fn step_in_head(&mut self) -> Step {
        match self.op().as_str() {
            "+TEMPLATE" => {
                self.formatting.insert_marker();
                self.mode = Mode::Template;
                self.template_modes.push(Mode::Template);
                self.insert_html_element(self.current());
                Ok(true)
            }
            "-TEMPLATE" => {
                if !self.open.contains("TEMPLATE") {
                    return self.next();
                }
                // Popping through the template closes the elements that
                // generating implied end tags thoroughly would close first.
                self.pop_until("TEMPLATE");
                self.formatting.clear_up_to_last_marker();
                self.template_modes.pop();
                self.reset_insertion_mode_appropriately();
                Ok(true)
            }
            // `base`, `link`, `meta`, `script`, `style`, `title` and the
            // like; in a fragment the encoding is settled.
            _ => {
                self.insert_html_element(self.current());
                Ok(true)
            }
        }
    }

    /// `step_in_body()`.
    pub(super) fn step_in_body(&mut self) -> Step {
        let op = self.op();
        let name = self.get_tag().unwrap_or_default();
        match op.as_str() {
            "#text" => {
                if self.tags.text_class() == TextClass::NullSequence {
                    return self.next();
                }
                self.reconstruct_active_formatting_elements()?;
                self.insert_html_element(self.current());
                Ok(true)
            }
            "#comment" | "#funky-comment" | "#presumptuous-tag" | "#processing-instruction" => {
                self.insert_html_element(self.current());
                Ok(true)
            }
            "+BASE" | "+BASEFONT" | "+BGSOUND" | "+LINK" | "+META" | "+NOFRAMES" | "+SCRIPT"
            | "+STYLE" | "+TEMPLATE" | "+TITLE" | "-TEMPLATE" => self.step_in_head(),
            // The `<body>` context is no element on the stack, so `<body>`,
            // `<frameset>`, `</body>` and `</html>` are ignored with the rest.
            "html" | "+HTML" | "+BODY" | "+FRAMESET" | "-BODY" | "-HTML" | "+CAPTION" | "+COL"
            | "+COLGROUP" | "+FRAME" | "+HEAD" | "+TBODY" | "+TD" | "+TFOOT" | "+TH" | "+THEAD"
            | "+TR" => self.next(),
            "+ADDRESS" | "+ARTICLE" | "+ASIDE" | "+BLOCKQUOTE" | "+CENTER" | "+DETAILS"
            | "+DIALOG" | "+DIR" | "+DIV" | "+DL" | "+FIELDSET" | "+FIGCAPTION" | "+FIGURE"
            | "+FOOTER" | "+HEADER" | "+HGROUP" | "+MAIN" | "+MENU" | "+NAV" | "+OL" | "+P"
            | "+SEARCH" | "+SECTION" | "+SUMMARY" | "+UL" | "+PRE" | "+LISTING" => {
                self.close_p_in_button_scope();
                self.insert_html_element(self.current());
                Ok(true)
            }
            "+H1" | "+H2" | "+H3" | "+H4" | "+H5" | "+H6" => {
                self.close_p_in_button_scope();
                if self
                    .open
                    .current_node()
                    .is_some_and(|node| is_heading(&node.node_name))
                {
                    self.pop();
                }
                self.insert_html_element(self.current());
                Ok(true)
            }
            "+FORM" => {
                let in_template = self.open.contains("TEMPLATE");
                if self.form_element.is_some() && !in_template {
                    return self.next();
                }
                self.close_p_in_button_scope();
                self.insert_html_element(self.current());
                if !in_template {
                    self.form_element = Some(self.current());
                }
                Ok(true)
            }
            "+DD" | "+DT" | "+LI" => {
                self.close_list_item(&name);
                self.close_p_in_button_scope();
                self.insert_html_element(self.current());
                Ok(true)
            }
            "+PLAINTEXT" => {
                self.close_p_in_button_scope();
                self.bail("Cannot process PLAINTEXT elements.")
            }
            "+BUTTON" => {
                if self.open.has_element_in_scope("BUTTON") {
                    self.generate_implied_end_tags(None);
                    self.pop_until("BUTTON");
                }
                self.reconstruct_active_formatting_elements()?;
                self.insert_html_element(self.current());
                Ok(true)
            }
            "-ADDRESS" | "-ARTICLE" | "-ASIDE" | "-BLOCKQUOTE" | "-BUTTON" | "-CENTER"
            | "-DETAILS" | "-DIALOG" | "-DIR" | "-DIV" | "-DL" | "-FIELDSET" | "-FIGCAPTION"
            | "-FIGURE" | "-FOOTER" | "-HEADER" | "-HGROUP" | "-LISTING" | "-MAIN" | "-MENU"
            | "-NAV" | "-OL" | "-PRE" | "-SEARCH" | "-SECTION" | "-SELECT" | "-SUMMARY" | "-UL" => {
                if !self.open.has_element_in_scope(&name) {
                    return self.next();
                }
                self.generate_implied_end_tags(None);
                self.pop_until(&name);
                Ok(true)
            }
            "-FORM" => self.close_form(),
            "-P" => {
                if !self.open.has_p_in_button_scope() {
                    self.insert_html_element(self.current());
                }
                self.close_a_p_element();
                Ok(true)
            }
            "-DD" | "-DT" | "-LI" => {
                let in_scope = if name == "LI" {
                    self.open.has_element_in_list_item_scope("LI")
                } else {
                    self.open.has_element_in_scope(&name)
                };
                if !in_scope {
                    return self.next();
                }
                self.generate_implied_end_tags(Some(&name));
                self.pop_until(&name);
                Ok(true)
            }
            "-H1" | "-H2" | "-H3" | "-H4" | "-H5" | "-H6" => {
                if !self.open.has_element_in_scope(ANY_HEADING) {
                    return self.next();
                }
                self.generate_implied_end_tags(None);
                self.pop_until(ANY_HEADING);
                Ok(true)
            }
            "+A" => {
                let open_a = self
                    .formatting
                    .walk_up()
                    .take_while(|item| item.node_name != "marker")
                    .find(|item| item.node_name == "A")
                    .cloned();
                if let Some(item) = open_a {
                    self.run_adoption_agency_algorithm()?;
                    self.formatting.remove_node(&item);
                    self.remove_node(&item);
                }
                self.insert_formatting_element()
            }
            "+B" | "+BIG" | "+CODE" | "+EM" | "+FONT" | "+I" | "+S" | "+SMALL" | "+STRIKE"
            | "+STRONG" | "+TT" | "+U" => self.insert_formatting_element(),
            "+NOBR" => {
                self.reconstruct_active_formatting_elements()?;
                if self.open.has_element_in_scope("NOBR") {
                    self.run_adoption_agency_algorithm()?;
                }
                self.insert_formatting_element()
            }
            "-A" | "-B" | "-BIG" | "-CODE" | "-EM" | "-FONT" | "-I" | "-NOBR" | "-S" | "-SMALL"
            | "-STRIKE" | "-STRONG" | "-TT" | "-U" => {
                self.run_adoption_agency_algorithm()?;
                Ok(true)
            }
            "+APPLET" | "+MARQUEE" | "+OBJECT" => {
                self.reconstruct_active_formatting_elements()?;
                self.insert_html_element(self.current());
                self.formatting.insert_marker();
                Ok(true)
            }
            "-APPLET" | "-MARQUEE" | "-OBJECT" => {
                if !self.open.has_element_in_scope(&name) {
                    return self.next();
                }
                self.generate_implied_end_tags(None);
                self.pop_until(&name);
                self.formatting.clear_up_to_last_marker();
                Ok(true)
            }
            // Fragments are in no-quirks mode.
            "+TABLE" => {
                self.close_p_in_button_scope();
                self.insert_html_element(self.current());
                self.mode = Mode::Table;
                Ok(true)
            }
            "+INPUT" => {
                if self.open.has_element_in_scope("SELECT") {
                    self.pop_until("SELECT");
                }
                self.reconstruct_active_formatting_elements()?;
                self.insert_html_element(self.current());
                Ok(true)
            }
            "+PARAM" | "+SOURCE" | "+TRACK" | "+NOEMBED" | "+TEXTAREA" | "+IFRAME" => {
                self.insert_html_element(self.current());
                Ok(true)
            }
            "+HR" => {
                self.close_p_in_button_scope();
                if self.open.has_element_in_scope("SELECT") {
                    self.generate_implied_end_tags(None);
                }
                self.insert_html_element(self.current());
                Ok(true)
            }
            "+XMP" => {
                self.close_p_in_button_scope();
                self.reconstruct_active_formatting_elements()?;
                self.insert_html_element(self.current());
                Ok(true)
            }
            "+SELECT" => {
                if self.open.has_element_in_scope("SELECT") {
                    self.pop_until("SELECT");
                    return self.next();
                }
                self.reconstruct_active_formatting_elements()?;
                self.insert_html_element(self.current());
                Ok(true)
            }
            "+OPTION" | "+OPTGROUP" => {
                if self.open.has_element_in_scope("SELECT") {
                    let except = (name == "OPTION").then_some("OPTGROUP");
                    self.generate_implied_end_tags(except);
                } else if self.open.current_node_is("OPTION") {
                    self.pop();
                }
                self.reconstruct_active_formatting_elements()?;
                self.insert_html_element(self.current());
                Ok(true)
            }
            "+RB" | "+RTC" | "+RP" | "+RT" => {
                if self.open.has_element_in_scope("RUBY") {
                    let except = matches!(name.as_str(), "RP" | "RT").then_some("RTC");
                    self.generate_implied_end_tags(except);
                }
                self.insert_html_element(self.current());
                Ok(true)
            }
            "+MATH" | "+SVG" => {
                self.reconstruct_active_formatting_elements()?;
                let token = self.current();
                token.namespace.set(if name == "MATH" {
                    Namespace::Math
                } else {
                    Namespace::Svg
                });
                self.insert_html_element(Rc::clone(&token));
                if token.self_closing {
                    self.pop();
                }
                Ok(true)
            }
            _ if self.tags.is_tag_closer() => self.in_body_any_other_end_tag(),
            _ => {
                self.reconstruct_active_formatting_elements()?;
                self.insert_html_element(self.current());
                Ok(true)
            }
        }
    }

    /// A formatting element: inserted, and remembered for reconstruction.
    fn insert_formatting_element(&mut self) -> Step {
        self.reconstruct_active_formatting_elements()?;
        self.insert_html_element(self.current());
        self.formatting.push(self.current());
        Ok(true)
    }

    /// `</form>`.
    fn close_form(&mut self) -> Step {
        if self.open.contains("TEMPLATE") {
            if !self.open.has_element_in_scope("FORM") {
                return self.next();
            }
            self.generate_implied_end_tags(None);
            self.pop_until("FORM");
            return Ok(true);
        }
        let node = self.form_element.take();
        let Some(node) = node.filter(|_| self.open.has_element_in_scope("FORM")) else {
            return self.next();
        };
        self.generate_implied_end_tags(None);
        if !self
            .open
            .current_node()
            .is_some_and(|top| Rc::ptr_eq(&top, &node))
        {
            return self.bail("Cannot close a FORM when other elements remain open as this would throw off the breadcrumbs for the following tokens.");
        }
        self.remove_node(&node);
        Ok(true)
    }

    /// The `<li>`, `<dd>` and `<dt>` loop: closes an open item of the same
    /// kind unless another special element than `address`, `div` or `p` is nearer.
    fn close_list_item(&mut self, name: &str) {
        let is_li = name == "LI";
        let mut matched = None;
        for node in self.open.walk_up() {
            let same = if is_li {
                node.node_name == "LI"
            } else {
                matches!(node.node_name.as_str(), "DD" | "DT")
            };
            if same {
                matched = Some(node.node_name.clone());
                break;
            }
            if !matches!(node.node_name.as_str(), "ADDRESS" | "DIV" | "P") && is_special(node) {
                break;
            }
        }
        if let Some(node_name) = matched {
            self.generate_implied_end_tags(Some(&node_name));
            self.pop_until(&node_name);
        }
    }

    /// `in_body_any_other_end_tag()`.
    fn in_body_any_other_end_tag(&mut self) -> Step {
        let name = self.token_name();
        // The first node of the name, unless a special element comes first;
        // the root `HTML` is special, so one of the two is found.
        let found = self
            .open
            .walk_up()
            .find(|node| {
                node.namespace.get() == Namespace::Html && node.node_name == name
                    || is_special(node)
            })
            .filter(|node| node.namespace.get() == Namespace::Html && node.node_name == name)
            .cloned();
        let Some(node) = found else {
            return self.next();
        };
        self.generate_implied_end_tags(Some(&name));
        self.pop_through(&node);
        Ok(true)
    }

    // The algorithms.

    fn close_p_in_button_scope(&mut self) {
        if self.open.has_p_in_button_scope() {
            self.close_a_p_element();
        }
    }

    /// `close_a_p_element()`.
    fn close_a_p_element(&mut self) {
        self.generate_implied_end_tags(Some("P"));
        self.pop_until("P");
    }

    /// `generate_implied_end_tags()`, but for one element.
    pub(super) fn generate_implied_end_tags(&mut self, except: Option<&str>) {
        while except.is_none_or(|name| !self.open.current_node_is(name))
            && self
                .open
                .current_node()
                .is_some_and(|node| IMPLIED_END_TAGS.contains(&node.node_name.as_str()))
        {
            self.pop();
        }
    }

    /// `reconstruct_active_formatting_elements()`: WordPress bails where
    /// elements would have to be reopened.
    fn reconstruct_active_formatting_elements(&mut self) -> Result<(), super::Bail> {
        let Some(last) = self.formatting.current_node() else {
            return Ok(());
        };
        if last.node_name == "marker" || self.open.contains_node(&last) {
            return Ok(());
        }
        self.bail("Cannot reconstruct active formatting elements when advancing and rewinding is required.")
    }

    /// `run_adoption_agency_algorithm()`, as far as WordPress goes: it bails
    /// where a furthest block would need its elements moved.
    fn run_adoption_agency_algorithm(&mut self) -> Result<(), super::Bail> {
        // Without the Noah's Ark clause every open formatting element is
        // listed, so the first step's early return never applies.
        let subject = self.get_tag().unwrap_or_default();
        let formatting_element = self
            .formatting
            .walk_up()
            .take_while(|item| item.node_name != "marker")
            .find(|item| item.node_name == subject)
            .cloned();
        let Some(formatting_element) = formatting_element else {
            return self.bail("Cannot run adoption agency when \"any other end tag\" is required.");
        };
        if !self.open.contains_node(&formatting_element) {
            self.formatting.remove_node(&formatting_element);
            return Ok(());
        }
        if !self
            .open
            .has_element_in_scope(&formatting_element.node_name)
        {
            return Ok(());
        }
        let furthest_block = self
            .open
            .walk_up()
            .take_while(|item| item.bookmark != formatting_element.bookmark)
            .any(|item| is_special(item));
        if furthest_block {
            return self.bail("Cannot extract common ancestor in adoption agency algorithm.");
        }
        self.pop_through(&formatting_element);
        self.formatting.remove_node(&formatting_element);
        Ok(())
    }

    /// `reset_insertion_mode_appropriately()`.
    pub(super) fn reset_insertion_mode_appropriately(&mut self) {
        // The bottom node stands for the `<body>` context, so the walk skips
        // it and ends in body; a fragment has no head or frameset open.
        let above_root = self.open.count().saturating_sub(1);
        let nodes: Vec<_> = self.open.walk_up().take(above_root).cloned().collect();
        for node in nodes {
            if node.namespace.get() != Namespace::Html {
                continue;
            }
            self.mode = match node.node_name.as_str() {
                "TD" | "TH" => Mode::Cell,
                "TR" => Mode::Row,
                "TBODY" | "THEAD" | "TFOOT" => Mode::TableBody,
                "CAPTION" => Mode::Caption,
                "COLGROUP" => Mode::ColumnGroup,
                "TABLE" => Mode::Table,
                "TEMPLATE" => self.template_modes.last().copied().unwrap_or(Mode::Body),
                _ => continue,
            };
            return;
        }
        self.mode = Mode::Body;
    }
}

/// `is_special()`: elements with special parsing rules.
fn is_special(token: &super::Token) -> bool {
    let name = token.namespaced_name();
    matches!(
        name.as_str(),
        "ADDRESS"
            | "APPLET"
            | "AREA"
            | "ARTICLE"
            | "ASIDE"
            | "BASE"
            | "BASEFONT"
            | "BGSOUND"
            | "BLOCKQUOTE"
            | "BODY"
            | "BR"
            | "BUTTON"
            | "CAPTION"
            | "CENTER"
            | "COL"
            | "COLGROUP"
            | "DD"
            | "DETAILS"
            | "DIR"
            | "DIV"
            | "DL"
            | "DT"
            | "EMBED"
            | "FIELDSET"
            | "FIGCAPTION"
            | "FIGURE"
            | "FOOTER"
            | "FORM"
            | "FRAME"
            | "FRAMESET"
            | "H1"
            | "H2"
            | "H3"
            | "H4"
            | "H5"
            | "H6"
            | "HEAD"
            | "HEADER"
            | "HGROUP"
            | "HR"
            | "HTML"
            | "IFRAME"
            | "IMG"
            | "INPUT"
            | "KEYGEN"
            | "LI"
            | "LINK"
            | "LISTING"
            | "MAIN"
            | "MARQUEE"
            | "MENU"
            | "META"
            | "NAV"
            | "NOEMBED"
            | "NOFRAMES"
            | "NOSCRIPT"
            | "OBJECT"
            | "OL"
            | "P"
            | "PARAM"
            | "PLAINTEXT"
            | "PRE"
            | "SCRIPT"
            | "SEARCH"
            | "SECTION"
            | "SELECT"
            | "SOURCE"
            | "STYLE"
            | "SUMMARY"
            | "TABLE"
            | "TBODY"
            | "TD"
            | "TEMPLATE"
            | "TEXTAREA"
            | "TFOOT"
            | "TH"
            | "THEAD"
            | "TITLE"
            | "TR"
            | "TRACK"
            | "UL"
            | "WBR"
            | "XMP"
            | "math MI"
            | "math MO"
            | "math MN"
            | "math MS"
            | "math MTEXT"
            | "math ANNOTATION-XML"
            | "svg DESC"
            | "svg FOREIGNOBJECT"
            | "svg TITLE"
    )
}
