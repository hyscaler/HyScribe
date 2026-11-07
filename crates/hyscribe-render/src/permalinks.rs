// SPDX-License-Identifier: AGPL-3.0-or-later

//! The links WordPress builds under a pretty permalink structure, from
//! `WP_Rewrite`'s permastructs: posts, pages, terms, dates, feeds, search,
//! authors and paging. Each returns `None` under plain permalinks, where
//! the caller's plain link stands. Callers still apply `home_url()`'s
//! scheme, `esc_url()`, and `get_post_embed_url()`'s fallback for a path a
//! page already takes.

use hyscribe_model::{Post, Term};
use hyscribe_phpser::{Value, absint, remove_query_arg, urlencode};

use crate::link;
use crate::site::Site;

/// The permalink options, as `WP_Rewrite::init()` reads them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Permalinks {
    structure: String,
    category_base: String,
    tag_base: String,
}

/// What a post's link needs besides the post: the slugs from the top of
/// its category with the lowest ID, or of `default_category` when it has
/// none (`%category%`), its author's nicename (`%author%`) and, for a
/// page, its path of names (`get_page_uri()`).
#[derive(Debug, Default)]
pub struct PostParts {
    /// `parent/child` slugs of the category the link names.
    pub category: String,
    /// `user_nicename` of the post's author.
    pub author: String,
    /// The page's name after those of its ancestors.
    pub page_uri: String,
}

impl Permalinks {
    /// From the `permalink_structure`, `category_base` and `tag_base`
    /// options.
    pub fn new(structure: &str, category_base: &str, tag_base: &str) -> Permalinks {
        Permalinks {
            structure: structure.to_string(),
            category_base: category_base.to_string(),
            tag_base: tag_base.to_string(),
        }
    }

    /// The site's options.
    pub fn of(site: &dyn Site) -> Permalinks {
        let option =
            |name| String::from_utf8_lossy(&site.option(name).unwrap_or_default()).into_owned();
        Permalinks::new(
            &option("permalink_structure"),
            &option("category_base"),
            &option("tag_base"),
        )
    }

    /// Whether the structure has `tag`, such as `%category%`.
    pub fn uses(&self, tag: &str) -> bool {
        self.structure.contains(tag)
    }

    /// `using_permalinks()`.
    pub fn pretty(&self) -> bool {
        !self.structure.is_empty()
    }

    /// The structure up to its first tag: `WP_Rewrite::$front`.
    pub fn front(&self) -> &str {
        let end = self.structure.find('%').unwrap_or(0);
        self.structure.get(..end).unwrap_or_default()
    }

    /// `using_index_permalinks()`: links go through `index.php/`.
    pub fn index(&self) -> bool {
        let rest = self.structure.trim_start_matches('/').as_bytes();
        rest.starts_with(b"index") && rest.get(6..9) == Some(b"php")
    }

    /// `WP_Rewrite::$root`.
    pub fn root(&self) -> &'static str {
        if self.index() { "index.php/" } else { "" }
    }

    /// `user_trailingslashit()`: the structure's own trailing slash.
    pub fn trailing(&self, url: &str) -> String {
        let url = untrailingslashit(url);
        if self.structure.ends_with('/') {
            format!("{url}/")
        } else {
            url.to_string()
        }
    }

    /// `get_date_permastruct()`.
    pub fn date_structure(&self) -> String {
        let endian = [
            "%year%/%monthnum%/%day%",
            "%day%/%monthnum%/%year%",
            "%monthnum%/%day%/%year%",
        ]
        .into_iter()
        .find(|e| self.structure.contains(e))
        .unwrap_or("%year%/%monthnum%/%day%");
        let tokens = tokens(&self.structure);
        let date = if tokens.iter().take(3).any(|t| *t == "%post_id%") {
            "date/"
        } else {
            ""
        };
        format!("{}{date}{endian}", self.front())
    }

    /// `get_extra_permastruct()` of a taxonomy's terms, as
    /// `create_initial_taxonomies()` registers them; `None` for the others.
    pub fn term_structure(&self, taxonomy: &str) -> Option<String> {
        let (base, slug) = match taxonomy {
            "category" => (self.category_base.as_str(), "category"),
            "post_tag" => (self.tag_base.as_str(), "tag"),
            "post_format" => ("", "type"),
            _ => return None,
        };
        let front = if base.is_empty() || self.index() {
            self.front()
        } else {
            ""
        };
        let base = if base.is_empty() { slug } else { base };
        Some(format!("{front}{base}/%{taxonomy}%"))
    }

    /// `get_permalink()` of a post of a type core registers, other than
    /// attachments, or `get_page_link()` of a page; `None` for plain
    /// permalinks, for other types, and for a post a visitor cannot see
    /// (`wp_force_plain_post_permalink()`), which keeps its plain link.
    pub fn post_link(
        &self,
        home: &str,
        front_page: Option<u64>,
        post: &Post,
        parts: &PostParts,
    ) -> Option<String> {
        if post.post_type == "page" && front_page == Some(post.id) {
            return Some(format!("{home}/"));
        }
        if !self.pretty() || post.status != "publish" {
            return None;
        }
        if post.post_type == "page" {
            let path = format!("{}{}", self.root(), parts.page_uri);
            return Some(self.trailing(&home_url(home, &path)));
        }
        if !crate::link::BUILTIN_TYPES.contains(&post.post_type.as_str()) {
            return None;
        }
        let date: Vec<&str> = post.date.split(['-', ':', ' ']).collect();
        let date = |i: usize| date.get(i).copied().unwrap_or_default();
        let name = String::from_utf8_lossy(&post.name);
        let path = [
            ("%year%", date(0)),
            ("%monthnum%", date(1)),
            ("%day%", date(2)),
            ("%hour%", date(3)),
            ("%minute%", date(4)),
            ("%second%", date(5)),
            ("%postname%", &name),
            ("%post_id%", &post.id.to_string()),
            ("%category%", &parts.category),
            ("%author%", &parts.author),
            ("%pagename%", &name),
        ]
        .iter()
        .fold(self.structure.clone(), |path, (tag, value)| {
            path.replace(tag, value)
        });
        Some(self.trailing(&home_url(home, &path)))
    }

    /// `get_term_link()`: `slugs` from the top term down.
    pub fn term_link(&self, home: &str, taxonomy: &str, slugs: &[&str]) -> Option<String> {
        if !self.pretty() {
            return None;
        }
        let slug = slugs.join("/");
        // `_post_format_link()` drops a format's prefix.
        let slug = slug
            .strip_prefix("post-format-")
            .filter(|_| taxonomy == "post_format")
            .unwrap_or(&slug);
        let path = self
            .term_structure(taxonomy)?
            .replace(&format!("%{taxonomy}%"), slug);
        Some(home_url(home, &self.trailing(&path)))
    }

    /// `get_year_link()`, `get_month_link()` and `get_day_link()`: the
    /// date archive of `year`, and of a month, and its day, when given.
    pub fn date_link(
        &self,
        home: &str,
        year: u64,
        month: Option<(u64, Option<u64>)>,
    ) -> Option<String> {
        if !self.pretty() {
            return None;
        }
        let day = month.and_then(|(_, day)| day);
        let month = month.map(|(month, _)| month);
        let mut path = self.date_structure();
        if day.is_none() {
            path = path.replace("%day%", "");
        }
        if month.is_none() {
            path = path.replace("%monthnum%", "");
        }
        let path = collapse_slashes(&path)
            .replace("%year%", &year.to_string())
            .replace("%monthnum%", &format!("{:02}", month.unwrap_or_default()))
            .replace("%day%", &format!("{:02}", day.unwrap_or_default()));
        Some(home_url(home, &self.trailing(&path)))
    }

    /// `get_feed_link( $feed )`: `comments_` before the feed names the
    /// comments feed, and the default feed, `rss2`, goes unnamed.
    pub fn feed_link(&self, home: &str, feed: &str) -> Option<String> {
        if !self.pretty() {
            return None;
        }
        let (base, feed) = match feed.strip_prefix("comments_") {
            Some(feed) => ("comments/feed", feed),
            None => ("feed", feed),
        };
        let feed = if feed == "rss2" { "" } else { feed };
        let path = collapse_slashes(&format!("/{}{base}/{feed}", self.root()));
        Some(home_url(home, &self.trailing(&path)))
    }

    /// A feed under a link: `get_post_comments_feed_link()`,
    /// `get_term_feed_link()` and `get_author_feed_link()`.
    pub fn feed_under(&self, link: &str, feed: &str) -> String {
        if feed == "rss2" {
            self.under(link, "feed")
        } else {
            self.under(link, &format!("feed/{feed}"))
        }
    }

    /// `get_search_link( $search )`.
    pub fn search_link(&self, home: &str, search: &[u8]) -> Option<String> {
        if !self.pretty() {
            return None;
        }
        let search = String::from_utf8_lossy(&urlencode(search)).replace("%2F", "/");
        let path = format!("{}search/{search}", self.root());
        Some(home_url(home, &self.trailing(&path)))
    }

    /// `get_search_feed_link( $search, $feed )`, which always ends in a
    /// slash.
    pub fn search_feed_link(&self, home: &str, search: &[u8], feed: &str) -> Option<String> {
        let link = self.search_link(home, search)?;
        Some(format!("{}feed/{feed}/", trailingslashit(&link)))
    }

    /// `get_search_comments_feed_link( $search, $feed )`.
    pub fn search_comments_feed_link(
        &self,
        home: &str,
        search: &[u8],
        feed: &str,
    ) -> Option<String> {
        let link = self.search_feed_link(home, search, feed)?;
        Some(format!("{link}?withcomments=1"))
    }

    /// `get_author_posts_url()` of the author with `nicename`.
    pub fn author_link(&self, home: &str, nicename: &str) -> Option<String> {
        if !self.pretty() {
            return None;
        }
        let path = format!("{}author/{nicename}", self.front());
        Some(home_url(home, &self.trailing(&path)))
    }

    /// A link's `embed/`, `trackback/` or `comment-page-N/`, or a post
    /// page's number: `get_post_embed_url()`, `get_trackback_url()`,
    /// `get_comments_pagenum_link()` and `_wp_link_page()`.
    pub fn under(&self, link: &str, part: &str) -> String {
        format!("{}{}", trailingslashit(link), self.trailing(part))
    }

    /// `get_sitemap_url()`: the index for `index`, or a sitemap's page,
    /// from 1.
    pub fn sitemap_url(&self, home: &str, name: &str, subtype: &str, page: u64) -> Option<String> {
        if !self.pretty() {
            return None;
        }
        let page = page.max(1);
        let file = match (name, subtype) {
            ("index", _) => "wp-sitemap.xml".to_string(),
            (_, "") => format!("wp-sitemap-{name}-{page}.xml"),
            _ => format!("wp-sitemap-{name}-{subtype}-{page}.xml"),
        };
        Some(home_url(home, &file))
    }

    /// `get_rest_url()` of `route`.
    pub fn rest_url(&self, home: &str, route: &str) -> Option<String> {
        if !self.pretty() {
            return None;
        }
        let prefix = if self.index() {
            "index.php/wp-json"
        } else {
            "wp-json"
        };
        Some(format!("{home}/{prefix}/{}", route.trim_start_matches('/')))
    }

    /// `get_pagenum_link( $page, false )` of the request `uri` (path and
    /// query), before `sanitize_url()`.
    pub fn pagenum_link(&self, home: &str, uri: &[u8], page: u64) -> Option<String> {
        if !self.pretty() {
            return None;
        }
        let uri = String::from_utf8_lossy(&remove_query_arg(&[b"paged"], uri)).into_owned();
        let uri = strip_prefix_ignore_case(&uri, home_path(home)).trim_start_matches('/');
        let (request, query) = uri.split_at(uri.find('?').unwrap_or(uri.len()));
        let request = strip_paging(request);
        let request = strip_prefix_ignore_case(request, "index.php").trim_start_matches('/');
        let mut parts = vec![untrailingslashit(home).to_string()];
        if self.index() && (page > 1 || !request.is_empty()) {
            parts.push("index.php".into());
        }
        parts.push(untrailingslashit(request).to_string());
        if page > 1 {
            parts.extend(["page".to_string(), page.to_string()]);
        }
        // `array_filter()` drops empty parts, and "0".
        parts.retain(|p| !p.is_empty() && p != "0");
        Some(format!("{}{query}", self.trailing(&parts.join("/"))))
    }
}

/// `get_permalink( $post )` on `site`, pretty or plain.
pub fn permalink(site: &dyn Site, post: &Post) -> Vec<u8> {
    let option = |name| site.option(name).unwrap_or_default();
    let front_page = link::front_page(&option("show_on_front"), &option("page_on_front"));
    link_with_front(site, post, front_page)
}

/// A post's link with `front_page` as the page on the front.
fn link_with_front(site: &dyn Site, post: &Post, front_page: Option<u64>) -> Vec<u8> {
    let links = Permalinks::of(site);
    let home = site.home_url();
    if !links.pretty() {
        return link::permalink(&home, front_page, post);
    }
    let parts = PostParts {
        category: if links.uses("%category%") {
            category_path(site, post)
        } else {
            String::new()
        },
        author: if links.uses("%author%") {
            site.user(post.author)
                .map(|user| String::from_utf8_lossy(&user.nicename).into_owned())
                .unwrap_or_default()
        } else {
            String::new()
        },
        page_uri: if post.post_type == "page" {
            page_uri(site, post)
        } else {
            String::new()
        },
    };
    links
        .post_link(&String::from_utf8_lossy(&home), front_page, post, &parts)
        .map_or_else(
            || link::permalink(&home, front_page, post),
            String::into_bytes,
        )
}

/// Page `page` of a post's `link`, as `wp_get_canonical_url()` gives it.
pub fn paged(site: &dyn Site, link: &[u8], page: u64) -> Vec<u8> {
    let links = Permalinks::of(site);
    let page = page.to_string();
    if links.pretty() {
        links
            .under(&String::from_utf8_lossy(link), &page)
            .into_bytes()
    } else {
        hyscribe_phpser::add_query_arg(&[(b"page", Some(page.as_bytes()))], link)
    }
}

/// `_wp_link_page( $i )`'s link to page `page` (from 2) of `post`, whose
/// link is `link`: in the query for plain permalinks or a draft, after
/// `page/` for the page on the front.
pub fn post_page(site: &dyn Site, post: &Post, link: &[u8], page: usize) -> Vec<u8> {
    let links = Permalinks::of(site);
    let number = page.to_string();
    let option = |name| site.option(name).unwrap_or_default();
    let front = link::front_page(&option("show_on_front"), &option("page_on_front"));
    if !links.pretty() || matches!(post.status.as_str(), "draft" | "pending") {
        hyscribe_phpser::add_query_arg(&[(b"page", Some(number.as_bytes()))], link)
    } else if front == Some(post.id) {
        links
            .under(&String::from_utf8_lossy(link), &format!("page/{number}"))
            .into_bytes()
    } else {
        links
            .under(&String::from_utf8_lossy(link), &number)
            .into_bytes()
    }
}

/// `get_term_link( $term, $taxonomy )` on `site`, pretty or plain.
pub fn term_link(site: &dyn Site, taxonomy: &str, term: &Term) -> Vec<u8> {
    let home = site.home_url();
    let slugs = term_slugs(site, taxonomy, term);
    let slugs: Vec<&str> = slugs.iter().map(String::as_str).collect();
    Permalinks::of(site)
        .term_link(&String::from_utf8_lossy(&home), taxonomy, &slugs)
        .map_or_else(
            || link::term_link(&home, taxonomy, term),
            String::into_bytes,
        )
}

/// `get_feed_link( $feed )` on `site`.
pub fn feed_link(site: &dyn Site, feed: &str) -> Vec<u8> {
    let home = site.home_url();
    Permalinks::of(site)
        .feed_link(&String::from_utf8_lossy(&home), feed)
        .map_or_else(
            || {
                let feed = feed.replace("comments_", "comments-");
                [&home[..], b"/?feed=", feed.as_bytes()].concat()
            },
            String::into_bytes,
        )
}

/// `get_post_comments_feed_link( $post, $feed )` on `site`.
pub fn post_comments_feed_link(site: &dyn Site, post: &Post, feed: &str) -> Vec<u8> {
    let links = Permalinks::of(site);
    if !links.pretty() {
        let key: &[u8] = if post.post_type == "page" {
            b"page_id"
        } else {
            b"p"
        };
        let id = post.id.to_string();
        return hyscribe_phpser::add_query_arg(
            &[(b"feed", Some(feed.as_bytes())), (key, Some(id.as_bytes()))],
            &[&site.home_url()[..], b"/"].concat(),
        );
    }
    // `_get_page_link()` gives the page on the front its own link here.
    let link = link_with_front(site, post, None);
    links
        .feed_under(&String::from_utf8_lossy(&link), feed)
        .into_bytes()
}

/// `get_term_feed_link( $term, $taxonomy, $feed )` on `site`.
pub fn term_feed_link(site: &dyn Site, taxonomy: &str, term: &Term, feed: &str) -> Vec<u8> {
    let links = Permalinks::of(site);
    if !links.pretty() {
        let query = match taxonomy {
            "category" => format!("cat={}", term.id).into_bytes(),
            "post_tag" => [&b"tag="[..], &term.slug].concat(),
            _ => [format!("{taxonomy}=").as_bytes(), &term.slug].concat(),
        };
        return [
            &site.home_url()[..],
            b"/?feed=",
            feed.as_bytes(),
            b"&amp;",
            &query,
        ]
        .concat();
    }
    let link = term_link(site, taxonomy, term);
    links
        .feed_under(&String::from_utf8_lossy(&link), feed)
        .into_bytes()
}

/// `get_search_feed_link( $search, $feed )` on `site`.
pub fn search_feed_link(site: &dyn Site, search: &[u8], feed: &str) -> Vec<u8> {
    let home = site.home_url();
    Permalinks::of(site)
        .search_feed_link(&String::from_utf8_lossy(&home), search, feed)
        .map_or_else(
            || {
                let link = [&home[..], b"/?s=", &urlencode(search)].concat();
                hyscribe_phpser::add_query_arg(&[(b"feed", Some(feed.as_bytes()))], &link)
            },
            String::into_bytes,
        )
}

/// `get_search_comments_feed_link( $search, $feed )` on `site`.
pub fn search_comments_feed_link(site: &dyn Site, search: &[u8], feed: &str) -> Vec<u8> {
    let home = site.home_url();
    Permalinks::of(site)
        .search_comments_feed_link(&String::from_utf8_lossy(&home), search, feed)
        .map_or_else(
            || {
                let link = [&home[..], b"/?s=", &urlencode(search)].concat();
                let feed = format!("comments-{feed}");
                hyscribe_phpser::add_query_arg(&[(b"feed", Some(feed.as_bytes()))], &link)
            },
            String::into_bytes,
        )
}

/// `get_author_posts_url( $id )` on `site`: the author's nicename, which a
/// missing user leaves empty, under pretty permalinks.
pub fn author_posts_url(site: &dyn Site, id: i64) -> Vec<u8> {
    let home = site.home_url();
    let nicename = || {
        u64::try_from(id)
            .ok()
            .and_then(|id| site.user(id))
            .map(|user| String::from_utf8_lossy(&user.nicename).into_owned())
            .unwrap_or_default()
    };
    let links = Permalinks::of(site);
    if links.pretty() {
        links
            .author_link(&String::from_utf8_lossy(&home), &nicename())
            .unwrap_or_default()
            .into_bytes()
    } else {
        link::author_posts_url(&home, id)
    }
}

/// The slugs of `term` and the terms above it, from the top. A loop in
/// the parents ends the walk.
fn term_slugs(site: &dyn Site, taxonomy: &str, term: &Term) -> Vec<String> {
    let mut slugs = vec![String::from_utf8_lossy(&term.slug).into_owned()];
    let mut seen = vec![term.id];
    let mut parent = term.parent;
    while parent != 0
        && let Some(term) = site
            .term(parent, taxonomy)
            .filter(|t| !seen.contains(&t.id))
    {
        slugs.insert(0, String::from_utf8_lossy(&term.slug).into_owned());
        seen.push(term.id);
        parent = term.parent;
    }
    slugs
}

/// The path of slugs, from the top, of each of the post's categories.
pub fn category_paths(site: &dyn Site, post_id: u64) -> Vec<String> {
    site.post_terms(post_id, "category")
        .iter()
        .map(|term| term_slugs(site, "category", term).join("/"))
        .collect()
}

/// `%category%`: the path of slugs of the post's category with the lowest
/// ID, or the default category's slug alone.
fn category_path(site: &dyn Site, post: &Post) -> String {
    match site
        .post_terms(post.id, "category")
        .into_iter()
        .min_by_key(|term| term.id)
    {
        Some(term) => term_slugs(site, "category", &term).join("/"),
        None => {
            let id = absint(&Value::String(
                site.option("default_category").unwrap_or_default(),
            ));
            site.term(id, "category").map_or_else(String::new, |term| {
                String::from_utf8_lossy(&term.slug).into_owned()
            })
        }
    }
}

/// `get_page_uri()`: the page's name after those of its ancestors that
/// have one. A loop in the parents ends the walk.
fn page_uri(site: &dyn Site, post: &Post) -> String {
    let mut uri = String::from_utf8_lossy(&post.name).into_owned();
    let mut seen = vec![post.id];
    let mut parent = post.parent;
    while parent != 0
        && let Some(page) = site.post(parent).filter(|p| !seen.contains(&p.id))
    {
        if !page.name.is_empty() {
            uri = format!("{}/{uri}", String::from_utf8_lossy(&page.name));
        }
        seen.push(page.id);
        parent = page.parent;
    }
    uri
}

/// `home_url( $path )`, for a path.
fn home_url(home: &str, path: &str) -> String {
    format!("{home}/{}", path.trim_start_matches('/'))
}

/// The path of a home URL, as `home_url()` gives it, without a slash.
pub fn home_path(home: &str) -> &str {
    home.split_once("://")
        .and_then(|(_, rest)| rest.find('/').and_then(|at| rest.get(at..)))
        .unwrap_or_default()
}

/// `untrailingslashit()`.
fn untrailingslashit(url: &str) -> &str {
    url.trim_end_matches(['/', '\\'])
}

/// `trailingslashit()`.
fn trailingslashit(url: &str) -> String {
    format!("{}/", untrailingslashit(url))
}

/// `preg_replace( '#/+#', '/' )`.
pub fn collapse_slashes(path: &str) -> String {
    let mut out = String::new();
    for c in path.chars() {
        if !(c == '/' && out.ends_with('/')) {
            out.push(c);
        }
    }
    out
}

/// `preg_replace( '|^prefix|i', '' )`.
fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> &'a str {
    text.get(..prefix.len())
        .filter(|start| start.eq_ignore_ascii_case(prefix))
        .and_then(|_| text.get(prefix.len()..))
        .unwrap_or(text)
}

/// `preg_replace( '|page/\d+/?$|', '' )`: a final page number, and the
/// `page/` before it.
fn strip_paging(request: &str) -> &str {
    let rest = request.strip_suffix('/').unwrap_or(request);
    let digits = rest.len() - rest.trim_end_matches(|c: char| c.is_ascii_digit()).len();
    rest.get(..rest.len() - digits)
        .and_then(|head| head.strip_suffix("page/"))
        .filter(|_| digits > 0)
        .unwrap_or(request)
}

/// `preg_match_all( '/%.+?%/' )`: the tags of a structure.
pub fn tokens(structure: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(start) = structure
        .get(at..)
        .and_then(|s| s.find('%'))
        .map(|i| i + at)
    {
        let Some(end) = structure
            .get(start + 2..)
            .and_then(|s| s.find('%'))
            .map(|i| i + start + 3)
        else {
            break;
        };
        found.extend(structure.get(start..end));
        at = end;
    }
    found
}

#[cfg(test)]
mod tests;
