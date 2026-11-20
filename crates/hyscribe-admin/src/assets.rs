// SPDX-License-Identifier: AGPL-3.0-or-later

//! The admin's styles and scripts as `WP_Styles` and `WP_Scripts` print
//! them: dependencies first, the core files concatenated through
//! `load-styles` and `load-scripts`, the rest as their own tags.

use std::collections::HashMap;
use std::sync::OnceLock;

use hyscribe_kses::{esc_attr, esc_url, esc_url_raw};
use hyscribe_phpser::{Key, Value, json_decode, rawurlencode};
use hyscribe_render::head::tag;

/// What a request registers, with placeholders for its URLs and nonces.
const REGISTRY: &[u8] = include_bytes!("../../../wordpress/wp-includes/registry.json");

/// `$wp_version`, the default version, with its newline.
const VERSION: &str = include_str!("../../../wordpress/VERSION");

/// The site a page is printed for.
pub trait Urls {
    /// `site_url()`, the base of every relative source.
    fn site_url(&self) -> String;
    /// `admin_url( $path )`.
    fn admin_url(&self, path: &str) -> String;
    /// The registry's placeholders filled in for this request.
    fn fill(&self, text: &str) -> String;
}

/// `preg_replace( '|https?://[^/]+|i', '', $url . '/' )`: a URL's path, for
/// `COOKIEPATH` and `SITECOOKIEPATH`.
pub fn cookie_path(url: &str) -> String {
    let url = format!("{url}/");
    let lower = url.to_ascii_lowercase();
    let scheme = ["http://", "https://"]
        .into_iter()
        .find(|scheme| lower.starts_with(scheme));
    match scheme {
        Some(scheme) => {
            let rest = url.get(scheme.len()..).unwrap_or_default();
            rest.find('/').map_or(String::new(), |at| {
                rest.get(at..).unwrap_or_default().into()
            })
        }
        None => url,
    }
}

/// `text` with each `placeholder` and the admin file after it as hs-admin
/// links them: `base`, then the file without `.php`.
pub fn admin_links(text: &str, placeholder: &str, base: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some((before, after)) = rest.split_once(placeholder) {
        out.push_str(before);
        out.push_str(base);
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.')))
            .unwrap_or(after.len());
        let (file, tail) = after.split_at(end);
        out.push_str(file.strip_suffix(".php").unwrap_or(file));
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// A registered style or script.
struct Item {
    /// A path or URL; `None` for an alias.
    src: Option<String>,
    /// A `src` of `true`, the admin color scheme's stylesheet.
    color_scheme: bool,
    deps: Vec<String>,
    ver: String,
    /// A script printed in the footer.
    footer: bool,
    extra: Value,
    textdomain: bool,
}

fn registry() -> &'static HashMap<(bool, String), Item> {
    static ITEMS: OnceLock<HashMap<(bool, String), Item>> = OnceLock::new();
    ITEMS.get_or_init(|| {
        let data = json_decode(REGISTRY).unwrap_or(Value::Null);
        let text = |value: &Value| String::from_utf8_lossy(&value.to_php_string()).into_owned();
        let mut items = HashMap::new();
        for (scripts, kind) in [(false, "styles"), (true, "scripts")] {
            let entries = data
                .get_str(kind)
                .and_then(Value::as_array)
                .unwrap_or_default();
            for (handle, item) in entries {
                let field = |name| item.get_str(name).cloned().unwrap_or(Value::Null);
                let deps = field("deps")
                    .as_array()
                    .unwrap_or_default()
                    .iter()
                    .map(|(_, d)| text(d))
                    .collect();
                let item = Item {
                    src: Some(field("src"))
                        .filter(|src| !src.is_empty())
                        .map(|src| text(&src)),
                    color_scheme: field("src") == Value::Bool(true),
                    deps,
                    ver: text(&field("ver")),
                    footer: field("args") == Value::Int(1),
                    extra: field("extra"),
                    textdomain: !field("textdomain").is_empty(),
                };
                items.insert(
                    (
                        scripts,
                        String::from_utf8_lossy(&handle.to_php_string()).into_owned(),
                    ),
                    item,
                );
            }
        }
        items
    })
}

/// `wp_get_inline_script_tag()`'s trim of its text.
fn trim(text: &str) -> &str {
    text.trim_matches(['\n', '\r', ' '])
}

/// A registered script's `extra` data under `key`, as text.
pub(crate) fn registered_script_data(handle: &str, key: &str) -> String {
    registry()
        .get(&(true, handle.to_string()))
        .and_then(|item| item.extra.get_str(key))
        .map(|data| String::from_utf8_lossy(&data.to_php_string()).into_owned())
        .unwrap_or_default()
}

/// What a request registers with no source, as `wp_register_style( $handle,
/// false )` does before adding inline styles.
static ALIAS: Item = Item {
    src: None,
    color_scheme: false,
    deps: Vec::new(),
    ver: String::new(),
    footer: false,
    extra: Value::Null,
    textdomain: false,
};

/// `wp-block-styles`, the script alias `enqueue_editor_block_styles_assets()`
/// registers for its inline script, in the footer; `wp-blocks`, which it
/// needs, is printed before by the editor's scripts.
static BLOCK_STYLES: Item = Item {
    src: None,
    color_scheme: false,
    deps: Vec::new(),
    ver: String::new(),
    footer: true,
    extra: Value::Null,
    textdomain: false,
};

/// The queue and printing state of `WP_Styles` or `WP_Scripts`.
#[derive(Default)]
struct Dependencies {
    scripts: bool,
    /// `extra` values this request sets, by handle and key.
    extra: HashMap<(String, String), Value>,
    /// The admin color scheme's stylesheet, `wp_style_loader_src()`'s.
    color_scheme: Option<String>,
    queue: Vec<String>,
    /// The styles this request registers, a theme's own.
    added: HashMap<String, &'static Item>,
    to_do: Vec<(String, &'static Item)>,
    done: Vec<String>,
    groups: HashMap<String, i64>,
    concat: String,
    print_code: String,
    print_html: String,
    do_concat: bool,
}

/// A page's styles and scripts.
pub struct Assets {
    styles: Dependencies,
    scripts: Dependencies,
    concatenate: bool,
}

impl Assets {
    /// No styles or scripts yet; `concatenate` as `$concatenate_scripts`,
    /// which the admin and the login page set.
    pub fn new(concatenate: bool) -> Assets {
        Assets {
            styles: Dependencies::default(),
            scripts: Dependencies {
                scripts: true,
                ..Dependencies::default()
            },
            concatenate,
        }
    }

    /// `wp_enqueue_style()` of a registered handle.
    pub fn enqueue_style(&mut self, handle: &str) {
        self.styles.queue.push(handle.to_string());
    }

    /// `wp_register_style( $handle, $src, array(), $ver )` of a file of the
    /// theme, `$src` a path under the site's address; its registration is
    /// kept once, as those of the registry are.
    pub fn register_style(&mut self, handle: &str, src: &str, ver: &str) {
        type Known = HashMap<(String, String, String), &'static Item>;
        static KNOWN: OnceLock<std::sync::Mutex<Known>> = OnceLock::new();
        let key = (handle.to_string(), src.to_string(), ver.to_string());
        let known = KNOWN.get_or_init(Default::default).lock();
        let mut known = known.unwrap_or_else(std::sync::PoisonError::into_inner);
        let item = *known.entry(key).or_insert_with(|| {
            Box::leak(Box::new(Item {
                src: Some(src.to_string()),
                color_scheme: false,
                deps: Vec::new(),
                ver: ver.to_string(),
                footer: false,
                extra: Value::Null,
                textdomain: false,
            }))
        });
        self.styles.added.insert(handle.to_string(), item);
    }

    /// `wp_enqueue_style()` of a registered handle, queued right after
    /// `after` if that is queued, else last.
    pub fn enqueue_style_after(&mut self, after: &str, handle: &str) {
        let queue = &mut self.styles.queue;
        let at = queue.iter().position(|queued| queued == after);
        queue.insert(at.map_or(queue.len(), |at| at + 1), handle.to_string());
    }

    /// A style taken as printed already, as `$wp_styles->done` lists it.
    pub fn style_done(&mut self, handle: &str) {
        self.styles.done.push(handle.to_string());
    }

    /// `wp_add_inline_script( $handle, $script, $position )`, after what is
    /// registered and added before.
    pub fn add_inline_script(&mut self, handle: &str, position: &str, script: &str) {
        let mut list = self
            .scripts
            .data(handle, position)
            .and_then(Value::as_array)
            .unwrap_or_default()
            .to_vec();
        list.push((
            Key::Int(list.len() as i64),
            Value::String(script.as_bytes().to_vec()),
        ));
        self.set_script_data(handle, position, Value::Array(list));
    }

    /// `wp_enqueue_script()` of a registered handle.
    pub fn enqueue_script(&mut self, handle: &str) {
        self.scripts.queue.push(handle.to_string());
    }

    /// `add_data()` on a style, over what was registered; an unregistered
    /// handle becomes an alias, as `wp_register_style( $handle, false )`.
    pub fn set_style_data(&mut self, handle: &str, key: &str, value: Value) {
        self.styles
            .extra
            .insert((handle.to_string(), key.to_string()), value);
    }

    /// `add_data()` on a registered script, over what was registered: this
    /// request's inline scripts and localized data.
    pub fn set_script_data(&mut self, handle: &str, key: &str, value: Value) {
        self.scripts
            .extra
            .insert((handle.to_string(), key.to_string()), value);
    }

    /// The URL of the admin color scheme's stylesheet, `None` for a scheme
    /// without one.
    pub fn set_color_scheme(&mut self, url: Option<String>) {
        self.styles.color_scheme = url;
    }

    /// The script modules the queued and printed scripts import, in the
    /// order `WP_Script_Modules::get_import_map()` finds them.
    pub fn module_dependencies(&self) -> Vec<String> {
        let scripts = &self.scripts;
        let mut handles: Vec<String> = scripts.queue.iter().chain(&scripts.done).cloned().collect();
        let mut processed: Vec<String> = Vec::new();
        let mut found = Vec::new();
        while let Some(handle) = handles.pop() {
            let Some(item) = scripts
                .item(&handle)
                .filter(|_| !processed.contains(&handle))
            else {
                continue;
            };
            processed.push(handle.clone());
            let modules = scripts.data(&handle, "module_dependencies");
            for (_, module) in modules.and_then(Value::as_array).unwrap_or_default() {
                let id = module.get_str("id").unwrap_or(module);
                found.push(String::from_utf8_lossy(&id.to_php_string()).into_owned());
            }
            handles.extend(item.deps.iter().filter(|d| !processed.contains(d)).cloned());
        }
        found
    }

    /// `print_admin_styles()`, and `print_late_styles()` after it.
    pub fn print_admin_styles(&mut self, urls: &dyn Urls) -> Vec<u8> {
        let mut out = Vec::new();
        self.styles.do_concat = self.concatenate;
        self.styles.do_items(urls, None, &mut out);
        out.extend(self.styles.print_concatenated(urls));
        self.styles.reset();
        out
    }

    /// `print_head_scripts()`.
    pub fn print_head_scripts(&mut self, urls: &dyn Urls) -> Vec<u8> {
        self.print_scripts(urls, 0)
    }

    /// `print_footer_scripts()`.
    pub fn print_footer_scripts(&mut self, urls: &dyn Urls) -> Vec<u8> {
        self.print_scripts(urls, 1)
    }

    fn print_scripts(&mut self, urls: &dyn Urls, group: i64) -> Vec<u8> {
        let mut out = Vec::new();
        self.scripts.do_concat = self.concatenate;
        self.scripts.do_items(urls, Some(group), &mut out);
        out.extend(self.scripts.print_concatenated(urls));
        self.scripts.reset();
        out
    }
}

impl Dependencies {
    fn item(&self, handle: &str) -> Option<&'static Item> {
        registry()
            .get(&(self.scripts, handle.to_string()))
            .or_else(|| self.added.get(handle).copied())
            .or_else(|| {
                let added = !self.scripts && self.extra.keys().any(|(h, _)| h == handle);
                added.then_some(&ALIAS)
            })
            .or_else(|| (self.scripts && handle == "wp-block-styles").then_some(&BLOCK_STYLES))
    }

    /// `get_data( $handle, $key )`.
    fn data(&self, handle: &str, key: &str) -> Option<&Value> {
        match self.extra.get(&(handle.to_string(), key.to_string())) {
            Some(value) => Some(value),
            None => self.item(handle)?.extra.get_str(key),
        }
    }

    /// `reset()`; `WP_Styles` keeps its inline styles, which the late
    /// styles print again.
    fn reset(&mut self) {
        self.do_concat = false;
        self.concat.clear();
        if self.scripts {
            self.print_code.clear();
        }
        self.print_html.clear();
    }

    /// `set_group()`: a script's group from its registration, never later
    /// than its dependent's; true when it moves.
    fn set_group(&mut self, handle: &str, group: Option<i64>) -> bool {
        let calculated = if self.scripts {
            if self.item(handle).is_some_and(|item| item.footer) {
                1
            } else {
                self.data(handle, "group").map_or(0, Value::to_int)
            }
        } else {
            0
        };
        let group = group.map_or(calculated, |group| calculated.min(group));
        if self.groups.get(handle).is_some_and(|known| *known <= group) {
            return false;
        }
        self.groups.insert(handle.to_string(), group);
        true
    }

    /// `all_deps()`: the handles and their dependencies in order, skipping
    /// an unregistered handle (core registers every dependency).
    fn all_deps(&mut self, handles: &[String], recursion: bool, group: Option<i64>) {
        for handle in handles {
            let handle = handle.split('?').next().unwrap_or_default().to_string();
            let queued = self.to_do.iter().any(|(h, _)| *h == handle);
            if self.done.contains(&handle) {
                continue;
            }
            let moved = self.set_group(&handle, if recursion { group } else { None });
            let Some(item) = self.item(&handle).filter(|_| !queued || moved) else {
                continue;
            };
            let new_group = self.groups.get(&handle).copied();
            self.all_deps(&item.deps, true, new_group);
            if !queued {
                self.to_do.push((handle, item));
            }
        }
    }

    /// `do_items( false, $group )`.
    fn do_items(&mut self, urls: &dyn Urls, group: Option<i64>, out: &mut Vec<u8>) {
        let queue = self.queue.clone();
        self.all_deps(&queue, false, None);
        for (handle, item) in std::mem::take(&mut self.to_do) {
            let done = if self.scripts {
                self.do_script(urls, &handle, item, group, out)
            } else {
                self.do_style(urls, &handle, item, out)
            };
            if done {
                self.done.push(handle);
            }
        }
    }

    /// Where `$this->default_dirs` start, for a source to concatenate.
    fn in_default_dir(&self, src: &str) -> bool {
        if self.scripts {
            src.starts_with("/wp-admin/js/") || src.starts_with("/wp-includes/js/")
        } else {
            src.starts_with("/wp-admin/") || src.starts_with("/wp-includes/css/")
        }
    }

    /// The version a tag carries, WordPress's for an empty one.
    fn version(item: &Item) -> &str {
        match item.ver.as_str() {
            "" | "0" => VERSION.trim(),
            ver => ver,
        }
    }

    /// `$src` with its `ver` argument, escaped by `escape`.
    fn versioned(
        &self,
        urls: &dyn Urls,
        item: &Item,
        src: &str,
        escape: fn(&[u8]) -> Vec<u8>,
    ) -> Vec<u8> {
        let src = self.href(urls, src);
        let separator = if src.contains('?') { '&' } else { '?' };
        let ver = rawurlencode(Self::version(item).as_bytes());
        escape(format!("{src}{separator}ver={}", String::from_utf8_lossy(&ver)).as_bytes())
    }

    /// `$this->base_url . $src`, unless the source is a URL already.
    fn href(&self, urls: &dyn Urls, src: &str) -> String {
        let src = urls.fill(src);
        if src.starts_with("http://") || src.starts_with("https://") || src.starts_with("//") {
            return src;
        }
        match src.strip_prefix("/wp-admin/") {
            Some(admin) => urls.admin_url(admin),
            None => format!("{}{src}", urls.site_url()),
        }
    }

    /// `WP_Styles::do_item()`.
    fn do_style(&mut self, urls: &dyn Urls, handle: &str, item: &Item, out: &mut Vec<u8>) -> bool {
        if self.data(handle, "conditional").is_some() {
            return false;
        }
        if item.color_scheme {
            if let Some(url) = self.color_scheme.clone() {
                let href = self.versioned(urls, item, &url, esc_url);
                let link = format!(
                    "<link rel='stylesheet' id='{handle}-css' href='{}' media='all' />\n",
                    String::from_utf8_lossy(&href)
                );
                self.emit(out, link.as_bytes());
            }
            return true;
        }
        let Some(src) = &item.src else {
            self.inline_style_tag(urls, handle, out);
            return true;
        };
        if self.do_concat && self.in_default_dir(src) {
            self.concat.push_str(&format!("{handle},"));
            let inline = self.inline_style(urls, handle).unwrap_or_default();
            self.print_code.push_str(&inline);
            return true;
        }
        let href = self.versioned(urls, item, src, esc_url);
        let link = format!(
            "<link rel='stylesheet' id='{}-css' href='{}' media='all' />\n",
            esc_attr(handle.as_bytes()),
            String::from_utf8_lossy(&href)
        );
        self.emit(out, link.as_bytes());
        self.inline_style_tag(urls, handle, out);
        true
    }

    /// The `<style>` of `handle`'s inline styles, if it has any.
    fn inline_style_tag(&mut self, urls: &dyn Urls, handle: &str, out: &mut Vec<u8>) {
        if let Some(css) = self.inline_style(urls, handle) {
            let id = format!("{handle}-inline-css");
            let text = format!("\n{css}\n");
            let style = tag(
                b"style",
                &[(b"id", Some(id.as_bytes()))],
                Some(text.as_bytes()),
            );
            self.emit(out, &style);
        }
    }

    /// `print_inline_style( $handle, false )`: the `after` styles, with their
    /// source URL when they print on their own.
    fn inline_style(&self, urls: &dyn Urls, handle: &str) -> Option<String> {
        let after = self.data(handle, "after")?.as_array().unwrap_or_default();
        let mut lines: Vec<String> = after
            .iter()
            .map(|(_, css)| urls.fill(&String::from_utf8_lossy(&css.to_php_string())))
            .collect();
        if !self.do_concat {
            let source = rawurlencode(format!("{handle}-inline-css").as_bytes());
            lines.push(format!(
                "/*# sourceURL={} */",
                String::from_utf8_lossy(&source)
            ));
        }
        Some(lines.join("\n"))
    }

    /// Output that waits for the concatenated tag, or goes out now.
    fn emit(&mut self, out: &mut Vec<u8>, markup: &[u8]) {
        if self.do_concat {
            self.print_html.push_str(&String::from_utf8_lossy(markup));
        } else {
            out.extend_from_slice(markup);
        }
    }

    /// `get_inline_script_tag( $handle, $position )`.
    fn inline_script_tag(&self, urls: &dyn Urls, handle: &str, position: &str) -> Vec<u8> {
        let lines = self
            .data(handle, position)
            .and_then(Value::as_array)
            .unwrap_or_default();
        let mut parts: Vec<String> = lines
            .iter()
            .filter(|(_, line)| !line.is_empty())
            .map(|(_, line)| urls.fill(&String::from_utf8_lossy(&line.to_php_string())))
            .collect();
        if parts.is_empty() {
            return Vec::new();
        }
        parts.push(format!(
            "//# sourceURL={}",
            String::from_utf8_lossy(&rawurlencode(format!("{handle}-js-{position}").as_bytes()))
        ));
        let js = parts.join("\n");
        let id = format!("{handle}-js-{position}");
        let text = format!("\n{}\n", trim(&js));
        tag(
            b"script",
            &[(b"id", Some(id.as_bytes()))],
            Some(text.as_bytes()),
        )
    }

    /// `print_extra_script()`'s localized data.
    fn extra_script(&self, urls: &dyn Urls, handle: &str) -> Option<String> {
        let data = self.data(handle, "data").filter(|data| !data.is_empty())?;
        Some(urls.fill(&String::from_utf8_lossy(&data.to_php_string())))
    }

    /// A registered `extra` value as text, empty when there is none.
    fn text(&self, handle: &str, key: &str) -> String {
        self.data(handle, key)
            .map(|value| String::from_utf8_lossy(&value.to_php_string()).into_owned())
            .unwrap_or_default()
    }

    /// `WP_Scripts::do_item()`. In English no script has translations, and
    /// no core script with a strategy has dependents or inline scripts, so
    /// each keeps its intended strategy and fetch priority.
    fn do_script(
        &mut self,
        urls: &dyn Urls,
        handle: &str,
        item: &Item,
        group: Option<i64>,
        out: &mut Vec<u8>,
    ) -> bool {
        let own_group = self.groups.get(handle).copied().unwrap_or(0);
        // The head leaves the footer's scripts for later.
        if group == Some(0) && own_group > 0 {
            return false;
        }
        if self.data(handle, "conditional").is_some() {
            return false;
        }
        let strategy = self.text(handle, "strategy");
        let fetchpriority = self.text(handle, "fetchpriority");
        let before = self.inline_script_tag(urls, handle, "before");
        let after = self.inline_script_tag(urls, handle, "after");
        if self.do_concat {
            let concatenated = item
                .src
                .as_deref()
                .is_some_and(|src| self.in_default_dir(src));
            let stops =
                !before.is_empty() || !after.is_empty() || item.textdomain || !strategy.is_empty();
            if concatenated && stops {
                out.extend(self.print_concatenated(urls));
                self.reset();
            } else if concatenated {
                let extra = self.extra_script(urls, handle).unwrap_or_default();
                self.print_code.push_str(&extra);
                self.concat.push_str(&format!("{handle},"));
                return true;
            }
        }
        // Concatenation has always stopped by here: mediaelement, core's only
        // data outside it, follows mediaelement-core's inline script.
        if let Some(extra) = self.extra_script(urls, handle) {
            let id = format!("{handle}-js-extra");
            let source = rawurlencode(id.as_bytes());
            let extra = format!(
                "{extra}\n//# sourceURL={}",
                String::from_utf8_lossy(&source)
            );
            let text = format!("\n{}\n", trim(&extra));
            let script = tag(
                b"script",
                &[(b"id", Some(id.as_bytes()))],
                Some(text.as_bytes()),
            );
            out.extend(script);
        }
        // An alias prints its inline scripts alone.
        let Some(src) = &item.src else {
            self.emit(out, &[&before[..], &after[..]].concat());
            return true;
        };
        let src = self.versioned(urls, item, src, esc_url_raw);
        let id = format!("{handle}-js");
        let mut attributes: Vec<(&[u8], Option<&[u8]>)> =
            vec![(b"src", Some(&src)), (b"id", Some(id.as_bytes()))];
        if !strategy.is_empty() {
            attributes.push((strategy.as_bytes(), None));
            attributes.push((b"data-wp-strategy", Some(strategy.as_bytes())));
        }
        if !fetchpriority.is_empty() {
            attributes.push((b"fetchpriority", Some(fetchpriority.as_bytes())));
        }
        let script = tag(b"script", &attributes, None);
        let markup = [before, script, after].concat();
        self.emit(out, &markup);
        true
    }

    /// `_print_styles()` and `_print_scripts()`: the concatenated tag, in
    /// chunks of 128 characters, then what waited for it.
    fn print_concatenated(&mut self, urls: &dyn Urls) -> Vec<u8> {
        let mut out = Vec::new();
        let concat = self
            .concat
            .trim_matches(|c| c == ',' || c == ' ')
            .to_string();
        if !concat.is_empty() {
            let chunks: String = concat
                .as_bytes()
                .chunks(128)
                .enumerate()
                .map(|(key, chunk)| {
                    format!("&load%5Bchunk_{key}%5D={}", String::from_utf8_lossy(chunk))
                })
                .collect();
            if self.scripts {
                if !self.print_code.is_empty() {
                    let source = rawurlencode(format!("js-inline-concat-{concat}").as_bytes());
                    let code = format!(
                        "{}\n//# sourceURL={}",
                        self.print_code,
                        String::from_utf8_lossy(&source)
                    );
                    let text = format!("\n{}\n", trim(&code));
                    out.extend(tag(b"script", &[], Some(text.as_bytes())));
                }
                let src = format!(
                    "{}?c=0{chunks}&ver={}",
                    urls.admin_url("load-scripts.php"),
                    VERSION.trim()
                );
                out.extend(tag(b"script", &[(b"src", Some(src.as_bytes()))], None));
            } else {
                let href = format!(
                    "{}?c=0&dir=ltr{chunks}&ver={}",
                    urls.admin_url("load-styles.php"),
                    VERSION.trim()
                );
                out.extend(
                    format!(
                        "<link rel='stylesheet' href='{}' media='all' />\n",
                        esc_attr(href.as_bytes())
                    )
                    .into_bytes(),
                );
                if !self.print_code.is_empty() {
                    let source = rawurlencode(format!("css-inline-concat-{concat}").as_bytes());
                    let text = format!(
                        "\n{}\n/*# sourceURL={} */\n",
                        self.print_code,
                        String::from_utf8_lossy(&source)
                    );
                    out.extend(tag(b"style", &[], Some(text.as_bytes())));
                }
            }
        }
        out.extend(self.print_html.as_bytes());
        out
    }
}

#[cfg(test)]
mod tests;

/// `wp_site_icon()`: the tags of the site icon `icon` gives at a size, if
/// it has one at 512.
pub(crate) fn site_icon_tags(icon: &dyn Fn(i64) -> Option<Vec<u8>>) -> String {
    if icon(512).is_none() {
        return String::new();
    }
    let tags = [
        (32, "<link rel=\"icon\" href=\"%s\" sizes=\"32x32\" />"),
        (192, "<link rel=\"icon\" href=\"%s\" sizes=\"192x192\" />"),
        (180, "<link rel=\"apple-touch-icon\" href=\"%s\" />"),
        (
            270,
            "<meta name=\"msapplication-TileImage\" content=\"%s\" />",
        ),
    ];
    let mut out = String::new();
    // An icon with a full image has every smaller size.
    for (size, tag) in tags {
        let url = esc_url(&icon(size).unwrap_or_default());
        out.push_str(&tag.replace("%s", &String::from_utf8_lossy(&url)));
        out.push('\n');
    }
    out
}
