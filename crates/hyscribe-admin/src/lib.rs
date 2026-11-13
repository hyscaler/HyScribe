// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin, the dashboard emulating `wp-admin` (ADR 0008).

pub mod admin_bar;
pub mod assets;
pub mod comments;
pub mod dashboard;
pub mod die;
pub mod list_table;
pub mod login;
pub mod menu;
pub mod page;
pub mod posts;
pub mod profile;
pub mod settings_more;
pub mod terms;
#[cfg(test)]
mod tests;
pub mod tools;
pub mod users;
