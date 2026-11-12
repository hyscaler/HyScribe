// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's site, for the goldens: its URLs and users filled in, a
//! request's nonces left as the placeholders the goldens keep.

use std::sync::OnceLock;

use crate::assets::Urls;
use crate::login::{Site, User};

pub const SITE: &str = "http://127.0.0.1:8081";

/// The oracle, with every user's `use_ssl` option as given.
#[derive(Default)]
pub struct Oracle {
    pub use_ssl: bool,
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
