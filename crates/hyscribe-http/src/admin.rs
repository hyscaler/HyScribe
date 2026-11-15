// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin's screens at `/hs-admin/` (ADR 0008): `auth_redirect()` before
//! any of them; screens not ported yet answer `501`.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::Response;
use hyscribe_admin::actions::{ActionSite, Asked, Body, Refusal, admin_ajax};
use hyscribe_admin::admin_bar::{BarSite, Browser};
use hyscribe_admin::appearance::themes;
use hyscribe_admin::assets::Urls;
use hyscribe_admin::comments::{
    CommentArgs, CommentQuery, CommentsSite, MODERATED, comment_order, edit_comments,
};
use hyscribe_admin::dashboard::{
    CommentCounts, DashboardSite, RecentComment, comment_status, dashboard,
};
use hyscribe_admin::login::{READ_ONLY, Site as LoginSite};
use hyscribe_admin::media::{
    AttachmentArgs, LibraryArgs, MediaSite, NewArgs, attachment_edit, media_new, upload,
};
use hyscribe_admin::menu::MenuUser;
use hyscribe_admin::page::{PageSite, REMOVABLE, Request, query_strings};
use hyscribe_admin::posts::{
    Answer, BULK, ListArgs, PostType, PostsSite, edit, kept_ids, sorted_ids,
};
use hyscribe_admin::profile::{EditArgs, ProfileSite, profile, user_edit};
use hyscribe_admin::settings::{SettingsSite, general, reading, writing};
use hyscribe_admin::settings_more;
use hyscribe_admin::terms::{
    Taxonomy as Terms, TermArgs, TermEditArgs, TermsSite, edit_tags, term_edit,
};
use hyscribe_admin::tools::{self, Requests, RequestsArgs, ToolsSite, privacy_requests};
use hyscribe_admin::users::{
    DeleteArgs, ListedUser, UserArgs, UserQuery, UsersSite, delete_users, user_order, users,
};
use hyscribe_auth::caps::{CapPost, CapabilityData, User as Capabilities, roles_with};
use hyscribe_db::{CommentFilter, StatusOrder, StatusQuery, UserFilter};
use hyscribe_model::{Post, PostQuery, Term, TermOrder, TermQuery, User};
use hyscribe_phpser::{
    Key, Value, addslashes, is_empty_string, lookup, parse_str, remove_query_arg, stripslashes,
    urlencode,
};
use hyscribe_render::comment_author_of;
use hyscribe_render::date::Zone;
use hyscribe_render::site::{Archive, Site as RenderSite, Taxonomy};
use tokio::runtime::Handle;

use crate::login::{
    LoginReads, Names, PATH, cookie, cookies, is_ssl, joined, nocache, renewed_cookies, response,
};
use crate::redirect::wp_sanitize_redirect;
use crate::{Site, now};

mod saves;
mod views;

/// Whether `path` is an hs-admin screen: its files have an extension and
/// are served, or missing, as Apache serves `wp-admin`'s.
pub(crate) fn is_admin(path: &str) -> bool {
    let screen = path
        .rsplit('/')
        .next()
        .is_none_or(|name| !name.contains('.'));
    (path == "/hs-admin" || path.starts_with("/hs-admin/")) && screen
}

/// The answer to a request for an admin screen; `form` is `$_POST`, `None`
/// for a body only PHP reads.
pub(crate) async fn respond(
    site: &Arc<Site>,
    uri: Uri,
    headers: HeaderMap,
    post: bool,
    form: Option<Value>,
) -> Response {
    let site = Arc::clone(site);
    let (now, handle) = (now(), Handle::current());
    joined(
        tokio::task::spawn_blocking(move || {
            let asked = Asking {
                uri: &uri,
                headers: &headers,
                post,
                form: form.as_ref(),
            };
            answer(&site, &asked, now, handle)
        })
        .await,
    )
}

/// An admin request as read.
struct Asking<'a> {
    uri: &'a Uri,
    headers: &'a HeaderMap,
    post: bool,
    form: Option<&'a Value>,
}

/// `entries` of `$_GET` or `$_POST`.
fn entries(value: Option<&Value>) -> &[(Key, Value)] {
    value.and_then(Value::as_array).unwrap_or_default()
}

/// The answer to a refused action.
fn refused(public: bool, refusal: Refusal) -> Response {
    let status = StatusCode::from_u16(refusal.status).unwrap_or(StatusCode::FORBIDDEN);
    let (kind, body) = match refusal.body {
        Body::Page(title, message) => {
            let page = hyscribe_admin::die::page(public, title, &message);
            return response(status, screen_headers(), page);
        }
        Body::Text(text) => ("text/html; charset=UTF-8", text.into_bytes()),
        Body::Json(json) => ("application/json; charset=UTF-8", json.as_bytes().to_vec()),
    };
    ajax_text(status, kind, body)
}

/// An answer as `admin-ajax.php` sends it, of the type `kind`.
fn ajax_text(status: StatusCode, kind: &str, body: Vec<u8>) -> Response {
    let mut headers = nocache();
    headers.push((header::CONTENT_TYPE, kind.to_string()));
    headers.push((header::X_CONTENT_TYPE_OPTIONS, "nosniff".to_string()));
    headers.push((
        header::HeaderName::from_static("x-robots-tag"),
        "noindex".to_string(),
    ));
    response(status, headers, body)
}

/// `501`, for what is not ported.
fn not_ported() -> Response {
    response(StatusCode::NOT_IMPLEMENTED, nocache(), Vec::new())
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
    /// The `Referer` header, if sent.
    referer: Option<String>,
    /// `$_SERVER['REQUEST_URI']`, slashed.
    uri: String,
    /// The comments of each post, of every status, as paged comment links
    /// read them.
    post_comments: RefCell<HashMap<u64, Vec<(hyscribe_model::Comment, String)>>>,
    /// The Quick Draft form's auto-draft this view wrote, if it did.
    draft: Option<u64>,
    /// `show_welcome_panel` as this view's `?welcome=` saves it.
    welcome: Option<Vec<u8>>,
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
            referer: None,
            uri: String::new(),
            post_comments: RefCell::default(),
            draft: None,
            welcome: None,
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
        let link = self.comment_link(&comment, post.as_ref().map(|_| permalink.as_str()));
        RecentComment {
            link,
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

    /// `get_comment_link()` of `comment` on the post at `permalink`; only a
    /// site that pages comments reads the post's comments.
    fn comment_link(&self, comment: &hyscribe_model::Comment, permalink: Option<&str>) -> String {
        let option = |name: &str| self.site.raw_option(name).to_vec();
        let paged = !hyscribe_phpser::is_empty_string(&option("page_comments"));
        let mut cache = self.post_comments.borrow_mut();
        let all = cache.entry(comment.post_id).or_insert_with(|| {
            let filter = CommentFilter {
                approved: &["0", "1", "spam", "trash", "post-trashed"],
                user: None,
                types: &[],
                search: None,
                post: comment.post_id,
            };
            let read = self.site.db.list_comments(&filter, &[], u32::MAX, 0);
            if paged {
                self.reads.reads.wait(read)
            } else {
                Vec::new()
            }
        });
        let all: Vec<_> = all
            .iter()
            .map(|(c, approved)| (c, approved.as_str()))
            .collect();
        hyscribe_admin::comments::comment_link(
            &option,
            permalink,
            &self.uri,
            &all,
            comment,
            self.user.id,
        )
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
    fn comment_post(&self, id: u64) -> Option<u64> {
        let comment = self.reads.reads.wait(self.site.db.any_comment(id));
        comment.map(|comment| comment.post_id)
    }
    fn term_taxonomy(&self, id: u64) -> Option<String> {
        self.reads.reads.wait(self.site.db.term_taxonomy(id))
    }
    fn constant(&self, name: &str) -> bool {
        self.reads.constant(name)
    }
}

impl ActionSite for Admin<'_> {
    fn current_user_can(&self, cap: &str, args: &[Value]) -> bool {
        self.caps.has_cap(self, cap, args)
    }
    fn verify_nonce(&self, nonce: &[u8], action: &str) -> bool {
        self.reads.verify_nonce(nonce, action)
    }
    fn validate_redirect(&self, location: &[u8]) -> Vec<u8> {
        ProfileSite::validate_redirect(self, location, "")
    }
    fn comment_approved(&self, id: u64) -> Option<Vec<u8>> {
        CommentsSite::comment_approved(self, id).map(String::into_bytes)
    }
    fn user_roles(&self) -> Option<Vec<u8>> {
        UsersSite::user_roles(self)
    }
    fn lock_holder(&self, post: u64) -> Option<String> {
        // The checks ask only about a post they found.
        let post = PostsSite::post(self, post).unwrap_or_default();
        let holder = hyscribe_admin::posts::lock_holder(self, &post)?;
        Some(String::from_utf8_lossy(&holder.display_name).into_owned())
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
    fn site_icon(&self, size: i64) -> Option<Vec<u8>> {
        hyscribe_admin::media::site_icon_url(self, size)
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
    fn user_option(&self, name: &str) -> Option<Vec<u8>> {
        self.reads.user_option(self.user.id, name)
    }
    fn settings_cookie(&self) -> Option<String> {
        self.settings.clone()
    }
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
    fn stylesheet(&self) -> String {
        self.site.option("stylesheet")
    }
    fn font_faces(&self, with_theme: bool) -> Vec<u8> {
        let url = self.reads.site_url();
        self.site.theme.admin_font_faces(&url, with_theme)
    }
}

impl DashboardSite for Admin<'_> {
    fn show_welcome_panel(&self) -> Option<Vec<u8>> {
        let stored = || self.reads.user_meta(self.user.id, "show_welcome_panel");
        self.welcome.clone().or_else(stored)
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
        if self.draft.is_some() {
            return self.draft;
        }
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
    fn userdata(&self, id: u64) -> Option<hyscribe_model::User> {
        self.reads.reads.wait(self.site.db.user(id))
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
    fn owns_content(&self, id: u64) -> bool {
        self.reads.reads.wait(self.site.db.owns_content(id))
    }
}

impl ProfileSite for Admin<'_> {
    fn profile_user(&self, id: u64) -> Option<hyscribe_model::User> {
        self.userdata(id)
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
    fn named_status_counts(&self, post_type: &str, name: &str) -> Vec<(String, u64)> {
        let counts = self.site.db.named_status_counts(post_type, name);
        self.reads.reads.wait(counts)
    }
}

impl MediaSite for Admin<'_> {
    fn attachment_mime_types(&self) -> Vec<String> {
        self.reads.reads.wait(self.site.db.mime_types("attachment"))
    }
    fn slug_taken(&self, slug: &[u8], id: u64) -> bool {
        let slug = String::from_utf8_lossy(slug);
        self.reads
            .reads
            .wait(self.site.db.post_name_taken(&slug, id))
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

/// Whether `users.php` is asked for its `delete` action.
fn is_deletion(query: &str) -> bool {
    parse_str(query.as_bytes())
        .get_str("action")
        .and_then(Value::as_bytes)
        == Some(b"delete")
}

/// The arguments of the deletion's confirmation page, and those of the
/// users list's bulk form it ignores; `None` for others.
fn delete_args(query: &str) -> Option<DeleteArgs> {
    const KNOWN: [&str; 10] = [
        "action",
        "action2",
        "new_role",
        "new_role2",
        "users",
        "user",
        "error",
        "wp_http_referer",
        "_wpnonce",
        "_wp_http_referer",
    ];
    let parsed = parse_str(query.as_bytes());
    let request = parsed.as_array().unwrap_or_default();
    let known = |key: &Key| KNOWN.contains(&&*String::from_utf8_lossy(&key.to_php_string()));
    if !request.iter().all(|(key, _)| known(key)) {
        return None;
    }
    let users = match lookup(request, "users").filter(|users| !users.is_empty()) {
        Some(Value::Array(users)) => users.iter().map(|(_, id)| id.to_int()).collect(),
        Some(user) => vec![user.to_int()],
        None => lookup(request, "user")
            .filter(|user| !user.is_empty())
            .map(Value::to_int)
            .into_iter()
            .collect(),
    };
    let referer = match lookup(request, "wp_http_referer") {
        // `remove_query_arg()` of an array is a fatal error.
        Some(Value::Array(_)) => return None,
        referer => {
            referer.map(|referer| String::from_utf8_lossy(&referer.to_php_string()).into_owned())
        }
    };
    Some(DeleteArgs {
        users,
        error: lookup(request, "error").is_some(),
        referer,
    })
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
            // What a bulk action leaves of its request, which the list does not read.
            (Key::String(key), _)
                if [
                    &b"_wpnonce"[..],
                    b"doaction",
                    b"screen",
                    b"sticky",
                    b"post_category",
                    b"tax_input",
                    b"post_parent",
                    b"page_template",
                    b"keep_private",
                ]
                .contains(&key.as_slice()) => {}
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

/// `get_site_icon_url( $size )` outside the admin, as the login screen asks.
pub(crate) fn site_icon(reads: &LoginReads, size: i64) -> Option<Vec<u8>> {
    let admin = Admin::new(reads, reads.site, hyscribe_model::User::default());
    hyscribe_admin::media::site_icon_url(&admin, size)
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
    // The Settings screens are sent back with `settings-updated` or `updated` alone.
    let settings = || {
        let back = only_argument(query, "settings-updated").or(only_argument(query, "updated"));
        back.map(drop)
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
        // Their actions are not ported.
        "/hs-admin/export-personal-data" | "/hs-admin/erase-personal-data" => {
            let requests = match path {
                "/hs-admin/export-personal-data" => Requests::Export,
                _ => Requests::Erase,
            };
            let args = RequestsArgs::parse(query)?;
            let answer = privacy_requests(admin, requests, &args, &request);
            return answer.map(|answer| served(admin, answer));
        }
        // Their arguments and actions are not ported, but the Themes
        // screen's broken theme notice.
        "/hs-admin/themes" | "/hs-admin/import" | "/hs-admin/export" => {
            let broken = only_argument(query, "broken").filter(|_| path == "/hs-admin/themes");
            if request_uri.contains('?') && broken.is_none() {
                return None;
            }
            match path {
                "/hs-admin/themes" => themes(admin, &request, broken.flatten().is_some()),
                "/hs-admin/import" => tools::import(admin, &request),
                _ => tools::export(admin, &request),
            }
        }
        // Their actions are not ported.
        "/hs-admin/upload" => upload(admin, &LibraryArgs::parse(query)?, &request)?,
        "/hs-admin/media-new" => {
            let args = NewArgs::parse(query, admin.settings.clone())?;
            media_new(admin, &args, &request)
        }
        // Only an attachment's edit screen is ported.
        "/hs-admin/post" => {
            let args = AttachmentArgs::parse(query, admin.referer.clone())?;
            attachment_edit(admin, &args, &request)?
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
            let args = edit_args(query).filter(|_| !origin.starts_with("https://"))?;
            profile(admin, &args, &request)
        }
        "/hs-admin/user-edit" => {
            let args = edit_args(query).filter(|_| !origin.starts_with("https://"))?;
            user_edit(admin, &args, &request)
        }
        "/hs-admin/user-new" => {
            let (mut update, mut user_id) = (None, None);
            for (key, value) in query_strings(query)? {
                match key.as_str() {
                    "update" => update = Some(value),
                    "user_id" => user_id = Some(value),
                    _ => return None,
                }
            }
            let args = (update.as_deref(), user_id.as_deref());
            hyscribe_admin::user_new::user_new(admin, args, &request)
        }
        // Past its checks, the deletion asks for its confirmation.
        "/hs-admin/users" if is_deletion(query) => {
            delete_users(admin, &delete_args(query)?, &request)?
        }
        "/hs-admin/users" => users(admin, &user_args(query)?, &request),
        "/hs-admin/" | "/hs-admin/index" => {
            // `welcome` saves the welcome panel's option for a user who sees
            // it, which shadow mode does not.
            let welcome = parse_str(query.as_bytes()).get_str("welcome").is_some();
            if welcome && admin.can("edit_theme_options") && admin.welcome.is_none() {
                return None;
            }
            Answer::from_page(dashboard(admin, &request))
        }
        "/hs-admin/edit-comments" => {
            let args = comment_args(query)?;
            // Only posts', pages' and attachments' comments are listed, and
            // a draft's Preview link is not ported.
            let post = args
                .post
                .as_deref()
                .map(|p| Value::String(p.into()).to_int());
            let listed = post.and_then(|id| PostsSite::post(admin, id.unsigned_abs()));
            if listed.is_some_and(|post| {
                !["post", "page", "attachment"].contains(&post.post_type.as_str())
                    || post.status == "draft"
            }) {
                return None;
            }
            edit_comments(admin, &args, admin.settings.as_deref(), &request)?
        }
        "/hs-admin/edit-tags" => edit_tags(admin, &term_args(query)?, &request),
        "/hs-admin/term" => {
            let args = TermEditArgs::parse(query, admin.referer.clone())?;
            term_edit(admin, &args, &request)
        }
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
        Answer::Missing(message) => died(admin, StatusCode::NOT_FOUND, message),
        Answer::Conflict(message) => died(admin, StatusCode::CONFLICT, message),
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

/// The answer to an action past its checks: 501 without a handler, shadow
/// mode's `wp_die()` without a writer, else the handler's in one action.
fn save(admin: &mut Admin, screen: &str, asked: &Asked) -> Response {
    let action = hyscribe_admin::actions::action(screen, asked);
    let Some(handler) = saves::handler(screen, &action) else {
        return not_ported();
    };
    let Some(writer) = &admin.site.writer else {
        // `wp_die()` in an Ajax request sends its message alone.
        return match screen {
            "admin-ajax" => ajax_text(StatusCode::FORBIDDEN, HTML, READ_ONLY.into()),
            _ => denied(admin, READ_ONLY),
        };
    };
    let act = |admin: &Admin| {
        let begun = async { saves::act(writer.begin().await, handler, asked, admin).await };
        Handle::current().block_on(begun)
    };
    if action == b"post-quickdraft-save" {
        return quick_draft(admin, writer, asked, act);
    }
    match act(admin) {
        // Only `admin-ajax.php`'s handlers answer with text: its `wp_die()`
        // and `wp_send_json()`.
        Ok(Answer::Page(body)) => {
            let kind = match body.starts_with('{') {
                true => "application/json; charset=UTF-8",
                false => "text/html; charset=UTF-8",
            };
            ajax_text(StatusCode::OK, kind, body.into_bytes())
        }
        Ok(answer) => served(admin, answer),
        Err(err) => failed(admin, err),
    }
}

/// An `admin-ajax.php` action with a `saves::Ajax` handler past its checks,
/// as [`save`]; shadow mode's `wp_die()` is the message alone. `None` for
/// an action without one.
fn ajax_save(admin: &Admin, asked: &Asked) -> Option<Response> {
    let action = asked.request("action").map(Value::to_php_string);
    let handler = saves::ajax_handler(&action.unwrap_or_default())?;
    let Some(writer) = &admin.site.writer else {
        return Some(ajax_text(StatusCode::FORBIDDEN, HTML, READ_ONLY.into()));
    };
    let saved = Handle::current()
        .block_on(async { saves::act(writer.begin().await, handler, asked, admin).await });
    Some(match saved {
        Ok(answer) => saves::ajax_served(admin, asked, answer),
        Err(err) => failed(admin, err),
    })
}

/// The type of `admin-ajax.php`'s answers but for XML and JSON.
const HTML: &str = "text/html; charset=UTF-8";

/// The answer to a write function's error: 501 for what is not ported,
/// else the database error page.
fn failed(admin: &Admin, err: hyscribe_write::Error) -> Response {
    match err {
        hyscribe_write::Error::Unported(_) => not_ported(),
        err => crate::html(crate::database_error(admin.site, &format!("{err:?}"))),
    }
}

/// `post.php`'s Quick Draft once checked: saved by `act` with a valid
/// nonce and a title or content, then the form, a page view, with its notice.
fn quick_draft(
    admin: &mut Admin,
    writer: &hyscribe_db::Writer,
    asked: &Asked,
    act: impl FnOnce(&Admin) -> Result<Answer, hyscribe_write::Error>,
) -> Response {
    let nonce = asked.request("_wpnonce").map(Value::to_php_string);
    let verified = nonce.is_some_and(|nonce| ActionSite::verify_nonce(admin, &nonce, "add-post"));
    let notice = match hyscribe_admin::actions::quickdraft_filled(asked) {
        _ if !verified => (
            "Unable to submit this form, please refresh and try again.",
            "error",
        ),
        None => return not_ported(),
        Some(false) => (
            "Cannot create a draft post with empty title and content.",
            "error",
        ),
        Some(true) => match act(admin) {
            Ok(_) => {
                // The checks read the auto-draft the save made a draft.
                admin.posts.borrow_mut().clear();
                ("Draft created successfully.", "success")
            }
            Err(err) => return failed(admin, err),
        },
    };
    let view = views::quick_press(admin);
    let uri = String::from_utf8_lossy(asked.uri).into_owned();
    views::viewed(admin, writer, view, |admin| {
        let form = hyscribe_admin::dashboard::quick_press(admin, &uri, Some(notice));
        response(StatusCode::OK, screen_headers(), form.into_bytes())
    })
}

/// `response` setting `cookies` too.
fn with_cookies(mut response: Response, cookies: Vec<String>) -> Response {
    for cookie in cookies {
        if let Ok(value) = HeaderValue::from_str(&cookie) {
            response.headers_mut().append(header::SET_COOKIE, value);
        }
    }
    response
}

/// What an action reads of the request: `$_GET` and `$_POST`, unslashed.
fn action_request<'a>(
    site: &'a Site,
    asking: &'a Asking,
    get: &'a Value,
    uri: &'a [u8],
    (user, now): (u64, i64),
) -> Asked<'a> {
    Asked {
        get: entries(Some(get)),
        post: entries(asking.form),
        referer: asking
            .headers
            .get(header::REFERER)
            .map(HeaderValue::as_bytes),
        uri,
        home: &site.home,
        user,
        now,
    }
}

/// `admin-ajax.php`, which sends no one to log in: the user of the auth
/// cookie, else of the logged in cookie.
fn ajax_answer(
    site: &Site,
    asking: &Asking,
    reads: &mut LoginReads,
    cookies: &[(Vec<u8>, Vec<u8>)],
) -> Response {
    let names = Names::new(site);
    let auth = cookie(cookies, &names.auth(is_ssl(site)));
    let user = reads.auth_user(auth, asking.post, is_ssl(site));
    let user =
        user.or_else(|| reads.logged_in_user(cookie(cookies, &names.logged_in()), asking.post));
    let get = parse_str(asking.uri.query().unwrap_or_default().as_bytes());
    let uri = asking
        .uri
        .path_and_query()
        .map_or("", |p| p.as_str())
        .as_bytes();
    let id = user.as_ref().map_or(0, |(id, _)| *id);
    let asked = action_request(site, asking, &get, uri, (id, reads.now));
    let checked = match user {
        Some((id, token)) => {
            reads.act_as(id, token);
            let user = reads.reads.wait(site.db.user(id)).unwrap_or_default();
            let mut admin = Admin::new(reads, site, user);
            match admin_ajax(Some(&admin), &asked) {
                Ok(())
                    if hyscribe_admin::actions::action("admin-ajax", &asked)
                        == b"destroy-sessions" =>
                {
                    let name = |id| {
                        PostsSite::userdata(&admin, id)
                            .unwrap_or_default()
                            .display_name
                    };
                    let sessions = site.sessions.as_deref();
                    let json =
                        saves::destroy_sessions(sessions, &asked, reads.session_token(), name);
                    return ajax_text(StatusCode::OK, "application/json; charset=UTF-8", json);
                }
                Ok(()) => {
                    let saved = ajax_save(&admin, &asked);
                    return saved.unwrap_or_else(|| save(&mut admin, "admin-ajax", &asked));
                }
                refused => refused,
            }
        }
        None => admin_ajax(None, &asked),
    };
    checked.map_or_else(
        |refusal| refused(reads.public(), refusal),
        |()| not_ported(),
    )
}

fn answer(site: &Site, asking: &Asking, now: i64, handle: Handle) -> Response {
    let (uri, headers, post) = (asking.uri, asking.headers, asking.post);
    let names = Names::new(site);
    let https = is_ssl(site);
    let cookies = cookies(
        headers
            .get_all(header::COOKIE)
            .iter()
            .map(HeaderValue::as_bytes),
    );
    let mut reads = LoginReads::new(site, handle, now);
    if asking.form.is_none() {
        return not_ported();
    }
    if uri.path() == "/hs-admin/admin-ajax" {
        let answer = ajax_answer(site, asking, &mut reads, &cookies);
        return match reads.reads.finish() {
            Ok(()) => answer,
            Err(err) => crate::html(crate::database_error(site, &err)),
        };
    }
    let user = reads.auth_user(cookie(&cookies, &names.auth(https)), post, https);
    // `$_SERVER['HTTP_HOST'] . $_SERVER['REQUEST_URI']`.
    let host = headers
        .get(header::HOST)
        .map_or(&b""[..], HeaderValue::as_bytes);
    // `wp_magic_quotes()` slashes `$_SERVER['REQUEST_URI']` too.
    let raw_uri = uri.path_and_query().map_or("", |p| p.as_str());
    let requested = addslashes(raw_uri.as_bytes());
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
                let get = parse_str(uri.query().unwrap_or_default().as_bytes());
                let asked = action_request(site, asking, &get, raw_uri.as_bytes(), (id, now));
                let name = uri.path().strip_prefix("/hs-admin/").unwrap_or_default();
                match hyscribe_admin::actions::admin(&admin, name, &asked, post) {
                    Some(Err(refusal)) => return refused(reads.public(), refusal),
                    // The deletion's confirmation page, a screen.
                    Some(Ok(()))
                        if !post
                            && name == "users"
                            && is_deletion(uri.query().unwrap_or_default()) => {}
                    Some(Ok(())) => {
                        let saved = save(&mut admin, name, &asked);
                        let logged_in = cookie(&cookies, &names.logged_in());
                        let renewed = renewed_cookies(&reads, &names, logged_in, https);
                        return with_cookies(saved, renewed);
                    }
                    None if post => return not_ported(),
                    None => {}
                }
                let settings = cookie(&cookies, &format!("wp-settings-{id}"));
                admin.settings = settings.map(|value| String::from_utf8_lossy(value).into_owned());
                let header = |name: &str| headers.get(name).map(HeaderValue::as_bytes);
                admin.referer =
                    header("referer").map(|value| String::from_utf8_lossy(value).into_owned());
                admin.uri.clone_from(&requested);
                let browser = Browser {
                    user_agent: header("user-agent").unwrap_or_default(),
                    mobile_hint: header("sec-ch-ua-mobile"),
                };
                let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
                let url = text(&[scheme, &here].concat());
                let origin = text(&[scheme, host].concat());
                let render = |admin: &Admin| {
                    screen(admin, uri.path(), &requested, &url, &origin, &browser)
                        .unwrap_or_else(not_ported)
                };
                match &site.writer {
                    Some(writer) => {
                        let view = views::plan(&mut admin, &cookies, uri.path(), &get, now);
                        views::viewed(&mut admin, writer, view, render)
                    }
                    None => render(&admin),
                }
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
pub(crate) mod tests;
