// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's site, for the goldens: its URLs and users filled in, a
//! request's nonces left as the placeholders the goldens keep.

use std::collections::HashMap;
use std::sync::OnceLock;

use hyscribe_auth::caps::{CapPost, CapabilityData, User as Capabilities};
use hyscribe_testkit::SqlValue;

use crate::assets::{Urls, admin_links};
use crate::login::{Site, User};
use crate::menu::{MenuUser, link};
use crate::page::PageSite;

pub const SITE: &str = "http://127.0.0.1:8081";

/// The oracle, with every user's `use_ssl` option as given.
#[derive(Default)]
pub struct Oracle {
    pub use_ssl: bool,
    /// `blog_public` is 0.
    pub private: bool,
}

impl Urls for Oracle {
    fn site_url(&self) -> String {
        SITE.into()
    }
    fn admin_url(&self, path: &str) -> String {
        format!("{SITE}/wp-admin/{path}")
    }
    fn fill(&self, text: &str) -> String {
        text.replace("%rest_url%", &format!("{SITE}/index.php?rest_route=/"))
            .replace("%admin_url%", &format!("{SITE}/wp-admin/"))
            .replace("\\\"%admin_path%", "\\\"/wp-admin/")
            .replace("\"%admin_path%", "\"/wp-admin/")
            .replace("%site_url%", SITE)
            .replace("%user_id%", "0")
    }
}

/// The fixture's administrator, `admin`, whose password is
/// `reference-only-password`.
fn admin(use_ssl: bool) -> User {
    static HASH: OnceLock<String> = OnceLock::new();
    let hash = HASH.get_or_init(|| {
        hyscribe_auth::hash_password(b"reference-only-password").unwrap_or_default()
    });
    User {
        id: 1,
        pass: hash.clone().into_bytes(),
        use_ssl,
    }
}

impl Site for Oracle {
    fn blogname(&self) -> String {
        "HyScribe Default Install".into()
    }
    fn name(&self) -> String {
        "HyScribe Default Install".into()
    }
    fn nonce(&self, action: &str) -> String {
        format!("%nonce:{action}%")
    }
    fn public(&self) -> bool {
        !self.private
    }
    fn home_url(&self) -> String {
        format!("{SITE}/")
    }
    fn login_url(&self, query: &str) -> String {
        format!("{SITE}/wp-login.php{query}")
    }
    fn user_by_login(&self, login: &[u8]) -> Option<User> {
        login
            .eq_ignore_ascii_case(b"admin")
            .then(|| admin(self.use_ssl))
    }
    fn user_by_email(&self, email: &[u8]) -> Option<User> {
        email
            .eq_ignore_ascii_case(b"admin@default-install.test")
            .then(|| admin(self.use_ssl))
    }
}

/// The text around the first difference of `php` and `ours`, for a report.
pub fn first_difference(php: &str, ours: &str) -> String {
    let at = ours
        .bytes()
        .zip(php.bytes())
        .position(|(a, b)| a != b)
        .unwrap_or(ours.len().min(php.len()));
    let near = |text: &str| {
        text.get(at.saturating_sub(80)..(at + 120).min(text.len()))
            .unwrap_or_default()
            .to_string()
    };
    format!("\n  php:  {:?}\n  ours: {:?}", near(php), near(ours))
}

/// The default install's options.
pub fn options() -> &'static HashMap<String, Vec<u8>> {
    static OPTIONS: OnceLock<HashMap<String, Vec<u8>>> = OnceLock::new();
    OPTIONS.get_or_init(|| {
        let version = hyscribe_testkit::default_install_versions().pop().unwrap();
        let dump = hyscribe_testkit::default_install(&version).unwrap();
        dump.table("wp_options")
            .filter_map(|row| match (&row[1], &row[2]) {
                (SqlValue::Text(name), SqlValue::Text(value)) => {
                    Some((String::from_utf8_lossy(name).into_owned(), value.clone()))
                }
                _ => None,
            })
            .collect()
    })
}

/// User 1 with a role's capabilities, on HyScribe's constants, as
/// `reference/admin-menu/as-role.php` sets PHP up.
pub struct Role(pub &'static str);

impl CapabilityData for Role {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        options().get(name).cloned()
    }
    fn user_meta(&self, _: u64, key: &str) -> Option<Vec<u8>> {
        (key == "wp_capabilities")
            .then(|| format!("a:1:{{s:{}:\"{}\";b:1;}}", self.0.len(), self.0).into_bytes())
    }
    fn user_exists(&self, id: u64) -> bool {
        id == 1
    }
    fn post(&self, _: u64) -> Option<CapPost> {
        None
    }
    fn post_meta(&self, _: u64, _: &str) -> Option<Vec<u8>> {
        None
    }
    fn comment_post(&self, _: u64) -> Option<u64> {
        None
    }
    fn term_taxonomy(&self, _: u64) -> Option<String> {
        None
    }
    fn constant(&self, name: &str) -> bool {
        name == "DISALLOW_UNFILTERED_HTML"
    }
}

impl MenuUser for Role {
    fn can(&self, cap: &str) -> bool {
        Capabilities::load(self, "wp_", 1).has_cap(self, cap, &[])
    }
    fn moderated_comments(&self) -> u64 {
        0
    }
    fn transient(&self, _: &str) -> Option<Vec<u8>> {
        None
    }
}

impl Urls for Role {
    fn site_url(&self) -> String {
        SITE.into()
    }
    fn admin_url(&self, path: &str) -> String {
        format!("{SITE}/hs-admin/{}", link(path))
    }
    fn fill(&self, text: &str) -> String {
        admin_links(text, "%admin_url%", &format!("{SITE}/hs-admin/"))
            .replace("%rest_url%", &format!("{SITE}/index.php?rest_route=/"))
            .replace("%site_url%", SITE)
            .replace("%user_id%", "1")
    }
}

impl PageSite for Role {
    fn user_id(&self) -> u64 {
        1
    }
    fn nonce(&self, action: &str) -> String {
        format!("%nonce:{action}%")
    }
    fn admin_color(&self) -> Option<Vec<u8>> {
        Some(b"modern".to_vec())
    }
    fn preferences(&self) -> Option<Vec<u8>> {
        None
    }
    fn compression_tested(&self) -> bool {
        false
    }
    fn login_url(&self) -> String {
        format!("{SITE}/hs-login")
    }
    fn template(&self) -> String {
        "twentytwentyfive".into()
    }
    fn font_faces(&self) -> Vec<u8> {
        static FACES: OnceLock<Vec<u8>> = OnceLock::new();
        FACES
            .get_or_init(|| hyscribe_render::theme::twentytwentyfive().admin_font_faces(SITE))
            .clone()
    }
}
