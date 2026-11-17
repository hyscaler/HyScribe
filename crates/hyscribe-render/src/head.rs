// SPDX-License-Identifier: AGPL-3.0-or-later

//! `wp_head()`: what a page's `<head>` prints, from the styles and script
//! modules the rendered blocks enqueued. The generator names HyScribe, and
//! the RSD tag is left out while XML-RPC is off.

use std::sync::OnceLock;

use hyscribe_kses::{esc_attr, esc_url, esc_url_raw, strip_tags, wp_strip_all_tags};
use hyscribe_model::Term;
use hyscribe_phpser::{Key, Value, add_query_arg, is_empty_string, json_decode, urlencode};

use crate::comment_pages::{comments_pagenum_link, cpage};
use crate::core_blocks::comments_open;
use crate::interactivity::array_replace_recursive;
use crate::layout::{Rule, stylesheet};
use crate::link;
use crate::permalinks;
use crate::post::the_title;
use crate::site::{Archive, Request, Site, Taxonomy};
use crate::template::Renderer;
use crate::theme::Theme;

mod block_styles;
mod embed;
mod footer;

use block_styles::BLOCK_STYLES;
pub use embed::{Share, embed_page};
pub use footer::{emoji_detection_script, wp_footer};

/// The WordPress version the bundled assets come from.
const WP_VERSION: &str = include_str!("../../../wordpress/VERSION");
const COMMON_CSS: &str =
    include_str!("../../../wordpress/wp-includes/css/dist/block-library/common.min.css");
const SKIP_LINK_CSS: &str =
    include_str!("../../../wordpress/wp-includes/css/wp-block-template-skip-link.min.css");
const SCRIPT_MODULES: &[u8] = include_bytes!("../../../wordpress/wp-includes/script-modules.json");

/// `styles_inline_size_limit`: how many bytes of stylesheets are inlined.
const INLINE_LIMIT: usize = 40_000;

const EMOJI_CSS: &str = "
\timg.wp-smiley, img.emoji {
\t\tdisplay: inline !important;
\t\tborder: none !important;
\t\tbox-shadow: none !important;
\t\theight: 1em !important;
\t\twidth: 1em !important;
\t\tmargin: 0 0.07em !important;
\t\tvertical-align: -0.1em !important;
\t\tbackground: none !important;
\t\tpadding: 0 !important;
\t}";

/// What rendering enqueued for the head and footer.
#[derive(Debug, Default)]
pub struct Assets {
    /// Style handles, in the order blocks enqueued them.
    pub(crate) styles: Vec<String>,
    /// The `block-supports` style engine store.
    pub(crate) rules: Vec<Rule>,
    /// Script module IDs, in the order they were enqueued.
    pub(crate) modules: Vec<String>,
    /// The inline CSS of `block-style-variation-styles`.
    pub(crate) variations: Vec<String>,
    /// Script handles, in the order they were enqueued.
    pub(crate) scripts: Vec<String>,
    /// The Interactivity API's server state, by store namespace.
    pub(crate) state: Vec<(Key, Value)>,
}

impl Assets {
    /// `wp_enqueue_style( $handle )`.
    pub(crate) fn enqueue_style(&mut self, handle: &str) {
        enqueue(&mut self.styles, handle);
    }

    /// `wp_enqueue_script( $handle )`.
    pub(crate) fn enqueue_script(&mut self, handle: &str) {
        enqueue(&mut self.scripts, handle);
    }

    /// `wp_enqueue_script_module( $id )`.
    pub(crate) fn enqueue_module(&mut self, id: &str) {
        enqueue(&mut self.modules, id);
    }

    /// `wp_interactivity_state( $namespace, $state )`: merged into what the
    /// namespace holds.
    pub(crate) fn interactivity_state(&mut self, namespace: &str, state: Vec<(Key, Value)>) {
        let entry = (
            Key::String(namespace.as_bytes().to_vec()),
            Value::Array(state),
        );
        self.state = array_replace_recursive(std::mem::take(&mut self.state), vec![entry]);
    }
}

/// A queue takes each name once, where it was first enqueued.
fn enqueue(queue: &mut Vec<String>, name: &str) {
    if !queue.iter().any(|queued| queued == name) {
        queue.push(name.to_owned());
    }
}

/// The `<head>` of a page rendered from a block template, as
/// `template-canvas.php` prints it around `wp_head()`; `title` is
/// `wp_get_document_title()`.
pub fn wp_head(
    theme: &Theme,
    site: &dyn Site,
    request: &Request,
    assets: &Assets,
    title: &[u8],
) -> Vec<u8> {
    let renderer = Renderer::new(theme, site, request);
    let head = Head {
        theme,
        site,
        request,
        context: &renderer.context,
    };
    let charset = site.option("blog_charset").unwrap_or_default();
    let mut out = [&b"<head>\n\t<meta charset=\""[..], &charset, b"\" />\n\t"].concat();
    out.extend_from_slice(
        b"<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n",
    );
    out.extend(head.robots());
    if let Some(post) = request.posts.first().filter(|_| request.preview) {
        out.extend(post_preview_js(post.id));
    }
    out.extend([&b"<title>"[..], title, b"</title>\n"].concat());
    out.extend(head.feed_links());
    out.extend(head.oembed_links());
    let siteurl = head.siteurl();
    for style in head.styles(assets) {
        out.extend(style.print(&siteurl));
    }
    out.extend(head.rest_links());
    // wp_generator(), with HyScribe's name and version.
    out.extend(
        format!(
            "<meta name=\"generator\" content=\"HyScribe {}\" />\n",
            env!("CARGO_PKG_VERSION")
        )
        .into_bytes(),
    );
    out.extend(head.canonical(true));
    let registry = registered_modules(&head.siteurl());
    out.extend(
        Modules {
            registry: &registry,
            queue: &assets.modules,
        }
        .head(),
    );
    out.extend(head.font_faces());
    out.extend_from_slice(b"</head>");
    out
}

/// `wp_post_preview_js()` for the global post `id`.
fn post_preview_js(id: u64) -> Vec<u8> {
    format!(
        "<script>\n\t( function() {{\n\t\tvar query = document.location.search;\n\n\t\tif ( query && query.indexOf( 'preview=true' ) !== -1 ) {{\n\t\t\twindow.name = 'wp-preview-{id}';\n\t\t}}\n\n\t\tif ( window.addEventListener ) {{\n\t\t\twindow.addEventListener( 'pagehide', function() {{ window.name = ''; }} );\n\t\t}}\n\t}}());\n\t//# sourceURL=wp_post_preview_js\t\n</script>\n"
    )
    .into_bytes()
}

struct Head<'a> {
    theme: &'a Theme,
    site: &'a dyn Site,
    request: &'a Request,
    context: &'a crate::core_blocks::Context<'a>,
}

impl Head<'_> {
    fn option(&self, name: &str) -> Vec<u8> {
        self.site.option(name).unwrap_or_default()
    }

    fn siteurl(&self) -> String {
        String::from_utf8_lossy(&self.option("siteurl")).into_owned()
    }

    fn truthy(&self, name: &str) -> bool {
        !is_empty_string(&self.option(name))
    }

    /// The main query's post on a singular request.
    fn queried_post(&self) -> Option<&hyscribe_model::Post> {
        self.request.posts.first().filter(|_| self.request.singular)
    }

    /// `wp_robots()` with the default `wp_robots` filters.
    fn robots(&self) -> Vec<u8> {
        let public = self.truthy("blog_public");
        let mut robots: Vec<(&str, Option<&str>)> = Vec::new();
        // wp_robots_no_robots()
        let no_robots = |robots: &mut Vec<(&str, Option<&str>)>| {
            set(robots, "noindex", None);
            set(robots, if public { "follow" } else { "nofollow" }, None);
        };
        if !public || self.request.embed || self.request.search.is_some() {
            no_robots(&mut robots);
        }
        if public {
            set(&mut robots, "max-image-preview", Some("large"));
        }
        // A comment reply link, or an unapproved comment's preview.
        let set = |name| self.request.get.get_str(name).is_some();
        if set("replytocom") || set("unapproved") && set("moderation-hash") {
            no_robots(&mut robots);
        }
        let content: Vec<String> = robots
            .iter()
            .map(|(name, value)| {
                value.map_or((*name).to_owned(), |value| format!("{name}:{value}"))
            })
            .collect();
        format!(
            "<meta name='robots' content='{}' />\n",
            esc_attr(content.join(", ").as_bytes())
        )
        .into_bytes()
    }

    /// `feed_links()` and `feed_links_extra()`.
    fn feed_links(&self) -> Vec<u8> {
        let name = String::from_utf8_lossy(&self.option("blogname")).into_owned();
        let link = |title: &str, href: &[u8]| {
            format!(
                "<link rel=\"alternate\" type=\"application/rss+xml\" title=\"{}\" href=\"{}\" />\n",
                esc_attr(title.as_bytes()),
                String::from_utf8_lossy(&esc_url(href))
            )
        };
        let mut out = link(
            &format!("{name} &raquo; Feed"),
            &permalinks::feed_link(self.site, "rss2"),
        );
        out.push_str(&link(
            &format!("{name} &raquo; Comments Feed"),
            &permalinks::feed_link(self.site, "comments_rss2"),
        ));
        let extra = if let Some(post) = self.queried_post() {
            // comments_open(), pings_open() or comments: none has a side effect.
            let open = [
                comments_open(self.context, post),
                post.ping_status == "open",
                post.comment_count > 0,
            ]
            .contains(&true);
            open.then(|| {
                let title = esc_attr(&strip_tags(&the_title(post)));
                let href = permalinks::post_comments_feed_link(self.site, post, "rss2");
                (format!("{name} &raquo; {title} Comments Feed"), href)
            })
        } else if let Some(Archive::Term(taxonomy, term)) = &self.request.archive {
            // A term archive without its term has no feed of its own, and
            // no search feed either.
            term.as_ref()
                .map(|term| self.term_feed_link(&name, *taxonomy, term))
        } else {
            self.request.search.as_ref().map(|search| {
                let title = format!(
                    "{name} &raquo; Search Results for &#8220;{}&#8221; Feed",
                    String::from_utf8_lossy(search)
                );
                (
                    title,
                    permalinks::search_feed_link(self.site, search, "rss2"),
                )
            })
        };
        if let Some((title, href)) = extra {
            out.push_str(&link(&title, &href));
        }
        out.into_bytes()
    }

    /// `feed_links_extra()` of a term archive: the feed's name and
    /// get_term_feed_link().
    fn term_feed_link(&self, name: &str, taxonomy: Taxonomy, term: &Term) -> (String, Vec<u8>) {
        let slug = taxonomy.name();
        let kind = match taxonomy {
            Taxonomy::Category => "Category",
            Taxonomy::Tag => "Tag",
            Taxonomy::Format => "Format",
        };
        let title = format!(
            "{name} &raquo; {} {kind} Feed",
            String::from_utf8_lossy(&term.name)
        );
        (
            title,
            permalinks::term_feed_link(self.site, slug, term, "rss2"),
        )
    }

    /// The styles `wp_print_styles()` prints: the queue as the head's
    /// actions build it, after `wp_maybe_inline_styles()`.
    fn styles(&self, assets: &Assets) -> Vec<Style> {
        let siteurl = self.siteurl();
        let mut queue = vec![Style::inline(
            "wp-img-auto-sizes-contain",
            "img:is([sizes=auto i],[sizes^=\"auto,\" i]){contain-intrinsic-size:3000px 1500px}",
        )];
        for handle in &assets.styles {
            let mut style = block_style(handle, &siteurl);
            // enqueue_block_styles_assets(): the theme's block styles' CSS
            // follows their block's stylesheet.
            for (block, css) in &self.theme.inline_block_styles {
                if *handle == format!("wp-block-{}", block.trim_start_matches("core/")) {
                    style.after.push(css.clone());
                }
            }
            queue.push(style);
        }
        let on_page = |name: &str| {
            let handle = format!("wp-block-{}", name.trim_start_matches("core/"));
            queue.iter().any(|style| style.handle == handle)
        };
        let global = self.theme.global.inline_css(&on_page);
        let library = Style::file(
            "wp-block-library",
            "/wp-includes/css/dist/block-library/common.min.css",
            COMMON_CSS,
            None,
        );
        let global = Style::inline("global-styles", &global);
        let emoji = Style::inline("wp-emoji-styles", EMOJI_CSS);
        if assets.variations.is_empty() {
            queue.extend([emoji, library, global]);
        } else {
            // The variation styles, enqueued before emoji styles, print
            // after what they depend on.
            let variations = Style {
                after: assets.variations.clone(),
                ..Style::inline("block-style-variation-styles", "")
            };
            queue.extend([library, global, variations, emoji]);
        }
        queue.push(Style::inline(
            "core-block-supports",
            &stylesheet(&assets.rules),
        ));
        queue.push(Style::file(
            "wp-block-template-skip-link",
            "/wp-includes/css/wp-block-template-skip-link.min.css",
            SKIP_LINK_CSS,
            None,
        ));
        queue.push(Style::file(
            &format!("{}-style", self.theme.slug),
            &format!(
                "{siteurl}/wp-content/themes/{}/style.min.css",
                self.theme.slug
            ),
            self.theme.stylesheet,
            Some(&self.theme.version),
        ));
        inline_styles(&mut queue);
        queue
    }

    /// `rel_canonical()`, and with `shortlink`, `wp_shortlink_wp_head()`.
    fn canonical(&self, shortlink: bool) -> Vec<u8> {
        let Some(post) = self.queried_post().filter(|post| post.id != 0) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        // wp_get_canonical_url()
        if post.status == "publish" {
            let mut url = self.context.permalink(post);
            if self.request.page >= 2 {
                url = permalinks::paged(self.site, &url, self.request.page);
            }
            let cpage = cpage(self.request);
            if !cpage.is_empty() {
                url = comments_pagenum_link(self.context, cpage.to_int(), 0);
            }
            out.extend(
                [
                    &b"<link rel=\"canonical\" href=\""[..],
                    &esc_url(&url),
                    b"\" />\n",
                ]
                .concat(),
            );
        }
        if !shortlink {
            return out;
        }
        let front_page =
            link::front_page(&self.option("show_on_front"), &self.option("page_on_front"));
        let shortlink =
            link::shortlink(&self.site.home_url(), front_page, self.request).unwrap_or_default();
        out.extend(
            [
                &b"<link rel='shortlink' href='"[..],
                &esc_url(&shortlink),
                b"' />\n",
            ]
            .concat(),
        );
        out
    }

    /// `wp_oembed_add_discovery_links()`: a singular request's oEmbed
    /// endpoints for its post's permalink.
    fn oembed_links(&self) -> Vec<u8> {
        let Some(post) = self.queried_post() else {
            return Vec::new();
        };
        // `get_oembed_endpoint_url()`: `add_query_arg()` encodes the plain
        // route.
        let links = permalinks::Permalinks::of(self.site);
        let (route, separator): (&str, &[u8]) = if links.pretty() {
            ("/oembed/1.0/embed", b"?url=")
        } else {
            ("%2Foembed%2F1.0%2Fembed", b"&url=")
        };
        let endpoint = [
            &link::rest_url(&self.site.home_url(), &links, route)[..],
            separator,
            &urlencode(&self.context.permalink(post)),
        ]
        .concat();
        let link = |title: &str, kind: &str, href: &[u8]| {
            format!(
                "<link rel=\"alternate\" title=\"{title}\" type=\"{kind}\" href=\"{}\" />\n",
                String::from_utf8_lossy(&esc_url(href))
            )
        };
        let xml = [&endpoint[..], b"&format=xml"].concat();
        [
            link("oEmbed (JSON)", "application/json+oembed", &endpoint),
            link("oEmbed (XML)", "text/xml+oembed", &xml),
        ]
        .concat()
        .into_bytes()
    }

    /// `rest_output_link_wp_head()`, printed without a trailing newline,
    /// as PHP prints it.
    fn rest_links(&self) -> Vec<u8> {
        let links = permalinks::Permalinks::of(self.site);
        let (root, route) = link::rest_links(&self.site.home_url(), &links, self.request);
        let mut out = format!(
            "<link rel=\"https://api.w.org/\" href=\"{}\" />",
            String::from_utf8_lossy(&esc_url(&root))
        );
        if let Some(route) = route {
            out.push_str(&format!(
                "<link rel=\"alternate\" title=\"JSON\" type=\"application/json\" href=\"{}\" />",
                String::from_utf8_lossy(&esc_url(&route))
            ));
        }
        out.into_bytes()
    }

    /// `wp_print_font_faces()`: the theme's font faces from `theme.json`.
    fn font_faces(&self) -> Vec<u8> {
        let siteurl = self.siteurl();
        font_faces(
            self.theme.global.setting(&["typography", "fontFamilies"]),
            &format!("{siteurl}/wp-content/themes/{}", self.theme.slug),
        )
    }
}

impl Theme {
    /// `wp_print_font_faces()` `with_theme` (not for a missing child), then
    /// `wp_print_font_faces_from_style_variations()`, as the admin prints.
    pub fn admin_font_faces(&self, siteurl: &str, with_theme: bool) -> Vec<u8> {
        let theme_uri = format!("{siteurl}/wp-content/themes/{}", self.slug);
        let families = self.global.setting(&["typography", "fontFamilies"]);
        let mut out = if with_theme {
            font_faces(families, &theme_uri)
        } else {
            Vec::new()
        };
        let mut fonts = Vec::new();
        for variation in crate::theme::style_variations() {
            let families = variation
                .get_str("settings")
                .and_then(|v| v.get_str("typography"))
                .and_then(|v| v.get_str("fontFamilies"))
                .and_then(Value::as_array)
                .unwrap_or_default();
            fonts.extend(families.iter().map(|(_, family)| family.clone()));
        }
        let setting = Value::Array(vec![(
            hyscribe_phpser::Key::String(b"theme".to_vec()),
            Value::Array(
                fonts
                    .into_iter()
                    .enumerate()
                    .map(|(i, family)| (hyscribe_phpser::Key::Int(i as i64), family))
                    .collect(),
            ),
        )]);
        out.extend(font_faces(Some(&setting), &theme_uri));
        out
    }
}

/// `wp_print_font_faces()` for a `fontFamilies` setting, with `file:./`
/// sources under `theme_uri`.
fn font_faces(families: Option<&Value>, theme_uri: &str) -> Vec<u8> {
    let mut css = Vec::new();
    for (_, origin) in families.and_then(Value::as_array).unwrap_or_default() {
        for (_, family) in origin.as_array().unwrap_or_default() {
            let (Some(faces), Some(name)) = (
                family.get_str("fontFace").filter(|v| !v.is_empty()),
                family.get_str("fontFamily").filter(|v| !v.is_empty()),
            ) else {
                continue;
            };
            // maybe_parse_name_from_comma_separated_list()
            let name = name.to_php_string();
            let name = name.split(|&b| b == b',').next().unwrap_or_default();
            let name = trim_quotes(name);
            if name.is_empty() {
                continue;
            }
            for (_, face) in faces.as_array().unwrap_or_default() {
                if let Some(rule) = font_face(face, name, theme_uri) {
                    css.push(rule);
                }
            }
        }
    }
    if css.is_empty() {
        return Vec::new();
    }
    // An `@font-face` rule is left after wp_strip_all_tags().
    let css = wp_strip_all_tags(&css.join(&b'\n'), false);
    [
        &b"<style class=\"wp-fonts-local\">\n"[..],
        &style_text(&css),
        b"\n</style>\n",
    ]
    .concat()
}

/// One `@font-face` rule, as `WP_Font_Face_Resolver` converts a
/// `fontFace` entry and `WP_Font_Face` validates and prints it.
fn font_face(face: &Value, family: &[u8], theme_uri: &str) -> Option<Vec<u8>> {
    // convert_font_face_properties(): the family set, then the keys
    // kebab-cased, which moves camelCase keys to the end.
    let mut properties: Vec<(String, Value)> = face
        .as_array()
        .unwrap_or_default()
        .iter()
        .map(|(key, value)| {
            (
                String::from_utf8_lossy(&key.to_php_string()).into_owned(),
                value.clone(),
            )
        })
        .collect();
    set(
        &mut properties,
        "font-family".to_owned(),
        Value::String(family.to_vec()),
    );
    for (key, value) in properties.clone() {
        let kebab = hyscribe_theme_json::kebab_case(&key);
        set(&mut properties, kebab.clone(), value);
        if kebab != key {
            properties.retain(|(k, _)| *k != key);
        }
    }
    // An empty source fails validation converted or not.
    if let Some((_, src)) = properties.iter_mut().find(|(k, _)| k == "src") {
        *src = theme_file_uris(src, theme_uri);
    }
    // validate_font_face_declarations(), after wp_parse_args().
    let mut face: Vec<(String, Value)> =
        ["font-family", "font-style", "font-weight", "font-display"]
            .iter()
            .zip(["", "normal", "400", "fallback"])
            .map(|(key, default)| (key.to_string(), Value::String(default.as_bytes().to_vec())))
            .collect();
    for (key, value) in properties {
        set(&mut face, key, value);
    }
    let get = |face: &[(String, Value)], key: &str| {
        face.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    };
    // to_kebab_case() loops over a copy that ends with the definition's
    // family, so that name wins and is never empty.
    let family = get(&face, "font-family").unwrap_or_default();
    let src = get(&face, "src").filter(|v| !v.is_empty())?;
    // A source that is not empty is a list by now.
    let sources: Vec<Vec<u8>> = src
        .as_array()
        .unwrap_or_default()
        .iter()
        .map(|(_, v)| match v {
            Value::String(s) if !is_empty_string(s) => Some(s.clone()),
            _ => None,
        })
        .collect::<Option<_>>()?;
    if !matches!(
        get(&face, "font-weight"),
        Some(Value::String(_) | Value::Int(_))
    ) {
        return None;
    }
    let display = get(&face, "font-display")
        .map(|v| v.to_php_string())
        .unwrap_or_default();
    if !["auto", "block", "fallback", "swap", "optional"]
        .iter()
        .any(|d| d.as_bytes() == display)
    {
        set(
            &mut face,
            "font-display".to_owned(),
            Value::String(b"fallback".to_vec()),
        );
    }
    face.retain(|(key, _)| VALID_FONT_PROPERTIES.contains(&key.as_str()));
    let mut css = Vec::new();
    for (key, value) in &face {
        let value = match key.as_str() {
            "src" => compile_src(&sources),
            "font-family" => {
                let name = family.to_php_string();
                if name.contains(&b' ') && !name.contains(&b'"') && !name.contains(&b'\'') {
                    [&b"\""[..], &name, b"\""].concat()
                } else {
                    name
                }
            }
            "font-variation-settings" if matches!(value, Value::Array(_)) => value
                .as_array()
                .unwrap_or_default()
                .iter()
                .flat_map(|(k, v)| [k.to_php_string(), b" ".to_vec(), v.to_php_string()].concat())
                .collect(),
            _ => value.to_php_string(),
        };
        if !is_empty_string(&value) {
            css.extend([key.as_bytes(), b":", &value, b";"].concat());
        }
    }
    Some([&b"@font-face{"[..], &css, b"}"].concat())
}

/// `to_theme_file_uri()`: `file:./` sources as URLs in the theme.
fn theme_file_uris(src: &Value, theme_uri: &str) -> Value {
    let uri = |url: &Value| match url {
        Value::String(s) if s.starts_with(b"file:./") => {
            Value::String([theme_uri.as_bytes(), b"/", s.get(7..).unwrap_or_default()].concat())
        }
        other => other.clone(),
    };
    match src {
        Value::Array(list) => Value::Array(list.iter().map(|(k, v)| (k.clone(), uri(v))).collect()),
        other => Value::Array(vec![(hyscribe_phpser::Key::Int(0), uri(other))]),
    }
}

const VALID_FONT_PROPERTIES: [&str; 14] = [
    "ascent-override",
    "descent-override",
    "font-display",
    "font-family",
    "font-stretch",
    "font-style",
    "font-weight",
    "font-variant",
    "font-feature-settings",
    "font-variation-settings",
    "line-gap-override",
    "size-adjust",
    "src",
    "unicode-range",
];

/// `$array[ $key ] = $value`: in place, or appended.
fn set<K: PartialEq, V>(entries: &mut Vec<(K, V)>, key: K, value: V) {
    match entries.iter_mut().find(|(k, _)| *k == key) {
        Some(entry) => entry.1 = value,
        None => entries.push((key, value)),
    }
}

/// `order_src()` and `compile_src()`: data URLs first, then one URL per
/// format, best first.
fn compile_src(sources: &[Vec<u8>]) -> Vec<u8> {
    let mut by_format: Vec<(Vec<u8>, &[u8])> = Vec::new();
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for url in sources {
        if hyscribe_phpser::php_trim(url).starts_with(b"data:") {
            parts.push([&b"url("[..], url, b")"].concat());
            continue;
        }
        let base = url.rsplit(|&b| b == b'/').next().unwrap_or_default();
        let extension = base
            .iter()
            .rposition(|&b| b == b'.')
            .map_or(&b""[..], |at| base.get(at + 1..).unwrap_or_default());
        set(&mut by_format, extension.to_vec(), url);
    }
    for (extension, format) in [
        ("woff2", "woff2"),
        ("woff", "woff"),
        ("ttf", "truetype"),
        ("eot", "embedded-opentype"),
        ("otf", "opentype"),
    ] {
        if let Some((_, url)) = by_format
            .iter()
            .find(|(f, url)| f == extension.as_bytes() && !is_empty_string(url))
        {
            parts.push([&b"url('"[..], url, b"') format('", format.as_bytes(), b"')"].concat());
        }
    }
    parts.join(&b", "[..])
}

fn trim_quotes(text: &[u8]) -> &[u8] {
    let start = text
        .iter()
        .position(|&b| b != b'"' && b != b'\'')
        .unwrap_or(text.len());
    let end = text
        .iter()
        .rposition(|&b| b != b'"' && b != b'\'')
        .map_or(start, |at| at + 1);
    text.get(start..end).unwrap_or_default()
}

/// A style a rendered block enqueued, by its handle: a core block's
/// stylesheet, or nothing for a block without one.
fn block_style(handle: &str, siteurl: &str) -> Style {
    let css = handle
        .strip_prefix("wp-block-")
        .and_then(|name| BLOCK_STYLES.iter().find(|(folder, _)| *folder == name));
    match css {
        Some((folder, css)) => Style::file(
            handle,
            &format!("{siteurl}/wp-includes/blocks/{folder}/style.min.css"),
            css,
            None,
        ),
        None => Style::inline(handle, ""),
    }
}

/// A registered style as `WP_Styles` prints it.
struct Style {
    handle: String,
    /// The stylesheet URL, `None` once inlined or for inline-only styles.
    src: Option<String>,
    /// The stylesheet's contents, its `path` data.
    css: Option<&'static str>,
    /// `ver`; `None` for the WordPress version.
    version: Option<String>,
    /// The `after` inline styles.
    after: Vec<String>,
    /// `inlined_src`.
    inlined_src: Option<String>,
}

impl Style {
    fn file(handle: &str, src: &str, css: &'static str, version: Option<&str>) -> Style {
        Style {
            handle: handle.to_owned(),
            src: Some(src.to_owned()),
            css: Some(css),
            version: version.map(str::to_owned),
            after: Vec::new(),
            inlined_src: None,
        }
    }

    /// `wp_register_style( $handle, false )` with `wp_add_inline_style()`,
    /// which ignores empty code.
    fn inline(handle: &str, css: &str) -> Style {
        Style {
            handle: handle.to_owned(),
            src: None,
            css: None,
            version: None,
            after: (!is_empty_string(css.as_bytes()))
                .then(|| css.to_owned())
                .into_iter()
                .collect(),
            inlined_src: None,
        }
    }

    /// `WP_Styles::do_item()`, a stylesheet's path linked from `base`, the
    /// site's address.
    fn print(&self, base: &str) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(src) = &self.src {
            let version = self.version.as_deref().unwrap_or(WP_VERSION.trim());
            let absolute = ["http://", "https://", "//"]
                .iter()
                .any(|scheme| src.starts_with(scheme));
            let base = if absolute { "" } else { base };
            let href = format!(
                "{base}{src}?ver={}",
                String::from_utf8_lossy(&hyscribe_phpser::rawurlencode(version.as_bytes()))
            );
            out.extend(
                format!(
                    "<link rel='stylesheet' id='{}-css' href='{}' media='all' />\n",
                    esc_attr(self.handle.as_bytes()),
                    String::from_utf8_lossy(&esc_url(href.as_bytes()))
                )
                .into_bytes(),
            );
        }
        if self.after.is_empty() {
            return out;
        }
        // print_inline_style(), with its source URL comment.
        let source = match (&self.inlined_src, self.after.len()) {
            (Some(src), 1) => String::from_utf8_lossy(&esc_url_raw(src.as_bytes())).into_owned(),
            _ => String::from_utf8_lossy(&hyscribe_phpser::rawurlencode(
                format!("{}-inline-css", self.handle).as_bytes(),
            ))
            .into_owned(),
        };
        let text = [self.after.join("\n"), format!("/*# sourceURL={source} */")].join("\n");
        out.extend(
            [
                format!(
                    "<style id=\"{}-inline-css\">\n",
                    esc_attr(self.handle.as_bytes())
                )
                .as_bytes(),
                &style_text(text.as_bytes()),
                b"\n</style>\n",
            ]
            .concat(),
        );
        out
    }
}

/// `wp_maybe_inline_styles()`: the smallest stylesheets inlined, until
/// the next would pass the limit.
fn inline_styles(queue: &mut [Style]) {
    let mut candidates: Vec<&mut Style> = queue
        .iter_mut()
        // A stylesheet with contents has a source until it is inlined.
        .filter(|style| style.css.is_some())
        .collect();
    candidates.sort_by_key(|style| style.css.map_or(0, str::len));
    let mut total = 0;
    for style in candidates {
        let size = style.css.map_or(0, str::len);
        if total + size > INLINE_LIMIT {
            break;
        }
        let src = style.src.take().unwrap_or_default();
        style.after.insert(
            0,
            normalize_relative_css_links(style.css.unwrap_or_default(), &src),
        );
        style.inlined_src = Some(src);
        total += size;
    }
}

/// `_wp_normalize_relative_css_links()`: relative `url()`s made relative
/// to the site root instead.
fn normalize_relative_css_links(css: &str, src: &str) -> String {
    let dir = std::str::from_utf8(crate::formatting::dirname(src.as_bytes())).unwrap_or_default();
    let bytes = css.as_bytes();
    let space = |b: &u8| b.is_ascii_whitespace() || *b == 0x0b;
    let skip = |at: usize| {
        at + bytes
            .get(at..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| space(b))
            .count()
    };
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while let Some(found) = crate::formatting::find(bytes, at, b"url") {
        // `(url\s*\(\s*['"]?\s*)([^'"\)]+)`
        let open = skip(found + 3);
        if bytes.get(open) != Some(&b'(') {
            out.extend_from_slice(bytes.get(at..found + 3).unwrap_or_default());
            at = found + 3;
            continue;
        }
        // The spaces before and after an opening quote, and the last of them.
        let before = skip(open + 1);
        let quote = usize::from(matches!(bytes.get(before), Some(b'\'' | b'"')));
        let mut start = skip(before + quote);
        let last_space = if start > before + quote {
            Some(start - 1)
        } else {
            (before > open + 1).then(|| before - 1)
        };
        let mut len = bytes
            .get(start..)
            .unwrap_or_default()
            .iter()
            .take_while(|b| !matches!(b, b'\'' | b'"' | b')'))
            .count();
        // The capture takes one byte or more, so `\s*` gives back the last
        // space when nothing else follows.
        if len == 0
            && let Some(space) = last_space
        {
            start = space;
            len = 1;
        }
        out.extend_from_slice(bytes.get(at..start).unwrap_or_default());
        let url = bytes.get(start..start + len).unwrap_or_default();
        let keep = len == 0
            || [&b"http:"[..], b"https:", b"/", b"#", b"data:"]
                .iter()
                .any(|p| url.starts_with(p));
        if keep {
            out.extend_from_slice(url);
        } else {
            let absolute = format!("{dir}/{}", String::from_utf8_lossy(url)).replace("/./", "/");
            out.extend(make_link_relative(&absolute).into_bytes());
        }
        at = start + len;
    }
    out.extend_from_slice(bytes.get(at..).unwrap_or_default());
    String::from_utf8_lossy(&out).into_owned()
}

/// `wp_make_link_relative()`.
fn make_link_relative(link: &str) -> String {
    // `|^(https?:)?//[^/]+(/?.*)|i`
    let lower = link.to_ascii_lowercase();
    let scheme = ["https:", "http:"]
        .iter()
        .find(|p| lower.starts_with(*p))
        .map_or(0, |p| p.len());
    match link.get(scheme..).and_then(|rest| rest.strip_prefix("//")) {
        Some(rest) if !rest.is_empty() && !rest.starts_with('/') => {
            rest.find('/').map_or(String::new(), |at| {
                rest.get(at..).unwrap_or_default().to_owned()
            })
        }
        _ => link.to_owned(),
    }
}

/// `set_modifiable_text()` on a `<style>`: `</style` escaped.
fn style_text(css: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(css.len());
    let mut at = 0;
    while let Some(&b) = css.get(at) {
        let closer = css
            .get(at..at + 7)
            .is_some_and(|w| w.eq_ignore_ascii_case(b"</style"));
        if closer {
            out.extend_from_slice(b"\\3c\\2f");
            out.extend_from_slice(css.get(at + 2..at + 7).unwrap_or_default());
            at += 7;
        } else {
            out.push(b);
            at += 1;
        }
    }
    out
}

/// A registered script module.
#[derive(Debug)]
struct Module {
    id: String,
    /// The source as registered, empty for none.
    src: String,
    /// A version string, `false` for the WordPress version, or `null`.
    version: Value,
    /// Each dependency's ID and import type, `static` or `dynamic`.
    dependencies: Vec<(String, String)>,
    in_footer: bool,
    fetchpriority: String,
}

/// The script modules `wp_default_script_modules()` registers, with
/// sources under the site URL.
fn registered_modules(siteurl: &str) -> Vec<Module> {
    static JSON: OnceLock<Value> = OnceLock::new();
    let json = JSON.get_or_init(|| json_decode(SCRIPT_MODULES).unwrap_or_default());
    let text = |v: Option<&Value>| {
        String::from_utf8_lossy(&v.map(Value::to_php_string).unwrap_or_default()).into_owned()
    };
    json.as_array()
        .unwrap_or_default()
        .iter()
        .map(|(id, module)| Module {
            id: String::from_utf8_lossy(&id.to_php_string()).into_owned(),
            src: format!("{siteurl}{}", text(module.get_str("src"))),
            version: module.get_str("version").cloned().unwrap_or_default(),
            dependencies: module
                .get_str("dependencies")
                .and_then(Value::as_array)
                .unwrap_or_default()
                .iter()
                .map(|(_, d)| (text(d.get_str("id")), text(d.get_str("import"))))
                .collect(),
            in_footer: module.get_str("in_footer") == Some(&Value::Bool(true)),
            fetchpriority: text(module.get_str("fetchpriority")),
        })
        .collect()
}

/// `WP_Script_Modules` over a registry and its queue.
struct Modules<'a> {
    registry: &'a [Module],
    queue: &'a [String],
}

/// `$priorities`, lowest first.
const PRIORITIES: [&str; 3] = ["low", "auto", "high"];

impl Modules<'_> {
    fn module(&self, id: &str) -> Option<&Module> {
        self.registry.iter().find(|m| m.id == id)
    }

    fn imports<'b>(
        &self,
        module: &'b Module,
        static_only: bool,
    ) -> impl Iterator<Item = &'b String> {
        module
            .dependencies
            .iter()
            .filter(move |(_, import)| import == "static" || (!static_only && import == "dynamic"))
            .map(|(id, _)| id)
    }

    /// `get_src()`.
    fn src(&self, id: &str) -> Vec<u8> {
        let Some(module) = self.module(id).filter(|m| !m.src.is_empty()) else {
            return Vec::new();
        };
        match &module.version {
            Value::Bool(false) => add_query_arg(
                &[(b"ver", Some(WP_VERSION.trim().as_bytes()))],
                module.src.as_bytes(),
            ),
            Value::Null => module.src.as_bytes().to_vec(),
            version => add_query_arg(
                &[(b"ver", Some(&version.to_php_string()))],
                module.src.as_bytes(),
            ),
        }
    }

    /// `get_dependencies()`: every module `ids` import, breadth first.
    fn dependencies(&self, ids: &[String], static_only: bool) -> Vec<String> {
        let mut found: Vec<String> = Vec::new();
        let mut queue: std::collections::VecDeque<String> = ids.iter().cloned().collect();
        while let Some(id) = queue.pop_front() {
            let Some(module) = self.module(&id) else {
                continue;
            };
            for dependency in self.imports(module, static_only) {
                if !found.contains(dependency) && self.module(dependency).is_some() {
                    found.push(dependency.clone());
                    queue.push_back(dependency.clone());
                }
            }
        }
        found
    }

    /// `get_sorted_dependencies()`: dependencies before their dependents,
    /// leaving out what has an unregistered dependency. PHP recurses
    /// forever on a cycle; here a cycle counts as unregistered.
    fn sorted(&self, ids: &[String], static_only: bool) -> Vec<String> {
        let mut sorted = Vec::new();
        for id in ids {
            self.visit(id, static_only, &mut sorted, &mut Vec::new());
        }
        sorted
    }

    fn visit(
        &self,
        id: &str,
        static_only: bool,
        sorted: &mut Vec<String>,
        path: &mut Vec<String>,
    ) -> bool {
        if sorted.iter().any(|s| s == id) {
            return true;
        }
        let Some(module) = self.module(id).filter(|_| !path.iter().any(|p| p == id)) else {
            return false;
        };
        let wanted: Vec<&String> = self.imports(module, static_only).collect();
        if wanted.iter().any(|d| self.module(d).is_none()) {
            return false;
        }
        path.push(id.to_owned());
        for dependency in wanted {
            if !self.visit(dependency, static_only, sorted, path) {
                return false;
            }
        }
        path.pop();
        sorted.push(id.to_owned());
        true
    }

    /// `get_highest_fetchpriority()`.
    fn highest(&self, ids: &[&String]) -> &'static str {
        let index = ids
            .iter()
            .filter_map(|id| self.module(id))
            .map(|m| {
                PRIORITIES
                    .iter()
                    .position(|p| *p == m.fetchpriority)
                    .unwrap_or(0)
            })
            .max()
            .unwrap_or(0);
        PRIORITIES.get(index).copied().unwrap_or("auto")
    }

    /// The queued modules that import `id`, directly or not.
    fn queued_dependents(&self, id: &str) -> Vec<&String> {
        self.queue
            .iter()
            .filter(|q| {
                self.dependencies(&[(*q).clone()], false)
                    .iter()
                    .any(|d| d == id)
            })
            .collect()
    }

    /// `print_import_map()` of `ids`.
    fn import_map(&self, ids: &[String]) -> Vec<u8> {
        let imports: Vec<(hyscribe_phpser::Key, Value)> = ids
            .iter()
            .map(|id| {
                (
                    hyscribe_phpser::Key::String(id.as_bytes().to_vec()),
                    Value::String(self.src(id)),
                )
            })
            .filter(|(_, src)| !src.is_empty())
            .collect();
        if imports.is_empty() {
            return Vec::new();
        }
        let map = Value::Array(vec![(
            hyscribe_phpser::Key::String(b"imports".to_vec()),
            Value::Array(imports),
        )]);
        tag(
            b"script",
            &[
                (b"type", Some(b"importmap")),
                (b"id", Some(b"wp-importmap")),
            ],
            Some(&[&b"\n"[..], &json_hex_tag(&map), b"\n"].concat()),
        )
    }

    /// `print_import_map()`, `print_head_enqueued_script_modules()` and
    /// `print_script_module_preloads()`.
    fn head(&self) -> Vec<u8> {
        let mut out = self.import_map(&self.dependencies(self.queue, false));
        for id in self.sorted(self.queue, false) {
            if let Some(module) = self.module(&id).filter(|_| self.in_head(&id)) {
                out.extend(self.script(module));
            }
        }
        for id in self.sorted(self.queue, true) {
            if self.queue.contains(&id) {
                continue;
            }
            let src = self.src(&id);
            if src.is_empty() {
                continue;
            }
            let highest = self.highest(&self.queued_dependents(&id));
            out.extend(
                format!(
                    "<link rel=\"modulepreload\" href=\"{}\" id=\"{}\"",
                    String::from_utf8_lossy(&esc_url(&src)),
                    esc_attr(format!("{id}-js-modulepreload").as_bytes())
                )
                .into_bytes(),
            );
            if highest != "auto" {
                out.extend(
                    format!(" fetchpriority=\"{}\"", esc_attr(highest.as_bytes())).into_bytes(),
                );
            }
            let own = self.module(&id).map_or("", |m| m.fetchpriority.as_str());
            if highest != own && own != "auto" {
                out.extend(
                    format!(" data-wp-fetchpriority=\"{}\"", esc_attr(own.as_bytes())).into_bytes(),
                );
            }
            out.extend_from_slice(b">\n");
        }
        out
    }

    /// Whether `print_head_enqueued_script_modules()` prints `id`: a
    /// queued module for the head with no queued footer dependency.
    fn in_head(&self, id: &String) -> bool {
        self.queue.contains(id)
            && self.module(id).is_some_and(|m| !m.in_footer)
            && !self
                .dependencies(std::slice::from_ref(id), false)
                .iter()
                .any(|d| self.queue.contains(d) && self.module(d).is_some_and(|m| m.in_footer))
    }

    /// `print_script_module_data()`: the Interactivity API's server state,
    /// for a page that loads it; namespaces with an empty state are left
    /// out.
    fn data(&self, state: &[(Key, Value)]) -> Vec<u8> {
        let id = "@wordpress/interactivity";
        let loaded = self.queue.iter().any(|q| q == id)
            || self.dependencies(self.queue, false).iter().any(|d| d == id);
        let state: Vec<(Key, Value)> = state
            .iter()
            .filter(|(_, value)| !value.is_empty())
            .cloned()
            .collect();
        if !loaded || state.is_empty() {
            return Vec::new();
        }
        let data = Value::Array(vec![(Key::String(b"state".to_vec()), Value::Array(state))]);
        tag(
            b"script",
            &[
                (
                    b"id",
                    Some(format!("wp-script-module-data-{id}").as_bytes()),
                ),
                (b"type", Some(b"application/json")),
            ],
            Some(&inline_script(
                &hyscribe_phpser::json_encode_script_data(&data).unwrap_or_default(),
            )),
        )
    }

    /// `print_enqueued_script_modules()` in the footer: the queued modules
    /// the head left out.
    fn footer(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for id in self.sorted(self.queue, false) {
            if let Some(module) = self
                .module(&id)
                .filter(|_| self.queue.contains(&id) && !self.in_head(&id))
            {
                out.extend(self.script(module));
            }
        }
        out
    }

    /// `print_script_module()`, once per module. Core block modules support
    /// client-side navigation, which the Interactivity API marks.
    fn script(&self, module: &Module) -> Vec<u8> {
        let src = self.src(&module.id);
        if src.is_empty() {
            return Vec::new();
        }
        let id = format!("{}-js-module", module.id);
        let mut dependents = self.queued_dependents(&module.id);
        dependents.push(&module.id);
        let fetchpriority = self.highest(&dependents);
        let mut attributes: Vec<(&[u8], Option<&[u8]>)> = vec![
            (b"type", Some(b"module")),
            (b"src", Some(&src)),
            (b"id", Some(id.as_bytes())),
        ];
        if fetchpriority != "auto" {
            attributes.push((b"fetchpriority", Some(fetchpriority.as_bytes())));
        }
        if fetchpriority != module.fetchpriority {
            attributes.push((
                b"data-wp-fetchpriority",
                Some(module.fetchpriority.as_bytes()),
            ));
        }
        if module.id.starts_with("@wordpress/block-library") {
            attributes.push((
                b"data-wp-router-options",
                Some(br#"{"loadOnClientNavigation":true}"#),
            ));
        }
        tag(b"script", &attributes, None)
    }
}

/// A `wp-includes/js` script file as WordPress prints it inline: trimmed,
/// with its source URL.
fn script_source(siteurl: &str, name: &str, js: &str) -> Vec<u8> {
    let url = esc_url_raw(format!("{siteurl}/wp-includes/js/{name}.min.js").as_bytes());
    [js.trim().as_bytes(), b"\n//# sourceURL=", &url].concat()
}

/// The text `wp_get_inline_script_tag()` gives a script: between
/// newlines, after trimming newlines and spaces, which these scripts have
/// none of at their ends.
fn inline_script(data: &[u8]) -> Vec<u8> {
    [&b"\n"[..], data, b"\n"].concat()
}

/// A tag as `WP_HTML_Tag_Processor` builds it from `<name></name>`, with
/// `attributes` set (`None` for a boolean one) and, given, its text; and a
/// newline after it.
pub fn tag(name: &[u8], attributes: &[(&[u8], Option<&[u8]>)], text: Option<&[u8]>) -> Vec<u8> {
    let markup = [
        &b"<"[..],
        name,
        b">",
        text.unwrap_or_default(),
        b"</",
        name,
        b">",
    ]
    .concat();
    let mut tags = hyscribe_html::TagProcessor::new(&markup);
    tags.next_tag(None, None);
    for (attribute, value) in attributes {
        match value {
            Some(value) => tags.set_attribute(attribute, value),
            None => tags.set_boolean_attribute(attribute),
        };
    }
    [tags.get_updated_html(), b"\n"].concat()
}

/// The admin's `print_import_map()`, where no script module is enqueued:
/// the registered modules classic scripts import, and what they import.
pub fn admin_import_map(siteurl: &str, imports: &[String]) -> Vec<u8> {
    let registry = registered_modules(siteurl);
    let modules = Modules {
        registry: &registry,
        queue: &[],
    };
    let mut ids: Vec<String> = Vec::new();
    for id in imports.iter().filter(|id| modules.module(id).is_some()) {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    for id in modules.dependencies(&ids, false) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    modules.import_map(&ids)
}

/// `wp_json_encode( $value, JSON_HEX_TAG | JSON_UNESCAPED_SLASHES )`:
/// PHP's default encoding with slashes left alone and `<` and `>` as
/// escapes, which only strings hold.
pub fn json_hex_tag(value: &Value) -> Vec<u8> {
    let json = hyscribe_phpser::json_encode(value).unwrap_or_default();
    let mut out = Vec::with_capacity(json.len());
    let mut bytes = json.iter();
    while let Some(&b) = bytes.next() {
        match b {
            // An escape and the byte it escapes; `\/` loses its backslash.
            b'\\' => {
                let escaped = bytes.next().copied().unwrap_or_default();
                if escaped != b'/' {
                    out.push(b);
                }
                out.push(escaped);
            }
            b'<' => out.extend_from_slice(b"\\u003C"),
            b'>' => out.extend_from_slice(b"\\u003E"),
            b => out.push(b),
        }
    }
    out
}

#[cfg(test)]
mod tests;
