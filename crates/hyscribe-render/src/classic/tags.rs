// SPDX-License-Identifier: AGPL-3.0-or-later

//! The template tags of the global post and of the page beyond the loop:
//! titles, links, content, excerpts, thumbnails, tags, site information,
//! archive and search titles, pagination and the escaping functions a
//! template calls on what the host answers. A filter whose arguments
//! include a post object crosses to no extension yet.

use hyscribe_kses::{esc_attr, esc_html, esc_url};
use hyscribe_model::hooks::{apply_filters, do_action};
use hyscribe_model::{Post, User};
use hyscribe_phpser::{Key, Value, is_empty_string, remove_query_arg};

use super::{Classic, Scope, Unserved};
use crate::attachment::{ImageOptions, post_thumbnail_id, wp_get_attachment_image};
use crate::content::wp_link_pages;
use crate::core_blocks::{Look, Style, TITLE, Thread, comments_open_at, open_comment_form};
use crate::excerpt::EXCERPT_LENGTH;
use crate::formatting::{CORE_SHORTCODE_TAGS, convert_chars, encode_ampersands, wptexturize};
use crate::media::Loading;
use crate::permalinks::{feed_link, permalink, term_link};
use crate::post::{post_class, the_title};
use crate::query_pagination::{home_path, is_single, link, pagenum_link};
use crate::site::{Archive, Request, Site};
use crate::template::Renderer;
use crate::title::archive_title;

impl Classic {
    /// The global post.
    fn post<'r>(&self, request: &'r Request) -> Option<&'r Post> {
        request.posts.get(self.global)
    }

    /// The global post, for a tag that needs one: PHP's answer without
    /// one is not ported.
    fn needs_post<'r>(&self, request: &'r Request) -> Result<&'r Post, Unserved> {
        self.post(request).ok_or(Unserved)
    }

    /// `$authordata`: the global post's author once `setup_postdata()` has
    /// run, else `queried`, the author archive's user.
    fn authordata(
        &self,
        site: &dyn Site,
        request: &Request,
        queried: &Option<User>,
    ) -> Option<User> {
        match self.post(request).filter(|_| self.set_up) {
            Some(post) => site.user(post.author),
            None => queried.clone(),
        }
    }

    /// The page's image counts, in the main loop while it runs.
    fn loop_loading(&self) -> Loading {
        let mut loading = self.loading.clone();
        loading.in_the_loop = self.in_the_loop;
        loading
    }

    /// A renderer of `post` as the global post, counting on from the
    /// page's tags so far.
    fn renderer<'a>(&self, scope: &Scope<'a>, post: &Post) -> Renderer<'a> {
        let mut renderer = Renderer::new(scope.theme, scope.site, scope.request);
        renderer.context.post_id = Some(post.id);
        renderer.context.loading = self.loop_loading();
        renderer.autop_restored = self.autop_restored;
        renderer
    }

    /// The conditional tags of the global post.
    pub(super) fn is_post(
        &self,
        site: &dyn Site,
        request: &Request,
        name: &str,
        args: &[Value],
    ) -> Result<bool, Unserved> {
        if name == "post_type_supports" {
            return match args {
                [post_type, feature] => Ok(post_type_supports(
                    &post_type.to_php_string(),
                    &feature.to_php_string(),
                )),
                _ => Err(Unserved),
            };
        }
        // The global post's only: a post given is not ported.
        if !args.iter().all(Value::is_empty) {
            return Err(Unserved);
        }
        if name == "have_comments" {
            return Ok(self.comments.as_ref().is_some_and(Thread::have_comments));
        }
        let post = self.needs_post(request)?;
        Ok(match name {
            "has_post_thumbnail" => {
                let has = post_thumbnail_id(site, Some(post.id)) != 0;
                apply_filters("has_post_thumbnail", has, &[&(post.id as i64)])
            }
            "has_tag" => !site.post_terms(post.id, "post_tag").is_empty(),
            "comments_open" => {
                let option = |name: &str| site.option(name).unwrap_or_default();
                let close = (
                    &*option("close_comments_for_old_posts"),
                    &*option("close_comments_days_old"),
                );
                let at = (&*post.comment_status, &*post.post_type, &*post.date_gmt);
                let open = comments_open_at(close, at, request.now);
                apply_filters("comments_open", open, &[&(post.id as i64)])
            }
            _ => return Err(Unserved),
        })
    }

    /// The tags that print, beyond the page's own.
    pub(super) fn print_tag(
        &mut self,
        scope: &Scope<'_>,
        name: &str,
        args: &[Value],
    ) -> Result<Vec<u8>, Unserved> {
        let (site, request) = (scope.site, scope.request);
        let text = |at: usize| args.get(at).map(Value::to_php_string);
        let first = text(0).unwrap_or_default();
        Ok(match (name, args.len()) {
            ("bloginfo", 1) => get_bloginfo(site, &Value::String(first), true)?,
            ("esc_url", 1) => esc_url(&first),
            ("esc_attr", 1) => esc_attr(&first).into_bytes(),
            ("esc_html", 1) => esc_html(&first).into_bytes(),
            ("wp_kses_post", 1) => {
                let host = crate::link::host(&site.option("siteurl").unwrap_or_default());
                crate::kses::wp_kses_post(&first, &host)
            }
            ("the_title", 0..=2) => {
                let title = the_title(self.needs_post(request)?);
                if title.is_empty() {
                    return Ok(Vec::new());
                }
                [
                    text(0).unwrap_or_default(),
                    title,
                    text(1).unwrap_or_default(),
                ]
                .concat()
            }
            ("the_permalink", 0) => esc_url(&permalink(site, self.needs_post(request)?)),
            ("post_class", 0 | 1) => {
                let post = self.needs_post(request)?;
                let given = match args.first() {
                    Some(Value::Array(classes)) => {
                        let classes = classes.iter().map(|(_, class)| class.to_php_string());
                        classes.collect::<Vec<_>>().join(&b' ')
                    }
                    Some(class) => class.to_php_string(),
                    None => Vec::new(),
                };
                let given = String::from_utf8_lossy(&given);
                let mut classes = post_class(site, Some(request), &given, post);
                // Only a theme that supports thumbnails marks a post with one.
                let marked = given.split_whitespace().any(|c| c == "has-post-thumbnail");
                if !(scope.supports)("post-thumbnails") && !marked {
                    let kept = classes
                        .split(|&b| b == b' ')
                        .filter(|c| *c != b"has-post-thumbnail");
                    classes = kept.collect::<Vec<_>>().join(&b' ');
                }
                [&b"class=\""[..], &classes, b"\""].concat()
            }
            ("the_content", 0) => {
                let post = self.needs_post(request)?;
                let mut renderer = self.renderer(scope, post);
                let content = renderer.the_content(post);
                self.autop_restored = renderer.autop_restored;
                content
            }
            ("the_excerpt", 0) => {
                let post = self.needs_post(request)?;
                let mut renderer = self.renderer(scope, post);
                let excerpt = renderer.the_excerpt_filtered(post, EXCERPT_LENGTH);
                self.autop_restored = renderer.autop_restored;
                excerpt
            }
            ("wp_link_pages", 0) => {
                let post = self.needs_post(request)?;
                wp_link_pages(&self.renderer(scope, post), post)
            }
            ("the_tags", 0..=3) => {
                let post = self.needs_post(request)?;
                let before = label(args, b"Tags: ");
                let separator = text(1).unwrap_or_else(|| b", ".to_vec());
                the_tags(
                    site,
                    post,
                    (&before, &separator, &text(2).unwrap_or_default()),
                )
            }
            ("comments_template", 0) => {
                let post = self.needs_post(request)?;
                // Comments behind a password are not ported.
                if !post.password.is_empty() {
                    return Err(Unserved);
                }
                if request.singular {
                    let thread = Thread::of(&self.renderer(scope, post).context, post);
                    self.comments = Some(thread);
                }
                Vec::new()
            }
            ("wp_list_comments", 0 | 1) => {
                let mut style = Style {
                    avatar: 32,
                    ol: false,
                };
                for (key, value) in args.first().and_then(Value::as_array).unwrap_or_default() {
                    match (key_name(key), value) {
                        (b"style", Value::String(kind)) if kind == b"ol" || kind == b"ul" => {
                            style.ol = kind == b"ol";
                        }
                        (b"avatar_size", size) => style.avatar = size.to_int(),
                        (b"short_ping", _) => {}
                        _ => return Err(Unserved),
                    }
                }
                // Comments are only set up over a global post.
                let (Some(thread), Some(post)) = (&self.comments, self.post(request)) else {
                    return Ok(Vec::new());
                };
                thread
                    .list(&self.renderer(scope, post).context, &style)
                    .into_bytes()
            }
            ("the_comments_navigation", 0) => {
                let thread = self.comments.as_ref().filter(|t| t.pages() > 1);
                let (Some(thread), Some(post)) = (thread, self.post(request)) else {
                    return Ok(Vec::new());
                };
                let context = &self.renderer(scope, post).context;
                let mut links = String::new();
                if let Some(older) = thread.older(context, "Older comments") {
                    links.push_str(&format!("<div class=\"nav-previous\">{older}</div>"));
                }
                if let Some(newer) = thread.newer(context, "Newer comments") {
                    links.push_str(&format!("<div class=\"nav-next\">{newer}</div>"));
                }
                format!("\n\t<nav class=\"navigation comment-navigation\" aria-label=\"Comments\">\n\t\t<h2 class=\"screen-reader-text\">Comments navigation</h2>\n\t\t<div class=\"nav-links\">{links}</div>\n\t</nav>").into_bytes()
            }
            ("comment_form", 0 | 1) => {
                let (mut before, mut after) = (TITLE.0.to_string(), TITLE.1.to_string());
                for (key, value) in args.first().and_then(Value::as_array).unwrap_or_default() {
                    let value = String::from_utf8_lossy(&value.to_php_string()).into_owned();
                    match key_name(key) {
                        b"title_reply_before" => before = value,
                        b"title_reply_after" => after = value,
                        _ => return Err(Unserved),
                    }
                }
                let look = Look {
                    title_before: &before,
                    title_after: &after,
                    submit_field: "form-submit",
                    submit_class: "submit",
                };
                let post = self.needs_post(request)?;
                open_comment_form(&self.renderer(scope, post).context, post, &look)
            }
            ("the_archive_title", 0..=2) => {
                let title = self.archive_title(scope);
                [
                    text(0).unwrap_or_default(),
                    title,
                    text(1).unwrap_or_default(),
                ]
                .concat()
            }
            ("the_archive_description", 0..=2) => {
                // Of a term, only one without a description is ported; an
                // author's is `$authordata`'s, as is.
                match &request.archive {
                    Some(Archive::Term(_, Some(term))) if !term.description.is_empty() => {
                        return Err(Unserved);
                    }
                    Some(Archive::Author(_, queried)) => {
                        let user = self.authordata(site, request, queried);
                        match user.filter(|user| !user.description.is_empty()) {
                            Some(user) => [
                                text(0).unwrap_or_default(),
                                user.description,
                                text(1).unwrap_or_default(),
                            ]
                            .concat(),
                            None => Vec::new(),
                        }
                    }
                    _ => Vec::new(),
                }
            }
            ("previous_posts_link", 0 | 1) => {
                let paged = paged(request);
                if is_single(request) || paged <= 1 {
                    return Ok(Vec::new());
                }
                let label = label(args, b"&laquo; Previous Page");
                let href = pagenum(site, request, paged - 1)?;
                let attributes = apply_filters("previous_posts_link_attributes", Vec::new(), &[]);
                link(&href, &attributes, &encode_ampersands(&label, false))
            }
            ("next_posts_link", 0..=2) => {
                let max = args.get(1).map_or(0, Value::to_int);
                let max = if max == 0 {
                    request.max_pages as i64
                } else {
                    max
                };
                let next = paged(request).max(1) + 1;
                if is_single(request) || next as i64 > max {
                    return Ok(Vec::new());
                }
                let label = label(args, b"Next Page &raquo;");
                let href = pagenum(site, request, next)?;
                let attributes = apply_filters("next_posts_link_attributes", Vec::new(), &[]);
                link(&href, &attributes, &encode_ampersands(&label, false))
            }
            _ => return Err(Unserved),
        })
    }

    /// The tags that answer a value, beyond the page's own.
    pub(super) fn get_tag(
        &mut self,
        scope: &Scope<'_>,
        name: &str,
        args: &[Value],
    ) -> Result<Value, Unserved> {
        let (site, request) = (scope.site, scope.request);
        let string = Value::String;
        Ok(match (name, args) {
            ("get_bloginfo", [show]) => string(get_bloginfo(site, show, false)?),
            ("get_bloginfo", [show, filter]) => {
                let display = filter.to_php_string() == b"display";
                string(get_bloginfo(site, show, display)?)
            }
            ("home_url", []) => string(home_url(site, b"")),
            ("home_url", [Value::String(path)]) => string(home_url(site, path)),
            ("get_the_ID", []) => self
                .post(request)
                .map_or(Value::Bool(false), |post| Value::Int(post.id as i64)),
            ("get_the_title", []) => string(the_title(self.needs_post(request)?)),
            ("get_permalink", []) => string(permalink(site, self.needs_post(request)?)),
            ("get_post_type", []) => string(self.needs_post(request)?.post_type.clone().into()),
            ("get_the_post_thumbnail", [] | [Value::Null] | [Value::Null, _]) => {
                let size = args
                    .get(1)
                    .map_or(b"post-thumbnail".to_vec(), Value::to_php_string);
                string(self.post_thumbnail(scope, &String::from_utf8_lossy(&size))?)
            }
            ("get_search_query", [] | [_]) => {
                let query = apply_filters("get_search_query", request.search_query.clone(), &[]);
                match args.first() {
                    Some(escaped) if escaped.is_empty() => string(query),
                    _ => string(esc_attr(&query).into_bytes()),
                }
            }
            ("get_comments_number", []) => {
                let post = self.needs_post(request)?;
                let count = post.comment_count.to_string().into_bytes();
                string(apply_filters(
                    "get_comments_number",
                    count,
                    &[&(post.id as i64)],
                ))
            }
            // `$wp_query->max_num_pages`.
            ("max_num_pages", []) => Value::Int(request.max_pages as i64),
            _ => return Err(Unserved),
        })
    }

    /// `get_the_archive_title()`: `Archives` outside an archive.
    fn archive_title(&self, scope: &Scope<'_>) -> Vec<u8> {
        let request = scope.request;
        let Some(archive) = &request.archive else {
            return b"Archives".to_vec();
        };
        let site = scope.site;
        let author;
        let archive = match archive {
            Archive::Author(id, queried) => {
                author = Archive::Author(*id, self.authordata(site, request, queried));
                &author
            }
            _ => archive,
        };
        let date_format = site.option("date_format").unwrap_or_default();
        let posted = request.posts.first().map_or("", |post| post.date.as_str());
        let post = Post::default();
        let zone = self.renderer(scope, &post).context.zone();
        archive_title(archive, posted, zone, &date_format, true)
    }

    /// `get_the_post_thumbnail( null, $size )` of the global post.
    fn post_thumbnail(&self, scope: &Scope<'_>, size: &str) -> Result<Vec<u8>, Unserved> {
        let post = self.needs_post(scope.request)?;
        let id = post.id as i64;
        let thumbnail = post_thumbnail_id(scope.site, Some(post.id));
        let size = apply_filters("post_thumbnail_size", size.to_string(), &[&id]);
        let html = if thumbnail == 0 {
            Vec::new()
        } else {
            let args: [&dyn std::any::Any; 3] = [&id, &(thumbnail as i64), &size];
            do_action("begin_fetch_post_thumbnail_html", &args);
            let loading = self.loop_loading();
            let options = ImageOptions {
                post_thumbnail: true,
                ..ImageOptions::default()
            };
            let image =
                wp_get_attachment_image(scope.site, &loading, thumbnail, &size, &[], &options);
            do_action("end_fetch_post_thumbnail_html", &args);
            image
        };
        let args: [&dyn std::any::Any; 4] = [&id, &(thumbnail as i64), &size, &String::new()];
        Ok(apply_filters("post_thumbnail_html", html, &args))
    }
}

/// `get_pagenum_link( $page )`, escaped: with pretty permalinks the
/// request's path without its page, then `page/<n>`; pretty permalinks
/// under `index.php` are not ported.
fn pagenum(site: &dyn Site, request: &Request, page: u64) -> Result<Vec<u8>, Unserved> {
    let home = site.home_url();
    let structure = site.option("permalink_structure").unwrap_or_default();
    if structure.is_empty() {
        return Ok(esc_url(&pagenum_link(request, &home, page)));
    }
    if structure.starts_with(b"/index.php") {
        return Err(Unserved);
    }
    let uri = remove_query_arg(&[b"paged"], &request.uri);
    let root = home_path(&home);
    let homed = uri
        .get(..root.len())
        .filter(|start| start.eq_ignore_ascii_case(&root));
    let uri = homed.and_then(|_| uri.get(root.len()..)).unwrap_or(&uri);
    let at = uri.iter().position(|&b| b == b'?').unwrap_or(uri.len());
    let (path, query) = uri.split_at(at);
    let path = without_page(trim(path, b'/', true));
    let path = match path
        .get(..9)
        .filter(|start| start.eq_ignore_ascii_case(b"index.php"))
    {
        Some(_) => path.get(9..).unwrap_or_default(),
        None => path,
    };
    let path = trim(trim(path, b'/', true), b'/', false);
    let page = if page > 1 {
        format!("page/{page}").into_bytes()
    } else {
        Vec::new()
    };
    // `array_filter()` of the parts.
    let parts = [trim(&home, b'/', false), path, &page];
    let parts: Vec<&[u8]> = parts.into_iter().filter(|part| !part.is_empty()).collect();
    let mut url = parts.join(&b'/');
    // user_trailingslashit()
    if structure.ends_with(b"/") {
        url.push(b'/');
    }
    url.extend_from_slice(query);
    Ok(esc_url(&url))
}

/// `path` without `page/<n>` and a slash at its end.
pub(super) fn without_page(path: &[u8]) -> &[u8] {
    let trimmed = path.strip_suffix(b"/").unwrap_or(path);
    let digits = trimmed
        .iter()
        .rev()
        .take_while(|b| b.is_ascii_digit())
        .count();
    let before = trimmed.get(..trimmed.len() - digits).unwrap_or_default();
    match before.strip_suffix(b"page/") {
        Some(rest) if digits > 0 => rest,
        _ => path,
    }
}

/// `text` without `byte`s at its start, or at its end when `start` is false.
fn trim(text: &[u8], byte: u8, start: bool) -> &[u8] {
    if start {
        let at = text.iter().position(|&b| b != byte).unwrap_or(text.len());
        text.get(at..).unwrap_or_default()
    } else {
        let at = text.iter().rposition(|&b| b != byte).map_or(0, |at| at + 1);
        text.get(..at).unwrap_or_default()
    }
}

/// The global `$paged`: the query var, which a page not found keeps.
fn paged(request: &Request) -> u64 {
    let var = request.vars.get_str("paged").map_or(0, Value::to_int);
    request.paged.max(var.unsigned_abs())
}

/// The first argument, a link's label or text before a list: the one
/// given, or `default` for none or `null`.
fn label(args: &[Value], default: &[u8]) -> Vec<u8> {
    match args.first() {
        None | Some(Value::Null) => default.to_vec(),
        Some(label) => label.to_php_string(),
    }
}

/// An argument array's key, as a string.
fn key_name(key: &Key) -> &[u8] {
    match key {
        Key::String(name) => name,
        Key::Int(_) => b"",
    }
}

/// `post_type_supports()` of core's post types.
fn post_type_supports(post_type: &[u8], feature: &[u8]) -> bool {
    let features: &[&[u8]] = match post_type {
        b"post" => &[
            b"title",
            b"editor",
            b"author",
            b"thumbnail",
            b"excerpt",
            b"trackbacks",
            b"custom-fields",
            b"comments",
            b"revisions",
            b"post-formats",
        ],
        b"page" => &[
            b"title",
            b"editor",
            b"author",
            b"thumbnail",
            b"page-attributes",
            b"custom-fields",
            b"comments",
            b"revisions",
        ],
        b"attachment" => &[b"title", b"author", b"comments"],
        _ => &[],
    };
    features.contains(&feature)
}

/// `the_tags( $before, $sep, $after )` of `post`: its tags linked, nothing
/// without any.
fn the_tags(
    site: &dyn Site,
    post: &Post,
    (before, separator, after): (&[u8], &[u8], &[u8]),
) -> Vec<u8> {
    let terms = site.post_terms(post.id, "post_tag");
    if terms.is_empty() {
        return Vec::new();
    }
    let links: Vec<Vec<u8>> = terms
        .iter()
        .map(|term| {
            let href = esc_url(&term_link(site, "post_tag", term));
            [
                &b"<a href=\""[..],
                &href,
                b"\" rel=\"tag\">",
                &term.name,
                b"</a>",
            ]
            .concat()
        })
        .collect();
    let links = apply_filters("term_links-post_tag", links, &[]);
    let list = [before, &links.join(separator)[..], after].concat();
    let args: [&dyn std::any::Any; 4] = [
        &before.to_vec(),
        &separator.to_vec(),
        &after.to_vec(),
        &(post.id as i64),
    ];
    apply_filters("the_tags", list, &args)
}

/// `get_bloginfo( $show, $filter )`, `display` running the `bloginfo` or
/// `bloginfo_url` filters; the theme's and other files' URLs, the admin
/// email and the version are not served.
fn get_bloginfo(site: &dyn Site, show: &Value, display: bool) -> Result<Vec<u8>, Unserved> {
    let show = String::from_utf8_lossy(&show.to_php_string()).into_owned();
    let option = |name| site.option(name).unwrap_or_default();
    let output = match show.as_str() {
        "home" | "siteurl" | "url" => home_url(site, b""),
        "wpurl" => option("siteurl"),
        "description" => option("blogdescription"),
        "rdf_url" | "rss_url" | "rss2_url" | "atom_url" => {
            feed_link(site, show.trim_end_matches("_url"))
        }
        "comments_atom_url" | "comments_rss2_url" => feed_link(site, show.trim_end_matches("_url")),
        "pingback_url" => [&option("siteurl")[..], b"/xmlrpc.php"].concat(),
        "charset" => {
            let charset = option("blog_charset");
            if charset.is_empty() {
                b"UTF-8".to_vec()
            } else {
                charset
            }
        }
        "html_type" => option("html_type"),
        "language" => b"en-US".to_vec(),
        "text_direction" => b"ltr".to_vec(),
        "stylesheet_url"
        | "stylesheet_directory"
        | "template_directory"
        | "template_url"
        | "admin_email"
        | "version" => return Err(Unserved),
        _ => option("blogname"),
    };
    if !display {
        return Ok(output);
    }
    let shown = Value::String(show.clone().into_bytes());
    if ["url", "directory", "home"]
        .iter()
        .any(|part| show.contains(part))
    {
        return Ok(apply_filters("bloginfo_url", output, &[&shown]));
    }
    let output = esc_html(&convert_chars(&wptexturize(&output, &CORE_SHORTCODE_TAGS)));
    Ok(apply_filters("bloginfo", output.into_bytes(), &[&shown]))
}

/// `home_url( $path )`: a `path` that is `empty()` adds nothing.
fn home_url(site: &dyn Site, path: &[u8]) -> Vec<u8> {
    let mut url = site.home_url();
    if !is_empty_string(path) {
        url.push(b'/');
        let start = path.iter().position(|&b| b != b'/').unwrap_or(path.len());
        url.extend_from_slice(path.get(start..).unwrap_or_default());
    }
    let args: [&dyn std::any::Any; 3] = [&path.to_vec(), &Value::Null, &Value::Null];
    apply_filters("home_url", url, &args)
}
