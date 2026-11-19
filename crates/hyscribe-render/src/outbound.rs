// SPDX-License-Identifier: AGPL-3.0-or-later

//! What rendering fetches from other sites through (ADR 0019): the request
//! a block makes, the answer it reads, the filters around it under
//! WordPress's names, and the site transients the feed cache keeps.

use hyscribe_model::Post;
use hyscribe_phpser::{Key, Maybe, Value, maybe_unserialize};

use crate::shortcode::Shortcode;
use crate::site::Site;

/// The message of a fetch stopped because a redirect left the allowed
/// hosts, which no PHP request has: the embed stays a plain link
/// and the feed is unreachable, and nothing is cached.
pub const NOT_ALLOWED: &[u8] = b"A redirect left the allowed hosts.";

/// Where a fetch's redirects may lead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Redirects {
    /// Anywhere the client allows.
    #[default]
    Any,
    /// To a host `RSS_ALLOWED_HOSTS` names.
    Feeds,
    /// To one of these hosts, in lower case.
    Hosts(Vec<Vec<u8>>),
}

/// `wp_safe_remote_get()`'s arguments that rendering sets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Get {
    /// `timeout`, in seconds.
    pub timeout: u64,
    /// `limit_response_size`, if any.
    pub limit: Option<usize>,
    /// `user-agent`.
    pub user_agent: Vec<u8>,
    /// Where redirects may lead: a deliberate difference from PHP.
    pub redirects: Redirects,
}

impl Get {
    /// WordPress's defaults: 5 seconds, the whole body, and the user agent
    /// `WordPress/7.1.3; <home>`.
    pub fn new(site: &dyn Site) -> Get {
        Get {
            timeout: 5,
            limit: None,
            user_agent: [&b"WordPress/7.1.3; "[..], &site.home_url()].concat(),
            redirects: Redirects::Any,
        }
    }
}

/// Where embeds may fetch from (`EMBED_ALLOWED_HOSTS`, an open question): the
/// providers of WordPress's list, and further hosts, in lower case, whose
/// pages may also be discovered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EmbedHosts {
    /// Whether the providers of WordPress's list are allowed.
    pub providers: bool,
    /// The further hosts.
    pub hosts: Vec<Vec<u8>>,
}

/// The providers only, as the setting's default.
impl Default for EmbedHosts {
    fn default() -> EmbedHosts {
        EmbedHosts {
            providers: true,
            hosts: Vec::new(),
        }
    }
}

/// A response: status, headers (names in lower case) and body.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Response {
    /// The status code.
    pub status: u16,
    /// The headers in the order sent.
    pub headers: Vec<(String, Vec<u8>)>,
    /// The body.
    pub body: Vec<u8>,
}

/// The size `wp_embed_defaults()` gives an embed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    /// `width`.
    pub width: i64,
    /// `height`.
    pub height: i64,
}

/// The filters the embeds, the RSS block and the core shortcodes run, under
/// WordPress's names and in its argument order; each returns its first argument unless a
/// plugin changes it. None skips a check: the fetch client's rules hold
/// for whatever URL they give.
pub trait Hooks: std::fmt::Debug {
    /// `oembed_fetch_url`: the provider URL with its arguments.
    fn oembed_fetch_url(&self, provider: Vec<u8>, _url: &[u8], _args: Size) -> Vec<u8> {
        provider
    }

    /// `oembed_result`: the HTML of an embed, `None` for `false`.
    fn oembed_result(&self, html: Option<Vec<u8>>, _url: &[u8], _args: Size) -> Option<Vec<u8>> {
        html
    }

    /// `embed_oembed_html`: an embed's HTML, cached or fetched.
    fn embed_oembed_html(&self, html: Vec<u8>, _url: &[u8], _attr: Size, _post: u64) -> Vec<u8> {
        html
    }

    /// `wp_feed_cache_transient_lifetime`, given the feed's URL by
    /// `fetch_feed()` and the cache's name by the transient cache.
    fn wp_feed_cache_transient_lifetime(&self, lifetime: i64, _url_or_name: &[u8]) -> i64 {
        lifetime
    }

    /// `rss_enclosure`: an enclosure element of an RSS item.
    fn rss_enclosure(&self, element: Vec<u8>) -> Vec<u8> {
        element
    }

    /// `atom_enclosure`: an enclosure link of an Atom entry.
    fn atom_enclosure(&self, link: Vec<u8>) -> Vec<u8> {
        link
    }

    /// `pre_do_shortcode_tag`: the output that replaces a shortcode, `None`
    /// for `false`.
    fn pre_do_shortcode_tag(
        &self,
        _tag: &[u8],
        _attr: &[(Key, Value)],
        _matched: &Shortcode<'_>,
    ) -> Option<Vec<u8>> {
        None
    }

    /// `do_shortcode_tag`: a shortcode's output.
    fn do_shortcode_tag(
        &self,
        output: Vec<u8>,
        _tag: &[u8],
        _attr: &[(Key, Value)],
        _matched: &Shortcode<'_>,
    ) -> Vec<u8> {
        output
    }

    /// `shortcode_atts_{$shortcode}`: the attributes with their defaults.
    fn shortcode_atts(
        &self,
        _shortcode: &str,
        out: Vec<(String, Value)>,
        _pairs: &[(&str, Value)],
        _atts: &[(Key, Value)],
    ) -> Vec<(String, Value)> {
        out
    }

    /// `img_caption_shortcode`: markup that replaces the caption's own, empty
    /// for none.
    fn img_caption_shortcode(&self, _attr: &[(Key, Value)], _content: &[u8]) -> Vec<u8> {
        Vec::new()
    }

    /// `img_caption_shortcode_width`: the width of the caption's box.
    fn img_caption_shortcode_width(
        &self,
        width: i64,
        _atts: &[(String, Value)],
        _content: &[u8],
    ) -> i64 {
        width
    }

    /// `post_gallery`: markup that replaces the gallery's own, empty for
    /// none; its attributes and the gallery's number.
    fn post_gallery(&self, _attr: &[(Key, Value)], _instance: u64) -> Vec<u8> {
        Vec::new()
    }

    /// `use_default_gallery_style`.
    fn use_default_gallery_style(&self, default: bool) -> bool {
        default
    }

    /// `gallery_style`: the gallery's style and opening `div`.
    fn gallery_style(&self, html: Vec<u8>) -> Vec<u8> {
        html
    }

    /// `wp_audio_shortcode_override`, `wp_video_shortcode_override`: markup
    /// that replaces the player's own, empty for none.
    fn media_shortcode_override(
        &self,
        _kind: &str,
        _attr: &[(Key, Value)],
        _content: &[u8],
        _instance: u64,
    ) -> Vec<u8> {
        Vec::new()
    }

    /// `wp_audio_extensions`, `wp_video_extensions`.
    fn media_extensions(&self, _kind: &str, extensions: Vec<String>) -> Vec<String> {
        extensions
    }

    /// `wp_audio_shortcode_library`, `wp_video_shortcode_library`.
    fn media_shortcode_library(&self, _kind: &str, library: String) -> String {
        library
    }

    /// `wp_audio_shortcode_class`, `wp_video_shortcode_class`.
    fn media_shortcode_class(
        &self,
        _kind: &str,
        class: Vec<u8>,
        _atts: &[(String, Value)],
    ) -> Vec<u8> {
        class
    }

    /// `wp_mediaelement_fallback`: the no-script fallback of a player.
    fn wp_mediaelement_fallback(&self, output: Vec<u8>, _url: &[u8]) -> Vec<u8> {
        output
    }

    /// `wp_navigation_should_create_fallback`.
    fn wp_navigation_should_create_fallback(&self, create: bool) -> bool {
        create
    }

    /// `nav_menu_attr_title`: a classic menu item's title attribute.
    fn nav_menu_attr_title(&self, title: Vec<u8>) -> Vec<u8> {
        title
    }

    /// `nav_menu_description`: a classic menu item's description.
    fn nav_menu_description(&self, description: Vec<u8>) -> Vec<u8> {
        description
    }

    /// `wp_audio_shortcode`, `wp_video_shortcode`: a player's markup.
    fn media_shortcode(
        &self,
        _kind: &str,
        html: Vec<u8>,
        _atts: &[(String, Value)],
        _attachment: Option<&Post>,
        _post_id: u64,
        _library: &str,
    ) -> Vec<u8> {
        html
    }
}

/// No filters attached.
impl Hooks for () {}

/// `get_site_transient( $name )` on a single site without an object cache
/// at `now`: the value, unserialized; a transient whose timeout has passed
/// is deleted and gives nothing.
pub(crate) fn get_site_transient(site: &dyn Site, name: &str, now: i64) -> Option<Value> {
    let timeout = site.option(&format!("_site_transient_timeout_{name}"));
    if timeout.is_some_and(|timeout| Value::String(timeout).to_int() < now) {
        site.delete_site_transient(name);
        return None;
    }
    let value = site.option(&format!("_site_transient_{name}"))?;
    match maybe_unserialize(&value) {
        Maybe::Value(value) => Some(value),
        // get_site_transient() gives false for a value that does not decode.
        Maybe::Invalid(_) => None,
        Maybe::Plain(_) => Some(Value::String(value)),
    }
}

#[cfg(test)]
#[path = "tests/outbound.rs"]
pub(crate) mod testing;
