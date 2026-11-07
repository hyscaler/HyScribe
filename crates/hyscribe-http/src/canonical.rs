// SPDX-License-Identifier: AGPL-3.0-or-later

//! `redirect_canonical()` under pretty permalinks: a request asked for
//! with query vars goes to the pretty link of what it found, and a pretty
//! path to its own form, trailing slash and all.

use std::collections::HashMap;
use std::sync::Arc;

use axum::http::Uri;
use hyscribe_phpser::{add_query_values, parse_str, rawurlencode_deep, remove_query_arg};
use hyscribe_render::permalinks;
use hyscribe_render::site::{Archive, Taxonomy};

use crate::query::{self, absint, clean_query};
use crate::{Outcome, Site, lossy};

/// `outcome` with the redirect `redirect_canonical()` gives under pretty
/// permalinks, from `original`, the request as made, and `vars`, the query
/// vars it stands for.
pub(crate) async fn pretty(
    site: &Arc<Site>,
    original: &Uri,
    vars: &HashMap<String, String>,
    outcome: Outcome,
) -> Outcome {
    let raw = original.query().unwrap_or_default();
    let get = parse_str(raw.as_bytes());
    // `! empty( $_GET[ $name ] )`.
    let given = |name: &str| get.get_str(name).is_some_and(|v| !v.is_empty());
    // Query vars the path gave, which a redirect does not carry on.
    let parsed: Vec<&[u8]> = vars
        .keys()
        .filter(|name| get.get_str(name).is_none())
        .map(|name| name.as_bytes())
        .collect();
    let page = match outcome {
        // Feeds are sent as asked.
        Outcome::Feed(page, mut feed) => {
            feed.redirect = None;
            return Outcome::Feed(page, feed);
        }
        Outcome::Redirect(location) => {
            return Outcome::Redirect(lossy(&remove_query_arg(&parsed, location.as_bytes())));
        }
        // Searches go as asked, and so do comment pages, which PHP's guard
        // against a chain of redirects turns down.
        Outcome::Page(page)
            if !page.hidden
                && page.request.search.is_none()
                && vars.get("cpage").is_none_or(|v| v.is_empty()) =>
        {
            page
        }
        outcome => return outcome,
    };
    let links = site.permalinks();
    let request = &page.request;
    let found = !request.not_found;
    let post = request.posts.first().filter(|_| request.singular && found);
    let mut strip: Vec<&str> = Vec::new();
    // A preview a visitor is not shown is no part of the link.
    if vars
        .get("preview")
        .is_some_and(|v| !hyscribe_phpser::is_empty_string(v.as_bytes()))
    {
        strip.push("preview");
    }
    let mut target = None;
    if given("sitemap") {
        let var = |name: &str| vars.get(name).map_or("", String::as_str);
        let page = absint(var("paged"));
        target = links.sitemap_url(&site.home, var("sitemap"), var("sitemap-subtype"), page);
        strip.extend(["sitemap", "sitemap-subtype", "paged"]);
    } else if post.is_some_and(|p| site.front_page() == Some(p.id) && !given("page_id")) {
        // The page on the front is the home page; its pages, `paged` before
        // `page` as `WP_Query` takes them, are the home page's.
        let var = |name: &str| {
            vars.get(name)
                .filter(|v| !hyscribe_phpser::is_empty_string(v.as_bytes()))
        };
        let page = absint(var("paged").or(var("page")).map_or("", String::as_str));
        let home = format!("{}/", site.home);
        target = Some(if page > 1 {
            strip.push("page");
            links.under(&home, &format!("page/{page}"))
        } else {
            home
        });
    } else if let Some(post) = post {
        // `is_page()` by `page_id`; `is_single()` by `p` or `name`, or under
        // a category it is not in.
        let (asked, misplaced): (&[&str], bool) = if post.post_type == "page" {
            (if given("page_id") { &["page_id"] } else { &[] }, false)
        } else {
            let asked: &[&str] = if given("p") {
                &["p", "post_type"]
            } else if given("name") {
                &["name"]
            } else {
                &[]
            };
            (
                asked,
                !in_category(site, post.id, vars.get("category_name")).await,
            )
        };
        if !asked.is_empty() || misplaced {
            target = Some(site.permalink(post).await);
            strip.extend(asked);
        }
    } else if given("m") && found && request.date {
        // `is_year()`, `is_month()` or `is_day()`, which `m` sets with a term
        // archive as well; the query var keeps only its digits.
        let m: String = vars
            .get("m")
            .map_or("", String::as_str)
            .chars()
            .filter(char::is_ascii_digit)
            .collect();
        let part = |range: std::ops::Range<usize>| m.get(range).and_then(|p| p.parse().ok());
        let date = match m.len() {
            4 => Some(None),
            6 => Some(part(4..6).map(|month| (month, None))),
            8 => Some(part(4..6).map(|month| (month, part(6..8)))),
            _ => None,
        };
        if let Some((year, date)) = part(0..4).zip(date) {
            target = links.date_link(&site.home, year, date);
            strip.push("m");
        }
    } else if let Some(archive) = request
        .archive
        .as_ref()
        .filter(|_| found && (given("year") || given("monthnum") || given("day")))
    {
        // A date archive asked for by its parts.
        let var = |name: &str| vars.get(name).map_or(0, |v| absint(v));
        let (year, month, day) = (var("year"), var("monthnum"), var("day"));
        let (link, parts): (Option<(u64, Option<u64>)>, &[&str]) = match archive {
            Archive::Day if year > 0 && month > 0 && given("day") => {
                (Some((month, Some(day))), &["year", "monthnum", "day"])
            }
            Archive::Month if year > 0 && given("monthnum") => {
                (Some((month, None)), &["year", "monthnum"])
            }
            // `$_GET['year']` or not, a year's link is the year's path.
            Archive::Year => (None, &["year"]),
            _ => (None, &[]),
        };
        if !parts.is_empty() {
            target = links.date_link(&site.home, year, link);
            strip.extend(parts);
        }
    } else if let Some(Archive::Term(taxonomy, Some(term))) = &request.archive
        && parsed.is_empty()
        && !raw.is_empty()
    {
        // A term archive whose vars all come from the query.
        let (name, vars): (&str, &[&str]) = match taxonomy {
            Taxonomy::Category => ("category", &["category_name", "cat"]),
            Taxonomy::Tag => ("post_tag", &["tag", "tag_id"]),
            Taxonomy::Format => ("post_format", &["post_format"]),
        };
        target = site
            .term_links(name, vec![term.clone()])
            .await
            .into_iter()
            .next();
        strip.extend(["term", "taxonomy"]);
        strip.extend(vars);
    }
    // `remove_query_arg()` rebuilds a query, so only when it removes.
    let query = if strip.is_empty() {
        raw.to_string()
    } else {
        let strip: Vec<&[u8]> = strip.iter().map(|name| name.as_bytes()).collect();
        lossy(&remove_query_arg(&strip, raw.as_bytes()))
    };
    let home_path = crate::home_path(&site.home);
    let origin = site.home.strip_suffix(home_path).unwrap_or(&site.home);
    let location = match target {
        Some(target) => {
            let args = rawurlencode_deep(parse_str(query.as_bytes()));
            let args = args.as_array().unwrap_or_default();
            let location = lossy(&add_query_values(args, target.as_bytes()));
            crate::clean_location(&location, clean_query)
        }
        None => {
            let path = original.path();
            let path = strip_index(path).unwrap_or_else(|| path.to_string());
            let mut path = query::strip_punctuation(&path).to_string();
            if !links.index() {
                path = path.replace("/index.php/", "/");
            }
            if found && !request.front_page {
                path = links.trailing(&path);
            }
            // `robots.txt` and sitemaps end without one.
            if ["robots", "sitemap", "sitemap-stylesheet"]
                .iter()
                .any(|name| {
                    vars.get(*name)
                        .is_some_and(|v| !hyscribe_phpser::is_empty_string(v.as_bytes()))
                })
            {
                path = path.trim_end_matches(['/', '\\']).to_string();
            }
            let mut path = permalinks::collapse_slashes(&path);
            // The home page keeps its slash.
            if path.trim_end_matches('/') == home_path {
                path = format!("{home_path}/");
            }
            crate::with_query(&format!("{origin}{path}"), &clean_query(&query))
        }
    };
    let requested = crate::with_query(&format!("{origin}{}", original.path()), raw);
    if location == requested {
        Outcome::Page(page)
    } else {
        Outcome::Redirect(location)
    }
}

/// Whether `post_id` is in the category of `path`, slugs from the top as
/// `get_category_by_path()` reads them, or the structure or the request
/// names no category.
async fn in_category(site: &Arc<Site>, post_id: u64, path: Option<&String>) -> bool {
    if !site.permalinks().uses("%category%") {
        return true;
    }
    let Some(path) = path.filter(|p| !p.is_empty()) else {
        return true;
    };
    let path = path.trim_matches('/').to_owned();
    site.read(true, move |reads| {
        permalinks::category_paths(reads, post_id).contains(&path)
    })
    .await
}

/// `preg_replace( '|/index.php/*?$|', '/' )`: a path ending in `/index.php`
/// and slashes ends in a slash.
fn strip_index(path: &str) -> Option<String> {
    let head = path.trim_end_matches('/').strip_suffix("/index.php")?;
    Some(format!("{head}/"))
}
