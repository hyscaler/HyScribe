// SPDX-License-Identifier: AGPL-3.0-or-later

//! The PHP oracle's site, for the goldens: its URLs and users filled in, a
//! request's nonces left as the placeholders the goldens keep.

use std::collections::HashMap;
use std::sync::OnceLock;

mod notices;

use hyscribe_auth::caps::{CapPost, CapabilityData, User as Capabilities};
use hyscribe_testkit::SqlValue;

use crate::admin_bar::Browser;
use crate::assets::{Urls, admin_links};
use crate::login::{Site, User};
use crate::menu::{MenuUser, link};
use crate::page::{PageSite, Request};

pub const SITE: &str = "http://127.0.0.1:8081";

thread_local! {
    /// The term queries the test site was asked, and how often it read
    /// every term of a taxonomy, on this thread.
    pub static TERM_QUERIES: std::cell::RefCell<Vec<hyscribe_model::TermQuery>> =
        std::cell::RefCell::default();
    pub static TERM_READS: std::cell::Cell<usize> = std::cell::Cell::default();
    /// How many comments the test site was last asked for.
    pub static COMMENTS_ASKED: std::cell::Cell<u64> = std::cell::Cell::default();
    /// The attachment queries the test site was asked, on this thread.
    pub static MEDIA_QUERIES: std::cell::RefCell<Vec<hyscribe_model::PostQuery>> =
        std::cell::RefCell::default();
}

/// The oracle, with every user's `use_ssl` option as given.
#[derive(Default)]
pub struct Oracle {
    pub use_ssl: bool,
    /// `blog_public` is 0.
    pub private: bool,
    /// `users_can_register` is 1.
    pub register: bool,
    /// Admin's `user_activation_key`.
    pub activation_key: &'static [u8],
    /// The ID of a `user_request` post.
    pub user_request: Option<u64>,
    /// The variant of the test site whose site icon the site has.
    pub icon: Option<&'static str>,
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
fn admin(oracle: &Oracle) -> User {
    static HASH: OnceLock<String> = OnceLock::new();
    let hash = HASH.get_or_init(|| {
        hyscribe_auth::hash_password(b"reference-only-password").unwrap_or_default()
    });
    User {
        id: 1,
        pass: hash.clone().into_bytes(),
        use_ssl: oracle.use_ssl,
        activation_key: oracle.activation_key.to_vec(),
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
        login.eq_ignore_ascii_case(b"admin").then(|| admin(self))
    }
    fn user_by_email(&self, email: &[u8]) -> Option<User> {
        email
            .eq_ignore_ascii_case(b"admin@default-install.test")
            .then(|| admin(self))
    }
    fn can_register(&self) -> bool {
        self.register
    }
    fn post_type(&self, id: u64) -> Option<String> {
        (Some(id) == self.user_request).then(|| "user_request".into())
    }
    fn site_icon(&self, size: i64) -> Option<Vec<u8>> {
        crate::media::site_icon_url(&Role(self.icon?), size)
    }
}

/// The desktop browser the goldens are recorded with.
pub const BROWSER: Browser = Browser {
    user_agent: b"Mozilla/5.0 (X11; Linux x86_64)",
    mobile_hint: None,
};

/// The request for `uri` at `canonical` from `browser`.
pub fn request<'a>(canonical: &'a str, uri: &'a str, browser: &'a Browser<'a>) -> Request<'a> {
    Request {
        canonical,
        origin: SITE,
        uri,
        browser,
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

/// The PHP oracle's `screen` for `role` in `pages.tsv`, under hs-admin's URLs.
pub fn page_golden(role: &str, screen: &str) -> String {
    let golden = include_str!("../tests/golden/pages.tsv");
    let row = golden
        .lines()
        .find(|line| line.starts_with(&format!("{role}\t{screen}\t")))
        .unwrap();
    hyscribe_testkit::hs_admin_page(
        &String::from_utf8(hyscribe_testkit::unhex(row.rsplit('\t').next().unwrap())).unwrap(),
    )
}

/// The personal options the `poptions` and `u3options` variants give.
fn personal_option(name: &str) -> Option<&'static [u8]> {
    let value: &[u8] = match name {
        "rich_editing" | "syntax_highlighting" | "show_admin_bar_front" | "infinite_scrolling" => {
            b"false"
        }
        "comment_shortcuts" => b"true",
        "admin_color" => b"ocean",
        _ => return None,
    };
    Some(value)
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

    /// The home URL, HTTPS in the `h9home` variant.
    pub fn home(&self) -> String {
        if self.variant() == "h9home" {
            return SITE.replace("http:", "https:");
        }
        SITE.into()
    }

    /// A variant of `reference/media/golden.sh` or
    /// `reference/attachment-edit/golden.sh` over the Theme Test Data.
    fn ttd(&self) -> bool {
        self.variant().starts_with("m4ttd") || self.variant().starts_with("a8ttd")
    }

    /// A variant of `reference/attachment-edit/golden.sh` over its content.
    fn a8(&self) -> bool {
        self.variant().starts_with("a8lock") || self.variant() == "g9attach" || self.j9_media()
    }

    /// A `j9` variant of `reference/attachment-edit/golden.sh`, all of
    /// which hold user 2's lock.
    fn j9_media(&self) -> bool {
        ["j9lock", "j9pretty", "j9noupload"]
            .iter()
            .any(|prefix| self.variant().starts_with(prefix))
    }

    /// A user of the content of a `reference/attachment-edit/golden.sh`
    /// variant.
    fn a8_user(&self, id: u64) -> Option<hyscribe_model::User> {
        if self.variant().starts_with("a8ttd") {
            return crate::media::content::user(id);
        }
        crate::media::attachment::content::user(id).filter(|_| self.a8())
    }

    /// The edit lock of an `a8` variant, user 2's as fresh as can be unless
    /// the variant names another, and the `_edit_last` of one without a user.
    fn a8_lock(&self, key: &str) -> Option<&'static str> {
        let lock = match (self.variant(), key) {
            ("a8locklast", "_edit_last") => "2",
            (_, "_edit_last") => return None,
            ("a8lockold", _) => "1794499850:2",
            ("a8locknew", _) => "1794499851:2",
            ("a8lockself", _) => "1794500000:1",
            ("a8locklast", _) => "1794500000",
            ("a8lockzero", _) => "0:2",
            ("a8lockgone", _) => "1794500000:99",
            (variant, _) if variant.ends_with("lock3") => "1794500000:3",
            _ => "1794500000:2",
        };
        let locked =
            self.variant().starts_with("a8") && self.variant().contains("lock") || self.j9_media();
        (locked && (key == "_edit_lock" || key == "_edit_last")).then_some(lock)
    }

    /// A variant of `reference/posts-list/golden.sh`, over its content.
    fn listing(&self) -> bool {
        [
            "posts", "hidden", "excerpt", "front", "h9locks", "h9home", "h9host",
        ]
        .contains(&self.variant())
    }

    /// A variant of `reference/terms-list/golden.sh` over its content.
    fn terms_content(&self) -> bool {
        ["terms", "paged", "stale"].contains(&self.variant())
    }

    /// Every term of `taxonomy` on the variant's site, by name.
    fn all_terms(&self, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        if self.terms_content() {
            return crate::terms::content::terms(taxonomy);
        }
        if self.listing() {
            return crate::posts::content::terms(taxonomy);
        }
        crate::posts::PostsSite::post_terms(self, &crate::posts::tests::hello(), taxonomy)
    }

    /// The user options the profile variants of
    /// `reference/profile/variants.php` give, and the nickname and roles.
    fn profile_option(&self, name: &str) -> Option<Vec<u8>> {
        if let Some(value) = personal_option(name).filter(|_| self.variant() == "poptions") {
            return Some(value.to_vec());
        }
        let value: &[u8] = match (self.variant(), name) {
            ("poptions", "nickname") => b"boss",
            (_, "nickname") => b"admin",
            ("ppending", "_new_email") => {
                b"a:2:{s:4:\"hash\";s:1:\"x\";s:8:\"newemail\";s:16:\"new@example.test\";}"
            }
            ("pcaps", "capabilities") => {
                b"a:3:{s:13:\"administrator\";b:1;s:10:\"edit_posts\";b:1;s:6:\"custom\";b:0;}"
            }
            (_, "capabilities") => {
                return Some(
                    format!("a:1:{{s:{}:\"{}\";b:1;}}", self.role().len(), self.role())
                        .into_bytes(),
                );
            }
            _ => return None,
        };
        Some(value.to_vec())
    }

    /// The options the variants of `reference/settings/variants.php` and
    /// `reference/settings-more/variants.php` give.
    fn variant_option(&self, name: &str) -> Option<&'static [u8]> {
        let value: &[u8] = match (self.variant(), name) {
            // `WP_SITEURL` and `WP_HOME`.
            (_, "siteurl" | "home") => SITE.as_bytes(),
            ("s1static" | "s1same" | "s1privacy" | "s1unset" | "s1hidden", "show_on_front") => {
                b"page"
            }
            ("s1icon", "site_icon") => b"5",
            ("h9icon", "site_icon") => b"951",
            ("h9iconbare", "site_icon") => b"952",
            ("h9iconpdf", "site_icon") => b"953",
            ("h9iconnone", "site_icon") => b"954",
            ("h9iconbig", "site_icon") => b"955",
            ("a5auto", "auto_update_themes") => b"a:1:{i:0;s:16:\"twentytwentyfive\";}",
            ("g9parent", "template") => b"twentytwentyfour",
            ("g9gone" | "g9noswitch" | "g9nooptions", "stylesheet" | "template")
            | ("g9kid", "template") => b"gone",
            ("g9kid" | "g9child", "stylesheet") => b"kid",
            ("g9pages" | "g9cpages" | "g9coldest", "page_comments" | "comments_per_page") => b"1",
            ("g9coldest", "default_comments_page") => b"oldest",
            ("s1static" | "s1same" | "s1privacy", "page_on_front") => b"2",
            ("s1same", "page_for_posts") => b"2",
            ("s1privacy", "page_for_posts") => b"3",
            ("s1private", "blog_public") => b"0",
            ("s1private", "rss_use_excerpt") => b"1",
            ("s1general", "blogname") => b"My <b>\"Site\"</b> & Co",
            ("s1general", "blogdescription") => b"Tag 'line' &amp; more",
            ("s1general", "date_format") => b"j F Y",
            ("s1general", "time_format") => b"H:i",
            ("s1general", "start_of_week") => b"0",
            ("s1general", "users_can_register") => b"1",
            ("s1general", "default_role") => b"author",
            ("s1general", "new_admin_email") => b"new<b>@example.test",
            ("s1general", "timezone_string") => b"America/New_York",
            ("s1offset" | "s1plus", "timezone_string") => b"",
            ("s1offset", "gmt_offset") => b"-5.5",
            ("s1offset", "date_format") => b"Y-m-d",
            ("s1offset", "time_format") => b"g:i A",
            ("s1etc", "timezone_string") => b"Etc/GMT+3",
            ("s1etc", "date_format") => b"",
            ("s1kolkata", "timezone_string") => b"Asia/Kolkata",
            ("s1kolkata", "default_role") => b"editor",
            ("s1calcutta", "timezone_string") => b"Asia/Calcutta",
            ("s1calcutta", "new_admin_email") => b"admin@default-install.test",
            ("s1utc", "timezone_string") => b"UTC",
            ("s1utc", "start_of_week") => b"6",
            ("s1plus", "gmt_offset") => b"5.75",
            ("s1writing", "initial_db_version") => b"30000",
            ("s1db", "initial_db_version") => b"32453",
            ("s1writing", "use_smilies" | "use_balanceTags" | "link_manager_enabled") => b"1",
            ("s1writing", "default_category") => b"99",
            ("s1writing", "default_post_format") => b"aside",
            ("s1writing", "mailserver_url") => b"mail.example.test",
            ("s1writing", "mailserver_port") => b"995",
            ("s1writing", "mailserver_login") => b"me@example.test",
            ("s1writing", "mailserver_pass") => b"p\"w&<x>",
            ("s1writing", "ping_sites") => b"http://a.test/\nhttp://b.test/?x=<y>&z",
            ("s2discussion", "default_ping_status" | "default_comment_status") => b"closed",
            (
                "s2discussion",
                "default_pingback_flag"
                | "require_name_email"
                | "show_comments_cookies_opt_in"
                | "thread_comments"
                | "comments_notify"
                | "moderation_notify"
                | "wp_notes_notify"
                | "comment_previously_approved",
            )
            | ("s2noavatars", "show_avatars")
            | ("s2media", "thumbnail_crop" | "uploads_use_yearmonth_folders" | "medium_size_h") => {
                b"0"
            }
            (
                "s2discussion",
                "comment_registration"
                | "close_comments_for_old_posts"
                | "page_comments"
                | "comment_moderation",
            ) => b"1",
            ("s2discussion", "close_comments_days_old") => b"7\"",
            ("s2discussion", "thread_comments_depth") => b"10",
            ("s2discussion", "comments_per_page") => b"20",
            ("s2discussion", "default_comments_page") => b"oldest",
            ("s2discussion", "comment_order") => b"desc",
            ("s2discussion", "comment_max_links") => b"5",
            ("s2discussion", "moderation_keys") => b"spam\n<b>&amp; \"x\"",
            ("s2discussion", "disallowed_keys") => b"viagra\n'y' & </textarea>",
            ("s2discussion", "avatar_rating") => b"PG",
            ("s2discussion", "avatar_default") => b"identicon",
            ("s2discussion", "show_avatars") => b"2",
            ("s2noavatars", "avatar_rating") => b"X",
            ("s2noavatars", "avatar_default") => b"gravatar_default",
            ("s2noavatars", "thread_comments_depth") => b"3x",
            ("s2media", "thumbnail_size_w") => b"100\"",
            ("s2media", "thumbnail_size_h") => b"80",
            ("s2media", "medium_size_w") => b"400",
            ("s2media", "large_size_w") => b"2048",
            ("s2media", "large_size_h") => b"1536",
            ("s2media", "upload_path") => b"files/<up>",
            ("s2urlpath" | "s2uploads", "upload_path") => b"wp-content/uploads",
            ("s2urlpath", "upload_url_path") => b"http://cdn.test/a\"b",
            ("s2day", "permalink_structure") => b"/%year%/%monthnum%/%day%/%postname%/",
            ("s2month", "permalink_structure") => b"/%year%/%monthnum%/%postname%/",
            ("s2numeric", "permalink_structure") => b"/archives/%post_id%",
            ("s2postname" | "j9pretty" | "j9prettyfront", "permalink_structure") => b"/%postname%/",
            ("s2custom", "permalink_structure") => b"/%category%/<%postname%>/",
            ("s2custom", "category_base") => b"topics",
            ("s2custom", "tag_base") => b"labels\"<",
            ("s2published", "wp_page_for_privacy_policy") => b"2",
            ("s2unset", "wp_page_for_privacy_policy") => b"0",
            ("s2missing", "wp_page_for_privacy_policy") => b"99",
            ("s2trashed", "wp_page_for_privacy_policy") => b"98",
            _ => return None,
        };
        Some(value)
    }

    /// The REST API's path, pretty with a permalink structure.
    fn rest_path(&self) -> &'static str {
        match self.variant_option("permalink_structure") {
            Some(_) => "wp-json/",
            None => "index.php?rest_route=/",
        }
    }

    /// A variant of `reference/user-edit/golden.sh`, over the users of
    /// `reference/users-list/content.sql`.
    fn edited(&self) -> bool {
        self.variant().starts_with("u3")
    }

    /// A user other than 1 in a `reference/user-edit/golden.sh` variant.
    fn other_user(&self, id: u64) -> Option<crate::users::ListedUser> {
        let users = crate::users::content::users(self.edited());
        users
            .into_iter()
            .find(|listed| listed.user.id == id && id != 1)
    }

    /// Another user's options, as `reference/user-edit/variants.php` gives
    /// them in the variant.
    fn other_option(&self, id: u64, name: &str) -> Option<Vec<u8>> {
        let listed = self.other_user(id)?;
        let options = (self.variant(), id) == ("u3options", 601);
        if let Some(value) = personal_option(name).filter(|_| options) {
            return Some(value.to_vec());
        }
        let value: &[u8] = match (self.variant(), id, name) {
            ("u3options", 601, "nickname") => b"eddie",
            (_, _, "nickname") => &listed.user.login,
            ("u3caps", 601, "capabilities") => {
                b"a:3:{s:6:\"editor\";b:1;s:10:\"edit_users\";b:1;s:6:\"custom\";b:0;}"
            }
            ("u3roles", 604, "capabilities") => b"a:1:{s:11:\"contributor\";b:1;}",
            ("u3roles", 606, "capabilities") => b"a:1:{s:13:\"administrator\";b:1;}",
            (_, _, "capabilities") if !listed.capabilities.is_empty() => &listed.capabilities,
            _ => return None,
        };
        Some(value.to_vec())
    }

    /// A variant of `reference/users-list/golden.sh` over its content.
    fn users_content(&self) -> bool {
        ["users", "upaged", "uhidden"].contains(&self.variant())
    }

    /// A variant of `reference/comments-list/golden.sh` over its content.
    fn comments_content(&self) -> bool {
        [
            "comments",
            "cpaged",
            "chidden",
            "g9cpages",
            "g9coldest",
            "g9attach",
        ]
        .contains(&self.variant())
    }

    /// The rows of a variant that pages comments, with their pages in their
    /// links, as `get_comment_link()` gives them on the comments list.
    fn paged(
        &self,
        mut rows: Vec<crate::dashboard::RecentComment>,
    ) -> Vec<crate::dashboard::RecentComment> {
        if !["g9pages", "g9cpages", "g9coldest"].contains(&self.variant()) {
            return rows;
        }
        let option = |name: &str| crate::posts::PostsSite::option(self, name).unwrap_or_default();
        let all: Vec<_> = rows
            .iter()
            .map(|row| {
                (
                    row.comment.clone(),
                    crate::dashboard::approved_of(row.status).to_string(),
                )
            })
            .collect();
        for row in &mut rows {
            let on_post: Vec<_> = all
                .iter()
                .filter(|(comment, _)| comment.post_id == row.comment.post_id)
                .map(|(comment, approved)| (comment, approved.as_str()))
                .collect();
            row.link = crate::comments::comment_link(
                &option,
                row.post.as_ref().map(|_| row.permalink.as_str()),
                "/hs-admin/edit-comments",
                &on_post,
                &row.comment,
                1,
            );
        }
        rows
    }

    /// The fixture's one comment, trashed when the list is empty, or the
    /// content's.
    fn comment_rows(&self) -> Vec<crate::dashboard::RecentComment> {
        if self.comments_content() {
            return self.paged(crate::comments::content::comments(self));
        }
        let mut hello = crate::dashboard::tests::hello_comment(self.can("edit_published_posts"));
        if self.variant() == "cempty" {
            hello.status = "trash";
        }
        self.paged(vec![hello])
    }

    /// The posts and pages of the variant.
    fn posts(&self) -> Vec<hyscribe_model::Post> {
        if self.ttd() {
            return crate::media::content::posts();
        }
        let mut posts = crate::posts::content::posts(self.listing(), self.variant() == "empty");
        if self.a8() {
            posts.extend(crate::media::attachment::content::posts());
            posts.extend(crate::media::attachment::content::unported());
        }
        match self.variant() {
            "s1nopages" | "s1hidden" => posts.retain(|post| post.post_type != "page"),
            // `reference/settings/content.sql`.
            "s1media" => posts.extend([("2025-03-04", "inherit"), ("2024-12-01", "trash")].map(
                |(day, status)| hyscribe_model::Post {
                    post_type: "attachment".into(),
                    status: status.into(),
                    date: format!("{day} 10:00:00"),
                    ..hyscribe_model::Post::default()
                },
            )),
            "a5zero" => posts.push(hyscribe_model::Post {
                post_type: "attachment".into(),
                status: "inherit".into(),
                date: "0000-00-00 00:00:00".into(),
                ..hyscribe_model::Post::default()
            }),
            "a5requests" => posts.push(hyscribe_model::Post {
                post_type: "user_request".into(),
                status: "request-pending".into(),
                ..hyscribe_model::Post::default()
            }),
            _ => {}
        }
        posts
    }
}

/// The site with one post known to `map_meta_cap()`.
struct WithPost<'a>(&'a Role, CapPost);

impl CapabilityData for WithPost<'_> {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        CapabilityData::option(self.0, name)
    }
    fn user_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.0.user_meta(id, key)
    }
    fn user_exists(&self, id: u64) -> bool {
        self.0.user_exists(id)
    }
    fn post(&self, id: u64) -> Option<CapPost> {
        (id == self.1.id).then(|| self.1.clone())
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.0.post_meta(id, key)
    }
    fn comment_post(&self, id: u64) -> Option<u64> {
        self.0.comment_post(id)
    }
    fn term_taxonomy(&self, id: u64) -> Option<String> {
        self.0.term_taxonomy(id)
    }
    fn constant(&self, name: &str) -> bool {
        self.0.constant(name)
    }
}

impl CapabilityData for Role {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        let set = self.variant_option(name).map(<[u8]>::to_vec);
        set.or_else(|| options().get(name).cloned())
    }
    /// User 1's role, less the capability a `u3no` variant denies, and the
    /// other users' as `reference/user-edit/variants.php` gives them.
    fn user_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        if key != "wp_capabilities" {
            return None;
        }
        if id != 1 && self.edited() {
            return self.other_option(id, "capabilities");
        }
        let role = self.role();
        let denied = match self.variant() {
            "u3nocreate" => "create_users",
            "u3nopromote" => "promote_users",
            "a5nooptions" | "g9nooptions" => "edit_theme_options",
            "a5noswitch" | "g9noswitch" => "switch_themes",
            "j9noupload" => "upload_files",
            _ => return Some(format!("a:1:{{s:{}:\"{role}\";b:1;}}", role.len()).into_bytes()),
        };
        Some(
            format!(
                "a:2:{{s:{}:\"{role}\";b:1;s:{}:\"{denied}\";b:0;}}",
                role.len(),
                denied.len()
            )
            .into_bytes(),
        )
    }
    fn user_exists(&self, id: u64) -> bool {
        id == 1
            || self.listing() && id == 2
            || self.other_user(id).is_some()
            || self.a8_user(id).is_some()
    }
    fn post(&self, _: u64) -> Option<CapPost> {
        None
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        let post = hyscribe_model::Post {
            id,
            ..hyscribe_model::Post::default()
        };
        crate::posts::content::meta(&post, key).filter(|_| self.listing())
    }
    fn comment_post(&self, _: u64) -> Option<u64> {
        None
    }
    fn term_taxonomy(&self, id: u64) -> Option<String> {
        ["category", "post_tag", "post_format"]
            .into_iter()
            .find(|taxonomy| self.all_terms(taxonomy).iter().any(|term| term.id == id))
            .map(String::from)
    }
    fn constant(&self, name: &str) -> bool {
        name == "DISALLOW_UNFILTERED_HTML"
    }
}

impl crate::terms::TermsSite for Role {
    fn list_terms(&self, query: &hyscribe_model::TermQuery) -> Vec<hyscribe_model::Term> {
        TERM_QUERIES.with_borrow_mut(|queries| queries.push(query.clone()));
        crate::terms::content::query(self.all_terms(&query.taxonomy), query)
    }
    fn count_terms(&self, taxonomy: &str, search: Option<&str>) -> u64 {
        let query = hyscribe_model::TermQuery {
            taxonomy: taxonomy.into(),
            search: search.map(String::from),
            ..hyscribe_model::TermQuery::default()
        };
        crate::terms::content::query(self.all_terms(taxonomy), &query).len() as u64
    }
    fn term_link(&self, term: &hyscribe_model::Term, taxonomy: &str) -> String {
        match taxonomy {
            "category" => format!("{SITE}/?cat={}", term.id),
            _ => format!("{SITE}/?tag={}", String::from_utf8_lossy(&term.slug)),
        }
    }
}

impl MenuUser for Role {
    fn can(&self, cap: &str) -> bool {
        Capabilities::load(self, "wp_", 1).has_cap(self, cap, &[])
    }
    fn moderated_comments(&self) -> u64 {
        match self.variant() {
            "content" => 1,
            "g9attach" => 2,
            _ if self.comments_content() => 1,
            _ if self.listing() => 3,
            _ => 0,
        }
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
    /// The site URL, HTTPS in the `h9ssl` variant.
    fn site_url(&self) -> String {
        if self.variant() == "h9ssl" {
            return SITE.replace("http:", "https:");
        }
        SITE.into()
    }
    fn admin_url(&self, path: &str) -> String {
        format!("{SITE}/hs-admin/{}", link(path))
    }
    fn fill(&self, text: &str) -> String {
        let text = admin_links(text, "%admin_url%", &format!("{SITE}/hs-admin/"));
        admin_links(&text, "\"%admin_path%", "\"/hs-admin/")
            .replace(
                "%rest_url%",
                &format!("{}/{}", self.home(), self.rest_path()),
            )
            .replace("%site_url%", SITE)
            .replace("%user_id%", "0")
            .replace(
                "%nonce:reset-password-for-user%",
                "%nonce:reset-password-for-0%",
            )
    }
}

impl PageSite for Role {
    fn user_option(&self, name: &str) -> Option<Vec<u8>> {
        let edit: &[u8] = match (self.variant(), name) {
            ("a8locklayout" | "a8ttdlocklayout", "screen_layout_attachment") => b"1",
            ("a8locklayout" | "a8ttdlocklayout", "metaboxhidden_attachment") => {
                b"a:1:{i:0;s:16:\"commentstatusdiv\";}"
            }
            ("a8locklayout" | "a8ttdlocklayout", "closedpostboxes_attachment") => {
                b"a:1:{i:0;s:7:\"slugdiv\";}"
            }
            ("a8locklayout" | "a8ttdlocklayout", "meta-box-order_attachment") => {
                b"a:2:{s:4:\"side\";s:19:\"authordiv,submitdiv\";s:6:\"normal\";s:7:\"slugdiv\";}"
            }
            ("a8locknorich", "rich_editing") => b"false",
            (variant, "rich_editing") if variant.starts_with("a8") || self.j9_media() => b"true",
            _ => b"",
        };
        if !edit.is_empty() {
            return Some(edit.to_vec());
        }
        match (self.variant(), name) {
            ("m4list" | "m4ttdlist", "media_library_mode") => return Some(b"list".to_vec()),
            ("m4ttdpaged", "upload_per_page") => return Some(b"5".to_vec()),
            ("m4ttdhidden", "manageuploadcolumnshidden") => {
                return Some(b"a:1:{i:0;s:6:\"author\";}".to_vec());
            }
            _ => {}
        }
        if self.variant() == "settings" && name == "user-settings" {
            return Some(b"mfold=f&posts_list_mode=excerpt".to_vec());
        }
        match (self.variant(), name) {
            ("g9keys", "comment_shortcuts") => return Some(b"true".to_vec()),
            ("f9folded", "user-settings") => return Some(b"mfold=f".to_vec()),
            ("f9unfold", "user-settings") => return Some(b"unfold=1&mfold=o".to_vec()),
            _ => {}
        }
        if self.variant() == "hidden" {
            let value: &[u8] = match name {
                "manageedit-postcolumnshidden" => b"a:2:{i:0;s:4:\"tags\";i:1;s:8:\"comments\";}",
                "edit_post_per_page" | "edit_page_per_page" => b"3",
                _ => return None,
            };
            return Some(value.to_vec());
        }
        if self.variant() == "a5hidden" {
            let value: &[u8] = match name {
                "manageexport-personal-datacolumnshidden" => b"a:1:{i:0;s:6:\"status\";}",
                "export_personal_data_requests_per_page" => b"50",
                "managepluginscolumnshidden" => b"a:1:{i:0;s:11:\"description\";}",
                "plugins_per_page" => b"10",
                _ => return None,
            };
            return Some(value.to_vec());
        }
        if let Some(value) = self.profile_option(name) {
            return Some(value);
        }
        if self.variant() == "upaged" && name == "users_per_page" {
            return Some(b"2".to_vec());
        }
        if self.variant() == "uhidden" && name == "manageuserscolumnshidden" {
            return Some(b"a:1:{i:0;s:5:\"email\";}".to_vec());
        }
        if self.variant() == "cpaged" && name == "edit_comments_per_page" {
            return Some(b"2".to_vec());
        }
        if self.variant() == "chidden" && name == "manageedit-commentscolumnshidden" {
            return Some(b"a:1:{i:0;s:8:\"response\";}".to_vec());
        }
        if self.variant() == "paged" {
            let value: &[u8] = match name {
                "edit_category_per_page" | "edit_post_tag_per_page" => b"3",
                "manageedit-categorycolumnshidden" | "manageedit-post_tagcolumnshidden" => {
                    b"a:1:{i:0;s:11:\"description\";}"
                }
                _ => return None,
            };
            return Some(value.to_vec());
        }
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
    fn settings_cookie(&self) -> Option<String> {
        None
    }
    fn user_id(&self) -> u64 {
        1
    }
    fn nonce(&self, action: &str) -> String {
        format!("%nonce:{action}%")
    }
    fn admin_color(&self) -> Option<Vec<u8>> {
        self.profile_option("admin_color")
            .or_else(|| Some(b"modern".to_vec()))
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
        let template = CapabilityData::option(self, "template").unwrap_or_default();
        String::from_utf8(template).unwrap()
    }
    fn stylesheet(&self) -> String {
        let stylesheet = CapabilityData::option(self, "stylesheet").unwrap_or_default();
        String::from_utf8(stylesheet).unwrap()
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
        post_type: "post".into(),
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

impl crate::users::UsersSite for Role {
    fn user_roles(&self) -> Option<Vec<u8>> {
        options().get("wp_user_roles").cloned()
    }
    /// Those of the listed users who have any on the site.
    fn all_capabilities(&self) -> Vec<Vec<u8>> {
        crate::users::content::users(self.users_content())
            .into_iter()
            .map(|listed| listed.capabilities)
            .filter(|caps| !caps.is_empty())
            .collect()
    }
    /// The users of the variant as `query` filters and sorts them, cases
    /// aside as MySQL compares.
    fn list_users(
        &self,
        query: &crate::users::UserQuery<'_>,
    ) -> (Vec<crate::users::ListedUser>, u64) {
        let lower = |bytes: &[u8]| String::from_utf8_lossy(bytes).to_lowercase();
        let mut found: Vec<_> = crate::users::content::users(self.users_content())
            .into_iter()
            .filter(|listed| {
                let user = &listed.user;
                let caps = lower(&listed.capabilities);
                query
                    .roles
                    .iter()
                    .all(|role| caps.contains(&format!("\"{role}\"")))
                    && (query.include.is_empty() || query.include.contains(&user.id))
                    && query.search.is_none_or(|(columns, text)| {
                        columns.iter().any(|column| match *column {
                            "ID" => user.id.to_string() == text,
                            "user_login" => lower(&user.login).contains(&text.to_lowercase()),
                            "user_email" => lower(&user.email).contains(&text.to_lowercase()),
                            "user_url" => lower(&user.url).contains(&text.to_lowercase()),
                            "user_nicename" => lower(&user.nicename).contains(&text.to_lowercase()),
                            _ => lower(&user.display_name).contains(&text.to_lowercase()),
                        })
                    })
            })
            .collect();
        let key = |listed: &crate::users::ListedUser, column: &str| {
            let user = &listed.user;
            match column {
                "ID" => (String::new(), user.id),
                "post_count" => (String::new(), listed.posts),
                // The fixture's administrator registered first.
                "user_registered" => (String::new(), u64::from(user.id != 1)),
                "user_email" => (lower(&user.email), 0),
                "display_name" => (lower(&user.display_name), 0),
                _ => (lower(&user.login), 0),
            }
        };
        found.sort_by(|a, b| {
            query
                .order
                .iter()
                .map(|(column, desc)| {
                    let ordering = key(a, column).cmp(&key(b, column));
                    if *desc { ordering.reverse() } else { ordering }
                })
                .find(|ordering| ordering.is_ne())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let total = found.len() as u64;
        let skip = usize::try_from(query.offset).unwrap();
        let take = usize::try_from(query.number).unwrap();
        (found.into_iter().skip(skip).take(take).collect(), total)
    }
    /// Those with capabilities naming none of the roles.
    fn users_without_role(&self, regex: &str) -> Vec<u64> {
        let roles: Vec<&str> = regex.split('|').collect();
        crate::users::content::users(self.users_content())
            .into_iter()
            .filter(|listed| {
                let caps = String::from_utf8_lossy(&listed.capabilities).to_lowercase();
                !caps.is_empty() && !roles.iter().any(|role| caps.contains(role))
            })
            .map(|listed| listed.user.id)
            .collect()
    }
    fn author_url(&self, id: u64) -> String {
        format!("{SITE}/?author={id}")
    }
}

impl crate::tools::ToolsSite for Role {
    /// User 1 wrote the posts and pages, and nobody the media.
    fn post_authors(&self, post_type: &str) -> Vec<u64> {
        if post_type == "attachment" {
            Vec::new()
        } else {
            vec![1]
        }
    }
}

impl crate::media::MediaSite for Role {
    fn attachment_mime_types(&self) -> Vec<String> {
        if ["m4types", "m4ttdtypes"].contains(&self.variant()) {
            return ["image/jpeg", "application/pdf", "application/vnd.ms-excel"]
                .into_iter()
                .chain(["application/zip", "text/plain"])
                .map(String::from)
                .collect();
        }
        let mut types: Vec<String> = self
            .posts()
            .into_iter()
            .filter(|post| post.post_type == "attachment" && !post.mime_type.is_empty())
            .map(|post| post.mime_type)
            .collect();
        types.sort();
        types.dedup();
        types
    }
    fn slug_taken(&self, slug: &[u8], id: u64) -> bool {
        let posts = self.posts();
        posts.iter().any(|post| post.id != id && post.name == slug)
    }
}

impl crate::settings::SettingsSite for Role {
    /// The fixture's `wp-config.php` sets both addresses.
    fn defined(&self, name: &str) -> bool {
        ["WP_HOME", "WP_SITEURL"].contains(&name) && self.variant() != "s1undefined"
    }
    fn random_key(&self) -> String {
        "%password%".into()
    }
}

impl crate::profile::ProfileSite for Role {
    /// The fixture's administrator, named in the options variant, or
    /// another user of the variant.
    fn profile_user(&self, id: u64) -> Option<hyscribe_model::User> {
        if id != 1 && (self.variant().starts_with("a8") || self.j9_media()) {
            return self.a8_user(id);
        }
        if id != 1 {
            let mut user = self.other_user(id)?.user;
            if self.variant() == "u3options" {
                user.description = b"Bio <b>bold</b> & more".to_vec();
            }
            return Some(user);
        }
        let named = self.variant() == "poptions";
        let value = |set: &[u8]| if named { set.to_vec() } else { Vec::new() };
        Some(hyscribe_model::User {
            id: 1,
            login: b"admin".to_vec(),
            nicename: b"admin".to_vec(),
            email: b"admin@default-install.test".to_vec(),
            url: b"http://default-install.test".to_vec(),
            display_name: b"admin".to_vec(),
            first_name: value(b"Ad &amp; Min"),
            last_name: value(b"Istrator"),
            description: value(b"Bio <b>bold</b> & more"),
        })
    }
    fn user_option_of(&self, id: u64, name: &str) -> Option<Vec<u8>> {
        if self.a8() && (id, name) == (3, "nickname") {
            return Some(b"sub".to_vec());
        }
        if id == 1 {
            return PageSite::user_option(self, name);
        }
        self.other_option(id, name)
    }
    fn user_can(&self, id: u64, cap: &str) -> bool {
        Capabilities::load(self, "wp_", id).has_cap(self, cap, &[])
    }
    fn sessions(&self, id: u64) -> usize {
        match (self.variant(), id) {
            ("psessions1", 1) | ("u3sessions", 601) => 1,
            ("psessions2", 1) => 2,
            _ => 0,
        }
    }
    /// The goldens' mask of the generated password.
    fn generated_password(&self) -> String {
        "%password%".into()
    }
    /// A path on the site, or else the fallback.
    fn validate_redirect(&self, location: &[u8], fallback: &str) -> Vec<u8> {
        if location.starts_with(b"/") || location.starts_with(SITE.as_bytes()) {
            location.to_vec()
        } else {
            fallback.as_bytes().to_vec()
        }
    }
}

impl crate::comments::CommentsSite for Role {
    /// `comment_rows()` as `query` filters them.
    fn list_comments(
        &self,
        query: &crate::comments::CommentQuery<'_>,
    ) -> (Vec<crate::dashboard::RecentComment>, u64) {
        COMMENTS_ASKED.set(query.number);
        let all = self.comment_rows();
        let lower = |bytes: &[u8]| String::from_utf8_lossy(bytes).to_lowercase();
        let mut found: Vec<_> = all
            .into_iter()
            .filter(|row| {
                let c = &row.comment;
                let fields = [
                    &c.author,
                    &c.author_email,
                    &c.author_url,
                    &c.author_ip,
                    &c.content,
                ];
                (query.post == 0 || c.post_id == query.post)
                    && query
                        .approved
                        .contains(&crate::dashboard::approved_of(row.status))
                    && query
                        .user
                        .is_none_or(|user| i64::try_from(c.user_id) == Ok(user))
                    && (query.types.is_empty()
                        || query
                            .types
                            .contains(&&*String::from_utf8_lossy(&c.comment_type)))
                    && query.search.is_none_or(|search| {
                        fields
                            .iter()
                            .any(|field| lower(field).contains(&search.to_lowercase()))
                    })
            })
            .collect();
        // As MySQL sorts the columns the goldens sort by, case aside.
        let key = |row: &crate::dashboard::RecentComment, column: &str| {
            let c = &row.comment;
            match column {
                "comment_author" => (lower(&c.author), 0),
                "comment_post_ID" => (String::new(), c.post_id),
                "comment_ID" => (String::new(), c.id),
                "comment_date" | "comment_date_gmt" => (c.date.clone(), 0),
                _ => (String::new(), 0),
            }
        };
        found.sort_by(|a, b| {
            query
                .order
                .iter()
                .map(|(column, desc)| {
                    let ordering = key(a, column).cmp(&key(b, column));
                    if *desc { ordering.reverse() } else { ordering }
                })
                .find(|ordering| ordering.is_ne())
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        let total = found.len() as u64;
        let skip = usize::try_from(query.offset).unwrap();
        let take = usize::try_from(query.number).unwrap();
        (found.into_iter().skip(skip).take(take).collect(), total)
    }
    /// The spam comment was approved before.
    fn approved_before(&self, id: u64) -> bool {
        id == 504
    }
    /// Counted from the comments on `post`.
    fn post_comment_counts(&self, post: u64) -> crate::dashboard::CommentCounts {
        if self.a8() && post == 801 {
            return crate::dashboard::CommentCounts {
                approved: 1,
                ..crate::dashboard::CommentCounts::default()
            };
        }
        let rows: Vec<_> = self
            .comment_rows()
            .into_iter()
            .filter(|row| row.comment.post_id == post)
            .collect();
        let count = |status: &str| rows.iter().filter(|row| row.status == status).count() as u64;
        crate::dashboard::CommentCounts {
            approved: count("approved"),
            moderated: count("unapproved"),
            spam: count("spam"),
            trash: count("trash"),
            mine: rows
                .iter()
                .filter(|row| {
                    row.comment.user_id == self.user_id()
                        && matches!(row.status, "approved" | "unapproved")
                })
                .count() as u64,
        }
    }
    fn parent_comment(&self, id: u64) -> Option<(String, Vec<u8>)> {
        let rows = self.comment_rows();
        let parent = rows.into_iter().find(|row| row.comment.id == id)?;
        Some((parent.link, parent.comment.author))
    }
    /// Comment 1's status, as `reference/notices/variants.php` gives it.
    fn comment_approved(&self, id: u64) -> Option<String> {
        let status = match self.variant() {
            "n6trash" => "trash",
            "n6spam" => "spam",
            "n6pending" => "0",
            _ => "1",
        };
        (id == 1).then(|| status.into())
    }
    /// The content has a pingback; the empty list only a trashed comment.
    fn has_comments(&self, types: &[&str]) -> bool {
        types.contains(&"comment") && self.variant() != "cempty" || self.comments_content()
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
        if self.comments_content() {
            // g9attach adds two approved comments and a pending one.
            let attached = u64::from(self.variant() == "g9attach");
            return crate::dashboard::CommentCounts {
                approved: 6 + 2 * attached,
                moderated: 1 + attached,
                spam: 1,
                trash: 1,
                mine: 1,
            };
        }
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
                approved: u64::from(!matches!(self.variant(), "empty" | "pending" | "cempty")),
                trash: u64::from(self.variant() == "cempty"),
                moderated: u64::from(self.variant() == "pending"),
                ..crate::dashboard::CommentCounts::default()
            }
        }
    }
    fn recent_comments(
        &self,
        _: Option<&hyscribe_model::Post>,
    ) -> Vec<crate::dashboard::RecentComment> {
        // `edit_comment` is `edit_post` of the comment's post, user 1's.
        let can_edit = self.can("edit_published_posts");
        let hello = crate::dashboard::tests::hello_comment(can_edit);
        if matches!(self.variant(), "empty" | "dates") {
            return Vec::new();
        }
        if self.variant() != "content" {
            return self.paged(vec![hello]);
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
}

impl crate::posts::PostsSite for Role {
    fn can_for(&self, cap: &str, id: u64) -> bool {
        let id = hyscribe_phpser::Value::Int(i64::try_from(id).unwrap());
        Capabilities::load(self, "wp_", 1).has_cap(self, cap, &[id])
    }
    fn can_post(&self, cap: &str, post: &hyscribe_model::Post) -> bool {
        // `reference/attachment-edit/variants.php` takes these away.
        let denied = match self.variant() {
            "a8locknodelete" => "delete_post",
            "a8locknoread" => "read_post",
            _ => "",
        };
        if (cap, post.id) == (denied, 801) {
            return false;
        }
        let known = WithPost(
            self,
            CapPost {
                id: post.id,
                author: post.author,
                status: post.status.clone(),
                post_type: post.post_type.clone(),
                parent: post.parent,
            },
        );
        let id = hyscribe_phpser::Value::Int(i64::try_from(post.id).unwrap());
        Capabilities::load(self, "wp_", 1).has_cap(&known, cap, &[id])
    }
    fn permalink(&self, post: &hyscribe_model::Post) -> String {
        if self.variant() == "front" && post.id == 316 {
            return format!("{SITE}/");
        }
        let home = self.home();
        // Pretty links of published posts and pages, as their slugs give.
        let pretty = self.variant().starts_with("j9pretty") && post.post_type != "attachment";
        if pretty && post.status == "publish" {
            return format!("{home}/{}/", String::from_utf8_lossy(&post.name));
        }
        let query = match post.post_type.as_str() {
            "page" => "page_id",
            "attachment" => "attachment_id",
            _ => "p",
        };
        format!("{home}/?{query}={}", post.id)
    }
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        if name == "sticky_posts" && (self.listing() || self.variant() == "empty") {
            return Some(b"a:2:{i:0;i:304;i:1;i:306;}".to_vec());
        }
        if self.variant() == "front" {
            let value: &[u8] = match name {
                "show_on_front" => b"page",
                "page_on_front" => b"316",
                "page_for_posts" => b"321",
                _ => b"",
            };
            if !value.is_empty() {
                return Some(value.to_vec());
            }
        }
        if let Some(icon) = self.variant_option(name).filter(|_| name == "site_icon") {
            return Some(icon.to_vec());
        }
        match (self.variant(), name) {
            ("a8lockcount" | "a8lockref" | "a8lockrefpost", "user_count") => {
                return Some(b"2".to_vec());
            }
            (variant, "user_count") if variant.starts_with("a8ttd") => return Some(b"3".to_vec()),
            ("j9prettyfront", "show_on_front") => return Some(b"page".to_vec()),
            ("j9prettyfront", "page_on_front") => return Some(b"2".to_vec()),
            ("m4ttdpages", "wp_attachment_pages_enabled") => return Some(b"1".to_vec()),
            ("m4large", "large_size_h") => return Some(b"0".to_vec()),
            // Postponed at the clock's time, or long ago.
            ("f9remind", "admin_email_lifespan") => return Some(b"1794759200".to_vec()),
            ("f9old", "admin_email_lifespan") => return Some(b"1".to_vec()),
            ("f9edge", "admin_email_lifespan") => return Some(b"1794759140".to_vec()),
            ("m4ttdcron", "cron") => {
                return Some(b"a:1:{i:1;a:1:{s:26:\"upgrader_scheduled_cleanup\";a:1:{s:1:\"k\";a:1:{s:4:\"args\";a:1:{i:0;i:611;}}}}}".to_vec());
            }
            ("m4ttdstates", "theme_mods_twentytwentyfive") => {
                return Some(b"a:1:{s:11:\"custom_logo\";i:1692;}".to_vec());
            }
            _ => {}
        }
        if name == "category_children" && self.variant() == "stale" {
            return Some(b"a:0:{}".to_vec());
        }
        if name == "show_avatars" && ["noavatars", "u3noavatars"].contains(&self.variant())
            || name == "blog_public" && self.variant() == "layout"
        {
            return Some(b"0".to_vec());
        }
        if name == "default_role" && self.variant() == "u3default" {
            return Some(b"editor".to_vec());
        }
        CapabilityData::option(self, name)
    }
    fn clock(&self) -> (hyscribe_render::date::Zone, i64) {
        if self.variant() == "dst" {
            // 2026-11-01 00:30 in New York, the day its clocks go back.
            let zone = hyscribe_render::date::site_zone(b"America/New_York", b"");
            return (zone, 1_793_507_400);
        }
        (hyscribe_render::date::Zone::Utc, 1_794_500_000)
    }
    fn post_class(&self, class: &str, post: &hyscribe_model::Post) -> String {
        crate::posts::content::post_class(class, post)
    }
    /// The fixture's Uncategorized for posts 1 to 99.
    fn post_terms(&self, post: &hyscribe_model::Post, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        if self.listing() {
            return crate::posts::content::post_terms(post, taxonomy);
        }
        if taxonomy != "category" || post.id > 99 || post.post_type != "post" {
            return Vec::new();
        }
        vec![hyscribe_model::Term {
            id: 1,
            name: b"Uncategorized".to_vec(),
            slug: b"uncategorized".to_vec(),
            count: 1,
            ..hyscribe_model::Term::default()
        }]
    }
    fn meta(&self, post: &hyscribe_model::Post, key: &str) -> Option<Vec<u8>> {
        if self.variant().starts_with("h9icon") {
            return icon_meta(post.id, key);
        }
        if let Some(lock) = self.a8_lock(key) {
            return Some(lock.as_bytes().to_vec());
        }
        if self.a8() {
            return crate::media::attachment::content::meta(post.id, key);
        }
        // The edit locks `reference/posts-list/golden.sh` adds.
        let lock: &[u8] = match (self.variant(), post.id, key) {
            ("h9locks", 301 | 307, "_edit_lock") => b"9999999999:2",
            ("h9locks", 302, "_edit_lock") => b"9999999999:1",
            ("h9locks", 303, "_edit_lock") => b"1000:2",
            ("h9locks", 304, "_edit_lock") => b"9999999999",
            ("h9locks", 304, "_edit_last") => b"2",
            ("h9locks", 305, "_edit_lock") => b"9999999999:99",
            _ => b"",
        };
        if !lock.is_empty() {
            return Some(lock.to_vec());
        }
        if self.variant() == "m4ttdrecent" && post.id == 1690 && key == "_thumbnail_id" {
            return Some(b"611".to_vec());
        }
        if self.ttd() {
            return crate::media::content::meta(post.id, key);
        }
        crate::posts::content::meta(post, key).filter(|_| self.listing())
    }
    fn userdata(&self, id: u64) -> Option<hyscribe_model::User> {
        if self.ttd() {
            return crate::media::content::user(id);
        }
        self.authors("edit_posts")
            .into_iter()
            .find(|user| user.id == id)
    }
    fn pending_comments(&self, post: &hyscribe_model::Post) -> u64 {
        u64::from(
            self.listing() && [309, 311, 321].contains(&post.id)
                || self.comments_content() && post.id == 1
                || self.variant() == "g9attach" && post.id == 805,
        )
    }
    /// `get_the_excerpt()` of the content's plain text.
    fn excerpt(&self, post: &hyscribe_model::Post) -> Vec<u8> {
        if !post.password.is_empty() {
            return b"There is no excerpt because this is a protected post.".to_vec();
        }
        if !post.excerpt.is_empty() {
            return post.excerpt.clone();
        }
        hyscribe_render::wp_trim_words(&post.content, 55, b" [&hellip;]")
    }
    fn terms(&self, taxonomy: &str) -> Vec<hyscribe_model::Term> {
        TERM_READS.set(TERM_READS.get() + 1);
        self.all_terms(taxonomy)
    }
    fn popular_terms(&self, taxonomy: &str) -> Vec<u64> {
        let mut terms = crate::posts::PostsSite::terms(self, taxonomy);
        terms.retain(|term| term.count > 0);
        terms.sort_by_key(|term| std::cmp::Reverse(term.count));
        terms.iter().take(10).map(|term| term.id).collect()
    }
    /// The content's author may edit posts, not pages.
    fn authors(&self, cap: &str) -> Vec<hyscribe_model::User> {
        if self.variant().starts_with("a8") || self.j9_media() {
            return [1, 2]
                .into_iter()
                .filter_map(|id| self.a8_user(id))
                .collect();
        }
        let mut authors = crate::posts::content::authors();
        if !self.listing() || cap == "edit_pages" {
            authors.truncate(1);
        }
        authors
    }
    fn list_posts(&self, query: &hyscribe_model::PostQuery) -> (Vec<hyscribe_model::Post>, u64) {
        if self.variant() == "s2nopages" && query.post_types == ["page"] {
            return (Vec::new(), 0);
        }
        if self.variant() == "s2orphan" && query.post_types == ["page"] {
            let orphan = hyscribe_model::Post {
                id: 60,
                parent: 999,
                title: b"Orphan".to_vec(),
                ..hyscribe_model::Post::default()
            };
            return (vec![orphan], 1);
        }
        if query.post_types == ["attachment"] {
            MEDIA_QUERIES.with_borrow_mut(|queries| queries.push(query.clone()));
            if self.variant() == "m4ttdtrash" {
                let mut post = crate::posts::PostsSite::post(self, 611).unwrap();
                post.status = "trash".into();
                return (vec![post], 1);
            }
            let (mut posts, found) = crate::media::content::list(self.posts(), query);
            // `reference/media/variants.php`'s newest upload, the user's two
            // hours ago; or attached to a template.
            let now = crate::posts::PostsSite::clock(self).1;
            let zone = hyscribe_render::date::Zone::Utc;
            let date = hyscribe_render::date::format_unix("Y-m-d H:i:s", now - 7200, zone);
            for post in posts.iter_mut().filter(|p| p.id == 1692) {
                match self.variant() {
                    "m4ttdrecent" => (post.date, post.author) = (date.clone(), 1),
                    "m4ttdtemplate" => post.parent = 9999,
                    _ => {}
                }
            }
            return (posts, found);
        }
        crate::posts::content::list(self.posts(), query)
    }
    fn status_counts(&self, post_type: &str, private_author: Option<u64>) -> Vec<(String, u64)> {
        crate::posts::content::status_counts(self.posts(), post_type, private_author)
    }
    fn own_posts(&self, post_type: &str) -> u64 {
        let own = |post: &hyscribe_model::Post| {
            post.post_type == post_type && post.author == 1 && post.status != "trash"
        };
        self.posts().iter().filter(|post| own(post)).count() as u64
    }
    fn sticky_count(&self, _: &str, ids: &[u64]) -> u64 {
        if self.listing() { ids.len() as u64 } else { 0 }
    }
    fn months(&self, post_type: &str, trash: bool) -> Vec<(u32, u32)> {
        crate::posts::content::months(self.posts(), post_type, trash)
    }
    fn post(&self, id: u64) -> Option<hyscribe_model::Post> {
        if self.variant().starts_with("h9icon") && (951..=955).contains(&id) {
            return Some(hyscribe_model::Post {
                id,
                post_type: "attachment".into(),
                mime_type: if id < 953 || id == 955 {
                    "image/png"
                } else {
                    "application/pdf"
                }
                .into(),
                ..hyscribe_model::Post::default()
            });
        }
        if self.variant() == "m4ttdtemplate" && id == 9999 {
            return Some(hyscribe_model::Post {
                id,
                post_type: "wp_template".into(),
                ..hyscribe_model::Post::default()
            });
        }
        if self.variant() == "n6media" && id == 9 {
            return Some(hyscribe_model::Post {
                id,
                author: 1,
                post_type: "attachment".into(),
                status: "inherit".into(),
                ..hyscribe_model::Post::default()
            });
        }
        if self.variant() == "s2trashed" && id == 98 {
            return Some(hyscribe_model::Post {
                id,
                post_type: "page".into(),
                status: "trash".into(),
                ..hyscribe_model::Post::default()
            });
        }
        if self.comments_content() {
            let attached = self.variant() == "g9attach" && [801, 802, 805].contains(&id);
            return ([1, 2, 501].contains(&id) || attached)
                .then(|| crate::comments::content::post(id));
        }
        self.posts().into_iter().find(|post| post.id == id)
    }
    fn page_templates(&self, post_type: &str) -> Vec<(String, String)> {
        if post_type != "page" {
            return Vec::new();
        }
        vec![("page-no-title".into(), "Page No Title".into())]
    }
    fn post_formats(&self) -> &'static [&'static str] {
        static THEME: OnceLock<hyscribe_render::theme::Theme> = OnceLock::new();
        THEME
            .get_or_init(hyscribe_render::theme::twentytwentyfive)
            .post_formats()
    }
}

/// The post meta of the site icons `reference/site-icon/content.sql` adds.
fn icon_meta(id: u64, key: &str) -> Option<Vec<u8>> {
    let value: &[u8] = match (id, key) {
        (951, "_wp_attached_file") => b"2026/11/icon.png",
        (951, "_wp_attachment_metadata") => b"a:6:{s:5:\"width\";i:512;s:6:\"height\";i:512;s:4:\"file\";s:16:\"2026/11/icon.png\";s:8:\"filesize\";i:9000;s:5:\"sizes\";a:6:{s:6:\"medium\";a:4:{s:4:\"file\";s:16:\"icon-300x300.png\";s:5:\"width\";i:300;s:6:\"height\";i:300;s:9:\"mime-type\";s:9:\"image/png\";}s:9:\"thumbnail\";a:4:{s:4:\"file\";s:16:\"icon-150x150.png\";s:5:\"width\";i:150;s:6:\"height\";i:150;s:9:\"mime-type\";s:9:\"image/png\";}s:13:\"site_icon-270\";a:4:{s:4:\"file\";s:16:\"icon-270x270.png\";s:5:\"width\";i:270;s:6:\"height\";i:270;s:9:\"mime-type\";s:9:\"image/png\";}s:13:\"site_icon-192\";a:4:{s:4:\"file\";s:16:\"icon-192x192.png\";s:5:\"width\";i:192;s:6:\"height\";i:192;s:9:\"mime-type\";s:9:\"image/png\";}s:13:\"site_icon-180\";a:4:{s:4:\"file\";s:16:\"icon-180x180.png\";s:5:\"width\";i:180;s:6:\"height\";i:180;s:9:\"mime-type\";s:9:\"image/png\";}s:12:\"site_icon-32\";a:4:{s:4:\"file\";s:14:\"icon-32x32.png\";s:5:\"width\";i:32;s:6:\"height\";i:32;s:9:\"mime-type\";s:9:\"image/png\";}}s:10:\"image_meta\";a:0:{}}",
        (952, "_wp_attached_file") => b"2026/11/tiny.png",
        (952, "_wp_attachment_metadata") => b"a:6:{s:5:\"width\";i:40;s:6:\"height\";i:40;s:4:\"file\";s:16:\"2026/11/tiny.png\";s:8:\"filesize\";i:100;s:5:\"sizes\";a:0:{}s:10:\"image_meta\";a:0:{}}",
        (953, "_wp_attached_file") => b"2026/11/doc.pdf",
        (953, "_wp_attachment_metadata") => b"a:2:{s:8:\"filesize\";i:5000;s:5:\"sizes\";a:2:{s:4:\"full\";a:4:{s:4:\"file\";s:11:\"doc-pdf.jpg\";s:5:\"width\";i:1088;s:6:\"height\";i:1408;s:9:\"mime-type\";s:10:\"image/jpeg\";}s:9:\"thumbnail\";a:4:{s:4:\"file\";s:19:\"doc-pdf-116x150.jpg\";s:5:\"width\";i:116;s:6:\"height\";i:150;s:9:\"mime-type\";s:10:\"image/jpeg\";}}}",
        (954, "_wp_attached_file") => b"2026/11/bare.pdf",
        (954, "_wp_attachment_metadata") => b"a:1:{s:8:\"filesize\";i:5000;}",
        (955, "_wp_attached_file") => b"2026/11/big.png",
        (955, "_wp_attachment_metadata") => {
            b"a:4:{s:5:\"width\";i:1024;s:6:\"height\";i:1024;s:4:\"file\";s:15:\"2026/11/big.png\";s:5:\"sizes\";a:1:{s:13:\"site_icon-512\";a:3:{s:4:\"file\";s:15:\"big-512x512.png\";s:5:\"width\";i:512;s:6:\"height\";i:512;}}}"
        }
        _ => return None,
    };
    Some(value.to_vec())
}
