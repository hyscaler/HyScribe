// SPDX-License-Identifier: AGPL-3.0-or-later

//! `core/rss`: `render_block_core_rss()` over `fetch_feed()`, which reads
//! the feed through the outbound client only from hosts on the site's
//! allowlist (`RSS_ALLOWED_HOSTS`, empty by default, the decision
//! of 2026-11-10); any other host, and every host while fetches are off,
//! renders as PHP renders a feed it cannot reach. The feed is cached as
//! SimplePie's transient cache keeps it, in primary mode only.
//!
//! Not ported: SimplePie's feed locator (a page that is not a feed gives
//! the error PHP gives when it finds none there), conditional requests
//! (the client sends no `If-Modified-Since`, ADR 0019), and character sets
//! other than UTF-8 and ISO-8859-1. A document type with an internal
//! subset is refused, where PHP expands its entities.

mod dates;
pub(crate) mod simplepie;

use hyscribe_kses::{esc_attr, esc_html, esc_url, url_host};
use hyscribe_phpser::{Key, Value, encode, is_empty_string, md5, php_trim};

use crate::blocks::Block;
use crate::core_blocks::prepared;
use crate::date::{Zone, format_unix, site_zone};
use crate::excerpt::wp_trim_words;
use crate::formatting::{html_entity_decode_quotes, untrailingslashit};
use crate::outbound::{Get, get_site_transient};
use crate::site::Site;
use crate::supports::get_block_wrapper_attributes;
use crate::template::Renderer;
use simplepie::{Feed, at, key};

/// `Misc::get_build()` of WordPress 7.1.3's SimplePie, which a cache must
/// carry to be read.
const BUILD: i64 = 1_760_643_936;

/// `12 * HOUR_IN_SECONDS`, the cache's default lifetime.
const LIFETIME: i64 = 43_200;

/// The message of a fetch WordPress refuses or cannot make.
const UNREACHABLE: &[u8] = b"WP HTTP Error: A valid URL was not provided.";

/// `<div class="components-placeholder"><div class="notice notice-error">`.
fn notice(message: &str) -> Vec<u8> {
    format!("<div class=\"components-placeholder\"><div class=\"notice notice-error\">{message}</div></div>")
        .into_bytes()
}

impl Renderer<'_> {
    /// `render_block_core_rss()`.
    pub(crate) fn rss(&mut self, block: &Block) -> Vec<u8> {
        let site = self.context.site;
        let parsed = block.attrs.as_deref().unwrap_or_default();
        let attributes = prepared("core/rss", parsed);
        let attribute = |name: &str| attributes.get_str(name).cloned().unwrap_or_default();
        let url = attribute("feedURL").to_php_string();
        let site_url = site.option("siteurl").unwrap_or_default();
        let trimmed = untrailingslashit(&url);
        if trimmed == site_url.as_slice() || trimmed == site.home_url().as_slice() {
            return notice(
                "Adding an RSS feed to this site’s homepage is not supported, as it could lead to a loop that slows down your site. Try using another block, like the <strong>Latest Posts</strong> block, to list posts from the site.",
            );
        }
        let data = match fetch_feed(site, &url, self.context.request.now) {
            Ok(data) => data,
            Err(message) => {
                return notice(&format!(
                    "<strong>RSS Error:</strong> {}",
                    esc_html(&message)
                ));
            }
        };
        let feed = Feed { data: &data };
        let items = feed.items();
        if items.is_empty() {
            return notice(
                "An error has occurred, which probably means the feed is down. Try again later.",
            );
        }
        let shown = attribute("itemsToShow").to_int();
        let shown = match shown {
            0 => items.len(),
            n if n < 0 => items
                .len()
                .saturating_sub(usize::try_from(n.unsigned_abs()).unwrap_or(usize::MAX)),
            n => usize::try_from(n).unwrap_or(usize::MAX),
        };
        let mut links = String::new();
        if !attribute("openInNewTab").is_empty() {
            links.push_str(" target=\"_blank\"");
        }
        let rel = attribute("rel");
        let rel = match rel.is_empty() {
            true => Vec::new(),
            false => php_trim(&rel.to_php_string()).to_vec(),
        };
        if !rel.is_empty() {
            links.push_str(&format!(" rel=\"{}\"", esc_attr(&rel)));
        }
        let options = |name: &str| site.option(name).unwrap_or_default();
        let zone = site_zone(&options("timezone_string"), &options("gmt_offset"));
        let offset = (Value::String(options("gmt_offset")).to_float() * 3600.0) as i64;
        let date_format = String::from_utf8_lossy(&options("date_format")).into_owned();
        let mut list = Vec::new();
        for item in items.iter().take(shown) {
            let title = item.title().unwrap_or_default();
            let title = html_entity_decode_quotes(&title);
            let title = esc_html(php_trim(&hyscribe_kses::strip_tags(&title)));
            let title = match title.is_empty() {
                true => "(no title)".to_string(),
                false => title,
            };
            let link = esc_url(&item.link().unwrap_or_default());
            let title = match link.is_empty() {
                true => title,
                false => format!(
                    "<a href='{}'{links}>{title}</a>",
                    String::from_utf8_lossy(&link)
                ),
            };
            let mut entry = format!(
                "<li class='wp-block-rss__item'><div class='wp-block-rss__item-title'>{title}</div>"
            );
            if !attribute("displayDate").is_empty()
                && let Some(timestamp) = item.date()
            {
                let local = timestamp + offset;
                let zone_offset = format_unix("P", timestamp, zone);
                let datetime = format_unix("Y-m-d\\TH:i:s", local, Zone::Utc) + &zone_offset;
                let shown = format_unix(&date_format, local, Zone::Utc);
                entry.push_str(&format!(
                    "<time datetime=\"{}\" class=\"wp-block-rss__item-publish-date\">{}</time> ",
                    esc_attr(datetime.as_bytes()),
                    esc_html(shown.as_bytes())
                ));
            }
            if !attribute("displayAuthor").is_empty() {
                let name = item.author().and_then(|author| author.name);
                if let Some(name) = name.filter(|name| !is_empty_string(name)) {
                    entry.push_str(&format!(
                        "<span class=\"wp-block-rss__item-author\">by {}</span>",
                        esc_html(&hyscribe_kses::strip_tags(&name))
                    ));
                }
            }
            let description = item.description().unwrap_or_default();
            if !attribute("displayExcerpt").is_empty() && !is_empty_string(&description) {
                let excerpt = html_entity_decode_quotes(&description);
                let length = attribute("excerptLength").to_int();
                let excerpt = esc_attr(&wp_trim_words(&excerpt, length, b" [&hellip;]"));
                let excerpt = match excerpt.strip_suffix("[...]") {
                    Some(cut) => format!("{cut}[&hellip;]"),
                    None => excerpt,
                };
                entry.push_str(&format!(
                    "<div class=\"wp-block-rss__item-excerpt\">{}</div>",
                    esc_html(excerpt.as_bytes())
                ));
            }
            entry.push_str("</li>");
            list.extend(entry.into_bytes());
        }
        let grid = attribute("blockLayout").to_php_string() == b"grid";
        let mut classes = Vec::new();
        if grid {
            classes.push("is-grid".to_string());
        }
        if grid && attributes.get_str("columns").is_some() {
            let columns =
                String::from_utf8_lossy(&attribute("columns").to_php_string()).into_owned();
            classes.push(format!("columns-{columns}"));
        }
        for (name, class) in [
            ("displayDate", "has-dates"),
            ("displayAuthor", "has-authors"),
            ("displayExcerpt", "has-excerpts"),
        ] {
            if !attribute(name).is_empty() {
                classes.push(class.to_string());
            }
        }
        let extra = [("class", Value::String(classes.join(" ").into_bytes()))];
        let wrapper =
            get_block_wrapper_attributes("core/rss", parsed, &extra, &self.context.theme.global);
        [b"<ul ", wrapper.as_bytes(), b">", &list, b"</ul>"].concat()
    }
}

/// `fetch_feed( $url )`: the feed's data tree, from the cache or fetched
/// and cached, or the error SimplePie gives.
fn fetch_feed(site: &dyn Site, url: &[u8], now: i64) -> Result<Value, Vec<u8>> {
    if url.is_empty() {
        return Ok(Value::Array(Vec::new()));
    }
    let host = url_host(url).map(|host| host.to_ascii_lowercase());
    if !host.is_some_and(|host| site.feed_hosts().contains(&host)) {
        return Err(UNREACHABLE.to_vec());
    }
    let hooks = site.hooks();
    let duration = hooks.wp_feed_cache_transient_lifetime(LIFETIME, url);
    let name = md5(url);
    let lifetime = hooks.wp_feed_cache_transient_lifetime(LIFETIME, name.as_bytes());
    let (feed, modified) = (format!("feed_{name}"), format!("feed_mod_{name}"));
    let cached = get_site_transient(site, &feed, now)
        .filter(|data| at(data, b"__cache_expiration_time").is_some_and(|t| t.to_int() >= now));
    if let Some(cached) = cached {
        let build = at(&cached, b"build").filter(|build| matches!(build, Value::Int(BUILD)));
        let located = at(&cached, b"url").is_some() || at(&cached, b"feed_url").is_some();
        if build.is_none() {
            site.delete_site_transient(&feed);
            site.delete_site_transient(&modified);
        } else if !located
            && at(&cached, b"cache_expiration_time").is_some_and(|t| t.to_int() >= now)
        {
            return Ok(cached);
        }
    }
    let get = Get {
        timeout: 10,
        ..Get::new(site)
    };
    let response = match site.remote_get(url, &get) {
        None => return Err(UNREACHABLE.to_vec()),
        Some(Err(message)) => return Err([&b"WP HTTP Error: "[..], &message].concat()),
        Some(Ok(response)) => response,
    };
    if !(response.status == 200 || (207..300).contains(&response.status)) {
        return Err(
            format!("Retrieved unsupported status code \"{}\"", response.status).into_bytes(),
        );
    }
    let header = |name: &str| {
        response
            .headers
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_slice())
            .collect::<Vec<_>>()
            .join(&b", "[..])
    };
    if !is_feed(&header("content-type"), &response.body) {
        return Err(format!(
            "A feed could not be found at `{}`; the status code is `{}` and content-type is `{}`",
            String::from_utf8_lossy(url),
            response.status,
            String::from_utf8_lossy(&header("content-type"))
        )
        .into_bytes());
    }
    let body = match std::str::from_utf8(&response.body) {
        Ok(_) => response.body.clone(),
        // ISO-8859-1, SimplePie's last try, converts any bytes.
        Err(_) => response
            .body
            .iter()
            .map(|&b| char::from(b))
            .collect::<String>()
            .into_bytes(),
    };
    let declared = body.starts_with(b"<?xml");
    let mut entries = simplepie::parse(&body).map_err(|error| {
        format!(
            "{} is invalid XML, likely due to invalid characters. XML error: {} at line {}, column {}",
            String::from_utf8_lossy(url),
            error.message,
            error.line + usize::from(declared),
            error.column
        )
        .into_bytes()
    })?;
    let kind = Feed::kind(&entries);
    if kind == 0 {
        return Err(format!(
            "A feed could not be found at `{}`. This does not appear to be a valid RSS or Atom feed.",
            String::from_utf8_lossy(url)
        )
        .into_bytes());
    }
    // Each name once, where it first came.
    let mut seen = std::collections::HashSet::new();
    let mut names: Vec<&str> = response.headers.iter().map(|(n, _)| n.as_str()).collect();
    names.retain(|name| seen.insert(*name));
    let headers = names
        .into_iter()
        .map(|name| (key(name.as_bytes()), Value::String(header(name))))
        .collect();
    entries.push((Key::String(b"type".to_vec()), Value::Int(kind)));
    entries.push((Key::String(b"headers".to_vec()), Value::Array(headers)));
    entries.push((Key::String(b"build".to_vec()), Value::Int(BUILD)));
    entries.push((key(b"cache_expiration_time"), Value::Int(duration + now)));
    entries.push((key(b"__cache_expiration_time"), Value::Int(now + duration)));
    let tree = Value::Array(entries);
    site.set_site_transient(&feed, &encode(&tree), lifetime);
    site.set_site_transient(&modified, &encode(&Value::Int(now)), lifetime);
    Ok(tree)
}

/// Whether SimplePie's locator takes the answer for a feed: its type, or
/// for HTML what its first tag is (`Sniffer::feed_or_html()`).
fn is_feed(content_type: &[u8], body: &[u8]) -> bool {
    let official = content_type
        .split(|&b| b == b';')
        .next()
        .map(|t| php_trim(t).to_ascii_lowercase())
        .unwrap_or_default();
    if official.ends_with(b"+xml") || official == b"text/xml" || official == b"application/xml" {
        return matches!(
            official.as_slice(),
            b"application/rss+xml"
                | b"application/rdf+xml"
                | b"application/atom+xml"
                | b"text/xml"
                | b"application/xml"
                | b"application/x-rss+xml"
        );
    }
    let mut at = body
        .iter()
        .take_while(|b| b"\t\n\r \xEF\xBB\xBF".contains(b))
        .count();
    while let Some(&byte) = body.get(at) {
        if b"\t\n\r ".contains(&byte) {
            at += 1;
            continue;
        }
        if byte != b'<' {
            return false;
        }
        let rest = body.get(at + 1..).unwrap_or_default();
        let skip = |end: &[u8]| {
            rest.windows(end.len())
                .position(|w| w == end)
                .map(|p| p + end.len())
        };
        let next = if rest.starts_with(b"!--") {
            skip(b"-->")
        } else if rest.starts_with(b"!") {
            skip(b">")
        } else if rest.starts_with(b"?") {
            skip(b"?>")
        } else {
            return rest.starts_with(b"rss")
                || rest.starts_with(b"rdf:RDF")
                || rest.starts_with(b"feed");
        };
        match next {
            Some(next) => at += 1 + next,
            None => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests;
