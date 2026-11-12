// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin, the dashboard emulating `wp-admin` (ADR 0008).

pub mod admin_bar;
pub mod assets;
pub mod die;
pub mod login;
pub mod menu;
#[cfg(test)]
mod tests;
