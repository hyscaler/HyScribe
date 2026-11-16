// SPDX-License-Identifier: AGPL-3.0-or-later

//! The site of a handler that reads nothing of it but `admin_url()` and
//! the configuration's constants.

use hyscribe_admin::actions::ActionSite;
use hyscribe_auth::caps::{CapPost, CapabilityData};
use hyscribe_config::Constants;
use hyscribe_phpser::Value;

use super::super::Acting;
use super::SITE;

/// A site any other read of fails the test: the write oracle's
/// `admin_url()`, and its constants, by default HyScribe's.
#[derive(Default)]
pub(in crate::admin::saves) struct Unread(pub Constants);

impl CapabilityData for Unread {
    fn option(&self, _: &str) -> Option<Vec<u8>> {
        unreachable!()
    }
    fn user_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        unreachable!()
    }
    fn user_exists(&self, _: u64) -> bool {
        unreachable!()
    }
    fn post(&self, _: u64) -> Option<CapPost> {
        unreachable!()
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        unreachable!()
    }
    fn comment_post(&self, _: u64) -> Option<u64> {
        unreachable!()
    }
    fn term_taxonomy(&self, _: u64) -> Option<String> {
        unreachable!()
    }
    fn constant(&self, _: &str) -> bool {
        unreachable!()
    }
}

impl ActionSite for Unread {
    fn current_user_can(&self, _: &str, _: &[Value]) -> bool {
        unreachable!()
    }
    fn verify_nonce(&self, _: &[u8], _: &str) -> bool {
        unreachable!()
    }
    fn validate_redirect(&self, _: &[u8]) -> Vec<u8> {
        unreachable!()
    }
    fn comment_approved(&self, _: u64) -> Option<Vec<u8>> {
        unreachable!()
    }
    fn user_roles(&self) -> Option<Vec<u8>> {
        unreachable!()
    }
    fn lock_holder(&self, _: u64) -> Option<String> {
        unreachable!()
    }
}

impl Acting for Unread {
    fn admin_url(&self, path: &str) -> String {
        format!("{SITE}/hs-admin/{}", hyscribe_admin::menu::link(path))
    }
    fn comment_url(&self, _: u64) -> String {
        unreachable!()
    }
    fn user_can(&self, _: u64, _: &str) -> bool {
        unreachable!()
    }
    fn create_nonce(&self, _: &str) -> String {
        unreachable!()
    }
    fn constants(&self) -> Constants {
        self.0
    }
    fn nonce_state(&self, _: &[u8], _: &str) -> u8 {
        unreachable!()
    }
    fn logged_in(&self) -> bool {
        unreachable!()
    }
}
