// SPDX-License-Identifier: AGPL-3.0-or-later

//! The posts' and pages' writes (MVP 4.3) where the goldens of
//! `reference/rest/post-writes.list` cannot reach: the plans the checks
//! give, the request bodies read, and the answers once written, over the
//! default install with the extra posts, a site changed where a case needs
//! it.

use hyscribe_model::{Comment, CommentQuery, CommentRow, Term, TermQuery, User, UserQuery};
use hyscribe_phpser::parse_str;
use hyscribe_render::site::Site as Reads;
use hyscribe_render::theme::Theme;

use super::posts::Oracle;
use super::site;
use crate::request::{array_merge, content_type, is_json};
use crate::*;

/// The default install with the extra posts, as `role` sees it.
fn oracle(role: &'static str, deny: &'static [&'static str]) -> Oracle {
    Oracle {
        role,
        fixture: site::Fixture {
            ttd: false,
            pretty: false,
            extra: true,
            state: None,
        },
        deny,
    }
}

/// The oracle's site with `options`, post meta and posts changed.
struct Changed {
    oracle: Oracle,
    options: Vec<(&'static str, &'static [u8])>,
    meta: Vec<(u64, &'static str, &'static [u8])>,
    posts: Vec<Post>,
}

impl Site for Changed {
    fn option(&self, name: &str) -> Option<Vec<u8>> {
        let changed = self.options.iter().find(|(known, _)| *known == name);
        changed.map_or_else(
            || self.oracle.option(name),
            |(_, value)| Some(value.to_vec()),
        )
    }
    fn rest_url(&self, path: &str) -> String {
        self.oracle.rest_url(path)
    }
    fn verify_nonce(&self, nonce: &[u8]) -> bool {
        self.oracle.verify_nonce(nonce)
    }
    fn create_nonce(&self) -> String {
        self.oracle.create_nonce()
    }
    fn nonce(&self, action: &str) -> String {
        self.oracle.nonce(action)
    }
    fn can(&self, user: u64, cap: &str, args: &[Value]) -> bool {
        self.oracle.can(user, cap, args)
    }
    fn terms(&self, query: &TermQuery) -> Vec<Term> {
        self.oracle.terms(query)
    }
    fn count_terms(&self, query: &TermQuery) -> u64 {
        self.oracle.count_terms(query)
    }
    fn term_link(&self, taxonomy: &str, term: &Term) -> String {
        self.oracle.term_link(taxonomy, term)
    }
    fn post(&self, id: u64) -> Option<Post> {
        let changed = self.posts.iter().find(|post| post.id == id).cloned();
        changed.or_else(|| self.oracle.post(id))
    }
    fn table_prefix(&self) -> String {
        self.oracle.table_prefix()
    }
    fn comments(&self, query: &CommentQuery) -> Vec<CommentRow> {
        self.oracle.comments(query)
    }
    fn count_comments(&self, query: &CommentQuery) -> u64 {
        self.oracle.count_comments(query)
    }
    fn comment_link(&self, comment: &Comment) -> String {
        self.oracle.comment_link(comment)
    }
    fn users(&self, query: &UserQuery) -> Vec<User> {
        self.oracle.users(query)
    }
    fn count_users(&self, query: &UserQuery) -> u64 {
        self.oracle.count_users(query)
    }
    fn user_meta(&self, id: u64, key: &str) -> Option<Vec<u8>> {
        self.oracle.user_meta(id, key)
    }
    fn user_registered(&self, id: u64) -> String {
        self.oracle.user_registered(id)
    }
    fn allcaps(&self, id: u64) -> Vec<(Key, Value)> {
        self.oracle.allcaps(id)
    }
    fn author_link(&self, user: &User) -> String {
        self.oracle.author_link(user)
    }
    fn has_posts(&self, author: u64, types: &[(String, bool)]) -> bool {
        self.oracle.has_posts(author, types)
    }
    fn posts_of_type(&self, post_type: &str) -> Vec<Post> {
        self.oracle.posts_of_type(post_type)
    }
    fn post_meta(&self, post: u64, key: &str) -> Option<Vec<u8>> {
        let changed = self
            .meta
            .iter()
            .find(|(id, name, _)| *id == post && *name == key);
        changed.map_or_else(
            || self.oracle.post_meta(post, key),
            |(_, _, value)| Some(value.to_vec()),
        )
    }
    fn revisions(&self, post: u64) -> Vec<u64> {
        self.oracle.revisions(post)
    }
    fn public_users(&self) -> bool {
        self.oracle.public_users()
    }
    fn reads(&self) -> &dyn Reads {
        self.oracle.reads()
    }
    fn theme(&self) -> &Theme {
        self.oracle.theme()
    }
}

/// The administrator's site with `options` and `posts` changed.
fn changed(options: Vec<(&'static str, &'static [u8])>, posts: Vec<Post>) -> Changed {
    Changed {
        oracle: oracle("administrator", &[]),
        options,
        meta: Vec::new(),
        posts,
    }
}

/// A request of `user` for `target` with `body` of the content type `kind`.
struct Asked {
    method: &'static str,
    route: Vec<u8>,
    get: Vec<(Key, Value)>,
    body: Vec<u8>,
    kind: Option<&'static [u8]>,
    post: Vec<(Key, Value)>,
    user: u64,
}

impl Asked {
    fn new(who: &str, method: &'static str, target: &str, body: &str) -> Asked {
        let (route, query) = target.split_once('&').unwrap_or((target, ""));
        let get = parse_str(query.as_bytes()).as_array().unwrap().to_vec();
        Asked {
            method,
            route: route.as_bytes().to_vec(),
            get,
            body: body.as_bytes().to_vec(),
            kind: Some(b"application/json"),
            post: Vec::new(),
            user: super::role(who).1,
        }
    }

    fn request(&self) -> Request<'_> {
        Request {
            method: self.method,
            route: &self.route,
            uri: b"/",
            get: &self.get,
            post: &self.post,
            cookie_user: self.user,
            origin: None,
            nonce: (self.user != 0).then_some(b"%nonce%"),
            method_override: None,
            content_type: self.kind,
            body: &self.body,
            host: None,
            now: 0,
        }
    }
}

/// What `asked` comes to on `site`: its answer, or its plan and the answer
/// once written as `outcome` says.
fn on(site: &dyn Site, asked: &Asked, outcome: Outcome) -> (Option<Plan>, Served) {
    let request = asked.request();
    match answer(site, &request) {
        Answer::Served(served) => (None, served),
        Answer::Write(pending) => {
            let plan = pending.plan.clone();
            (Some(plan), written(site, &request, *pending, outcome))
        }
    }
}

/// The plan of `who`'s `method target` with the JSON `body`.
fn plan(who: &'static str, method: &'static str, target: &str, body: &str) -> Save {
    let (plan, _) = on(
        &oracle(who, &[]),
        &Asked::new(who, method, target, body),
        Outcome::Written(1),
    );
    match plan.map(|plan| plan.write) {
        Some(Write::Save(save)) => *save,
        other => panic!("{other:?}"),
    }
}

/// The answer to `who`'s `method target` with the JSON `body`, written as
/// `outcome` says where it plans a write.
fn answered(who: &'static str, method: &'static str, target: &str, body: &str) -> Served {
    on(
        &oracle(who, &[]),
        &Asked::new(who, method, target, body),
        Outcome::Written(1),
    )
    .1
}

fn text_of(served: &Served) -> String {
    String::from_utf8_lossy(&served.body).into_owned()
}

/// A create gives `wp_insert_post()` the fields given, sanitized, and what
/// to do after: a new post is unstuck unless asked, its terms set, its
/// featured image and template cleared; its answer is a 201 with its
/// `Location`.
#[test]
fn a_create_plans_its_fields() {
    let body = r#"{"title":"T","content":"C","excerpt":{"raw":"E"},"status":"draft","slug":"A Slug","author":1,"comment_status":"closed","ping_status":"open","categories":[1,-3],"tags":[],"featured_media":0,"template":"","date":"2026-01-02T03:04:05"}"#;
    let save = plan("administrator", "POST", "/wp/v2/posts", body);
    let some = |text: &str| Some(text.as_bytes().to_vec());
    assert_eq!(
        save,
        Save {
            post_type: "post".into(),
            title: some("T"),
            content: some("C"),
            excerpt: some("E"),
            status: Some("draft".into()),
            // The extra posts' site is two hours east of UTC.
            date: Some(("2026-01-02 03:04:05".into(), "2026-01-02 01:04:05".into())),
            edit_date: true,
            slug: some("a-slug"),
            unique_slug: Some(0),
            author: Some(1),
            comment_status: Some("closed".into()),
            ping_status: Some("open".into()),
            sticky: Some(false),
            unset_thumbnail: true,
            clear_template: true,
            categories: Some(vec![1]),
            tags: Some(Vec::new()),
            ..Save::default()
        }
    );
    let served = answered("administrator", "POST", "/wp/v2/posts", body);
    assert_eq!(served.status, 201);
    let location = served.headers.iter().find(|(name, _)| name == "Location");
    let posts = "http://127.0.0.1:8081/index.php?rest_route=/wp/v2/posts/1";
    assert_eq!(location.map(|(_, value)| value.as_str()), Some(posts));
}

/// The dates as `rest_get_date_with_gmt()` reads them: with a zone of
/// their own, or the site's, and a null resetting both.
#[test]
fn dates_are_read_with_their_zones() {
    for (body, date) in [
        (
            r#"{"date":"2026-01-02T03:04:05Z"}"#,
            ("2026-01-02 05:04:05", "2026-01-02 03:04:05"),
        ),
        (
            r#"{"date":"2026-01-02T03:04:05+01:00"}"#,
            ("2026-01-02 04:04:05", "2026-01-02 02:04:05"),
        ),
        (
            r#"{"date":"2026-01-02T03:04:05-05"}"#,
            ("2026-01-02 10:04:05", "2026-01-02 08:04:05"),
        ),
        (
            r#"{"date_gmt":"2026-01-02T03:04:05"}"#,
            ("2026-01-02 05:04:05", "2026-01-02 03:04:05"),
        ),
        (
            r#"{"date":"2026-01-02T03:04:05","date_gmt":null}"#,
            ("", ""),
        ),
    ] {
        let save = plan("administrator", "POST", "/wp/v2/posts/1", body);
        let date = Some((date.0.to_string(), date.1.to_string()));
        assert_eq!(save.date, date, "{body}");
    }
    // The post's own date changes nothing.
    let post = oracle("", &[]).post(1).unwrap();
    let local = post.date.replace(' ', "T");
    let save = plan(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        &format!(r#"{{"date":"{local}"}}"#),
    );
    assert_eq!((save.date, save.edit_date), (None, false));
    let gmt = post.date_gmt.replace(' ', "T");
    let body = format!(r#"{{"date_gmt":"{gmt}"}}"#);
    let save = plan("administrator", "POST", "/wp/v2/posts/1", &body);
    assert_eq!((save.date, save.edit_date), (None, false));
}

/// `title`, `content` and `excerpt` are strings or arrays' `raw`, an empty
/// title's not taken, and a `raw` that is not a string not ported.
#[test]
fn texts_are_strings_or_raw() {
    let body = r#"{"title":{"raw":""},"content":{"raw":null},"excerpt":5,"slug":"0"}"#;
    let save = plan("administrator", "POST", "/wp/v2/posts/1", body);
    assert_eq!((save.title, save.content, save.excerpt), (None, None, None));
    assert_eq!(save.unique_slug, None);
    let body = r#"{"title":{"raw":"R"},"content":{"raw":""}}"#;
    let save = plan("administrator", "POST", "/wp/v2/posts/1", body);
    let texts = (save.title.as_deref(), save.content.as_deref());
    assert_eq!(texts, (Some(&b"R"[..]), Some(&b""[..])));
    let served = answered(
        "administrator",
        "POST",
        "/wp/v2/posts",
        r#"{"content":{"raw":5}}"#,
    );
    assert_eq!(served.status, 501);
}

/// Publishing and private posts need `publish_posts`; the post's own status
/// is valid whatever the schema says, and changes nothing.
#[test]
fn statuses_need_their_capabilities() {
    let refused = answered(
        "contributor",
        "POST",
        "/wp/v2/posts",
        r#"{"title":"x","status":"private"}"#,
    );
    assert!(
        text_of(&refused).contains("create private posts"),
        "{}",
        text_of(&refused)
    );
    let save = plan(
        "editor",
        "POST",
        "/wp/v2/posts",
        r#"{"title":"x","status":"future"}"#,
    );
    assert_eq!(save.status.as_deref(), Some("future"));
    let body = r#"{"title":"x","status":"publish"}"#;
    let refused = answered("contributor", "POST", "/wp/v2/posts", body);
    assert!(
        text_of(&refused).contains("publish posts"),
        "{}",
        text_of(&refused)
    );
    // A draft is given no unique slug without one asked for.
    let save = plan(
        "author",
        "POST",
        "/wp/v2/posts",
        r#"{"title":"x","status":"draft"}"#,
    );
    assert_eq!(save.unique_slug, None);
    // An author may publish a new post, `publish_posts`, as no post yet.
    let asked = Asked::new("author", "POST", "/wp/v2/posts", r#"{"title":"x"}"#);
    let (planned, _) = on(&oracle("author", &[]), &asked, Outcome::Written(1));
    assert!(planned.is_some_and(|planned| planned.can_publish));
    let save = plan(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        r#"{"status":"publish"}"#,
    );
    assert_eq!(save.status, None);
    // A draft's own status, not one a client may ask for, passes too.
    let draft = Post {
        status: "auto-draft".into(),
        ..oracle("", &[]).post(1).unwrap()
    };
    let site = changed(Vec::new(), vec![draft]);
    let asked = Asked::new(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        r#"{"status":"auto-draft"}"#,
    );
    let (plan, _) = on(&site, &asked, Outcome::Written(1));
    assert!(plan.is_some());
}

/// Another author must exist, and an ID below 1 never does.
#[test]
fn authors_are_checked() {
    let save = plan(
        "editor",
        "POST",
        "/wp/v2/posts",
        r#"{"title":"x","author":1}"#,
    );
    assert_eq!(save.author, Some(1));
    for author in ["999", "-1"] {
        let body = format!(r#"{{"title":"x","author":{author}}}"#);
        let served = answered("administrator", "POST", "/wp/v2/posts", &body);
        assert!(text_of(&served).contains("rest_invalid_author"), "{author}");
    }
}

/// A sticky post takes no password, nor a protected post stickiness, which
/// reads the post password cookie and is not ported.
#[test]
fn sticky_posts_have_no_password() {
    let sticky = changed(vec![("sticky_posts", b"a:1:{i:0;i:1;}")], Vec::new());
    let asked = Asked::new(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        r#"{"password":"pw"}"#,
    );
    let (_, served) = on(&sticky, &asked, Outcome::Written(1));
    assert!(text_of(&served).contains("A sticky post can not be password protected."));
    // No password, or a new post, is no matter: not even with 0 among the
    // sticky posts.
    let asked = Asked::new(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        r#"{"password":""}"#,
    );
    assert!(on(&sticky, &asked, Outcome::Written(1)).0.is_some());
    let zero = changed(vec![("sticky_posts", b"a:1:{i:0;i:0;}")], Vec::new());
    let asked = Asked::new(
        "administrator",
        "POST",
        "/wp/v2/posts",
        r#"{"password":"pw"}"#,
    );
    assert!(on(&zero, &asked, Outcome::Written(1)).0.is_some());
    let protected = Post {
        password: b"pw".to_vec(),
        ..oracle("", &[]).post(1).unwrap()
    };
    let site = changed(Vec::new(), vec![protected]);
    let asked = Asked::new(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        r#"{"sticky":true}"#,
    );
    assert_eq!(on(&site, &asked, Outcome::Written(1)).1.status, 501);
    let asked = Asked::new(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        r#"{"title":"x"}"#,
    );
    assert!(on(&site, &asked, Outcome::Written(1)).0.is_some());
    let save = plan(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        r#"{"password":"","sticky":true}"#,
    );
    assert_eq!(save.sticky, Some(true));
    let save = plan(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        r#"{"password":"","sticky":false}"#,
    );
    assert_eq!(
        (save.password, save.sticky),
        (Some(Vec::new()), Some(false))
    );
}

/// A page's parent must exist; none is 0.
#[test]
fn parents_are_checked() {
    let save = plan(
        "administrator",
        "POST",
        "/wp/v2/pages",
        r#"{"title":"x","parent":2,"menu_order":4}"#,
    );
    assert_eq!((save.parent, save.menu_order), (Some(2), Some(4)));
    let save = plan(
        "administrator",
        "PUT",
        "/wp/v2/pages/2",
        r#"{"parent":0,"slug":"x","status":"draft"}"#,
    );
    assert_eq!((save.parent, save.unique_slug), (Some(0), Some(0)));
    for parent in ["999", "-2"] {
        let body = format!(r#"{{"title":"x","parent":{parent}}}"#);
        let served = answered("administrator", "POST", "/wp/v2/pages", &body);
        assert!(
            text_of(&served).contains("Invalid post parent ID."),
            "{parent}"
        );
    }
}

/// A featured image that is an attachment is not ported; any other post
/// or none clears it, an ID with no post leaves it.
#[test]
fn featured_images_are_cleared() {
    let featured = |media: &str| {
        let body = format!(r#"{{"featured_media":{media}}}"#);
        plan("administrator", "POST", "/wp/v2/posts/1", &body).unset_thumbnail
    };
    assert!(featured("2") && featured("-2") && !featured("12345"));
    let served = answered(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        r#"{"featured_media":97}"#,
    );
    assert_eq!(served.status, 501);
}

/// Content whose answer is not ported, as stored after kses or unfiltered,
/// is not written; nor `format`, a template, `meta`, or content with
/// ignored hooked blocks.
#[test]
fn unported_writes_are_not_planned() {
    // A login block shows the logged in user more, as every writer is.
    let login = r#"{"title":"x","content":"<!-- wp:loginout /-->"}"#;
    assert_eq!(
        answered("administrator", "POST", "/wp/v2/posts", login).status,
        501
    );
    let post = Post {
        content: b"<!-- wp:loginout /-->".to_vec(),
        ..oracle("", &[]).post(1).unwrap()
    };
    let site = changed(Vec::new(), vec![post]);
    let asked = Asked::new("administrator", "DELETE", "/wp/v2/posts/1", "");
    assert_eq!(on(&site, &asked, Outcome::Written(1)).1.status, 501);
    let gallery = r#"{"title":"x","content":"[gallery]"}"#;
    assert_eq!(
        answered("administrator", "POST", "/wp/v2/posts", gallery).status,
        501
    );
    // kses drops the unknown tag, which leaves the shortcode.
    let hidden = r#"{"title":"x","content":"[<x>gallery]"}"#;
    assert_eq!(
        answered("administrator", "POST", "/wp/v2/posts", hidden).status,
        501
    );
    let unfiltered = oracle("administrator", &["DISALLOW_UNFILTERED_HTML"]);
    let asked = Asked::new("administrator", "POST", "/wp/v2/posts", hidden);
    let (plan, _) = on(&unfiltered, &asked, Outcome::Written(1));
    assert!(plan.is_some_and(|plan| plan.unfiltered_html));
    for body in [
        r#"{"format":"aside"}"#,
        r#"{"template":"x"}"#,
        r#"{"meta":null}"#,
    ] {
        assert_eq!(
            answered("administrator", "POST", "/wp/v2/posts/1", body).status,
            501,
            "{body}"
        );
    }
    let mut site = changed(Vec::new(), Vec::new());
    site.meta
        .push((1, "_wp_ignored_hooked_blocks", b"[\"core/x\"]"));
    let asked = Asked::new(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        r#"{"content":"x"}"#,
    );
    assert_eq!(on(&site, &asked, Outcome::Written(1)).1.status, 501);
}

/// A delete with `force` answers with the post as it was, prepared before;
/// without, it trashes a post not yet trashed whose answer is ported.
#[test]
fn deletes_trash_unless_forced() {
    let (plan, served) = on(
        &oracle("administrator", &[]),
        &Asked::new(
            "administrator",
            "DELETE",
            "/wp/v2/posts/1&force=1&context=view",
            "",
        ),
        Outcome::Written(1),
    );
    assert_eq!(plan.map(|plan| plan.write), Some(Write::Delete(1)));
    let body = text_of(&served);
    assert!(
        body.starts_with(r#"{"deleted":true,"previous":{"id":1,"#),
        "{body}"
    );
    assert!(body.contains(r#""raw":"Hello world!""#) && !body.contains("_links"));
    let served = answered("administrator", "DELETE", "/wp/v2/posts/1", "");
    assert_eq!(served.status, 200);
    let trashed = Post {
        status: "trash".into(),
        ..oracle("", &[]).post(1).unwrap()
    };
    let site = changed(Vec::new(), vec![trashed]);
    let asked = Asked::new("administrator", "DELETE", "/wp/v2/posts/1", "");
    assert!(text_of(&on(&site, &asked, Outcome::Written(1)).1).contains("rest_already_trashed"));
    // The extra post 94 has a gallery shortcode.
    let shortcode = Asked::new("administrator", "DELETE", "/wp/v2/posts/94", "");
    assert_eq!(
        on(
            &oracle("administrator", &[]),
            &shortcode,
            Outcome::Written(94)
        )
        .1
        .status,
        501
    );
}

/// What PHP refuses without writing is answered with its error.
#[test]
fn refusals_are_php_s() {
    for (code, status) in [("empty_content", 400), ("rest_cannot_delete", 500)] {
        let asked = Asked::new(
            "administrator",
            "POST",
            "/wp/v2/posts/1",
            r#"{"title":"x"}"#,
        );
        let (_, served) = on(
            &oracle("administrator", &[]),
            &asked,
            Outcome::Refused(code),
        );
        assert_eq!(served.status, status, "{code}");
        assert!(text_of(&served).contains(code), "{code}");
    }
}

/// The body read as `get_parameter_order()` reads it: JSON for a JSON type,
/// a form for a method other than POST, a multipart form and JSON PHP
/// refuses not at all.
#[test]
fn bodies_are_read_as_php_reads_them() {
    let status = |method: &'static str, kind: Option<&'static [u8]>, body: &str| {
        let mut asked = Asked::new("administrator", method, "/wp/v2/posts/1", body);
        asked.kind = kind;
        on(&oracle("administrator", &[]), &asked, Outcome::Written(1))
    };
    let form = Some(&b"application/x-www-form-urlencoded"[..]);
    for (method, kind) in [("PUT", None), ("PATCH", form), ("PUT", Some(&b"text"[..]))] {
        let (plan, _) = status(method, kind, "title=Form");
        let title = plan.and_then(|plan| match plan.write {
            Write::Save(save) => save.title,
            _ => None,
        });
        assert_eq!(title.as_deref(), Some(&b"Form"[..]), "{method}");
    }
    // A form for POST is `$_POST`, which the server reads.
    let (plan, _) = status("POST", form, "title=Form");
    assert!(plan.is_some_and(|plan| plan.write
        == Write::Save(Box::new(Save {
            id: 1,
            post_type: "post".into(),
            ..Save::default()
        }))));
    for body in ["", "5", "0", "null"] {
        assert!(
            status("POST", Some(b"application/json"), body).0.is_some(),
            "{body}"
        );
    }
    assert_eq!(status("POST", Some(b"application/json"), "{").1.status, 501);
    let multipart = Some(&b"multipart/form-data; boundary=x"[..]);
    assert_eq!(status("POST", multipart, "--x--").1.status, 501);
    // An empty multipart form gives `$_POST` nothing.
    assert!(status("POST", multipart, "").0.is_some());
    // `$_POST` of a request PHP reads as a GET is not among its parameters.
    let mut asked = Asked::new("", "POST", "/wp/v2/posts/1&_method=GET", "");
    asked.post = vec![(Key::String(b"context".to_vec()), text_value("edit"))];
    asked.kind = form;
    assert_eq!(
        on(&oracle("", &[]), &asked, Outcome::Written(1)).1.status,
        200
    );
    // A body "0" is empty: a write not ported is checked without arguments,
    // not refused as given some.
    let mut asked = Asked::new("", "PUT", "/wp/v2/categories/1", "0");
    asked.kind = None;
    assert_ne!(
        on(&oracle("", &[]), &asked, Outcome::Written(1)).1.status,
        501
    );
    // A write not ported is not answered with a body either.
    let mut asked = Asked::new(
        "administrator",
        "POST",
        "/wp/v2/categories",
        r#"{"name":"x"}"#,
    );
    asked.kind = Some(b"application/json");
    assert_eq!(
        on(&oracle("administrator", &[]), &asked, Outcome::Written(1))
            .1
            .status,
        501
    );
}

/// `get_content_type()` and `wp_is_json_media_type()`.
#[test]
fn json_types_are_php_s() {
    assert_eq!(content_type(None), None);
    assert_eq!(content_type(Some(b"0")), None);
    assert_eq!(content_type(Some(b"text")), None);
    assert_eq!(
        content_type(Some(b" Text/Plain ; x")).as_deref(),
        Some(&b"text/plain"[..])
    );
    assert_eq!(content_type(Some(b";a/b")).as_deref(), Some(&b";a/b"[..]));
    for (media, json) in [
        ("application/json", true),
        ("x, application/vnd.api+json;", true),
        ("application/a+b+json+oembed", true),
        ("application/json+oembed,", true),
        ("a,application/a-b+json", true),
        ("application/+json", false),
        ("xapplication/json", false),
        ("application/jsonx", false),
        ("application/a~+json", false),
        ("text/plain", false),
    ] {
        assert_eq!(is_json(media.as_bytes()), json, "{media}");
    }
}

/// `array_merge()`: integer keys numbered afresh, string keys replaced.
#[test]
fn arrays_merge_as_php_merges_them() {
    let key = |name: &str| Key::String(name.as_bytes().to_vec());
    let merged = array_merge(
        vec![(Key::Int(5), Value::Int(1)), (key("a"), Value::Int(2))],
        vec![(key("a"), Value::Int(3)), (Key::Int(5), Value::Int(4))],
    );
    let expected = vec![
        (Key::Int(0), Value::Int(1)),
        (key("a"), Value::Int(3)),
        (Key::Int(1), Value::Int(4)),
    ];
    assert_eq!(merged, expected);
}

fn text_value(text: &str) -> Value {
    Value::String(text.as_bytes().to_vec())
}

/// The answer once written is in the `edit` context, which the request
/// need not give.
#[test]
fn answers_are_in_the_edit_context() {
    let served = answered(
        "administrator",
        "POST",
        "/wp/v2/posts/1",
        r#"{"title":"x"}"#,
    );
    assert!(
        text_of(&served).contains(r#""raw":"#),
        "{}",
        text_of(&served)
    );
}

/// Each type's own arguments are checked by their schemas: a post's
/// `sticky` and terms, a page's `parent` and `menu_order`, and `force`.
#[test]
fn each_type_checks_its_own_arguments() {
    for (target, body, param) in [
        ("/wp/v2/pages", r#"{"title":"x","parent":"abc"}"#, "parent"),
        (
            "/wp/v2/pages",
            r#"{"title":"x","menu_order":"abc"}"#,
            "menu_order",
        ),
        (
            "/wp/v2/posts",
            r#"{"title":"x","sticky":"maybe"}"#,
            "sticky",
        ),
        (
            "/wp/v2/posts",
            r#"{"title":"x","categories":"abc"}"#,
            "categories",
        ),
        ("/wp/v2/posts", r#"{"title":"x","tags":["abc"]}"#, "tags"),
    ] {
        let served = answered("administrator", "POST", target, body);
        let text = text_of(&served);
        assert!(
            text.contains(&format!("Invalid parameter(s): {param}")),
            "{text}"
        );
    }
    let served = answered("administrator", "DELETE", "/wp/v2/posts/1&force=maybe", "");
    assert!(text_of(&served).contains("Invalid parameter(s): force"));
}

/// The `edit` context's raw content has its blocks serialized again, a
/// template part named after the theme, where it has blocks.
#[test]
fn raw_content_gets_its_block_hooks() {
    let raw = |content: &str| {
        let post = Post {
            content: content.as_bytes().to_vec(),
            ..oracle("", &[]).post(1).unwrap()
        };
        let site = changed(Vec::new(), vec![post]);
        let target = "/wp/v2/posts/1&context=edit&_fields=content.raw,content.block_version";
        let asked = Asked::new("administrator", "GET", target, "");
        text_of(&on(&site, &asked, Outcome::Written(1)).1)
    };
    let hooked = raw(r#"<!-- wp:core/template-part {"slug":"footer"} /-->"#);
    let theme = r#"{\"slug\":\"footer\",\"theme\":\"twentytwentyfive\"}"#;
    assert!(hooked.contains(theme), "{hooked}");
    assert!(hooked.contains(r#""block_version":1"#), "{hooked}");
    // A block PHP's `has_blocks()` misses stays in its freeform block, as
    // PHP leaves it (`apply_block_hooks_to_content_from_post_object()`).
    let missed = raw(r#"<!--  wp:core/template-part {"slug":"footer"} /-->"#);
    let freeform =
        format!(r#"<!-- wp:freeform --><!-- wp:template-part {theme} \/--><!-- \/wp:freeform -->"#);
    assert!(missed.contains(&freeform), "{missed}");
    assert!(missed.contains(r#""block_version":0"#), "{missed}");
    let classic = raw("<p>Classic</p>");
    assert!(
        classic.contains(r#""raw":"<p>Classic<\/p>","block_version":0"#),
        "{classic}"
    );
}

/// The `edit` context renders the content again, as
/// `insert_hooked_blocks_into_rest_response()` does: a block's unique ID
/// is the second one there, the first in the `view` context.
#[test]
fn the_edit_context_renders_twice() {
    let post = Post {
        content: br#"<!-- wp:archives {"displayAsDropdown":true} /-->"#.to_vec(),
        ..oracle("", &[]).post(1).unwrap()
    };
    let site = changed(Vec::new(), vec![post]);
    for (context, id) in [
        ("edit", "wp-block-archives-2"),
        ("view", "wp-block-archives-1"),
    ] {
        let target = format!("/wp/v2/posts/1&context={context}&_fields=content");
        let asked = Asked::new("administrator", "GET", &target, "");
        let body = text_of(&on(&site, &asked, Outcome::Written(1)).1);
        assert!(body.contains(id), "{context}: {body}");
    }
}

/// The checks the goldens make over HTTP, in this crate's tests too: terms
/// the user may not assign, a sticky post with a password, an ID on a
/// create, a `raw` not a string, a pending post's slug, and a forced
/// delete whose answer is not ported.
#[test]
fn checks_refuse_as_php_refuses() {
    let terms = oracle("administrator", &["assign_term"]);
    let body = r#"{"title":"x","categories":[1]}"#;
    let asked = Asked::new("administrator", "POST", "/wp/v2/posts", body);
    let served = on(&terms, &asked, Outcome::Written(1)).1;
    assert!(text_of(&served).contains("rest_cannot_assign_term"));
    let both = r#"{"title":"x","password":"pw","sticky":true}"#;
    let served = answered("administrator", "POST", "/wp/v2/posts", both);
    assert!(text_of(&served).contains("A post can not be sticky and have a password."));
    let served = answered(
        "administrator",
        "POST",
        "/wp/v2/posts",
        r#"{"title":"x","id":0}"#,
    );
    assert_eq!(served.status, 404);
    for body in [
        r#"{"title":{"raw":5}}"#,
        r#"{"title":"x","excerpt":{"raw":5}}"#,
    ] {
        let served = answered("administrator", "POST", "/wp/v2/posts", body);
        assert_eq!(served.status, 501, "{body}");
    }
    let body = r#"{"title":"x","status":"pending","slug":"p"}"#;
    let save = plan("administrator", "POST", "/wp/v2/posts", body);
    assert_eq!(save.unique_slug, Some(0));
    let served = answered("administrator", "DELETE", "/wp/v2/posts/94&force=1", "");
    assert_eq!(served.status, 501);
}

/// Only the posts' and pages' deletes are planned: a synced pattern's is
/// checked, then not ported.
#[test]
fn other_types_plan_no_delete() {
    let pattern = Post {
        post_type: "wp_block".into(),
        ..oracle("", &[]).post(1).unwrap()
    };
    let site = changed(Vec::new(), vec![pattern]);
    let asked = Asked::new("administrator", "DELETE", "/wp/v2/blocks/1", "");
    let (plan, served) = on(&site, &asked, Outcome::Written(1));
    assert_eq!((plan, served.status), (None, 501));
}

/// An upload to the media collection plans its fields, refuses a parent
/// that is a revision or an attachment and answers 501 for the fields not
/// ported; once written it answers the attachment with `201`, or the
/// upload's refusal.
#[test]
fn uploads_plan_their_fields() {
    let body = r#"{"title":"T","caption":{"raw":"C"},"description":"D","alt_text":"A","post":1}"#;
    let asked = Asked::new("administrator", "POST", "/wp/v2/media&_fields=id", body);
    let site = oracle("administrator", &[]);
    // The extra posts' attachment 95, written; its answer only its ID.
    let (plan, served) = on(&site, &asked, Outcome::Written(95));
    let some = |text: &str| Some(text.as_bytes().to_vec());
    let upload = Upload {
        title: some("T"),
        caption: some("C"),
        description: some("D"),
        alt_text: some("A"),
        post: 1,
    };
    let plan = plan.unwrap();
    assert_eq!(plan.write, Write::Upload(Box::new(upload)));
    assert!(!plan.unfiltered_html);
    assert_eq!(served.status, 201, "{}", text_of(&served));
    let location = served.headers.iter().find(|(name, _)| name == "Location");
    assert!(location.is_some_and(|(_, url)| url.ends_with("/wp/v2/media/95")));
    let refused = Outcome::Failed("rest_upload_no_data", "No data supplied.".into(), 400);
    let (_, served) = on(&site, &asked, refused);
    assert_eq!(served.status, 400);
    assert!(text_of(&served).contains(r#""code":"rest_upload_no_data""#));
    // The draft's image 97 and post 1's revision 99 are no parents.
    for parent in [97, 99] {
        let body = format!(r#"{{"post":{parent}}}"#);
        let served = answered("administrator", "POST", "/wp/v2/media", &body);
        assert_eq!(served.status, 400, "{parent}");
        assert!(
            text_of(&served).contains("Invalid parent type."),
            "{parent}"
        );
    }
    for body in [r#"{"status":"private"}"#, r#"{"generate_sub_sizes":false}"#] {
        let served = answered("administrator", "POST", "/wp/v2/media", body);
        assert_eq!(served.status, 501, "{body}");
    }
    let asked = Asked::new("administrator", "POST", "/wp/v2/media&slug=a", "");
    assert_eq!(on(&site, &asked, Outcome::Written(95)).1.status, 501);
}
