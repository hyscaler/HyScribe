// SPDX-License-Identifier: AGPL-3.0-or-later

//! The tabs blocks: the tabs' ID and its panels' IDs as the context
//! `block_core_tabs_provide_context()` gives, the tab list's buttons tied
//! to the panels, and the panels' IDs, with their directives.

use std::collections::HashMap;

use hyscribe_html::{Attribute, TagProcessor};
use hyscribe_kses::{esc_attr, wp_strip_all_tags};
use hyscribe_phpser::{Key, Value, is_empty_string, json_encode, lookup};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::template::Renderer;

/// The `core/tabs-id` and `core/tabs-list` context.
#[derive(Clone)]
pub(crate) struct TabsContext {
    id: Value,
    list: Vec<Vec<u8>>,
}

/// The tab panels' counters, by tabs ID, which PHP keeps for the request.
pub(crate) type TabCounters = HashMap<Vec<u8>, u64>;

impl Renderer<'_> {
    /// `block_core_tabs_render_block_callback()` around inner blocks given
    /// the tabs' context.
    pub(crate) fn tabs(&mut self, block: &Block) -> Vec<u8> {
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let id = match lookup(parsed, "anchor").filter(|id| **id != Value::Null) {
            Some(id) => id.clone(),
            None => Value::String(self.unique_id("tabs_")),
        };
        let context = TabsContext {
            list: tabs_list(&block.inner_blocks, &id),
            id,
        };
        let outer = self.tabs.replace(context.clone());
        let content = self.saved_markup(block);
        self.tabs = outer;
        if context.id.is_empty() {
            return Vec::new();
        }
        let active = prepared("core/tabs", parsed)
            .get_str("activeTabIndex")
            .cloned()
            .unwrap_or(Value::Null);
        let state = Value::Array(vec![
            (Key::String(b"tabsId".to_vec()), context.id),
            (Key::String(b"activeTabIndex".to_vec()), active),
        ]);
        let mut tags = TagProcessor::new(&content);
        tags.next_tag(None, Some(b"wp-block-tabs"));
        tags.set_attribute(b"data-wp-interactive", b"core/tabs");
        tags.set_attribute(b"data-wp-context", &json_encode(&state).unwrap_or_default());
        tags.set_attribute(b"data-wp-init", b"callbacks.onTabsInit");
        tags.set_attribute(b"data-wp-on--keydown", b"actions.handleTabKeyDown");
        tags.get_updated_html().to_vec()
    }

    /// `block_core_tab_list_render_callback()`: the list's label, and each
    /// button tied to its panel.
    pub(crate) fn tab_list(&mut self, block: &Block) -> Vec<u8> {
        let content = self.saved_markup(block);
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let label = prepared("core/tab-list", parsed)
            .get_str("ariaLabel")
            .filter(|label| !label.is_empty())
            .map_or(b"Tabbed content".to_vec(), |label| {
                wp_strip_all_tags(&label.to_php_string(), false)
            });
        let list = self
            .tabs
            .as_ref()
            .map(|tabs| &tabs.list[..])
            .unwrap_or_default();
        let mut tags = TagProcessor::new(&content);
        if tags.next_tag(None, Some(b"wp-block-tab-list")) {
            tags.set_attribute(b"aria-label", &label);
        }
        for (index, id) in list.iter().enumerate() {
            if !tags.next_tag(Some(b"BUTTON"), None) {
                break;
            }
            tags.set_attribute(b"id", &[b"tab__", &id[..]].concat());
            tags.set_attribute(b"aria-controls", id);
            tags.set_attribute(b"data-wp-on--click", b"actions.handleTabClick");
            tags.set_attribute(b"data-wp-on--keydown", b"actions.handleTabKeyDown");
            tags.set_attribute(b"data-wp-bind--aria-selected", b"state.isActiveTab");
            tags.set_attribute(b"data-wp-bind--tabindex", b"state.tabIndexAttribute");
            tags.set_attribute(
                b"data-wp-context",
                format!("{{\"tabIndex\":{index}}}").as_bytes(),
            );
        }
        tags.get_updated_html().to_vec()
    }

    /// `block_core_tab_panel_render()`: the panel's own ID, or one from its
    /// position among its tabs' panels, labelled by its button.
    pub(crate) fn tab_panel(&mut self, block: &Block) -> Vec<u8> {
        let content = self.saved_markup(block);
        let tabs_id = self
            .tabs
            .as_ref()
            .map(|tabs| tabs.id.to_php_string())
            .unwrap_or_default();
        let counter = self.tab_counters.entry(tabs_id.clone()).or_insert(0);
        let index = *counter;
        *counter += 1;
        let mut tags = TagProcessor::new(&content);
        tags.next_tag(None, Some(b"wp-block-tab-panel"));
        let mut id = match tags.get_attribute(b"id") {
            Some(Attribute::Value(id)) => id,
            Some(Attribute::True) => b"1".to_vec(),
            None => Vec::new(),
        };
        if is_empty_string(&id) {
            id = panel_id(&tabs_id, index);
            tags.set_attribute(b"id", &id);
        }
        tags.set_attribute(b"aria-labelledby", &[b"tab__", &id[..]].concat());
        tags.set_attribute(b"data-wp-bind--hidden", b"!state.isActiveTab");
        tags.get_updated_html().to_vec()
    }
}

/// `block_core_tabs_generate_tabs_list()`: each panel's anchor, or its
/// position's ID, in the first tab panels block.
fn tabs_list(inner: &[Block], tabs_id: &Value) -> Vec<Vec<u8>> {
    let Some(panels) = inner
        .iter()
        .find(|block| block.name.as_deref() == Some("core/tab-panels"))
    else {
        return Vec::new();
    };
    let tabs_id = tabs_id.to_php_string();
    panels
        .inner_blocks
        .iter()
        .filter(|block| block.name.as_deref() == Some("core/tab-panel"))
        .enumerate()
        .map(|(index, panel)| {
            let parsed = panel.attrs.as_deref().unwrap_or_default();
            let id = match lookup(parsed, "anchor").filter(|anchor| !anchor.is_empty()) {
                Some(anchor) => anchor.to_php_string(),
                None => panel_id(&tabs_id, index as u64),
            };
            esc_attr(&id).into_bytes()
        })
        .collect()
}

/// A panel's ID from its position: under its tabs' ID if there is one.
fn panel_id(tabs_id: &[u8], index: u64) -> Vec<u8> {
    if is_empty_string(tabs_id) {
        format!("tab-{index}").into_bytes()
    } else {
        [tabs_id, format!("-tab-{index}").as_bytes()].concat()
    }
}

#[cfg(test)]
mod tests;
