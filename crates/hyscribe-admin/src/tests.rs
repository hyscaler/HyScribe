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

impl Role {
    /// The role, after the variant if one is given as `variant:role`.
    pub fn role(&self) -> &'static str {
        self.0.rsplit(':').next().unwrap_or(self.0)
    }

    /// The variant of `reference/dashboard/golden.sh`, if any.
    pub fn variant(&self) -> &'static str {
        self.0.split_once(':').map_or("", |(variant, _)| variant)
    }
}

impl CapabilityData for Role {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        options().get(name).cloned()
    }
    fn user_meta(&self, _: u64, key: &str) -> Option<Vec<u8>> {
        (key == "wp_capabilities").then(|| {
            format!("a:1:{{s:{}:\"{}\";b:1;}}", self.role().len(), self.role()).into_bytes()
        })
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
        u64::from(self.variant() == "content")
    }
    fn transient(&self, name: &str) -> Option<Vec<u8>> {
        let counts: &[u8] = match (name, self.variant()) {
            ("health-check-site-status-result", "health") => {
                br#"{"good":5,"recommended":1,"critical":0}"#
            }
            ("health-check-site-status-result", "critical") => {
                br#"{"good":1,"recommended":2,"critical":2}"#
            }
            ("health-check-site-status-result", "one") => {
                br#"{"good":9,"recommended":0,"critical":1}"#
            }
            ("health-check-site-status-result", "passing") => {
                br#"{"good":9,"recommended":0,"critical":0}"#
            }
            ("health-check-site-status-result", "some") => {
                br#"{"good":9,"recommended":2,"critical":0}"#
            }
            _ => return None,
        };
        Some(counts.to_vec())
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
        let text = admin_links(text, "%admin_url%", &format!("{SITE}/hs-admin/"));
        admin_links(&text, "\"%admin_path%", "\"/hs-admin/")
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

/// A post by user 1, as the fixture and `reference/dashboard/content.sql`
/// hold them.
fn post(id: u64, title: &str, status: &str, date: &str, content: &str) -> hyscribe_model::Post {
    hyscribe_model::Post {
        id,
        author: 1,
        title: title.as_bytes().to_vec(),
        content: content.as_bytes().to_vec(),
        date: date.into(),
        date_gmt: date.into(),
        modified: date.into(),
        status: status.into(),
        ..hyscribe_model::Post::default()
    }
}

/// A comment of `reference/dashboard/content.sql`.
fn comment(
    id: u64,
    (author, email, url): (&str, &str, &str),
    content: &str,
    comment_type: &str,
    post: Option<hyscribe_model::Post>,
) -> hyscribe_model::Comment {
    hyscribe_model::Comment {
        id,
        post_id: post.map_or(0, |post| post.id),
        author: author.as_bytes().to_vec(),
        author_email: email.as_bytes().to_vec(),
        author_url: url.as_bytes().to_vec(),
        content: content.as_bytes().to_vec(),
        comment_type: comment_type.as_bytes().to_vec(),
        ..hyscribe_model::Comment::default()
    }
}

impl crate::dashboard::DashboardSite for Role {
    fn show_welcome_panel(&self) -> Option<Vec<u8>> {
        let shown: &[u8] = match self.variant() {
            "layout" => b"0",
            "owner" | "owned" => b"2",
            _ => b"1",
        };
        Some(shown.to_vec())
    }
    fn user_email(&self) -> Vec<u8> {
        if self.variant() == "owner" {
            return b"other@example.test".to_vec();
        }
        options()["admin_email"].clone()
    }
    fn published(&self, _: &str) -> u64 {
        match self.variant() {
            "empty" => 0,
            "dates" => 2,
            _ => 1,
        }
    }
    fn comment_counts(&self) -> crate::dashboard::CommentCounts {
        if self.variant() == "content" {
            crate::dashboard::CommentCounts {
                approved: 3,
                moderated: 1,
                spam: 1,
                trash: 1,
                mine: 1,
            }
        } else {
            crate::dashboard::CommentCounts {
                approved: u64::from(!matches!(self.variant(), "empty" | "pending")),
                moderated: u64::from(self.variant() == "pending"),
                ..crate::dashboard::CommentCounts::default()
            }
        }
    }
    fn recent_comments(&self) -> Vec<crate::dashboard::RecentComment> {
        // `edit_comment` is `edit_post` of the comment's post, user 1's.
        let can_edit = self.can("edit_published_posts");
        let hello = crate::dashboard::tests::hello_comment(can_edit);
        if matches!(self.variant(), "empty" | "dates") {
            return Vec::new();
        }
        if self.variant() != "content" {
            return vec![hello];
        }
        let first = || {
            Some(post(
                1,
                "Hello world!",
                "publish",
                "2026-10-27 08:08:15",
                "",
            ))
        };
        let row =
            |comment: hyscribe_model::Comment, status, user| crate::dashboard::RecentComment {
                link: format!("{SITE}/?p=1#comment-{}", comment.id),
                permalink: format!("{SITE}/?p={}", comment.post_id),
                post: (comment.post_id != 0).then(first).flatten(),
                comment,
                status,
                user,
                can_edit,
            };
        let admin = hyscribe_model::User {
            id: 1,
            nicename: b"admin".to_vec(),
            email: b"admin@default-install.test".to_vec(),
            display_name: b"admin".to_vec(),
            ..hyscribe_model::User::default()
        };
        let mut mine = comment(
            14,
            ("admin", "admin@default-install.test", SITE),
            "Mine",
            "comment",
            first(),
        );
        mine.user_id = 1;
        let mut rows = vec![
            row(mine, "approved", Some(admin)),
            row(
                comment(
                    11,
                    ("Nowhere", "nowhere@example.test", "http://"),
                    "On no post",
                    "comment",
                    None,
                ),
                "approved",
                None,
            ),
        ];
        if self.can("edit_posts") {
            rows.push(row(
                comment(
                    10,
                    ("Pinger", "", "http://pinger.test/"),
                    "A <em>ping</em> back",
                    "pingback",
                    first(),
                ),
                "unapproved",
                None,
            ));
        }
        rows.push(hello);
        rows
    }
    fn theme_name(&self) -> String {
        "Twenty Twenty-Five".into()
    }
    fn recent_posts(&self, status: &str) -> Vec<hyscribe_model::Post> {
        match (status, self.variant()) {
            (_, "empty") => Vec::new(),
            ("publish", "dates") => vec![
                post(2, "", "publish", "2026-11-12 08:00:00", ""),
                post(3, "Next", "publish", "2026-11-13 08:00:00", ""),
            ],
            ("publish", _) => vec![post(
                1,
                "Hello world!",
                "publish",
                "2026-10-27 08:08:15",
                "",
            )],
            ("future", "content") => vec![post(201, "Later", "future", "2030-01-01 10:00:00", "")],
            ("future", "dst") => vec![post(209, "Monday", "future", "2026-11-02 09:00:00", "")],
            _ => Vec::new(),
        }
    }
    /// `map_meta_cap()`'s `edit_post` for a post by user 1.
    fn can_edit_post(&self, post: &hyscribe_model::Post) -> bool {
        if post.status == "publish" {
            self.can("edit_published_posts")
        } else {
            self.can("edit_posts")
        }
    }
    fn permalink(&self, post: &hyscribe_model::Post) -> String {
        format!("{SITE}/?p={}", post.id)
    }
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        if name == "show_avatars" && self.variant() == "noavatars"
            || name == "blog_public" && self.variant() == "layout"
        {
            return Some(b"0".to_vec());
        }
        options().get(name).cloned()
    }
    fn user_option(&self, name: &str) -> Option<Vec<u8>> {
        if self.variant() == "twice" && name == "meta-box-order_dashboard" {
            return Some(b"a:3:{s:6:\"normal\";s:37:\"dashboard_activity,dashboard_activity\";s:4:\"side\";s:19:\"dashboard_right_now\";s:7:\"column3\";s:19:\"dashboard_right_now\";}".to_vec());
        }
        if self.variant() != "layout" {
            return None;
        }
        let value: &[u8] = match name {
            "metaboxhidden_dashboard" => b"a:1:{i:0;s:18:\"dashboard_activity\";}",
            "closedpostboxes_dashboard" => b"a:1:{i:0;s:19:\"dashboard_right_now\";}",
            "meta-box-order_dashboard" => b"a:2:{s:4:\"side\";s:39:\"dashboard_site_health,dashboard_primary\";s:6:\"normal\";s:21:\"dashboard_quick_press\";}",
            _ => return None,
        };
        Some(value.to_vec())
    }
    /// The auto-draft the goldens mask, as post 4.
    fn quick_draft(&self) -> Option<u64> {
        Some(4)
    }
    fn drafts(&self) -> Vec<hyscribe_model::Post> {
        if self.variant() == "dates" {
            let soon = |id| post(id, "Soon", "draft", "2026-11-12 08:00:00", "");
            return vec![soon(206), soon(207), soon(208)];
        }
        if self.variant() != "content" {
            return Vec::new();
        }
        vec![
            post(205, "Fourth draft", "draft", "2026-10-31 09:00:00", "Last"),
            post(204, "Third \"draft\"", "draft", "2026-10-30 09:00:00", ""),
            post(203, "", "draft", "2026-10-29 09:00:00", "<b>Bold</b> words"),
            post(
                202,
                "First draft",
                "draft",
                "2026-10-28 09:00:00",
                "One two three four five six seven eight nine ten eleven twelve",
            ),
        ]
    }
    fn clock(&self) -> (hyscribe_render::date::Zone, i64) {
        if self.variant() == "dst" {
            // 2026-11-01 00:30 in New York, the day its clocks go back.
            let zone = hyscribe_render::date::site_zone(b"America/New_York", b"");
            return (zone, 1_793_507_400);
        }
        (hyscribe_render::date::Zone::Utc, 1_794_500_000)
    }
}
