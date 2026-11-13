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

thread_local! {
    /// The term queries the test site was asked, and how often it read
    /// every term of a taxonomy, on this thread.
    pub static TERM_QUERIES: std::cell::RefCell<Vec<hyscribe_model::TermQuery>> =
        std::cell::RefCell::default();
    pub static TERM_READS: std::cell::Cell<usize> = std::cell::Cell::default();
    /// How many comments the test site was last asked for.
    pub static COMMENTS_ASKED: std::cell::Cell<u64> = std::cell::Cell::default();
}

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

    /// A variant of `reference/posts-list/golden.sh`, over its content.
    fn listing(&self) -> bool {
        ["posts", "hidden", "excerpt", "front"].contains(&self.variant())
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

    /// A variant of `reference/comments-list/golden.sh` over its content.
    fn comments_content(&self) -> bool {
        ["comments", "cpaged", "chidden"].contains(&self.variant())
    }

    /// The posts and pages of the variant.
    fn posts(&self) -> Vec<hyscribe_model::Post> {
        crate::posts::content::posts(self.listing(), self.variant() == "empty")
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
        options().get(name).cloned()
    }
    fn user_meta(&self, _: u64, key: &str) -> Option<Vec<u8>> {
        (key == "wp_capabilities").then(|| {
            format!("a:1:{{s:{}:\"{}\";b:1;}}", self.role().len(), self.role()).into_bytes()
        })
    }
    fn user_exists(&self, id: u64) -> bool {
        id == 1 || self.listing() && id == 2
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
    fn can_term(&self, cap: &str, term: &hyscribe_model::Term) -> bool {
        let id = hyscribe_phpser::Value::Int(i64::try_from(term.id).unwrap());
        Capabilities::load(self, "wp_", 1).has_cap(self, cap, &[id])
    }
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

impl crate::comments::CommentsSite for Role {
    /// The fixture's one comment, trashed when the list is empty, or the
    /// content's, as `query` filters them.
    fn list_comments(
        &self,
        query: &crate::comments::CommentQuery<'_>,
    ) -> (Vec<crate::dashboard::RecentComment>, u64) {
        COMMENTS_ASKED.set(query.number);
        let all = if self.comments_content() {
            crate::comments::content::comments(self)
        } else {
            let mut hello =
                crate::dashboard::tests::hello_comment(self.can("edit_published_posts"));
            if self.variant() == "cempty" {
                hello.status = "trash";
            }
            vec![hello]
        };
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
                query
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
    fn parent_comment(&self, id: u64) -> Option<(String, Vec<u8>)> {
        (id == 1).then(|| {
            let hello = crate::dashboard::tests::hello_comment(false);
            (hello.link, hello.comment.author)
        })
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
            return crate::dashboard::CommentCounts {
                approved: 6,
                moderated: 1,
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
    fn can_post(&self, cap: &str, post: &hyscribe_model::Post) -> bool {
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
        let query = if post.post_type == "page" {
            "page_id"
        } else {
            "p"
        };
        format!("{SITE}/?{query}={}", post.id)
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
        if name == "category_children" && self.variant() == "stale" {
            return Some(b"a:0:{}".to_vec());
        }
        if name == "show_avatars" && self.variant() == "noavatars"
            || name == "blog_public" && self.variant() == "layout"
        {
            return Some(b"0".to_vec());
        }
        options().get(name).cloned()
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
        crate::posts::content::meta(post, key).filter(|_| self.listing())
    }
    fn display_name(&self, id: u64) -> Option<Vec<u8>> {
        self.authors("edit_posts")
            .into_iter()
            .find(|user| user.id == id)
            .map(|user| user.display_name)
    }
    fn pending_comments(&self, post: &hyscribe_model::Post) -> u64 {
        u64::from(
            self.listing() && [309, 311, 321].contains(&post.id)
                || self.comments_content() && post.id == 1,
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
        let mut authors = crate::posts::content::authors();
        if !self.listing() || cap == "edit_pages" {
            authors.truncate(1);
        }
        authors
    }
    fn user_option(&self, name: &str) -> Option<Vec<u8>> {
        if self.variant() == "settings" && name == "user-settings" {
            return Some(b"mfold=f&posts_list_mode=excerpt".to_vec());
        }
        if self.variant() == "hidden" {
            let value: &[u8] = match name {
                "manageedit-postcolumnshidden" => b"a:2:{i:0;s:4:\"tags\";i:1;s:8:\"comments\";}",
                "edit_post_per_page" | "edit_page_per_page" => b"3",
                _ => return None,
            };
            return Some(value.to_vec());
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
    fn list_posts(&self, query: &hyscribe_model::PostQuery) -> (Vec<hyscribe_model::Post>, u64) {
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
