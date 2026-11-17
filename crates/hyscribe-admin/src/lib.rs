// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin, the dashboard emulating `wp-admin` (ADR 0008).

pub mod actions;
pub mod admin_bar;
pub mod appearance;
pub mod assets;
pub mod comments;
pub mod dashboard;
pub mod die;
pub mod editor;
pub mod list_table;
pub mod login;
pub mod media;
pub mod menu;
pub mod page;
pub mod posts;
pub mod profile;
pub mod revisions;
pub mod settings;
pub mod settings_more;
pub mod site_editor;
pub mod site_health;
pub mod terms;
#[cfg(test)]
mod tests;
pub mod tools;
pub mod user_new;
pub mod users;

/// Bytes as text, invalid UTF-8 replaced.
pub(crate) fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
