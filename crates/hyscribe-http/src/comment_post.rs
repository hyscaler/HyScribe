// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp-comments-post.php`: a comment saved in one writer action (ADR 0010),
//! refused in shadow mode; its notices are mailed once it commits (ADR 0016).

pub(crate) mod submit;

use std::future::Future;
use std::net::IpAddr;
use std::pin::Pin;
use std::sync::Arc;

use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::Response;
use hyscribe_admin::assets::Urls;
use hyscribe_admin::die::{page, page_with_back_link};
use hyscribe_admin::login::READ_ONLY;
use hyscribe_auth::caps::{CapPost, User as Capabilities, post_status};
use hyscribe_db::{DbError, Unapproved, Write};
use hyscribe_kses::esc_url;
use hyscribe_phpser::{Value, add_query_arg, addslashes, is_empty_string};
use hyscribe_render::site::Site as RenderSite;
use hyscribe_write::comments::{CommentCookie, Commenter, Context, sanitize_comment_cookie};
use tokio::runtime::Handle;

use crate::login::{
    LoginReads, Names, YEAR, cookie, cookies, joined, nocache, response, set_cookie,
};
use crate::mail::{Commented, MailSite, comment_notices};
use crate::redirect::wp_safe_redirect;
use crate::{Site, now, trackback};
use submit::{Failure, Fields, Poster};

/// The path of `wp-comments-post.php` under the site URL.
pub(crate) const PATH: &str = "/wp-comments-post.php";

/// A request for `wp-comments-post.php`.
pub(crate) struct Asked {
    /// Whether it is a POST.
    pub(crate) post: bool,
    pub(crate) headers: HeaderMap,
    pub(crate) body: axum::body::Bytes,
    /// The address the connection comes from.
    pub(crate) peer: Option<IpAddr>,
}

/// `$_POST` from a form body, URL-encoded or multipart; `None` for a body
/// only PHP reads (a multipart one this reader cannot take).
pub fn read_form(content_type: &[u8], body: &[u8]) -> Option<Value> {
    let form = match crate::multipart::boundary(content_type) {
        Some(boundary) => crate::multipart::read(body, boundary)?.post(),
        None => {
            let mut headers = HeaderMap::new();
            if let Ok(kind) = HeaderValue::from_bytes(content_type) {
                headers.insert(header::CONTENT_TYPE, kind);
            }
            let form = trackback::form(&headers, body);
            let read = body.is_empty() || form.as_array().is_some_and(|form| !form.is_empty());
            read.then_some(form)?
        }
    };
    // `$_POST['redirect_to'] . '#comment-'` of an array warns before PHP redirects.
    let array = |value: &Value| matches!(value, Value::Array(entries) if !entries.is_empty());
    let redirect = form.get_str("redirect_to").is_some_and(array);
    (!redirect).then_some(form)
}

/// `wp_get_current_commenter()`: the commenter cookies of a request, as
/// `sanitize_comment_cookies()` leaves them.
pub(crate) fn commenter(site: &Site, headers: &HeaderMap) -> hyscribe_render::site::Commenter {
    let values = headers.get_all(header::COOKIE).iter();
    let jar = cookies(values.map(HeaderValue::as_bytes));
    commenter_in(&jar, &Names::new(site).hash)
}

/// [`commenter`] of the cookies `jar` holds, under the cookie hash `hash`.
fn commenter_in(jar: &[(Vec<u8>, Vec<u8>)], hash: &str) -> hyscribe_render::site::Commenter {
    let read = |kind, name: &str| {
        cookie(jar, &format!("{name}_{hash}"))
            .map_or_else(Vec::new, |value| sanitize_comment_cookie(kind, value))
    };
    hyscribe_render::site::Commenter {
        author: read(CommentCookie::Author, "comment_author"),
        email: read(CommentCookie::Email, "comment_author_email"),
        url: read(CommentCookie::Url, "comment_author_url"),
    }
}

/// The answer to a request for `wp-comments-post.php`.
pub(crate) async fn respond(site: &Arc<Site>, asked: Asked) -> Response {
    if !asked.post {
        let headers = vec![
            (header::ALLOW, "POST".to_string()),
            (header::CONTENT_TYPE, "text/plain".to_string()),
        ];
        return response(StatusCode::METHOD_NOT_ALLOWED, headers, Vec::new());
    }
    let kind = asked.headers.get(header::CONTENT_TYPE);
    let kind = kind.map_or(&b""[..], HeaderValue::as_bytes);
    let Some(form) = read_form(kind, &asked.body) else {
        return response(StatusCode::NOT_IMPLEMENTED, nocache(), Vec::new());
    };
    let site = Arc::clone(site);
    let (now, handle) = (now(), Handle::current());
    joined(
        tokio::task::spawn_blocking(move || match &site.writer {
            Some(writer) => {
                let begin: Begin<'_> = Box::pin(writer.begin());
                answer(&site, &asked, &form, (now, handle), begin)
            }
            // Shadow mode, as an admin action there.
            None => {
                let body = page(site.public(), "WordPress &rsaquo; Error", READ_ONLY);
                response(StatusCode::FORBIDDEN, html_headers(), body)
            }
        })
        .await,
    )
}

/// The start of the writer action a comment is saved in, run once needed.
type Begin<'w> = Pin<Box<dyn Future<Output = Result<Write<'w>, DbError>> + Send + 'w>>;

/// `nocache_headers()` and PHP's default content type.
fn html_headers() -> Vec<(header::HeaderName, String)> {
    let mut headers = nocache();
    headers.push((header::CONTENT_TYPE, "text/html; charset=UTF-8".to_string()));
    headers
}

/// The post's answer, on a blocking thread: the current user from the
/// logged in cookie, then the submission in the action `begin` starts.
fn answer<'w>(
    site: &'w Site,
    asked: &Asked,
    form: &Value,
    (now, handle): (i64, Handle),
    begin: Begin<'w>,
) -> Response {
    let mut reads = LoginReads::new(site, handle.clone(), now);
    let jar = cookies(
        asked
            .headers
            .get_all(header::COOKIE)
            .iter()
            .map(HeaderValue::as_bytes),
    );
    let names = Names::new(site);
    let user = reads.logged_in_user(cookie(&jar, &names.logged_in()), true);
    let id = user.as_ref().map_or(0, |(id, _)| *id);
    if let Some((id, token)) = user {
        reads.act_as(id, token);
    }
    let caps = Capabilities::load(&reads, site.db.prefix(), id);
    let fields = Fields::read(form);
    let nonce = form.get_str("_wp_unfiltered_html_comment");
    let action = format!("unfiltered-html-comment_{}", fields.post_id);
    let nonce = nonce.map(Value::to_php_string).unwrap_or_default();
    let verified = reads.verify_nonce(&nonce, &action);
    let can = |cap| caps.has_cap(&reads, cap, &[]);
    let commenter = Commenter {
        moderate_comments: can("moderate_comments"),
        manage_options: can("manage_options"),
        unfiltered_html: verified && can("unfiltered_html"),
    };
    let saving = Saving {
        fields,
        jar: &jar,
        names: &names,
        asked,
        user: id,
        commenter,
        commenter_email: commenter_in(&jar, &names.hash).email,
    };
    let saved = save(&mut reads, &caps, saving, (&handle, begin));
    let answer = match saved {
        Ok(Ok(saved)) => redirect(site, (&reads, &names), form, id, &saved),
        Ok(Err(Failure::Silent)) => response(StatusCode::OK, html_headers(), Vec::new()),
        Ok(Err(Failure::Died(status, message))) => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            let message = format!("<p>{message}</p>");
            let body = page_with_back_link(site.public(), "Comment Submission Failure", &message);
            response(status, html_headers(), body)
        }
        Err(hyscribe_write::Error::Unported(_)) => {
            response(StatusCode::NOT_IMPLEMENTED, nocache(), Vec::new())
        }
        Err(err) => crate::html(crate::database_error(site, &format!("{err:?}"))),
    };
    match reads.reads.finish() {
        Ok(()) => answer,
        Err(err) => crate::html(crate::database_error(site, &err)),
    }
}

/// What a submission is made of.
struct Saving<'a> {
    fields: Fields,
    jar: &'a [(Vec<u8>, Vec<u8>)],
    names: &'a Names,
    asked: &'a Asked,
    user: u64,
    commenter: Commenter,
    /// `wp_get_current_commenter()`'s email, from the cookies.
    commenter_email: Vec<u8>,
}

/// A saved comment: what its cookies and redirect show.
struct Saved {
    id: u64,
    author: Vec<u8>,
    email: Vec<u8>,
    url: Vec<u8>,
    approved: String,
    date_gmt: String,
    /// `get_comment_link()`.
    link: String,
}

/// The submission in one action, committed once the comment is saved:
/// the post's status and `read_post` are worked out between its two parts.
fn save<'w>(
    reads: &mut LoginReads,
    caps: &Capabilities,
    saving: Saving,
    (handle, begin): (&Handle, Begin<'w>),
) -> Result<Result<Saved, Failure>, hyscribe_write::Error> {
    let mut write = handle.block_on(begin)?;
    let found = handle.block_on(submit::post(&mut write, &saving.fields))?;
    let post = match found {
        Ok(Some(post)) => post,
        Ok(None) => return Ok(Err(Failure::Silent)),
        Err(failure) => return Ok(Err(failure)),
    };
    let id = u64::try_from(saving.fields.post_id).unwrap_or_default();
    let found = CapPost {
        id,
        author: post.author,
        status: post.status.clone(),
        post_type: post.post_type.clone(),
        parent: post.parent,
    };
    reads.action_post = Some(found.clone());
    let status = post_status(reads, &found);
    let read_post = caps.has_cap(reads, "read_post", &[Value::Int(saving.fields.post_id)]);
    let permalink = reads
        .reads
        .post(id)
        .map(|post| hyscribe_render::permalinks::permalink(&reads.reads, &post));
    let hash = &saving.names.hash;
    let home = reads.site.home.as_bytes();
    let agent = saving.asked.headers.get(header::USER_AGENT);
    let poster = Poster {
        user: saving.user,
        commenter: saving.commenter,
        read_post,
        status: &status,
        postpass: cookie(saving.jar, &format!("wp-postpass_{hash}")),
        ip: &remote_addr(reads.site, saving.asked),
        agent: agent.map_or(&b""[..], HeaderValue::as_bytes),
        context: Context {
            now: reads.now,
            home,
            empty_trash_days: reads.site.constants.empty_trash_days,
        },
    };
    handle.block_on(async {
        let id = match submit::submit(&mut write, saving.fields, &post, &poster).await? {
            Ok(id) => id,
            Err(failure) => return Ok(Err(failure)),
        };
        // Its row as saved, which the action has just written.
        let comment = write.comment(id).await?.unwrap_or_default();
        let email = &saving.commenter_email;
        let who = match (saving.user, email.is_empty()) {
            (0, true) => None,
            (0, false) => Some(Unapproved::Email(email)),
            (user, _) => Some(Unapproved::User(user)),
        };
        let permalink = permalink.unwrap_or_default();
        let link = comment_link(&mut write, &permalink, id, who).await?;
        let mail_site = MailSite::read(&mut write, &reads.site.home).await?;
        let commented = Commented {
            id,
            user: saving.user,
            permalink: &permalink,
            link: link.as_bytes(),
        };
        let constants = reads.site.constants;
        let notices = comment_notices(&mut write, &mail_site, &commented, constants).await?;
        let saved = Saved {
            id,
            author: comment.author,
            email: comment.author_email,
            url: comment.author_url,
            approved: comment.approved,
            date_gmt: comment.date_gmt,
            link,
        };
        let committed = write.commit().await;
        // Mailed once the comment is kept.
        let kept = notices.into_iter().filter(|_| committed.is_ok());
        for (kind, mail) in kept {
            reads.site.mailer.wp_mail(mail, kind, false).await;
        }
        committed.map(|()| Ok(saved)).map_err(Into::into)
    })
}

/// `get_comment_link()` of comment `id` on the post at `permalink`, from the
/// action's own options and comments. On paged comments it names the page
/// from `get_page_of_comment()`, which counts the held comments of `who`.
async fn comment_link(
    write: &mut Write<'_>,
    permalink: &[u8],
    id: u64,
    who: Option<Unapproved<'_>>,
) -> Result<String, hyscribe_write::Error> {
    let names = [
        "page_comments",
        "comments_per_page",
        "permalink_structure",
        "default_comments_page",
        "thread_comments",
        "thread_comments_depth",
    ];
    let [paged, per_page, structure, default_page, threaded, depth] =
        submit::options(write, names).await?;
    let option = |name: &str| match name {
        "permalink_structure" => structure.clone(),
        "default_comments_page" => default_page.clone(),
        _ => Vec::new(),
    };
    // `empty( $per_page )` leaves the page 0.
    let mut page = None;
    if !is_empty_string(&paged) && !is_empty_string(&per_page) {
        let per_page = Value::String(per_page).to_int();
        // A `max_depth` over 1, -1 without threading.
        let threaded = !is_empty_string(&threaded) && Value::String(depth).to_int() > 1;
        page = page_of_comment(write, id, (per_page, threaded), who).await?;
    }
    let permalink = String::from_utf8_lossy(permalink);
    let link =
        hyscribe_admin::comments::comment_page_link(&option, Some(&permalink), "", (page, id));
    Ok(link)
}

/// `get_page_of_comment()` of comment `id`: its top level comment's place
/// among the post's approved comments and the held ones of `who`.
async fn page_of_comment(
    write: &mut Write<'_>,
    id: u64,
    (per_page, threaded): (i64, bool),
    who: Option<Unapproved<'_>>,
) -> Result<Option<i64>, hyscribe_write::Error> {
    if per_page < 1 {
        return Ok(Some(1));
    }
    let Some(mut top) = write.comment(id).await? else {
        return Ok(None);
    };
    while threaded && top.parent != 0 {
        match write.comment(top.parent).await? {
            Some(parent) => top = parent,
            None => return Ok(None),
        }
    }
    let older = write
        .comments_before(top.post_id, &top.date_gmt, who)
        .await?;
    // `ceil( ( $older + 1 ) / $per_page )`.
    Ok(Some(
        i64::try_from(older).unwrap_or(i64::MAX) / per_page + 1,
    ))
}

/// `$_SERVER['REMOTE_ADDR']`: the connection's address, or behind a proxy
/// the last address of `X-Forwarded-For`, the one the proxy added.
fn remote_addr(site: &Site, asked: &Asked) -> Vec<u8> {
    client_addr(site, &asked.headers, asked.peer)
}

/// [`remote_addr`] of a request's `headers` and connection `peer`: none
/// where the proxy's entry names no address.
pub(crate) fn client_addr(site: &Site, headers: &HeaderMap, peer: Option<IpAddr>) -> Vec<u8> {
    let ip = client_ip(site, headers, peer).ok().flatten();
    ip.map_or_else(Vec::new, |ip| ip.to_string().into_bytes())
}

/// Behind a proxy, the last `X-Forwarded-For` entry where it names no
/// address, as sent.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Unnamed(pub(crate) String);

/// The client's address as [`client_addr`] finds it. Behind a proxy, the
/// `X-Forwarded-For` lines are one list in order, whose last entry is the
/// one the proxy appended; never the proxy's own address, which every
/// client would share.
pub(crate) fn client_ip(
    site: &Site,
    headers: &HeaderMap,
    peer: Option<IpAddr>,
) -> Result<Option<IpAddr>, Unnamed> {
    let mut lines = headers.get_all("x-forwarded-for").iter();
    let Some(last) = lines.next_back().filter(|_| site.behind_proxy) else {
        return Ok(peer);
    };
    let entry = String::from_utf8_lossy(last.as_bytes());
    let entry = entry.rsplit(',').next().unwrap_or_default().trim();
    address(entry)
        .map(Some)
        .ok_or_else(|| Unnamed(entry.into()))
}

/// The address of an `X-Forwarded-For` entry: `ip`, `ip:port`, `[ipv6]` or
/// `[ipv6]:port`.
pub fn address(entry: &str) -> Option<IpAddr> {
    let bare = entry
        .strip_prefix('[')
        .and_then(|entry| entry.strip_suffix(']'));
    let with_port = || entry.parse::<std::net::SocketAddr>().ok().map(|at| at.ip());
    bare.unwrap_or(entry).parse().ok().or_else(with_port)
}

/// The redirect after a saved comment, with the commenter cookies of a
/// visitor: kept for a year if they agree to them, else removed.
fn redirect(
    site: &Site,
    (reads, names): (&LoginReads, &Names),
    form: &Value,
    user: u64,
    saved: &Saved,
) -> Response {
    let consent = form.get_str("wp-comment-cookies-consent").is_some();
    let mut to = match form.get_str("redirect_to") {
        // `$_POST` is slashed.
        Some(Value::String(to)) if !is_empty_string(to) => [
            addslashes(to),
            format!("#comment-{}", saved.id).into_bytes(),
        ]
        .concat(),
        _ => saved.link.clone().into_bytes(),
    };
    if !consent && saved.approved == "0" && !is_empty_string(&saved.email) {
        let id = saved.id.to_string();
        let hash = reads
            .reads
            .wp_hash(saved.date_gmt.as_bytes())
            .unwrap_or_default();
        let args = [
            (&b"unapproved"[..], Some(id.as_bytes())),
            (b"moderation-hash", Some(hash.as_bytes())),
        ];
        to = add_query_arg(&args, &to);
    }
    let mut headers = html_headers();
    if user == 0 {
        let (hash, path) = (&names.hash, &names.path);
        let secure = site.home.starts_with("https:");
        let url = esc_url(&saved.url);
        for (name, value) in [
            ("comment_author", &saved.author),
            ("comment_author_email", &saved.email),
            ("comment_author_url", &url),
        ] {
            let name = format!("{name}_{hash}");
            let cookie = match (consent, value.is_empty()) {
                (false, _) => set_cookie(&name, b" ", (reads.now - YEAR, reads.now), path, false),
                // PHP's `setcookie()` of an empty value deletes the cookie.
                (true, true) => set_cookie(&name, b"deleted", (1, reads.now), path, secure),
                (true, false) => {
                    set_cookie(&name, value, (reads.now + YEAR, reads.now), path, secure)
                }
            };
            headers.push((header::SET_COOKIE, cookie));
        }
    }
    let to = wp_safe_redirect(site, &to, &reads.admin_url(""));
    headers.push((
        header::HeaderName::from_static("x-redirect-by"),
        "WordPress".into(),
    ));
    headers.push((header::LOCATION, to));
    response(StatusCode::FOUND, headers, Vec::new())
}

#[cfg(test)]
mod tests;
