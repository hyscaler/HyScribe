// SPDX-License-Identifier: AGPL-3.0-or-later

//! A site where `DISALLOW_UNFILTERED_HTML` is false: the current user, an
//! administrator, may post unfiltered HTML, and so may comment authors
//! when `authors` says so.

use hyscribe_admin::actions::ActionSite;
use hyscribe_auth::caps::{CapPost, CapabilityData};
use hyscribe_phpser::Value;

use super::super::Acting;

/// The nonce the site creates.
pub(in crate::admin::saves) const NONCE: &str = "0123456789";

pub(in crate::admin::saves) struct Unfiltered {
    /// Whether comment authors may post unfiltered HTML.
    pub(in crate::admin::saves) authors: bool,
}

impl CapabilityData for Unfiltered {
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

impl ActionSite for Unfiltered {
    /// An administrator's, with `unfiltered_html`.
    fn current_user_can(&self, _: &str, _: &[Value]) -> bool {
        true
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
    fn user_option(&self, _: &str) -> Option<Vec<u8>> {
        unreachable!()
    }
}

impl Acting for Unfiltered {
    fn admin_url(&self, path: &str) -> String {
        format!("{}/hs-admin/{path}", super::SITE)
    }
    fn comment_url(&self, _: u64) -> String {
        unreachable!()
    }
    fn user_can(&self, _: u64, cap: &str) -> bool {
        cap == "unfiltered_html" && self.authors
    }
    fn create_nonce(&self, _: &str) -> String {
        NONCE.into()
    }
    fn constants(&self) -> hyscribe_config::Constants {
        hyscribe_config::Constants {
            disallow_unfiltered_html: false,
            ..hyscribe_config::Constants::default()
        }
    }
    fn nonce_state(&self, _: &[u8], _: &str) -> u8 {
        unreachable!()
    }
    fn logged_in(&self) -> bool {
        unreachable!()
    }
    fn delete_files(&self, _: Vec<(Vec<u8>, Vec<u8>)>) -> bool {
        unreachable!()
    }
    fn extensions(&self) -> Option<&hyscribe_ext::Runtime> {
        unreachable!()
    }
    fn outbox(&self) -> &crate::mail::Outbox {
        crate::mail::tests::outbox()
    }
}
