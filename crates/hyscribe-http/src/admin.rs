// SPDX-License-Identifier: AGPL-3.0-or-later

//! hs-admin's screens at `/hs-admin/` (ADR 0008): `auth_redirect()` before
//! any of them; screens not ported yet answer `501`.

use std::cell::{OnceCell, RefCell};
use std::collections::HashMap;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::Response;
use hyscribe_admin::admin_bar::{BarSite, Browser};
use hyscribe_admin::assets::Urls;
use hyscribe_admin::dashboard::{CommentCounts, DashboardSite, RecentComment, dashboard};
use hyscribe_admin::login::Site as LoginSite;
use hyscribe_admin::menu::MenuUser;
use hyscribe_admin::page::PageSite;
use hyscribe_admin::posts::{Answer, ListArgs, PostsSite, edit, kept_ids, sorted_ids};
use hyscribe_auth::caps::{CapPost, CapabilityData, User as Capabilities, roles_with};
use hyscribe_db::{StatusOrder, StatusQuery};
use hyscribe_model::{Post, PostQuery, Term, TermOrder, TermQuery, User};
use hyscribe_phpser::{Key, Value, is_empty_string, parse_str, remove_query_arg, urlencode};
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

/// `wp_removable_query_args()`: what the canonical admin URL leaves out.
const REMOVABLE: [&[u8]; 28] = [
    b"activate",
    b"activated",
    b"admin_email_remind_later",
    b"approved",
    b"core-major-auto-updates-saved",
    b"deactivate",
    b"delete_count",
    b"deleted",
    b"disabled",
    b"doing_wp_cron",
    b"enabled",
    b"error",
    b"hotkeys_highlight_first",
    b"hotkeys_highlight_last",
    b"ids",
    b"locked",
    b"message",
    b"same",
    b"saved",
    b"settings-updated",
    b"skipped",
    b"spammed",
    b"trashed",
    b"unspammed",
    b"untrashed",
    b"update",
    b"updated",
    b"wp-post-new-reload",
];

/// A logged in user in the admin, on a blocking thread.
struct Admin<'a> {
    reads: &'a LoginReads<'a>,
    site: &'a Site,
    caps: Capabilities,
    user: hyscribe_model::User,
    /// The posts read, each once, as WordPress's object cache keeps them.
    posts: RefCell<HashMap<u64, Option<hyscribe_model::Post>>>,
    comment_counts: OnceCell<CommentCounts>,
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
        }
    }

    /// `current_user_can( $cap, $id )` of an object.
    fn can_for(&self, cap: &str, id: u64) -> bool {
        let id = i64::try_from(id).unwrap_or_default();
        self.caps.has_cap(self, cap, &[Value::Int(id)])
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
    fn term_taxonomy(&self, _: u64) -> Option<String> {
        None
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
        *self.comment_counts.get_or_init(|| {
            let [approved, moderated, spam, trash, mine] = self
                .reads
                .reads
                .wait(self.site.db.comment_counts(self.user.id));
            CommentCounts {
                approved,
                moderated,
                spam,
                trash,
                mine,
            }
        })
    }
    /// `wp_dashboard_recent_comments()`'s search: batches of the latest
    /// comments, kept while the user may see their post.
    fn recent_comments(&self, global: Option<&hyscribe_model::Post>) -> Vec<RecentComment> {
        let pending = self.can("edit_posts");
        let (mut offset, mut number) = (0, 25);
        let mut found = Vec::new();
        loop {
            let batch = self
                .reads
                .reads
                .wait(self.site.db.latest_comments(pending, number, offset));
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
                let permalink = post
                    .as_ref()
                    .map(|post| self.permalink(post))
                    .unwrap_or_default();
                found.push(RecentComment {
                    link: format!("{permalink}#comment-{}", comment.id),
                    permalink,
                    status: if approved == "1" {
                        "approved"
                    } else {
                        "unapproved"
                    },
                    user: (comment.user_id != 0)
                        .then(|| self.reads.reads.wait(self.site.db.user(comment.user_id)))
                        .flatten(),
                    can_edit: self.comment_editable(post.as_ref()),
                    post: post.filter(|_| comment.post_id != 0),
                    comment,
                });
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
    fn authors(&self) -> Vec<User> {
        let prefix = self.site.db.prefix();
        let mut names = vec!["edit_posts".to_string()];
        names.extend(roles_with(self, prefix, "edit_posts"));
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
}

/// The headers of an admin screen: `nocache_headers()` and
/// `send_frame_options_header()`; every response has `wp_admin_headers()`'s.
fn screen_headers() -> Vec<(header::HeaderName, String)> {
    let mut headers = nocache();
    headers.push((header::CONTENT_TYPE, "text/html; charset=UTF-8".to_string()));
    headers.push((header::X_FRAME_OPTIONS, "SAMEORIGIN".to_string()));
    headers
}

/// The posts list's term and date filters, which `main_query()` reads.
const FILTERS: [&str; 5] = ["m", "cat", "category_name", "tag", "post_format"];

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
        base: true,
        ..ListArgs::default()
    };
    let mut vars = HashMap::new();
    for (key, value) in args.as_array().unwrap_or_default() {
        match (key, value) {
            (Key::String(key), Value::String(value)) if key == b"post_status" => {
                list.status = String::from_utf8(value.clone()).ok()?;
                list.base = false;
            }
            (Key::String(key), Value::String(value)) if key == b"post_type" && value == b"post" => {
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
                            _ => {}
                        }
                        vars.insert(name, value);
                    }
                }
                list.base = false;
            }
            (Key::String(key), Value::String(value)) if key == b"orderby" || key == b"order" => {
                let value = Some(String::from_utf8(value.clone()).ok()?);
                if key == b"orderby" {
                    list.orderby = value;
                } else {
                    list.order = value;
                }
                list.base = false;
            }
            _ => return None,
        }
    }
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
    let page = match path {
        "/hs-admin/tools" => hyscribe_admin::tools::tools(admin, &canonical, origin, browser),
        "/hs-admin/" | "/hs-admin/index" => {
            dashboard(admin, &canonical, origin, request_uri, browser)
        }
        "/hs-admin/edit" => {
            let args = list_args(admin, request_uri)?;
            match edit(admin, &args, &canonical, origin, request_uri, browser) {
                Some(Answer::Redirect(to)) => {
                    return Some(redirect(screen_headers(), to.as_bytes()));
                }
                Some(Answer::Page(html)) => Some(html),
                None => None,
            }
        }
        _ => return None,
    };
    Some(match page {
        Some(html) => response(StatusCode::OK, screen_headers(), html.into_bytes()),
        // `wp_die()` from `wp-admin/includes/menu.php`.
        None => {
            let body = hyscribe_admin::die::page(
                admin.reads.public(),
                "WordPress &rsaquo; Error",
                "Sorry, you are not allowed to access this page.",
            );
            response(StatusCode::FORBIDDEN, screen_headers(), body)
        }
    })
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
    let requested = uri.path_and_query().map_or("", |p| p.as_str());
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
                let admin = Admin::new(&reads, site, user);
                let header = |name: &str| headers.get(name).map(HeaderValue::as_bytes);
                let browser = Browser {
                    user_agent: header("user-agent").unwrap_or_default(),
                    mobile_hint: header("sec-ch-ua-mobile"),
                };
                let text = |bytes: &[u8]| String::from_utf8_lossy(bytes).into_owned();
                let url = text(&[scheme, &here].concat());
                let origin = text(&[scheme, host].concat());
                screen(&admin, uri.path(), requested, &url, &origin, &browser)
                    .unwrap_or_else(|| response(StatusCode::NOT_IMPLEMENTED, nocache(), Vec::new()))
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
