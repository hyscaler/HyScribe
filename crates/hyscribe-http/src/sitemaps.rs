// SPDX-License-Identifier: AGPL-3.0-or-later

//! `WP_Sitemaps::render_sitemaps()` with its posts and taxonomies
//! providers. The users provider is left out: it lists author archives,
//! which give away login names (docs/SECURITY_MODEL.md).

use std::collections::HashMap;

use hyscribe_db::DbError;
use hyscribe_kses::{esc_url, wp_strip_all_tags};
use hyscribe_model::{OrderBy, PostQuery};
use hyscribe_phpser::is_empty_string;
use hyscribe_render::date::{self, Zone};
use hyscribe_render::sitemap::{self, Entry};

use crate::query::absint;
use crate::{Outcome, Site, not_found};

/// `wp_sitemaps_get_max_urls()`.
const MAX_URLS: u32 = 2000;

/// The post types and taxonomies with sitemaps, in registration order.
const POST_TYPES: [&str; 2] = ["post", "page"];
const TAXONOMIES: [&str; 3] = ["category", "post_tag", "post_format"];

/// `outcome`, or the sitemap, stylesheet or 404 of the sitemap query vars,
/// which `render_sitemaps()` serves after redirect_canonical()'s redirects.
pub(crate) async fn serve(
    site: &Site,
    query: &HashMap<String, String>,
    outcome: Outcome,
) -> Result<Outcome, DbError> {
    match outcome {
        Outcome::Page(_) | Outcome::Feed(..) if !redirects(&outcome) => {
            Ok(route(site, query).await?.unwrap_or(outcome))
        }
        outcome => Ok(outcome),
    }
}

/// Whether `outcome` is a feed that redirects first.
pub(crate) fn redirects(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Feed(_, feed) if feed.redirect.is_some())
}

/// The sitemap, stylesheet or 404 of the sitemap query vars; `None`
/// without them, `0` being none, as PHP takes it.
async fn route(site: &Site, query: &HashMap<String, String>) -> Result<Option<Outcome>, DbError> {
    let var = |name: &str| query.get(name).map_or("", String::as_str);
    if ["sitemap", "sitemap-stylesheet"]
        .iter()
        .all(|name| is_empty_string(var(name).as_bytes()))
    {
        return Ok(None);
    }
    let missing = || Some(Outcome::Page(Box::new(not_found(site, false))));
    let (map, stylesheet) = (
        sanitize_text_field(var("sitemap")),
        sanitize_text_field(var("sitemap-stylesheet")),
    );
    let (map, stylesheet) = (map.as_str(), stylesheet.as_str());
    // Sitemaps are on only for a site search engines may index.
    let public = site.option("blog_public");
    let none = |v: &str| is_empty_string(v.as_bytes());
    if none(map) && none(stylesheet) || none(&public) {
        return Ok(missing());
    }
    if !none(stylesheet) {
        return Ok(sitemap::stylesheet(stylesheet)
            .map(|xsl| Outcome::Xml(xsl.as_bytes().to_vec()))
            .or_else(missing));
    }
    let home = site.home.as_bytes();
    if map == "index" {
        return Ok(Some(Outcome::Xml(sitemap::index(
            home,
            &index(site).await?,
        ))));
    }
    let paged = absint(var("paged")).max(1);
    let subtype = sanitize_text_field(var("sitemap-subtype"));
    let subtype = subtype.as_str();
    let entries = match map {
        "posts" if POST_TYPES.contains(&subtype) => posts(site, subtype, paged).await?,
        "taxonomies" if TAXONOMIES.contains(&subtype) => terms(site, subtype, paged).await?,
        _ => Vec::new(),
    };
    Ok(if entries.is_empty() {
        missing()
    } else {
        Some(Outcome::Xml(sitemap::urlset(home, &entries)))
    })
}

/// `sanitize_text_field()`: tags stripped, then the ends' whitespace and
/// percent-encoded octets.
fn sanitize_text_field(text: &str) -> String {
    let mut text = String::from_utf8_lossy(&wp_strip_all_tags(text.as_bytes(), false))
        .trim()
        .to_string();
    let octet = |t: &str| {
        t.as_bytes().windows(3).position(
            |w| matches!(w, [b'%', a, b] if a.is_ascii_hexdigit() && b.is_ascii_hexdigit()),
        )
    };
    while let Some(at) = octet(&text) {
        let found = text.get(at..at + 3).unwrap_or_default().to_string();
        text = text.replace(&found, "");
    }
    text.trim().to_string()
}

/// `do_robots()`: the admin's paths under the site URL's, which redirect
/// to hs-admin, and with sitemaps on, `WP_Sitemaps::add_robots()`'s line.
pub(crate) fn robots(site: &Site) -> Vec<u8> {
    let siteurl = site.option("siteurl");
    let after_scheme = siteurl.split_once("://").map_or("", |(_, rest)| rest);
    let path = after_scheme
        .split_once('/')
        .map_or(String::new(), |(_, path)| format!("/{path}"));
    let mut text = format!(
        "User-agent: *\nDisallow: {path}/wp-admin/\nAllow: {path}/wp-admin/admin-ajax.php\n"
    );
    if !is_empty_string(site.option("blog_public").as_bytes()) {
        let index = esc_url(format!("{}/?sitemap=index", site.home).as_bytes());
        text.push_str(&format!("\nSitemap: {}\n", String::from_utf8_lossy(&index)));
    }
    text.into_bytes()
}

/// `WP_Sitemaps_Index::get_sitemap_list()`: each page of each sitemap.
async fn index(site: &Site) -> Result<Vec<Vec<u8>>, DbError> {
    let sitemaps = POST_TYPES
        .iter()
        .map(|t| ("posts", *t))
        .chain(TAXONOMIES.iter().map(|t| ("taxonomies", *t)));
    let mut locs = Vec::new();
    for (map, subtype) in sitemaps {
        let found = match map {
            "posts" => site.db.count_posts(&of_type(subtype, 1)).await,
            _ => site.db.count_terms_with_posts(subtype).await,
        }?;
        let least = u64::from(lists_home(site, subtype));
        let pages = found.div_ceil(u64::from(MAX_URLS)).max(least);
        locs.extend((1..=pages).map(|page| {
            format!(
                "{}/?sitemap={map}&sitemap-subtype={subtype}&paged={page}",
                site.home
            )
            .into_bytes()
        }));
    }
    Ok(locs)
}

/// Whether the sitemap of `post_type` starts with the home page: the page
/// sitemap does where the home page lists the latest posts.
fn lists_home(site: &Site, post_type: &str) -> bool {
    post_type == "page" && site.option("show_on_front") == "posts"
}

/// Page `paged` of the published posts of `post_type`, by ID.
fn of_type(post_type: &str, paged: u64) -> PostQuery {
    PostQuery {
        post_types: vec![post_type.into()],
        order: vec![(OrderBy::Id, true)],
        limit: MAX_URLS,
        offset: u32::try_from((paged - 1).saturating_mul(u64::from(MAX_URLS))).unwrap_or(u32::MAX),
        ..PostQuery::default()
    }
}

/// `DATE_W3C` in the site's zone of a stored GMT time.
fn w3c(gmt: &str, zone: Zone) -> Option<String> {
    date::strtotime(gmt).map(|unix| date::format_unix("Y-m-d\\TH:i:sP", unix, zone))
}

/// `WP_Sitemaps_Posts::get_url_list()`: the home page first in the page
/// sitemap, dated by the latest change among the latest posts.
async fn posts(site: &Site, post_type: &str, paged: u64) -> Result<Vec<Entry>, DbError> {
    let mut entries = Vec::new();
    if paged == 1 && lists_home(site, post_type) {
        // The home list's query: `posts_per_page` posts, sticky ones first.
        let home = crate::post_query(site, &HashMap::new(), false, vec![], false);
        let latest = site.db.posts(&home).await?;
        let modified = latest.iter().map(|post| post.modified_gmt.as_str()).max();
        entries.push(Entry {
            loc: format!("{}/", site.home).into_bytes(),
            lastmod: modified.and_then(|gmt| w3c(gmt, site.zone())),
        });
    }
    for post in site.db.posts(&of_type(post_type, paged)).await? {
        entries.push(Entry {
            lastmod: w3c(&post.modified_gmt, site.zone()),
            loc: site.permalink(&post).into_bytes(),
        });
    }
    Ok(entries)
}

/// `WP_Sitemaps_Taxonomies::get_url_list()`: the terms with posts, linked
/// as get_term_link() links them.
async fn terms(site: &Site, taxonomy: &str, paged: u64) -> Result<Vec<Entry>, DbError> {
    let offset = (paged - 1).saturating_mul(u64::from(MAX_URLS));
    let terms = site
        .db
        .terms_with_posts(taxonomy, MAX_URLS.into(), offset)
        .await?;
    Ok(terms
        .into_iter()
        .map(|term| {
            let slug = String::from_utf8_lossy(&term.slug);
            let query = match taxonomy {
                "category" => format!("cat={}", term.id),
                "post_tag" => format!("tag={slug}"),
                // _post_format_link() links the format, not its term.
                _ => format!("post_format={}", slug.replace("post-format-", "")),
            };
            Entry {
                loc: format!("{}/?{query}", site.home).into_bytes(),
                lastmod: None,
            }
        })
        .collect())
}
