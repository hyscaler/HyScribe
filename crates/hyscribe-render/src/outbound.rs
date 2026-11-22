// SPDX-License-Identifier: AGPL-3.0-or-later

//! What rendering fetches from other sites through (ADR 0019): the request
//! a block makes, the answer it reads, the filters around it under
//! WordPress's names, and the site transients the feed cache keeps.

use hyscribe_model::hooks::apply_filters;
use hyscribe_model::{Post, Term};
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

/// A classic menu's item as `wp_setup_nav_menu_item()` sets it up, with the
/// properties the navigation block's conversion reads, as the
/// `wp_get_nav_menu_items` filter gets them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavMenuItem {
    /// `ID`, also `db_id`.
    pub id: u64,
    /// `menu_item_parent`, as stored.
    pub menu_item_parent: Vec<u8>,
    /// `type`: `post_type`, `post_type_archive`, `taxonomy` or `custom`.
    pub kind: Vec<u8>,
    /// `object`: the post type, taxonomy or `custom`.
    pub object: Vec<u8>,
    /// `object_id`, as stored.
    pub object_id: Vec<u8>,
    /// `post_parent`.
    pub post_parent: u64,
    /// `title`.
    pub title: Vec<u8>,
    /// `url`, `None` for the `false` an archive without a link has.
    pub url: Option<Vec<u8>>,
    /// `target`.
    pub target: Vec<u8>,
    /// `attr_title`.
    pub attr_title: Vec<u8>,
    /// `description`.
    pub description: Vec<u8>,
    /// `classes`: the stored ones, then those of the request's context.
    pub classes: Vec<Vec<u8>>,
    /// `xfn`.
    pub xfn: Vec<u8>,
    /// `current`: whether the request's context made it the current item.
    pub current: bool,
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

/// `$args` of an embed as PHP holds them: its width and height.
fn size_args(size: Size) -> Value {
    Value::Array(vec![
        (Key::String(b"width".to_vec()), Value::Int(size.width)),
        (Key::String(b"height".to_vec()), Value::Int(size.height)),
    ])
}

/// Attributes named by strings, as PHP's array.
fn named(atts: &[(String, Value)]) -> Value {
    let atts = atts
        .iter()
        .map(|(name, value)| (Key::String(name.as_bytes().to_vec()), value.clone()));
    Value::Array(atts.collect())
}

/// The filters the embeds, the RSS block and the core shortcodes run, under
/// WordPress's names and in its argument order; each returns its first argument unless a
/// plugin changes it, and those whose values cross reach the extensions
/// (`hyscribe_model::hooks`). None skips a check: the fetch client's rules
/// hold for whatever URL they give.
pub trait Hooks: std::fmt::Debug {
    /// `oembed_fetch_url`: the provider URL with its arguments.
    fn oembed_fetch_url(&self, provider: Vec<u8>, url: &[u8], args: Size) -> Vec<u8> {
        apply_filters(
            "oembed_fetch_url",
            provider,
            &[&url.to_vec(), &size_args(args)],
        )
    }

    /// `oembed_result`: the HTML of an embed, `None` for `false`.
    fn oembed_result(&self, html: Option<Vec<u8>>, _url: &[u8], _args: Size) -> Option<Vec<u8>> {
        html
    }

    /// `embed_oembed_html`: an embed's HTML, cached or fetched.
    fn embed_oembed_html(&self, html: Vec<u8>, url: &[u8], attr: Size, post: u64) -> Vec<u8> {
        let args: [&dyn std::any::Any; 3] = [&url.to_vec(), &size_args(attr), &post];
        apply_filters("embed_oembed_html", html, &args)
    }

    /// `wp_feed_cache_transient_lifetime`, given the feed's URL by
    /// `fetch_feed()` and the cache's name by the transient cache.
    fn wp_feed_cache_transient_lifetime(&self, lifetime: i64, url_or_name: &[u8]) -> i64 {
        apply_filters(
            "wp_feed_cache_transient_lifetime",
            lifetime,
            &[&url_or_name.to_vec()],
        )
    }

    /// `rss_enclosure`: an enclosure element of an RSS item.
    fn rss_enclosure(&self, element: Vec<u8>) -> Vec<u8> {
        apply_filters("rss_enclosure", element, &[])
    }

    /// `atom_enclosure`: an enclosure link of an Atom entry.
    fn atom_enclosure(&self, link: Vec<u8>) -> Vec<u8> {
        apply_filters("atom_enclosure", link, &[])
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
    fn img_caption_shortcode(&self, attr: &[(Key, Value)], content: &[u8]) -> Vec<u8> {
        let args: [&dyn std::any::Any; 2] = [&Value::Array(attr.to_vec()), &content.to_vec()];
        apply_filters("img_caption_shortcode", Vec::new(), &args)
    }

    /// `img_caption_shortcode_width`: the width of the caption's box.
    fn img_caption_shortcode_width(
        &self,
        width: i64,
        atts: &[(String, Value)],
        content: &[u8],
    ) -> i64 {
        let args: [&dyn std::any::Any; 2] = [&named(atts), &content.to_vec()];
        apply_filters("img_caption_shortcode_width", width, &args)
    }

    /// `post_gallery`: markup that replaces the gallery's own, empty for
    /// none; its attributes and the gallery's number.
    fn post_gallery(&self, attr: &[(Key, Value)], instance: u64) -> Vec<u8> {
        let args: [&dyn std::any::Any; 2] = [&Value::Array(attr.to_vec()), &instance];
        apply_filters("post_gallery", Vec::new(), &args)
    }

    /// `use_default_gallery_style`.
    fn use_default_gallery_style(&self, default: bool) -> bool {
        apply_filters("use_default_gallery_style", default, &[])
    }

    /// `gallery_style`: the gallery's style and opening `div`.
    fn gallery_style(&self, html: Vec<u8>) -> Vec<u8> {
        apply_filters("gallery_style", html, &[])
    }

    /// `wp_audio_shortcode_override`, `wp_video_shortcode_override`: markup
    /// that replaces the player's own, empty for none.
    fn media_shortcode_override(
        &self,
        kind: &str,
        attr: &[(Key, Value)],
        content: &[u8],
        instance: u64,
    ) -> Vec<u8> {
        let hook = format!("wp_{kind}_shortcode_override");
        let args: [&dyn std::any::Any; 3] =
            [&Value::Array(attr.to_vec()), &content.to_vec(), &instance];
        apply_filters(&hook, Vec::new(), &args)
    }

    /// `wp_audio_extensions`, `wp_video_extensions`.
    fn media_extensions(&self, kind: &str, extensions: Vec<String>) -> Vec<String> {
        apply_filters(&format!("wp_{kind}_extensions"), extensions, &[])
    }

    /// `wp_audio_shortcode_library`, `wp_video_shortcode_library`.
    fn media_shortcode_library(&self, kind: &str, library: String) -> String {
        apply_filters(&format!("wp_{kind}_shortcode_library"), library, &[])
    }

    /// `wp_audio_shortcode_class`, `wp_video_shortcode_class`.
    fn media_shortcode_class(
        &self,
        kind: &str,
        class: Vec<u8>,
        atts: &[(String, Value)],
    ) -> Vec<u8> {
        apply_filters(
            &format!("wp_{kind}_shortcode_class"),
            class,
            &[&named(atts)],
        )
    }

    /// `wp_mediaelement_fallback`: the no-script fallback of a player.
    fn wp_mediaelement_fallback(&self, output: Vec<u8>, url: &[u8]) -> Vec<u8> {
        apply_filters("wp_mediaelement_fallback", output, &[&url.to_vec()])
    }

    /// `wp_navigation_should_create_fallback`.
    fn wp_navigation_should_create_fallback(&self, create: bool) -> bool {
        apply_filters("wp_navigation_should_create_fallback", create, &[])
    }

    /// `wp_get_nav_menus`: the classic menus, with the arguments
    /// `wp_get_nav_menus()` gives `get_terms()` (`taxonomy`, `hide_empty`,
    /// `orderby`).
    fn wp_get_nav_menus(&self, menus: Vec<Term>, _args: &[(String, Value)]) -> Vec<Term> {
        menus
    }

    /// `wp_get_nav_menu_items`: a menu's items, its valid ones in menu
    /// order, with the arguments of `wp_get_nav_menu_items()` that are
    /// not the menu's own term (`tax_query` is left out).
    fn wp_get_nav_menu_items(
        &self,
        items: Vec<NavMenuItem>,
        _menu: &Term,
        _args: &[(String, Value)],
    ) -> Vec<NavMenuItem> {
        items
    }

    /// `nav_menu_attr_title`: a classic menu item's title attribute.
    fn nav_menu_attr_title(&self, title: Vec<u8>) -> Vec<u8> {
        apply_filters("nav_menu_attr_title", title, &[])
    }

    /// `nav_menu_description`: a classic menu item's description.
    fn nav_menu_description(&self, description: Vec<u8>) -> Vec<u8> {
        apply_filters("nav_menu_description", description, &[])
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

#[cfg(test)]
mod tests;
