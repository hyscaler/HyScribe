// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_HTML_Open_Elements` and `WP_HTML_Active_Formatting_Elements`.

use std::rc::Rc;

use super::Token;
use crate::tag_processor::Namespace;

/// The name `has_element_in_scope()` and `pop_until()` take for any of
/// `H1` to `H6`.
pub(super) const ANY_HEADING: &str = "(internal: H1 through H6 - do not use)";

/// The elements that end every scope.
const SCOPE: [&str; 19] = [
    "APPLET",
    "CAPTION",
    "HTML",
    "TABLE",
    "TD",
    "TH",
    "MARQUEE",
    "OBJECT",
    "SELECT",
    "TEMPLATE",
    "math MI",
    "math MO",
    "math MN",
    "math MS",
    "math MTEXT",
    "math ANNOTATION-XML",
    "svg FOREIGNOBJECT",
    "svg DESC",
    "svg TITLE",
];

pub(super) fn is_heading(name: &str) -> bool {
    matches!(name, "H1" | "H2" | "H3" | "H4" | "H5" | "H6")
}

/// `WP_HTML_Open_Elements`: the stack of open elements.
#[derive(Default)]
pub(super) struct OpenElements {
    stack: Vec<Rc<Token>>,
    has_p_in_button_scope: bool,
}

impl OpenElements {
    pub(super) fn count(&self) -> usize {
        self.stack.len()
    }

    pub(super) fn current_node(&self) -> Option<Rc<Token>> {
        self.stack.last().cloned()
    }

    /// The nodes from the top down.
    pub(super) fn walk_up(&self) -> impl Iterator<Item = &Rc<Token>> {
        self.stack.iter().rev()
    }

    /// `contains()`: an HTML element of the name is open.
    pub(super) fn contains(&self, name: &str) -> bool {
        self.stack
            .iter()
            .any(|t| t.namespace.get() == Namespace::Html && t.node_name == name)
    }

    pub(super) fn contains_node(&self, token: &Rc<Token>) -> bool {
        self.stack.iter().any(|t| Rc::ptr_eq(t, token))
    }

    /// `current_node_is()`, by name.
    pub(super) fn current_node_is(&self, name: &str) -> bool {
        self.stack.last().is_some_and(|node| node.node_name == name)
    }

    /// `has_element_in_specific_scope()`.
    fn has_element_in_specific_scope(&self, name: &str, termination: &[&str]) -> bool {
        for node in self.stack.iter().rev() {
            let namespaced = node.namespaced_name();
            if namespaced == name || name == ANY_HEADING && is_heading(&namespaced) {
                return true;
            }
            if termination.contains(&namespaced.as_str()) {
                return false;
            }
        }
        false
    }

    pub(super) fn has_element_in_scope(&self, name: &str) -> bool {
        self.has_element_in_specific_scope(name, &SCOPE)
    }

    pub(super) fn has_element_in_list_item_scope(&self, name: &str) -> bool {
        let mut termination = SCOPE.to_vec();
        termination.extend(["BUTTON", "OL", "UL"]);
        self.has_element_in_specific_scope(name, &termination)
    }

    pub(super) fn has_element_in_button_scope(&self, name: &str) -> bool {
        let mut termination = SCOPE.to_vec();
        termination.push("BUTTON");
        self.has_element_in_specific_scope(name, &termination)
    }

    pub(super) fn has_element_in_table_scope(&self, name: &str) -> bool {
        self.has_element_in_specific_scope(name, &["HTML", "TABLE", "TEMPLATE"])
    }

    pub(super) fn has_p_in_button_scope(&self) -> bool {
        self.has_p_in_button_scope
    }

    pub(super) fn push(&mut self, token: Rc<Token>) {
        let name = token.namespaced_name();
        if name == "P" {
            self.has_p_in_button_scope = true;
        } else if SCOPE.contains(&name.as_str()) || name == "BUTTON" {
            self.has_p_in_button_scope = false;
        }
        self.stack.push(token);
    }

    pub(super) fn pop(&mut self) -> Option<Rc<Token>> {
        let token = self.stack.pop()?;
        self.after_pop(&token);
        Some(token)
    }

    /// `remove_node()`: by bookmark name.
    pub(super) fn remove_node(&mut self, token: &Token) -> Option<Rc<Token>> {
        let at = self
            .stack
            .iter()
            .rposition(|t| t.bookmark == token.bookmark)?;
        let removed = self.stack.remove(at);
        self.after_pop(&removed);
        Some(removed)
    }

    fn after_pop(&mut self, token: &Token) {
        let name = token.namespaced_name();
        if name == "P" || name == "BUTTON" || SCOPE.contains(&name.as_str()) {
            self.has_p_in_button_scope = self.has_element_in_button_scope("P");
        }
    }
}

/// `WP_HTML_Active_Formatting_Elements`, with markers.
#[derive(Default)]
pub(super) struct ActiveFormatting {
    stack: Vec<Rc<Token>>,
}

impl ActiveFormatting {
    pub(super) fn current_node(&self) -> Option<Rc<Token>> {
        self.stack.last().cloned()
    }

    pub(super) fn walk_up(&self) -> impl Iterator<Item = &Rc<Token>> {
        self.stack.iter().rev()
    }

    pub(super) fn push(&mut self, token: Rc<Token>) {
        self.stack.push(token);
    }

    pub(super) fn insert_marker(&mut self) {
        self.stack.push(Rc::new(Token::new(None, "marker", false)));
    }

    /// `remove_node()`: by bookmark name.
    pub(super) fn remove_node(&mut self, token: &Token) {
        if let Some(at) = self
            .stack
            .iter()
            .rposition(|t| t.bookmark == token.bookmark)
        {
            self.stack.remove(at);
        }
    }

    pub(super) fn clear_up_to_last_marker(&mut self) {
        while let Some(token) = self.stack.pop() {
            if token.node_name == "marker" {
                break;
            }
        }
    }
}
