// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin's screens at `/hs-admin/` (ADR 0008): `auth_redirect()` before
//! any of them; screens not ported yet answer `501`.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::Response;
use hyscribe_admin::admin_bar::{BarSite, Browser};
use hyscribe_admin::appearance::themes;
use hyscribe_admin::assets::Urls;
use hyscribe_admin::comments::{
    CommentArgs, CommentQuery, CommentsSite, MODERATED, comment_order, edit_comments,
};
use hyscribe_admin::dashboard::{
    CommentCounts, DashboardSite, RecentComment, comment_status, dashboard,
};
use hyscribe_admin::login::Site as LoginSite;
use hyscribe_admin::media::{LibraryArgs, MediaSite, NewArgs, media_new, upload};
use hyscribe_admin::menu::MenuUser;
use hyscribe_admin::page::{PageSite, REMOVABLE, Request, query_strings};
use hyscribe_admin::posts::{
    Answer, BULK, ListArgs, PostType, PostsSite, edit, kept_ids, sorted_ids,
};
use hyscribe_admin::profile::{EditArgs, ProfileSite, profile, user_edit};
use hyscribe_admin::settings::{SettingsSite, general, reading, writing};
use hyscribe_admin::settings_more;
use hyscribe_admin::terms::{Taxonomy as Terms, TermArgs, TermsSite, edit_tags};
use hyscribe_admin::tools::{self, Requests, ToolsSite, privacy_requests};
use hyscribe_admin::users::{ListedUser, UserArgs, UserQuery, UsersSite, user_order, users};
use hyscribe_auth::caps::{CapPost, CapabilityData, User as Capabilities, roles_with};
use hyscribe_db::{CommentFilter, StatusOrder, StatusQuery, UserFilter};
use hyscribe_model::{Post, PostQuery, Term, TermOrder, TermQuery, User};
use hyscribe_phpser::{
    Key, Value, addslashes, is_empty_string, parse_str, remove_query_arg, stripslashes, urlencode,
};
use hyscribe_render::comment_author_of;
use hyscribe_render::date::Zone;
use hyscribe_render::site::{Archive, Site as RenderSite, Taxonomy};
use tokio::runtime::Handle;

use crate::login::{LoginReads, Names, PATH, cookie, cookies, is_ssl, joined, nocache, response};
use crate::redirect::wp_sanitize_redirect;
use crate::{Site, now};

/// Whether `path` is an hs-admin screen: its files have an extension and
/// are served, or missing, as Apache serves `wp-admin`'s.
pub(crate) fn is_admin(path: &str) -> bool {
    let screen = path
        .rsplit('/')
        .next()
        .is_none_or(|name| !name.contains('.'));
    (path == "/hs-admin" || path.starts_with("/hs-admin/")) && screen
}

/// The answer to a request for an admin screen.
pub(crate) async fn respond(
    site: &Arc<Site>,
    uri: Uri,
    headers: HeaderMap,
    post: bool,
) -> Response {
    let site = Arc::clone(site);
    let (now, handle) = (now(), Handle::current());
    joined(
        tokio::task::spawn_blocking(move || answer(&site, &uri, &headers, post, now, handle)).await,
    )
}

/// A `302` to `location`, as `wp_redirect()` sends it.
fn redirect(mut headers: Vec<(header::HeaderName, String)>, location: &[u8]) -> Response {
    let location = String::from_utf8_lossy(&wp_sanitize_redirect(location)).into_owned();
    headers.push((
        header::HeaderName::from_static("x-redirect-by"),
        "WordPress".into(),
    ));
    headers.push((header::LOCATION, location));
    response(StatusCode::FOUND, headers, Vec::new())
}

/// A logged in user's answer: over HTTPS if their `use_ssl` asks for it.
fn admitted(https: bool, use_ssl: bool, here: &[u8], serve: impl FnOnce() -> Response) -> Response {
    if !https && use_ssl {
        redirect(nocache(), &[&b"https://"[..], here].concat())
    } else {
        serve()
    }
}

/// A logged in user in the admin, on a blocking thread.
struct Admin<'a> {
    reads: &'a LoginReads<'a>,
    site: &'a Site,
    caps: Capabilities,
    user: hyscribe_model::User,
    /// The posts read, each once, as WordPress's object cache keeps them.
    posts: RefCell<HashMap<u64, Option<hyscribe_model::Post>>>,
    comment_counts: OnceCell<CommentCounts>,
    /// The user's `wp-settings-{ID}` cookie, if sent.
    settings: Option<String>,
}

impl<'a> Admin<'a> {
    fn new(reads: &'a LoginReads<'a>, site: &'a Site, user: hyscribe_model::User) -> Admin<'a> {
        Admin {
            caps: Capabilities::load(reads, site.db.prefix(), user.id),
            reads,
            site,
            user,
            posts: RefCell::default(),
            comment_counts: OnceCell::new(),
            settings: None,
        }
    }

    /// Each user's `{prefix}capabilities`, by user ID.
    fn capabilities(&self) -> Vec<(u64, Vec<u8>)> {
        let key = format!("{}capabilities", self.site.db.prefix());
        self.reads.reads.wait(self.site.db.users_meta(&key))
    }

    /// A comment with what its rows show, `post` the one it is read with.
    fn comment_row(
        &self,
        comment: hyscribe_model::Comment,
        approved: &str,
        post: Option<hyscribe_model::Post>,
    ) -> RecentComment {
        let permalink = post
            .as_ref()
            .map(|post| self.permalink(post))
            .unwrap_or_default();
        RecentComment {
            link: format!("{permalink}#comment-{}", comment.id),
            permalink,
            status: comment_status(approved),
            user: (comment.user_id != 0)
                .then(|| self.reads.reads.wait(self.site.db.user(comment.user_id)))
                .flatten(),
            can_edit: self.comment_editable(post.as_ref()),
            post: post.filter(|_| comment.post_id != 0),
            comment,
        }
    }

    /// `edit_comment` of a comment on `post`: `edit_posts` without one.
    fn comment_editable(&self, post: Option<&hyscribe_model::Post>) -> bool {
        post.map_or_else(
            || self.can("edit_posts"),
            |post| self.can_for("edit_post", post.id),
        )
    }

    fn full_post(&self, id: u64) -> Option<hyscribe_model::Post> {
        if let Some(post) = self.posts.borrow().get(&id) {
            return post.clone();
        }
        let post = self.reads.reads.wait(self.site.db.post(id));
        self.posts.borrow_mut().insert(id, post.clone());
        post
    }

    fn posts(
        &self,
        status: &str,
        author: Option<u64>,
        order: StatusOrder,
        limit: u32,
    ) -> Vec<hyscribe_model::Post> {
        let query = StatusQuery {
            status,
            author,
            order,
            limit,
        };
        self.reads.reads.wait(self.site.db.posts_by_status(&query))
    }
}

/// The admin's capability checks, with the posts and comments they name.
impl CapabilityData for Admin<'_> {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        CapabilityData::option(self.reads, name)
    }
    fn user_meta(&self, user_id: u64, key: &str) -> Option<Vec<u8>> {
        self.reads.user_meta(user_id, key)
    }
    fn user_exists(&self, user_id: u64) -> bool {
        self.reads.user_exists(user_id)
            || self.reads.reads.wait(self.site.db.user(user_id)).is_some()
    }
    fn post(&self, id: u64) -> Option<CapPost> {
        self.full_post(id).map(|post| CapPost {
            id: post.id,
            author: post.author,
            status: post.status,
            post_type: post.post_type,
            parent: post.parent,
        })
    }
    fn post_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.reads.reads.wait(self.site.db.post_meta(id, key))
    }
    /// The dashboard checks a comment through its post (`comment_editable`).
    fn comment_post(&self, _: u64) -> Option<u64> {
        None
    }
    fn term_taxonomy(&self, id: u64) -> Option<String> {
        self.reads.reads.wait(self.site.db.term_taxonomy(id))
    }
    fn constant(&self, name: &str) -> bool {
        self.reads.constant(name)
    }
}

impl MenuUser for Admin<'_> {
    fn can(&self, cap: &str) -> bool {
        self.caps.has_cap(self, cap, &[])
    }
    fn moderated_comments(&self) -> u64 {
        DashboardSite::comment_counts(self).moderated
    }
    /// `get_transient()` without its timeout: the transients read here
    /// never expire.
    fn transient(&self, name: &str) -> Option<Vec<u8>> {
        self.reads.reads.option(&format!("_transient_{name}"))
    }
}

impl BarSite for Admin<'_> {
    fn home_url(&self) -> String {
        LoginSite::home_url(self.reads)
    }
    fn admin_url(&self, path: &str) -> String {
        Urls::admin_url(self.reads, path)
    }
    fn blogname(&self) -> String {
        self.reads.name()
    }
    fn logout_url(&self) -> String {
        let nonce = LoginSite::nonce(self.reads, "log-out");
        self.reads
            .login_url(&format!("?action=logout&amp;_wpnonce={nonce}"))
    }
    fn names(&self) -> (String, String) {
        let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
        (text(&self.user.display_name), text(&self.user.login))
    }
    fn avatar(&self, size: i64) -> String {
        let avatar =
            hyscribe_render::user_avatar(&|name| self.reads.reads.option(name), &self.user, size);
        String::from_utf8_lossy(&avatar).into_owned()
    }
}

impl Urls for Admin<'_> {
    fn site_url(&self) -> String {
        self.reads.site_url()
    }
    fn admin_url(&self, path: &str) -> String {
        Urls::admin_url(self.reads, path)
    }
    fn fill(&self, text: &str) -> String {
        self.reads.fill(text)
    }
}

impl PageSite for Admin<'_> {
    fn user_id(&self) -> u64 {
        self.user.id
    }
    fn nonce(&self, action: &str) -> String {
        LoginSite::nonce(self.reads, action)
    }
    fn admin_color(&self) -> Option<Vec<u8>> {
        self.reads.user_option(self.user.id, "admin_color")
    }
    fn preferences(&self) -> Option<Vec<u8>> {
        let key = format!("{}persisted_preferences", self.site.db.prefix());
        self.reads.user_meta(self.user.id, &key)
    }
    fn compression_tested(&self) -> bool {
        self.reads.reads.option("can_compress_scripts").is_some()
    }
    fn login_url(&self) -> String {
        LoginSite::login_url(self.reads, "")
    }
    fn template(&self) -> String {
        self.site.option("template")
    }
    fn font_faces(&self) -> Vec<u8> {
        self.site.theme.admin_font_faces(&self.reads.site_url())
    }
}

impl DashboardSite for Admin<'_> {
    fn show_welcome_panel(&self) -> Option<Vec<u8>> {
        self.reads.user_meta(self.user.id, "show_welcome_panel")
    }
    fn user_email(&self) -> Vec<u8> {
        self.user.email.clone()
    }
    fn published(&self, post_type: &str) -> u64 {
        let query = hyscribe_model::PostQuery {
            post_types: vec![post_type.to_string()],
            ..hyscribe_model::PostQuery::default()
        };
        self.reads.reads.wait(self.site.db.count_posts(&query))
    }
    fn comment_counts(&self) -> CommentCounts {
        *self
            .comment_counts
            .get_or_init(|| self.post_comment_counts(0))
    }
    /// `wp_dashboard_recent_comments()`'s search: batches of the latest
    /// comments, kept while the user may see their post.
    fn recent_comments(&self, global: Option<&hyscribe_model::Post>) -> Vec<RecentComment> {
        let pending = self.can("edit_posts");
        let (mut offset, mut number) = (0, 25);
        let mut found = Vec::new();
        loop {
            let batch = self.reads.reads.wait(self.site.db.list_comments(
                &CommentFilter {
                    approved: if pending { &["0", "1"] } else { &["1"] },
                    user: None,
                    types: &[],
                    search: None,
                    post: 0,
                },
                &[("comment_date_gmt", true), ("comment_ID", true)],
                number,
                offset,
            ));
            if batch.is_empty() {
                return found;
            }
            for (comment, approved) in batch {
                let post = match comment.post_id {
                    0 => global.cloned(),
                    id => self.full_post(id),
                };
                let id = post.as_ref().map_or(0, |post| post.id);
                let protected = post.as_ref().is_some_and(|post| !post.password.is_empty());
                if !self.can_for("edit_post", id) && (protected || !self.can_for("read_post", id)) {
                    continue;
                }
                found.push(self.comment_row(comment, &approved, post));
                if found.len() == 5 {
                    return found;
                }
            }
            offset += number;
            number = 50;
        }
    }
    fn theme_name(&self) -> String {
        self.site.theme.name()
    }
    fn recent_posts(&self, status: &str) -> Vec<hyscribe_model::Post> {
        if status == "future" {
            // `perm => editable`: others' only for those who may edit them.
            let author = (!self.can("edit_others_posts")).then_some(self.user.id);
            return self.posts("future", author, StatusOrder::DateAsc, 5);
        }
        self.posts(status, None, StatusOrder::DateDesc, 5)
    }
    fn quick_draft(&self) -> Option<u64> {
        let stored = self
            .reads
            .user_option(self.user.id, "dashboard_quick_press_last_post_id")?;
        let id = u64::try_from(Value::String(stored).to_int()).unwrap_or_default();
        self.full_post(id)
            .filter(|post| post.status == "auto-draft")
            .map(|post| post.id)
    }
    fn drafts(&self) -> Vec<hyscribe_model::Post> {
        self.posts("draft", Some(self.user.id), StatusOrder::ModifiedDesc, 4)
    }
}

impl PostsSite for Admin<'_> {
    fn can_post(&self, cap: &str, post: &Post) -> bool {
        self.can_for(cap, post.id)
    }
    fn can_for(&self, cap: &str, id: u64) -> bool {
        let id = i64::try_from(id).unwrap_or_default();
        self.caps.has_cap(self, cap, &[Value::Int(id)])
    }
    fn permalink(&self, post: &Post) -> String {
        let link = hyscribe_render::permalinks::permalink(&self.reads.reads, post);
        String::from_utf8_lossy(&link).into_owned()
    }
    fn clock(&self) -> (Zone, i64) {
        (self.site.zone(), self.reads.now)
    }
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        CapabilityData::option(self.reads, name)
    }
    fn post_class(&self, class: &str, post: &Post) -> String {
        let classes = hyscribe_render::post::post_class(&self.reads.reads, None, class, post);
        String::from_utf8_lossy(&classes).into_owned()
    }
    fn post_terms(&self, post: &Post, taxonomy: &str) -> Vec<Term> {
        RenderSite::post_terms(&self.reads.reads, post.id, taxonomy)
    }
    fn meta(&self, post: &Post, key: &str) -> Option<Vec<u8>> {
        CapabilityData::post_meta(self, post.id, key)
    }
    fn display_name(&self, id: u64) -> Option<Vec<u8>> {
        let user = self.reads.reads.wait(self.site.db.user(id));
        user.map(|user| user.display_name)
    }
    fn pending_comments(&self, post: &Post) -> u64 {
        self.reads
            .reads
            .wait(self.site.db.pending_comments(post.id))
    }
    fn excerpt(&self, post: &Post) -> Vec<u8> {
        hyscribe_render::the_excerpt(&self.site.theme, &self.reads.reads, post)
    }
    fn terms(&self, taxonomy: &str) -> Vec<Term> {
        RenderSite::taxonomy_terms(&self.reads.reads, taxonomy)
    }
    fn popular_terms(&self, taxonomy: &str) -> Vec<u64> {
        let query = TermQuery {
            taxonomy: taxonomy.into(),
            nonempty: true,
            order: Some((TermOrder::Count, true)),
            limit: Some(10),
            ..TermQuery::default()
        };
        RenderSite::term_query(&self.reads.reads, &query)
            .iter()
            .map(|term| term.id)
            .collect()
    }
    fn authors(&self, cap: &str) -> Vec<User> {
        let prefix = self.site.db.prefix();
        let mut names = vec![cap.to_string()];
        names.extend(roles_with(self, prefix, cap));
        let key = format!("{prefix}capabilities");
        self.reads.reads.wait(self.site.db.users_with(&key, &names))
    }
    fn post_formats(&self) -> &'static [&'static str] {
        self.site.theme.post_formats()
    }
    fn list_posts(&self, query: &PostQuery) -> (Vec<Post>, u64) {
        let posts = self.reads.reads.wait(self.site.db.posts(query));
        let found = self.reads.reads.wait(self.site.db.count_posts(query));
        (posts, found)
    }
    fn status_counts(&self, post_type: &str, private_author: Option<u64>) -> Vec<(String, u64)> {
        let counts = self.site.db.status_counts(post_type, private_author);
        self.reads.reads.wait(counts)
    }
    fn own_posts(&self, post_type: &str) -> u64 {
        let count = self.site.db.own_posts(post_type, self.user.id);
        self.reads.reads.wait(count)
    }
    fn sticky_count(&self, post_type: &str, ids: &[u64]) -> u64 {
        self.reads
            .reads
            .wait(self.site.db.sticky_count(post_type, ids))
    }
    fn months(&self, post_type: &str, trash: bool) -> Vec<(u32, u32)> {
        self.reads
            .reads
            .wait(self.site.db.post_months(post_type, trash))
    }
    fn user_option(&self, name: &str) -> Option<Vec<u8>> {
        self.reads.user_option(self.user.id, name)
    }
    fn post(&self, id: u64) -> Option<Post> {
        self.full_post(id)
    }
    fn page_templates(&self, post_type: &str) -> Vec<(String, String)> {
        self.site.theme.page_templates(post_type)
    }
}

impl CommentsSite for Admin<'_> {
    fn list_comments(&self, query: &CommentQuery<'_>) -> (Vec<RecentComment>, u64) {
        let filter = CommentFilter {
            approved: query.approved,
            user: query.user,
            types: query.types,
            search: query.search,
            post: query.post,
        };
        let limit = u32::try_from(query.number).unwrap_or(u32::MAX);
        let offset = u32::try_from(query.offset).unwrap_or(u32::MAX);
        let found = self.reads.reads.wait(self.site.db.list_comments(
            &filter,
            query.order,
            limit,
            offset,
        ));
        let rows = found
            .into_iter()
            .map(|(comment, approved)| {
                let post = self.full_post(comment.post_id);
                self.comment_row(comment, &approved, post)
            })
            .collect();
        let total = self.reads.reads.wait(self.site.db.count_comments(&filter));
        (rows, total)
    }
    fn post_comment_counts(&self, post: u64) -> CommentCounts {
        let counts = self.site.db.comment_counts(self.user.id, post);
        let [approved, moderated, spam, trash, mine] = self.reads.reads.wait(counts);
        CommentCounts {
            approved,
            moderated,
            spam,
            trash,
            mine,
        }
    }
    fn approved_before(&self, id: u64) -> bool {
        let meta = self.site.db.comment_meta(id, "_wp_trash_meta_status");
        self.reads.reads.wait(meta).as_deref() == Some(b"1")
    }
    fn parent_comment(&self, id: u64) -> Option<(String, Vec<u8>)> {
        let comment = self.reads.reads.wait(self.site.db.any_comment(id))?;
        let post = self.full_post(comment.post_id);
        let row = self.comment_row(comment, "", post);
        let author = comment_author_of(&row.comment, row.user.as_ref());
        Some((row.link, author))
    }
    fn has_comments(&self, types: &[&str]) -> bool {
        self.reads.reads.wait(self.site.db.has_comment_types(types))
    }
    fn comment_approved(&self, id: u64) -> Option<String> {
        self.reads.reads.wait(self.site.db.comment_approved(id))
    }
}

impl UsersSite for Admin<'_> {
    fn user_roles(&self) -> Option<Vec<u8>> {
        self.reads
            .reads
            .option(&format!("{}user_roles", self.site.db.prefix()))
    }
    fn all_capabilities(&self) -> Vec<Vec<u8>> {
        self.capabilities()
            .into_iter()
            .map(|(_, caps)| caps)
            .collect()
    }
    fn list_users(&self, query: &UserQuery<'_>) -> (Vec<ListedUser>, u64) {
        let key = format!("{}capabilities", self.site.db.prefix());
        let filter = UserFilter {
            caps_key: &key,
            roles: query.roles,
            include: query.include,
            search: query.search,
            all_private: self.can("read_private_posts"),
            me: self.user.id,
        };
        let limit = u32::try_from(query.number).unwrap_or(u32::MAX);
        let offset = u32::try_from(query.offset).unwrap_or(u32::MAX);
        let users = self.site.db.list_users(&filter, query.order, limit, offset);
        let users = self.reads.reads.wait(users);
        let ids: Vec<u64> = users.iter().map(|user| user.id).collect();
        let counts = self
            .site
            .db
            .user_post_counts(&ids, filter.all_private, filter.me);
        let counts = self.reads.reads.wait(counts);
        let capabilities = self.capabilities();
        let found = |id: u64| {
            capabilities
                .iter()
                .find(|(user, _)| *user == id)
                .map(|(_, caps)| caps.clone())
                .unwrap_or_default()
        };
        let listed = users
            .into_iter()
            .map(|user| ListedUser {
                capabilities: found(user.id),
                posts: counts
                    .iter()
                    .find(|(author, _)| *author == user.id)
                    .map_or(0, |(_, n)| *n),
                user,
            })
            .collect();
        (
            listed,
            self.reads.reads.wait(self.site.db.count_users(&filter)),
        )
    }
    fn users_without_role(&self, regex: &str) -> Vec<u64> {
        let key = format!("{}capabilities", self.site.db.prefix());
        self.reads
            .reads
            .wait(self.site.db.users_without_role(&key, regex))
    }
    fn author_url(&self, id: u64) -> String {
        let id = i64::try_from(id).unwrap_or_default();
        let url = hyscribe_render::permalinks::author_posts_url(&self.reads.reads, id);
        String::from_utf8_lossy(&url).into_owned()
    }
}

impl ProfileSite for Admin<'_> {
    fn profile_user(&self, id: u64) -> Option<hyscribe_model::User> {
        self.reads.reads.wait(self.site.db.user(id))
    }
    fn user_option_of(&self, id: u64, name: &str) -> Option<Vec<u8>> {
        self.reads.user_option(id, name)
    }
    fn user_can(&self, id: u64, cap: &str) -> bool {
        Capabilities::load(self, self.site.db.prefix(), id).has_cap(self, cap, &[])
    }
    /// HyScribe's own sessions.
    fn sessions(&self, id: u64) -> usize {
        let sessions = self.site.sessions.as_ref();
        sessions.map_or(0, |sessions| sessions.count(id, self.reads.now))
    }
    fn generated_password(&self) -> String {
        hyscribe_auth::generate_password(24, true).unwrap_or_default()
    }
    fn validate_redirect(&self, location: &[u8], fallback: &str) -> Vec<u8> {
        let host = hyscribe_kses::url_host(self.site.home.as_bytes()).unwrap_or_default();
        crate::redirect::wp_validate_redirect(location, &host, fallback.as_bytes())
    }
}

impl ToolsSite for Admin<'_> {
    fn post_authors(&self, post_type: &str) -> Vec<u64> {
        self.reads.reads.wait(self.site.db.post_authors(post_type))
    }
}

impl MediaSite for Admin<'_> {
    fn attachment_mime_types(&self) -> Vec<String> {
        self.reads.reads.wait(self.site.db.mime_types("attachment"))
    }
}

impl SettingsSite for Admin<'_> {
    fn defined(&self, name: &str) -> bool {
        self.site.defined.contains(&name)
    }
    fn random_key(&self) -> String {
        hyscribe_auth::generate_password(8, false).unwrap_or_default()
    }
}

impl TermsSite for Admin<'_> {
    fn list_terms(&self, query: &TermQuery) -> Vec<Term> {
        RenderSite::term_query(&self.reads.reads, query)
    }
    fn count_terms(&self, taxonomy: &str, search: Option<&str>) -> u64 {
        let count = self.site.db.count_terms(taxonomy, search);
        self.reads.reads.wait(count)
    }
    fn term_link(&self, term: &Term, taxonomy: &str) -> String {
        let link = hyscribe_render::permalinks::term_link(&self.reads.reads, taxonomy, term);
        String::from_utf8_lossy(&link).into_owned()
    }
}

/// The terms list's arguments; `None` for one not ported yet.
fn term_args(query: &str) -> Option<TermArgs> {
    let (mut taxonomy, mut paged, mut search, mut orderby, mut order, mut message) =
        (None, None, None, None, None, None);
    let mut error = false;
    for (key, value) in query_strings(query)? {
        match key.as_str() {
            "taxonomy" => taxonomy = Some(Terms::named(value.as_bytes())?),
            "paged" => paged = Some(value),
            "s" => search = Some(value),
            "orderby" => orderby = Some(value),
            "order" => order = Some(value),
            "message" => message = Some(value),
            "error" => error = true,
            _ => return None,
        }
    }
    Some(TermArgs {
        taxonomy: taxonomy?,
        paged,
        search,
        orderby,
        order,
        message,
        error,
    })
}

/// The arguments of `edit-comments.php` ported so far; `None` for others.
fn comment_args(query: &str) -> Option<CommentArgs> {
    let mut args = CommentArgs::default();
    for (key, value) in query_strings(query)? {
        let value = Some(value);
        match key.as_str() {
            "comment_status" => args.status = value,
            "user_id" => args.user_id = value,
            "paged" => args.paged = value,
            "s" => args.search = value,
            "comment_type" => args.kind = value,
            "orderby" => args.orderby = value,
            "order" => args.order = value,
            "mode" => args.mode = value,
            "p" => args.post = value,
            "ids" => args.ids = value,
            "error" => args.error = value,
            // Sent back after unapproving, with no notice.
            "unapproved" => {}
            name => {
                let mut counts = MODERATED.iter().zip(&mut args.moderated);
                *counts.find(|(m, _)| **m == name)?.1 = value;
            }
        }
    }
    // The meta and `comment__in` orders are not ported.
    args.clauses = comment_order(
        args.orderby.as_deref().unwrap_or_default(),
        args.order.as_deref().unwrap_or_default(),
    )?;
    Some(args)
}

/// The arguments of `users.php` ported so far; `None` for others.
fn user_args(query: &str) -> Option<UserArgs> {
    let mut args = UserArgs::default();
    for (key, value) in query_strings(query)? {
        let value = Some(value);
        match key.as_str() {
            "role" => args.role = value,
            "s" => args.search = value,
            "orderby" => args.orderby = value,
            "order" => args.order = value,
            "paged" => args.paged = value,
            "update" => args.update = value,
            "delete_count" => args.delete_count = value,
            "reset_count" => args.reset_count = value,
            "id" => args.id = value,
            _ => return None,
        }
    }
    // The meta and list orders are not ported.
    let role = args.role.as_deref().unwrap_or_default();
    let roles = !is_empty_string(role.as_bytes()) && role != "none";
    args.clauses = user_order(args.orderby.as_deref(), args.order.as_deref(), roles)?;
    Some(args)
}

/// The arguments of `user-edit.php` and `profile.php` ported so far;
/// `None` for others.
fn edit_args(query: &str) -> Option<EditArgs> {
    let mut args = EditArgs::default();
    for (key, value) in query_strings(query)? {
        let value = Some(value);
        match key.as_str() {
            "user_id" => args.user_id = value,
            "wp_http_referer" => args.referer = value,
            "updated" => args.updated = true,
            "error" => args.error = value,
            _ => return None,
        }
    }
    Some(args)
}

/// The value of `name` in a query that may give only it: `None` for
/// another argument, `Some(None)` for none.
fn only_argument(query: &str, name: &str) -> Option<Option<String>> {
    let mut found = None;
    for (key, value) in query_strings(query)? {
        if key != name {
            return None;
        }
        found = Some(value);
    }
    Some(found)
}

/// The headers of an admin screen: `nocache_headers()` and
/// `send_frame_options_header()`; every response has `wp_admin_headers()`'s.
fn screen_headers() -> Vec<(header::HeaderName, String)> {
    let mut headers = nocache();
    headers.push((header::CONTENT_TYPE, "text/html; charset=UTF-8".to_string()));
    headers.push((header::X_FRAME_OPTIONS, "SAMEORIGIN".to_string()));
    headers
}

/// The posts list's term, date and search filters, which `main_query()`
/// reads.
const FILTERS: [&str; 8] = [
    "m",
    "cat",
    "category_name",
    "tag",
    "post_format",
    "s",
    "exact",
    "sentence",
];

/// `WP_Query`'s `cat` once cleaned: sorted integers unless that leaves it
/// empty.
fn cat_var(raw: &str) -> String {
    let kept = kept_ids(raw);
    if is_empty_string(kept.as_bytes()) {
        return kept;
    }
    let ids: Vec<String> = sorted_ids(&kept).iter().map(i64::to_string).collect();
    ids.join(",")
}

/// The posts list's arguments, with the filters of `main_query()` for
/// them; `None` for a query with arguments not ported yet.
fn list_args(admin: &Admin, request_uri: &str) -> Option<ListArgs> {
    let query = request_uri.split_once('?').map_or("", |(_, query)| query);
    let args = parse_str(query.as_bytes());
    let mut list = ListArgs {
        settings: admin.settings.clone(),
        ..ListArgs::default()
    };
    // `is_base_request()`: no argument but `paged`, or only the post type
    // or a view mode.
    let mut named = 0;
    let mut alone = false;
    let mut vars = HashMap::new();
    for (key, value) in args.as_array().unwrap_or_default() {
        if !matches!(key, Key::String(key) if key == b"paged") {
            named += 1;
        }
        match (key, value) {
            (Key::String(key), Value::String(value)) if key == b"post_status" => {
                list.status = String::from_utf8(value.clone()).ok()?;
            }
            (Key::String(key), Value::String(value)) if key == b"post_type" => {
                list.post_type = PostType::named(value)?;
                alone = true;
            }
            (Key::String(key), Value::String(value)) if key == b"mode" => {
                let mode = String::from_utf8(value.clone()).ok()?;
                alone = !is_empty_string(mode.as_bytes());
                list.mode = Some(mode);
            }
            (Key::String(key), Value::String(value)) if key == b"paged" => {
                list.paged = Some(String::from_utf8(value.clone()).ok()?);
            }
            (Key::String(key), Value::String(value))
                if [&b"author"[..], b"all_posts", b"show_sticky"].contains(&key.as_slice())
                    || FILTERS.iter().any(|name| name.as_bytes() == key) =>
            {
                let value = String::from_utf8(value.clone()).ok()?;
                let name = String::from_utf8_lossy(key).into_owned();
                match name.as_str() {
                    "author" => list.author = Some(value),
                    "all_posts" => list.all_posts = Some(value),
                    "show_sticky" => list.show_sticky = !is_empty_string(value.as_bytes()),
                    _ => {
                        match name.as_str() {
                            "m" => list.m = value.clone(),
                            "post_format" => list.post_format = value.clone(),
                            "cat" => list.cat = cat_var(&value),
                            "s" => list.search = Some(value.clone()),
                            _ => {}
                        }
                        vars.insert(name, value);
                    }
                }
            }
            (Key::String(key), Value::String(value)) if key == b"orderby" || key == b"order" => {
                let value = Some(String::from_utf8(value.clone()).ok()?);
                if key == b"orderby" {
                    list.orderby = value;
                } else {
                    list.order = value;
                }
            }
            // The arguments of a bulk action's notice; `skipped` has none.
            (Key::String(key), Value::String(value))
                if key == b"ids"
                    || key == b"skipped"
                    || BULK.iter().any(|b| b.as_bytes() == key) =>
            {
                let value = Some(String::from_utf8(value.clone()).ok()?);
                match BULK
                    .iter()
                    .zip(&mut list.counts)
                    .find(|(b, _)| b.as_bytes() == key)
                {
                    Some((_, count)) => *count = value,
                    None if key == b"ids" => list.ids = value,
                    None => {}
                }
            }
            _ => return None,
        }
    }
    list.base = named == 0 || named == 1 && alone;
    if !vars.is_empty() {
        let (query, resolved) = admin
            .reads
            .reads
            .wait(crate::main_query(admin.site, &vars, false));
        list.filters = PostQuery {
            terms: query.terms,
            year: query.year,
            month: query.month,
            day: query.day,
            time: query.time,
            search: query.search,
            exact: query.exact,
            sentence: query.sentence,
            search_count: query.search_count,
            // The admin's searches are a logged in user's.
            logged_in: true,
            ..PostQuery::default()
        };
        // `WP_Query` sets `cat` to the queried category.
        if let Some((Taxonomy::Category, term)) = resolved.archive.as_ref().and_then(Archive::term)
        {
            list.cat = term.id.to_string();
        }
    }
    Some(list)
}

/// The screen at `path` for `admin`, `None` for one not ported yet.
fn screen(
    admin: &Admin,
    path: &str,
    request_uri: &str,
    url: &str,
    origin: &str,
    browser: &Browser,
) -> Option<Response> {
    let canonical =
        String::from_utf8_lossy(&remove_query_arg(&REMOVABLE, url.as_bytes())).into_owned();
    let request = Request {
        canonical: &canonical,
        origin,
        uri: request_uri,
        browser,
    };
    let raw = String::from_utf8_lossy(&stripslashes(request_uri.as_bytes())).into_owned();
    let query = raw.split_once('?').map_or("", |(_, query)| query);
    // The Settings screens are sent back with `settings-updated` alone.
    let settings = || {
        let updated = only_argument(query, "settings-updated")?;
        // `settings_errors()` would print and delete what the save left.
        let left = [
            "_transient_settings_errors",
            "_transient_timeout_settings_errors",
        ]
        .iter()
        .any(|name| admin.reads.reads.option(name).is_some());
        let saved = updated.is_some_and(|updated| !Value::String(updated.into()).is_empty());
        (!saved || !left).then_some(())
    };
    let answer = match path {
        "/hs-admin/options-general" | "/hs-admin/options-writing" | "/hs-admin/options-reading" => {
            settings()?;
            match path {
                "/hs-admin/options-general" => general(admin, &request)?,
                "/hs-admin/options-writing" => writing(admin, &request),
                _ => reading(admin, &request)?,
            }
        }
        "/hs-admin/options-discussion"
        | "/hs-admin/options-media"
        | "/hs-admin/options-permalink"
        | "/hs-admin/options-privacy" => {
            settings()?;
            let screen = match path {
                "/hs-admin/options-discussion" => settings_more::discussion,
                "/hs-admin/options-media" => settings_more::media,
                "/hs-admin/options-permalink" => settings_more::permalink,
                _ => settings_more::privacy,
            };
            screen(admin, &request)
        }
        "/hs-admin/tools" => Answer::from_page(tools::tools(admin, &request)),
        // Their arguments and actions are not ported.
        "/hs-admin/themes"
        | "/hs-admin/import"
        | "/hs-admin/export"
        | "/hs-admin/export-personal-data"
        | "/hs-admin/erase-personal-data" => {
            if request_uri.contains('?') {
                return None;
            }
            match path {
                "/hs-admin/themes" => themes(admin, &request),
                "/hs-admin/import" => Some(tools::import(admin, &request)),
                "/hs-admin/export" => Some(tools::export(admin, &request)),
                "/hs-admin/export-personal-data" => {
                    privacy_requests(admin, Requests::Export, &request)
                }
                _ => privacy_requests(admin, Requests::Erase, &request),
            }?
        }
        // Their actions are not ported.
        "/hs-admin/upload" => upload(admin, &LibraryArgs::parse(query)?, &request)?,
        "/hs-admin/media-new" => {
            let args = NewArgs::parse(query, admin.settings.clone())?;
            media_new(admin, &args, &request)
        }
        // Their pages print the PHP and web server details HyScribe has no
        // equivalent for: refused as PHP refuses, else not ported.
        "/hs-admin/plugins" if !admin.can("activate_plugins") => Answer::from_page(None),
        "/hs-admin/site-health" if !admin.can("view_site_health_checks") => Answer::from_page(None),
        // Its page preloads REST responses not served yet, and its first view
        // writes the theme's `wp_global_styles` post.
        "/hs-admin/site-editor" if !admin.can("edit_theme_options") => Answer::from_page(None),
        // HTTPS offers application passwords, whose list is not ported.
        "/hs-admin/profile" => {
            // Only the arguments of a save.
            let args = edit_args(query).filter(|args| {
                args.user_id.is_none() && args.referer.is_none() && !origin.starts_with("https://")
            })?;
            profile(admin, &args, &request)
        }
        "/hs-admin/user-edit" => {
            let args = edit_args(query).filter(|_| !origin.starts_with("https://"))?;
            user_edit(admin, &args, &request)
        }
        "/hs-admin/user-new" => {
            let update = only_argument(query, "update")?;
            hyscribe_admin::user_new::user_new(admin, update.as_deref(), &request)
        }
        "/hs-admin/users" => users(admin, &user_args(query)?, &request),
        "/hs-admin/" | "/hs-admin/index" => Answer::from_page(dashboard(admin, &request)),
        "/hs-admin/edit-comments" => {
            let args = comment_args(query)?;
            // Only posts' and pages' comments are listed, and a draft's
            // Preview link is not ported.
            let post = args
                .post
                .as_deref()
                .map(|p| Value::String(p.into()).to_int());
            let listed = post.and_then(|id| PostsSite::post(admin, id.unsigned_abs()));
            if listed.is_some_and(|post| {
                !matches!(post.post_type.as_str(), "post" | "page") || post.status == "draft"
            }) {
                return None;
            }
            edit_comments(admin, &args, admin.settings.as_deref(), &request)?
        }
        "/hs-admin/edit-tags" => edit_tags(admin, &term_args(query)?, &request),
        "/hs-admin/edit" => {
            let args = list_args(admin, &raw)?;
            edit(admin, &args, &canonical, origin, request_uri, browser)?
        }
        _ => return None,
    };
    Some(served(admin, answer))
}

/// The response that serves a screen's answer.
fn served(admin: &Admin, answer: Answer) -> Response {
    match answer {
        Answer::Page(html) => response(StatusCode::OK, screen_headers(), html.into_bytes()),
        Answer::Redirect(to) => redirect(screen_headers(), to.as_bytes()),
        Answer::Denied(message) => denied(admin, message),
        Answer::Died(message) => died(admin, StatusCode::INTERNAL_SERVER_ERROR, message),
    }
}

/// `wp_die( $message, 403 )` on an admin screen.
fn denied(admin: &Admin, message: &str) -> Response {
    died(admin, StatusCode::FORBIDDEN, message)
}

/// `wp_die( $message )` with `status` on an admin screen.
fn died(admin: &Admin, status: StatusCode, message: &str) -> Response {
    let body = hyscribe_admin::die::page(admin.reads.public(), "WordPress &rsaquo; Error", message);
    response(status, screen_headers(), body)
}

fn answer(
    site: &Site,
    uri: &Uri,
    headers: &HeaderMap,
    post: bool,
    now: i64,
    handle: Handle,
) -> Response {
    let names = Names::new(site);
    let https = is_ssl(site);
    let cookies = cookies(
        headers
            .get_all(header::COOKIE)
            .iter()
            .map(HeaderValue::as_bytes),
    );
    let mut reads = LoginReads::new(site, handle, now);
    let user = reads.auth_user(cookie(&cookies, &names.auth(https)), post, https);
    // `$_SERVER['HTTP_HOST'] . $_SERVER['REQUEST_URI']`.
    let host = headers
        .get(header::HOST)
        .map_or(&b""[..], HeaderValue::as_bytes);
    // `wp_magic_quotes()` slashes `$_SERVER['REQUEST_URI']` too.
    let requested = addslashes(uri.path_and_query().map_or("", |p| p.as_str()).as_bytes());
    let requested = String::from_utf8_lossy(&requested).into_owned();
    let here = [host, requested.as_bytes()].concat();
    let scheme: &[u8] = if https { b"https://" } else { b"http://" };
    let answer = match user {
        Some((id, token)) => {
            let use_ssl = reads
                .user_option(id, "use_ssl")
                .is_some_and(|v| !Value::String(v).is_empty());
            admitted(https, use_ssl, &here, || {
                // The auth cookie's token, which `/hs-login` gives the logged
                // in cookie too: `wp_get_session_token()`'s.
                reads.act_as(id, token);
                let user = reads.reads.wait(site.db.user(id)).unwrap_or_default();
                let mut admin = Admin::new(&reads, site, user);
                let settings = cookie(&cookies, &format!("wp-settings-{id}"));
                admin.settings = settings.map(|value| String::from_utf8_lossy(value).into_owned());
                let header = |name: &str| headers.get(name).map(HeaderValue::as_bytes);
                let browser = Browser {
                    user_agent: header("user-agent").unwrap_or_default(),
                    mobile_hint: header("sec-ch-ua-mobile"),
                };
                let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
                let url = text(&[scheme, &here].concat());
                let origin = text(&[scheme, host].concat());
                // No screen's actions are ported.
                let page = (!post)
                    .then(|| screen(&admin, uri.path(), &requested, &url, &origin, &browser))
                    .flatten();
                page.unwrap_or_else(|| response(StatusCode::NOT_IMPLEMENTED, nocache(), Vec::new()))
            })
        }
        None => {
            // `wp_login_url( $redirect, true )`.
            let back = urlencode(&[scheme, &here].concat());
            let login = format!(
                "{}{PATH}?redirect_to={}&reauth=1",
                reads.site_url(),
                String::from_utf8_lossy(&back)
            );
            redirect(nocache(), login.as_bytes())
        }
    };
    match reads.reads.finish() {
        Ok(()) => answer,
        Err(err) => crate::html(crate::database_error(site, &err)),
    }
}

#[cfg(test)]
mod tests;
