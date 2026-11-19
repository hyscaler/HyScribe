// SPDX-License-Identifier: AGPL-3.0-or-later

//! What rendering fetches and caches (ADR 0019): `wp_safe_remote_get()`
//! through the site's outbound client, its failures worded as WordPress
//! words them, and the embed and feed caches written through the gated
//! writer, which shadow mode has none of.

use std::time::Duration;

use hyscribe_fetch::{Args, Kind};
use hyscribe_phpser::{Value, decode};
use hyscribe_render::outbound::{Get, NOT_ALLOWED, Redirects, Response};
use hyscribe_write::options::{Meta, expire_site_transient, set_site_transient, update_metadata};
use hyscribe_write::posts::{Context, PostArgs, wp_insert_post};

use crate::Site;
use crate::reads::Reads;

/// `WP_Error`'s message for a failed fetch. Refusals and names that do
/// not resolve read as WordPress's URL check words them; cURL's own
/// messages, which carry timings, are given without those.
pub(crate) fn message(kind: Kind) -> &'static [u8] {
    match kind {
        Kind::Invalid | Kind::Refused | Kind::Resolve => b"A valid URL was not provided.",
        Kind::Redirects => b"Too many redirects.",
        Kind::Disallowed => NOT_ALLOWED,
        Kind::Timeout => b"cURL error 28: Operation timed out",
        Kind::Connect => b"cURL error 7: Couldn't connect to server",
        Kind::Tls => b"cURL error 60: SSL certificate problem",
        Kind::Protocol => b"cURL error 56: Failure when receiving data from the peer",
    }
}

/// The client's arguments for what rendering asks.
pub(crate) fn args(get: &Get) -> Args {
    Args {
        timeout: Duration::from_secs(get.timeout),
        limit_response_size: get.limit,
        ..Args::new(String::from_utf8_lossy(&get.user_agent).into_owned())
    }
}

/// `wp_safe_remote_get()` through `site`'s client; `None` without one.
pub(crate) fn remote_get(
    site: &Site,
    reads: &Reads<'_>,
    url: &[u8],
    get: &Get,
) -> Option<Result<Response, Vec<u8>>> {
    let client = site.fetch.as_ref()?;
    let allowed = |host: &str| {
        let host = host.to_ascii_lowercase().into_bytes();
        match &get.redirects {
            Redirects::Any => true,
            Redirects::Feeds => site.feed_hosts.contains(&host),
            Redirects::Hosts(hosts) => hosts.contains(&host),
        }
    };
    let fetched = reads.block_on(client.get_within(url, site.home.as_bytes(), args(get), &allowed));
    Some(match fetched {
        Ok(fetched) => Ok(Response {
            status: fetched.status,
            headers: fetched.headers,
            body: fetched.body,
        }),
        Err(error) => Err(message(error.kind).to_vec()),
    })
}

/// A page view's cache write, in primary mode only; PHP goes on whether
/// it is written or not.
pub(crate) enum Write<'a> {
    /// `update_post_meta()`.
    Meta(u64, &'a str, &'a [u8]),
    /// `set_site_transient()` of a serialized value.
    Transient(&'a str, &'a [u8], i64),
    /// `delete_site_transient()`.
    Expire(&'a str),
    /// `wp_insert_post()` of the navigation fallback's post (a classic
    /// menu's or the default page list's), from its title, slug and content.
    Navigation(&'a [u8], &'a [u8], &'a [u8]),
}

/// Makes `write` on `site`'s writer, if it has one.
pub(crate) fn write(site: &Site, reads: &Reads<'_>, write: Write<'_>) {
    let Some(writer) = &site.writer else {
        return;
    };
    reads.block_on(async {
        for mut tx in writer.begin().await.into_iter() {
            let _ = match write {
                Write::Meta(post, key, value) => {
                    let value = Value::String(value.to_vec());
                    let previous = Value::Null;
                    update_metadata(&mut tx, Meta::Post, post, key, &value, &previous)
                        .await
                        .map(|_| ())
                        .map_err(|_| ())
                }
                Write::Transient(name, value, expiration) => {
                    let value = decode(value).unwrap_or(Value::String(value.to_vec()));
                    set_site_transient(&mut tx, name, &value, expiration)
                        .await
                        .map(|_| ())
                        .map_err(|_| ())
                }
                Write::Expire(name) => expire_site_transient(&mut tx, name).await.map_err(|_| ()),
                Write::Navigation(title, name, content) => {
                    let ctx = visitor(site);
                    let post = [title, name, content];
                    let inserted = insert_navigation(&mut tx, &ctx, post).await;
                    inserted.map(|_| ()).map_err(|_| ())
                }
            };
            let _ = tx.commit().await;
        }
    });
}

/// The posts' context of a visitor's page view, which writes as no user.
fn visitor(site: &Site) -> Context {
    Context {
        user: 0,
        can_publish: false,
        home: site.home.as_bytes().to_vec(),
        upload_host: hyscribe_render::link::host(site.raw_option("siteurl")),
        unfiltered_html: false,
        constants: site.constants,
        now: crate::now(),
    }
}

/// `WP_Navigation_Fallback::create_classic_menu_fallback()`'s and
/// `create_default_fallback()`'s `wp_insert_post()`: the published
/// `wp_navigation` post of a title, slug and content.
pub(crate) async fn insert_navigation(
    write: &mut hyscribe_db::Write<'_>,
    ctx: &Context,
    [title, name, content]: [&[u8]; 3],
) -> Result<u64, hyscribe_write::Error> {
    let args = PostArgs {
        title: title.to_vec(),
        name: Some(name.to_vec()),
        content: content.to_vec(),
        status: "publish".into(),
        post_type: "wp_navigation".into(),
        ..PostArgs::default()
    };
    wp_insert_post(write, ctx, args).await
}

#[cfg(test)]
pub(crate) mod tests;
