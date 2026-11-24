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
use hyscribe_admin::appearance::{Notice as ThemesNotice, themes};
use hyscribe_admin::assets::Urls;
use hyscribe_admin::comments::{
    CommentArgs, CommentEditSite, CommentQuery, CommentsSite, MODERATED, comment_edit,
    comment_order, edit_comments,
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
use hyscribe_admin::plugins::{Args as PluginArgs, Plugin, plugins};
use hyscribe_admin::posts::{
    Answer, BULK, ListArgs, PostType, PostsSite, edit, kept_ids, sorted_ids,
};
use hyscribe_admin::profile::{EditArgs, ProfileSite, profile, user_edit};
use hyscribe_admin::settings::{SettingsSite, general, reading, writing};
use hyscribe_admin::settings_more;
use hyscribe_admin::site_health::{EnvFile, Environment, HealthSite, Install, info, status};
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

mod media;
pub(crate) mod saves;
mod uploads;
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

/// The answer to a request for an admin screen, a POST or not, from the
/// connection's `peer`; `form` is `$_POST`, `None` for a body only PHP
/// reads, and `files` `$_FILES`.
pub(crate) async fn respond(
    site: &Arc<Site>,
    uri: Uri,
    headers: HeaderMap,
    (post, peer): (bool, Option<std::net::IpAddr>),
    (form, files): (Option<Value>, crate::uploads::Files),
) -> Response {
    let site = Arc::clone(site);
    let (now, handle) = (now(), Handle::current());
    joined(
        tokio::task::spawn_blocking(hyscribe_model::hooks::carry(move || {
            let ip = crate::comment_post::client_addr(&site, &headers, peer);
            let asked = Asking {
                uri: &uri,
                headers: &headers,
                post,
                form: form.as_ref(),
                files: &files,
                ip,
            };
            answer(&site, &asked, now, handle)
        }))
        .await,
    )
}

/// An admin request as read.
struct Asking<'a> {
    uri: &'a Uri,
    headers: &'a HeaderMap,
    post: bool,
    form: Option<&'a Value>,
    /// `$_FILES`, by field.
    files: &'a [(Vec<u8>, crate::uploads::Sent)],
    /// `$_SERVER['REMOTE_ADDR']`.
    ip: Vec<u8>,
}

/// `entries` of `$_GET` or `$_POST`.
fn entries(value: Option<&Value>) -> &[(Key, Value)] {
    value.and_then(Value::as_array).unwrap_or_default()
}

/// The answer to a refused action.
fn refused(public: bool, refusal: Refusal) -> Response {
    let status = StatusCode::from_u16(refusal.status).unwrap_or(StatusCode::FORBIDDEN);
    if status == StatusCode::NOT_IMPLEMENTED {
        return not_ported();
    }
    let (kind, body) = match refusal.body {
        Body::Page(title, message) => {
            let page = hyscribe_admin::die::page(public, title, &message);
            return response(status, screen_headers(), page);
        }
        Body::Text(text) => ("text/html; charset=UTF-8", text.into_bytes()),
        Body::Json(json) => (JSON, json.as_bytes().to_vec()),
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

/// A `302` to `location`, as `wp_redirect()` sends it, naming hs-admin's
/// screens without `.php` (ADR 0008).
fn redirect(mut headers: Vec<(header::HeaderName, String)>, location: &[u8]) -> Response {
    let location = wp_sanitize_redirect(location);
    let location = hyscribe_admin::menu::clean_links(&String::from_utf8_lossy(&location));
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
    /// The block editor's post and global styles post this view wrote its
    /// edit lock and first view for, and the lock.
    edited: Option<(u64, u64, String)>,
    /// The theme's global styles post the site editor's view wrote or found.
    styles: Option<u64>,
    /// `show_welcome_panel` as this view's `?welcome=` saves it.
    welcome: Option<Vec<u8>>,
    /// The mails of the action, sent once it commits (ADR 0016).
    outbox: crate::mail::Outbox,
    /// `$GLOBALS['login_grace_period']`: the auth cookie expired within
    /// its grace.
    grace: bool,
    /// The files an action deletes once it is committed.
    files: RefCell<Vec<(Vec<u8>, Vec<u8>)>>,
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
            edited: None,
            styles: None,
            welcome: None,
            grace: false,
            outbox: crate::mail::Outbox::new(Arc::clone(&site.mailer)),
            files: RefCell::default(),
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
    fn user_option(&self, name: &str) -> Option<Vec<u8>> {
        PageSite::user_option(self, name)
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
    fn plugin_pages(&self) -> Vec<hyscribe_admin::menu::PluginPage> {
        let running = self.site.running.as_ref();
        let pages = running
            .map(|running| running.request.pages())
            .unwrap_or_default();
        pages
            .into_iter()
            .map(|(_, page)| plugin_page(page))
            .collect()
    }
}

/// An active extension's admin page asked with `page` alone under
/// `screen` (`admin`, or a core menu's file), as `admin.php` serves it:
/// the access check first, then its callback if it is on the page's hook
/// for the user; `None` for a page no active extension has.
fn extension_screen(
    admin: &Admin,
    screen: &str,
    query: &str,
    request: &Request,
) -> Option<Response> {
    use hyscribe_admin::extension_page::{extension_page, settings_fields};
    use hyscribe_admin::menu::{Look, Screen, admin_menu, plugin_page_hook};
    let slug = only_argument(query, "page")??;
    let running = admin.site.running.as_ref()?;
    let pages = running.request.pages();
    let (ext, page) = pages.into_iter().find(|(_, page)| page.slug == slug)?;
    let pagenow = format!("{screen}.php");
    let asked = Screen {
        pagenow: &pagenow,
        typenow: "",
        taxonomy: "",
        parent_file: None,
        submenu_file: None,
        plugin_page: Some(&slug),
    };
    if admin_menu(admin, &asked, Look::Block).is_none() {
        return Some(served(admin, Answer::from_page(None)));
    }
    // A top level page asked under a menu the user may reach, without its
    // capability: no callback is on its hook.
    let Some(hook) = plugin_page_hook(admin, &pagenow, &slug) else {
        let message = format!("Cannot load {slug}.");
        return Some(died(admin, StatusCode::INTERNAL_SERVER_ERROR, &message));
    };
    let groups = running
        .request
        .settings()
        .into_iter()
        .map(|setting| setting.group);
    let fields = groups.map(|group| {
        let fields = settings_fields(admin, &group, request.uri);
        (group, fields.into_bytes())
    });
    running.fields.set(fields.collect());
    let printed = running.request.page(ext, &slug);
    let titles = (page.page_title.as_str(), printed.as_slice());
    let page = extension_page(admin, (&pagenow, &slug, &hook), titles, request);
    Some(served(admin, Answer::from_page(page)))
}

/// An extension's admin page as the menu adds it.
fn plugin_page(page: hyscribe_ext::AdminPage) -> hyscribe_admin::menu::PluginPage {
    hyscribe_admin::menu::PluginPage {
        parent: page.parent,
        page_title: page.page_title,
        menu_title: page.menu_title,
        capability: page.capability,
        slug: page.slug,
        icon: page.icon,
        position: page.position,
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
    fn active_theme(&self) -> &hyscribe_render::theme::Theme {
        &self.site.theme
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
    fn max_upload_size(&self) -> u64 {
        self.site.install.upload_max_size
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
    fn post_formats(&self) -> &[&'static str] {
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
    fn uses_trash(&self) -> bool {
        self.site.constants.empty_trash_days != 0
    }
}

impl CommentEditSite for Admin<'_> {
    fn comment_to_edit(&self, id: u64) -> Option<hyscribe_admin::dashboard::RecentComment> {
        let comment = self.reads.reads.wait(self.site.db.any_comment(id))?;
        let approved = CommentsSite::comment_approved(self, id).unwrap_or_default();
        let post = self.full_post(comment.post_id);
        Some(self.comment_row(comment, &approved, post))
    }
    fn referer(&self) -> Option<Vec<u8>> {
        let referer =
            hyscribe_admin::profile::wp_get_referer(self, self.referer.as_deref(), &self.uri);
        referer.map(String::into_bytes)
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
    fn application_passwords(&self, id: u64) -> Option<Vec<u8>> {
        self.reads.user_meta(id, "_application_passwords")
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

impl HealthSite for Admin<'_> {
    fn install(&self) -> Install {
        Install::clone(&self.site.install)
    }
    fn primary(&self) -> bool {
        self.site.writer.is_some()
    }
    fn login_limits(&self) -> [u32; 3] {
        let limits = self.site.limiter.limits();
        [limits.address, limits.account, limits.window]
    }
    fn table_prefix(&self) -> String {
        self.site.db.prefix().into()
    }
    fn database_server(&self) -> (String, String) {
        self.reads.reads.wait(self.site.db.server())
    }
    fn environment(&self) -> Environment {
        environment(&self.site.install.binary)
    }
}

/// The `.env` file, the disk and the service of the `hyscribe/` folder the
/// binary at `binary` is in.
fn environment(binary: &str) -> Environment {
    use std::os::unix::fs::PermissionsExt;
    let home = std::path::Path::new(binary)
        .parent()
        .filter(|home| !home.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let env_file = match std::fs::metadata(home.join(".env")) {
        Ok(file) => EnvFile::Mode(file.permissions().mode() & 0o777),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => EnvFile::Missing,
        Err(_) => EnvFile::Unreadable,
    };
    let service_pid = match hyscribe_service::status(&hyscribe_service::Paths::new(home)) {
        Ok(
            hyscribe_service::Status::Running(pid) | hyscribe_service::Status::Starting(Some(pid)),
        ) => Some(pid),
        _ => None,
    };
    Environment {
        env_file,
        disk_free: hyscribe_service::disk_free(home),
        service_pid,
        pid: std::process::id(),
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
            // The search form's own field: the only post type of these taxonomies.
            "post_type" if value == "post" => {}
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
            "_wp_http_referer" => {
                args.sent_back = value
                    .as_deref()
                    .is_some_and(|v| !is_empty_string(v.as_bytes()));
            }
            // The form's own fields: no action chosen, the list not read from them.
            "action" | "action2" if value.as_deref() == Some("-1") => {}
            "_wpnonce"
            | "filter_action"
            | "pagegen_timestamp"
            | "_total"
            | "_per_page"
            | "_page"
            | "_ajax_fetch_list_nonce" => {}
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
            "_wp_http_referer" => {
                args.sent_back = value
                    .as_deref()
                    .is_some_and(|v| !is_empty_string(v.as_bytes()));
            }
            // The form's own fields: no action chosen, the list not read from them.
            "action" | "action2" if value.as_deref() == Some("-1") => {}
            "_wpnonce" | "new_role" | "new_role2" => {}
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

/// An installed extension as the Plugins screen lists it, with what its
/// `origin.txt` and the kept `index` say of it.
fn listed(
    installed: hyscribe_ext::installed::Installed,
    index: Option<&hyscribe_registry::index::Index>,
) -> Plugin {
    use hyscribe_admin::plugins::{Origin, Update};
    use hyscribe_ext::origin::{self, Signer};
    let read = origin::read(&installed.folder).ok().flatten();
    let shown = match read.as_ref().map(|origin| &origin.signer) {
        Some(None) => Origin::Unsigned,
        Some(Some((Signer::Trusted, _))) => Origin::Trusted,
        _ => Origin::Unremarked,
    };
    let revoked = read.and_then(|origin| origin.revoked);
    let offer = index.and_then(|index| hyscribe_registry::install::offer(&installed, index));
    let update = match (revoked, offer) {
        (Some((_, reason)), _) => Some(Update::Revoked(reason)),
        (None, Some((_, version))) => Some(Update::Offer(version.clone())),
        (None, None) => None,
    };
    let manifest = installed.manifest;
    let limits = manifest.limits;
    let asked = |time: std::time::Duration| u64::try_from(time.as_millis()).unwrap_or(u64::MAX);
    Plugin {
        id: manifest.id,
        file: manifest.replaces.unwrap_or_default(),
        grants: manifest.grants.listed(),
        sensitive: manifest.grants.sensitive.into_iter().collect(),
        limits: (limits != hyscribe_ext::DEFAULT).then(|| {
            let memory = u64::try_from(limits.memory() >> 20).unwrap_or(u64::MAX);
            (asked(limits.call()), asked(limits.request()), memory)
        }),
        name: manifest.name,
        description: manifest.description,
        version: manifest.version,
        author: manifest.author,
        author_uri: manifest.author_uri,
        plugin_uri: manifest.plugin_uri,
        approved: installed.approved,
        disabled: installed.disabled,
        origin: shown,
        update,
    }
}

impl crate::Site {
    /// The extensions installed in the plugins folder, as the Plugins
    /// screen lists them at `now` (none without the folder), and whether
    /// the kept registry index is out of date.
    pub(crate) fn installed_plugins(&self, now: i64) -> (Vec<Plugin>, bool) {
        let runtime = self.extensions.as_deref();
        let found = runtime.map(|runtime| hyscribe_ext::installed::list(runtime.plugins()));
        let kept = self.registry.as_deref().map(|(folder, roots)| {
            let now = u64::try_from(now).unwrap_or_default();
            hyscribe_registry::kept::shown(folder, roots, now)
        });
        let index = kept.as_ref().and_then(|kept| kept.as_ref().ok()?.as_ref());
        let plugins = found.unwrap_or_default().into_iter();
        let plugins = plugins.map(|installed| listed(installed, index)).collect();
        (plugins, kept.is_some_and(|kept| kept.is_err()))
    }
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

/// The notice the Themes screen's arguments ask for, `None` for an argument
/// the screen does not take here.
fn themes_notice(query: &str) -> Option<ThemesNotice> {
    let (mut broken, mut activated, mut previewed) = (false, false, false);
    for (key, _) in query_strings(query)? {
        match key.as_str() {
            "broken" => broken = true,
            "activated" => activated = true,
            "previewed" => previewed = true,
            _ => return None,
        }
    }
    Some(match (broken, activated, previewed) {
        (true, ..) => ThemesNotice::Broken,
        (_, true, true) => ThemesNotice::Previewed,
        (_, true, false) => ThemesNotice::Activated,
        _ => ThemesNotice::None,
    })
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
            // The form's own fields: no action chosen, the list not read from them.
            (Key::String(key), Value::String(value))
                if [&b"action"[..], b"action2"].contains(&key.as_slice()) && value == b"-1" => {}
            (Key::String(key), Value::String(value)) if key == b"_wp_http_referer" => {
                list.sent_back = !is_empty_string(value);
            }
            // What a bulk action leaves of its request, which the list does not read.
            (Key::String(key), _)
                if [
                    &b"_wpnonce"[..],
                    b"filter_action",
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
/// The block editor's page of the post the view wrote its edit lock for,
/// once every preload is answered; `None` before.
fn block_editor(admin: &Admin, request: &Request) -> Option<Answer> {
    block_editor_with(admin, request, &rest_preloads(admin, request))
}

/// What answers the editors' preloads, `None` where one is not ported.
pub(super) type Preloads<'a> = dyn Fn(&[(&'static str, String)]) -> Option<Value> + 'a;

/// The REST API's answers to the editors' preloads for `admin`.
fn rest_preloads<'a>(
    admin: &'a Admin,
    request: &'a Request,
) -> impl Fn(&[(&'static str, String)]) -> Option<Value> + 'a {
    let host = request.origin.rsplit("//").next().unwrap_or_default();
    move |paths| crate::rest::preloads(admin.reads, (admin.user.id, host), paths)
}

/// [`block_editor`] with the preloads `preloads` answers.
fn block_editor_with(admin: &Admin, request: &Request, preloads: &Preloads) -> Option<Answer> {
    let (id, styles, lock) = admin.edited.as_ref()?;
    let post = admin.full_post(*id)?;
    let stylesheet = admin.site.option("stylesheet");
    let edit_theme = admin.can("edit_theme_options");
    let paths =
        hyscribe_admin::editor::page::preload_paths(&post, *styles, edit_theme, &stylesheet);
    let preloads = preloads(&paths)?;
    let editing = hyscribe_admin::editor::form::Editing {
        post: &post,
        lock,
        browser: request.browser,
        uri: request.uri,
    };
    let page = hyscribe_admin::editor::page::editor_page(admin, &editing, &preloads, request);
    Some(Answer::from_page(page))
}

/// The site editor's page `query` asks for, once its view found or wrote
/// the theme's global styles post and every preload is answered; `None`
/// before.
fn site_editor_page(admin: &Admin, query: &str, request: &Request) -> Option<Answer> {
    site_editor_with(admin, query, request, &rest_preloads(admin, request))
}

/// [`site_editor_page`] with the preloads `preloads` answers.
fn site_editor_with(
    admin: &Admin,
    query: &str,
    request: &Request,
    preloads: &Preloads,
) -> Option<Answer> {
    use hyscribe_admin::site_editor::{context_post, preload_paths, site_editor_page};
    let styles = admin.styles?;
    let post = context_post(admin, query)?;
    let stylesheet = admin.site.option("stylesheet");
    let paths = preload_paths(post.as_ref(), styles, &stylesheet);
    let preloads = preloads(&paths)?;
    let page = site_editor_page(admin, post.as_ref(), &preloads, request);
    Some(Answer::from_page(page))
}

fn screen(
    admin: &Admin,
    path: &str,
    request_uri: &str,
    url: &str,
    origin: &str,
    browser: &Browser,
) -> Option<Response> {
    // The screens with the media library's templates or its list, which
    // `MEDIA_TRASH` changes, are not ported with it.
    let media = ["upload", "post", "post-new", "options-general"];
    let screen = path.strip_prefix("/hs-admin/").unwrap_or_default();
    if admin.site.constants.media_trash && media.contains(&screen) {
        return None;
    }
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
    if let Some(response) = extension_screen(admin, screen, query, &request) {
        return Some(response);
    }
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
        // screen's notices.
        "/hs-admin/themes" | "/hs-admin/import" | "/hs-admin/export" => {
            let notice = (path == "/hs-admin/themes")
                .then(|| themes_notice(query))
                .flatten();
            if request_uri.contains('?') && notice.is_none() {
                return None;
            }
            match path {
                "/hs-admin/themes" => themes(admin, &request, notice.unwrap_or_default()),
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
        // An attachment's edit screen is ported, and of the block editor's
        // only the refusals.
        "/hs-admin/post" => {
            let args = AttachmentArgs::parse(query, admin.referer.clone())?;
            attachment_edit(admin, &args, &request).or_else(|| block_editor(admin, &request))?
        }
        "/hs-admin/post-new" => hyscribe_admin::editor::post_new(admin, query)
            .or_else(|| block_editor(admin, &request))?,
        "/hs-admin/revision" => hyscribe_admin::revisions::revision(admin, query, &request)?,
        // Nobody may update here (the updates are HyScribe's releases;
        // `update_core` is never granted): refused as PHP refuses the menu.
        "/hs-admin/update-core" => Answer::from_page(None),
        // The installed extensions listed.
        "/hs-admin/plugins" => {
            let (installed, stale) = admin.site.installed_plugins(admin.reads.now);
            plugins(
                admin,
                &request,
                (&installed, stale),
                &PluginArgs::parse(query)?,
            )?
        }
        "/hs-admin/site-health" if !admin.can("view_site_health_checks") => Answer::from_page(None),
        // The Info tab of the bundled theme, and the Status tab with
        // HyScribe's own checks.
        "/hs-admin/site-health" => {
            let tab = only_argument(query, "tab")?;
            let slug = admin.site.theme.slug().as_bytes();
            let bundled = |name| PostsSite::option(admin, name).as_deref() == Some(slug);
            if !bundled("stylesheet") || !bundled("template") {
                return None;
            }
            match tab.as_deref() {
                None => Answer::from_page(status(admin, &request)),
                Some("debug") => Answer::from_page(info(admin, &request)),
                Some(_) => return None,
            }
        }
        "/hs-admin/site-editor" if !admin.can("edit_theme_options") => Answer::from_page(None),
        "/hs-admin/site-editor" => hyscribe_admin::site_editor::site_editor(query, request_uri)
            .or_else(|| site_editor_page(admin, query, &request))?,
        "/hs-admin/profile" => profile(admin, &edit_args(query)?, &request)?,
        "/hs-admin/user-edit" => user_edit(admin, &edit_args(query)?, &request)?,
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
        // Only the edit screen is ported of `comment.php`'s pages.
        "/hs-admin/comment" => {
            let mut id = None;
            for (key, value) in query_strings(query)? {
                match (key.as_str(), value.as_str()) {
                    ("action", "editcomment") => {}
                    ("c", _) => id = Some(crate::query::absint(&value)),
                    _ => return None,
                }
            }
            comment_edit(admin, id?, &request)?
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
    let body = hyscribe_admin::die::page(admin.reads.public(), "HyScribe &rsaquo; Error", message);
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
        return denied(admin, READ_ONLY);
    };
    let act = |admin: &Admin| {
        let begun = async { saves::act(writer.begin().await, handler, asked, admin).await };
        Handle::current().block_on(begun)
    };
    if action == b"post-quickdraft-save" {
        return quick_draft(admin, writer, asked, act);
    }
    match act(admin) {
        Ok(answer) => served(admin, answer),
        Err(err) => failed(admin, err),
    }
}

/// An `admin-ajax.php` action past its checks, as [`save`]: 501 without a
/// handler, and shadow mode's `wp_die()` is the message alone.
fn ajax_save(admin: &Admin, asked: &Asked) -> Response {
    let action = asked.request("action").map(Value::to_php_string);
    let Some(handler) = saves::ajax_handler(&action.unwrap_or_default()) else {
        return not_ported();
    };
    let Some(writer) = &admin.site.writer else {
        return ajax_text(StatusCode::FORBIDDEN, HTML, READ_ONLY.into());
    };
    let saved = Handle::current()
        .block_on(async { saves::act(writer.begin().await, handler, asked, admin).await });
    match saved {
        Ok(answer) => saves::ajax_served(admin, asked, answer),
        Err(err) => failed(admin, err),
    }
}

/// The type of `admin-ajax.php`'s answers but for XML and JSON.
const HTML: &str = "text/html; charset=UTF-8";

/// The type of `wp_send_json()`'s answers.
const JSON: &str = "application/json; charset=UTF-8";

/// The answer to a write function's error: 501 for what is not ported,
/// text `wpdb` refuses among it, else the database error page.
fn failed(admin: &Admin, err: hyscribe_write::Error) -> Response {
    match err {
        hyscribe_write::Error::Unported(_) | hyscribe_write::Error::InvalidText(_) => not_ported(),
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
    views::viewed(admin, writer, view, &|admin| {
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
        ip: &asking.ip,
        agent: (asking.headers.get(header::USER_AGENT)).map_or(b"", HeaderValue::as_bytes),
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
    // `wp_doing_ajax()` gives every cookie an hour's grace.
    let auth = cookie(cookies, &names.auth(is_ssl(site)));
    let logged_in = cookie(cookies, &names.logged_in());
    let (user, valid) = match reads.auth_user(auth, true, is_ssl(site)) {
        Some(user) => (Some(user), auth),
        None => (reads.logged_in_user(logged_in, true), logged_in),
    };
    let expiration = valid.and_then(hyscribe_auth::cookie::parse_auth_cookie);
    let expiration = expiration.map_or(0, |c| Value::String(c.expiration.to_vec()).to_int());
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
            admin.grace = expiration < reads.now;
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
                    return ajax_text(StatusCode::OK, JSON, json);
                }
                Ok(()) => {
                    return media::read(&admin, &asked)
                        .unwrap_or_else(|| ajax_save(&admin, &asked));
                }
                refused => refused,
            }
        }
        None => match admin_ajax(None, &asked) {
            Ok(()) if hyscribe_admin::actions::action("admin-ajax", &asked) == b"heartbeat" => {
                let json = saves::heartbeat::nopriv(reads.now).into_bytes();
                return ajax_text(StatusCode::OK, JSON, json);
            }
            checked => checked,
        },
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
                if let Some(writer) = &site.writer {
                    views::scheduled(writer, now);
                }
                let get = parse_str(uri.query().unwrap_or_default().as_bytes());
                let asked = action_request(site, asking, &get, raw_uri.as_bytes(), (id, now));
                let name = uri.path().strip_prefix("/hs-admin/").unwrap_or_default();
                let logged_in = cookie(&cookies, &names.logged_in());
                if post && uploads::SCREENS.contains(&name) {
                    let answer = uploads::upload(&admin, name, &asked, asking.files);
                    let renewed = renewed_cookies(&reads, &names, logged_in, https);
                    return with_cookies(answer, renewed);
                }
                match hyscribe_admin::actions::admin(&admin, name, &asked, post) {
                    Some(Err(refusal)) => return refused(reads.public(), refusal),
                    // The deletion's confirmation page, a screen.
                    Some(Ok(()))
                        if !post
                            && name == "users"
                            && is_deletion(uri.query().unwrap_or_default()) => {}
                    Some(Ok(())) => {
                        let saved = save(&mut admin, name, &asked);
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
                        let view = views::plan(
                            &mut admin,
                            &cookies,
                            (
                                uri.path(),
                                uri.query().unwrap_or_default(),
                                &String::from_utf8_lossy(host),
                            ),
                            &get,
                            now,
                        );
                        views::viewed(&mut admin, writer, view, &render)
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
            let mut headers = nocache();
            headers.push((header::CONTENT_TYPE, HTML.to_string()));
            redirect(headers, login.as_bytes())
        }
    };
    match reads.reads.finish() {
        Ok(()) => answer,
        Err(err) => crate::html(crate::database_error(site, &err)),
    }
}

#[cfg(test)]
pub(crate) mod tests;

/// The Themes screen's arguments choose its notice: broken before the rest,
/// a preview only with an activation, any other argument none.
#[test]
fn the_themes_notice_follows_the_arguments() {
    for (query, notice) in [
        ("", Some(ThemesNotice::None)),
        ("previewed=1", Some(ThemesNotice::None)),
        ("activated=1", Some(ThemesNotice::Activated)),
        ("activated=1&previewed=1", Some(ThemesNotice::Previewed)),
        ("broken=1&activated=1", Some(ThemesNotice::Broken)),
        ("activated=1&x=1", None),
    ] {
        assert_eq!(themes_notice(query), notice, "{query}");
    }
}
