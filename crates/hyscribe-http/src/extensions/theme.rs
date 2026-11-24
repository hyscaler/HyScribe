// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a classic theme's calls reach in a request (ADR 0023): the features
//! it adds, request state as PHP's globals are, and the page it renders,
//! whose template tags `hyscribe_render::classic` answers, with its head
//! and footer, the styles the extensions enqueue among them. No lock is held
//! while a tag runs, so the hooks it fires may call back: such a call sees
//! the page as it was before the tag.

use std::sync::{Arc, Mutex};

use hyscribe_db::DbError;
use hyscribe_ext::{Asset, AssetKind, Part, TemplateFile, ThemeCall, locate};
use hyscribe_model::hooks::do_action;
use hyscribe_phpser::{Key, Value};
use hyscribe_render::classic::{Classic, Scope, Unserved};
use hyscribe_render::head::classic::{ClassicPage, Enqueued, Link, hoist};
use hyscribe_render::site::Request;
use hyscribe_render::theme::GlobalStyles;
use tokio::runtime::Handle;

use super::{UNSERVED, lock};
use crate::Site;
use crate::reads::Reads;

/// What a template tag answers outside a page's rendering.
const NO_PAGE: &str = "no page is being rendered";

/// A classic theme's state in one request.
#[derive(Default)]
pub(crate) struct Theme(Mutex<State>);

#[derive(Default)]
struct State {
    /// Whether the request is the admin's, where `is_admin()` is true.
    admin: bool,
    /// `add_theme_support()`'s features, in the order added.
    supports: Vec<String>,
    /// The styles registered, and by which extension.
    assets: Vec<(String, Asset)>,
    page: Option<Arc<Page>>,
}

/// The page a classic theme renders: what its template tags read, and
/// where its loop is.
struct Page {
    site: Arc<Site>,
    request: Request,
    files: Vec<TemplateFile>,
    handle: Handle,
    progress: Mutex<Progress>,
    /// The global styles of the theme's `theme.json`, none without one.
    global: Option<Arc<GlobalStyles>>,
    /// `wp_get_document_title()`.
    title: Vec<u8>,
    enqueued: Mutex<Enqueued>,
    /// Where the head leaves the global styles, unique to the page.
    hole: String,
}

struct Progress {
    classic: Classic,
    /// The first database error a tag's reads met, which fails the page.
    error: Option<DbError>,
}

/// Why a classic theme's page failed.
#[derive(Debug)]
pub(crate) enum Failed {
    /// A read of a template tag.
    Database(DbError),
    /// The theme's `render`.
    Theme(hyscribe_ext::Fault),
}

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failed::Database(failed) => failed.fmt(f),
            Failed::Theme(fault) => write!(f, "the theme failed: {fault:?}"),
        }
    }
}

impl Theme {
    /// The state of a request, to the admin when `admin`.
    pub(crate) fn new(admin: bool) -> Theme {
        Theme(Mutex::new(State {
            admin,
            ..State::default()
        }))
    }

    /// `add_theme_support( $feature )`, once.
    fn support(&self, feature: &str) -> Result<Value, String> {
        let supports = &mut lock(&self.0).supports;
        if !supports.iter().any(|known| known == feature) {
            supports.push(feature.into());
        }
        Ok(Value::Null)
    }

    /// The active theme's `call`, answered from the request's state. The
    /// menu locations, editor styles and content width are taken as PHP
    /// takes them, adding the features PHP adds; what reads them (the
    /// Menus screen, the block editor, embeds) follows with its port.
    pub(crate) fn answer(&self, call: &ThemeCall) -> Result<Value, String> {
        match call {
            ThemeCall::AddSupport(feature, _) => self.support(feature),
            ThemeCall::NavMenus(_) => self.support("menus"),
            ThemeCall::EditorStyle(_) => self.support("editor-style"),
            ThemeCall::ContentWidth(_) => Ok(Value::Null),
            ThemeCall::Conditional(name, args) if name == "is_admin" && args.is_empty() => {
                Ok(Value::Bool(lock(&self.0).admin))
            }
            ThemeCall::HavePosts => self.on_page(&|classic, _, shared| {
                Ok(Value::Bool(classic.have_posts(&shared.request)))
            }),
            ThemeCall::ThePost => self.on_page(&|classic, _, _| {
                classic.the_post();
                Ok(Value::Null)
            }),
            ThemeCall::RewindPosts => self.on_page(&|classic, _, _| {
                classic.rewind_posts();
                Ok(Value::Null)
            }),
            ThemeCall::ResetPostdata => self.on_page(&|classic, _, _| {
                classic.reset_postdata();
                Ok(Value::Null)
            }),
            ThemeCall::InTheLoop => {
                self.on_page(&|classic, _, _| Ok(Value::Bool(classic.in_the_loop())))
            }
            ThemeCall::Conditional(name, args) => self.on_page(&|classic, reads, shared| {
                classic
                    .is(reads, &shared.request, name, args)
                    .map(Value::Bool)
            }),
            ThemeCall::Print(name, args) if args.is_empty() && name == "wp_head" => {
                self.printed(true)
            }
            ThemeCall::Print(name, args) if args.is_empty() && name == "wp_footer" => {
                self.printed(false)
            }
            // A theme without its own `comments.php` gets core's compat
            // template, which is not ported for a classic theme.
            ThemeCall::Print(name, _)
                if name == "comments_template" && !self.has("comments.php") =>
            {
                Err(UNSERVED.into())
            }
            ThemeCall::Print(name, args) => {
                self.tag(&|classic, scope| classic.print(scope, (name, args)).map(Value::String))
            }
            ThemeCall::Get(name, args) => {
                self.tag(&|classic, scope| classic.get(scope, name, args))
            }
            ThemeCall::Part(kind, slug, name, args) => {
                self.on_page(&|_, _, shared| Ok(part(&shared.files, (*kind, slug, name, args))))
            }
            _ => Err(UNSERVED.to_string()),
        }
    }

    /// `tag` over the page being rendered, its loop's place kept after;
    /// one instance, so the coverage of its paths is counted together.
    fn on_page(
        &self,
        tag: &dyn Fn(&mut Classic, &Reads<'_>, &Page) -> Result<Value, Unserved>,
    ) -> Result<Value, String> {
        let page = lock(&self.0).page.clone().ok_or(NO_PAGE)?;
        let mut classic = lock(&page.progress).classic.clone();
        let reads = Reads::new(&page.site, page.handle.clone());
        let answer = tag(&mut classic, &reads, &page);
        let failed = reads.finish().err();
        let mut progress = lock(&page.progress);
        progress.classic = classic;
        if let Some(failed) = failed {
            progress.error.get_or_insert(failed);
        }
        answer.map_err(|Unserved| UNSERVED.to_string())
    }

    /// Whether the page's theme has the template file `file`.
    fn has(&self, file: &str) -> bool {
        let page = lock(&self.0).page.clone();
        page.is_some_and(|page| page.files.iter().any(|known| known.file == file))
    }

    /// A tag that prints or answers a value, over the page being rendered.
    fn tag(
        &self,
        tag: &dyn Fn(&mut Classic, &Scope<'_>) -> Result<Value, Unserved>,
    ) -> Result<Value, String> {
        let supports = lock(&self.0).supports.clone();
        let supported = |feature: &str| supports.iter().any(|s| s == feature);
        self.on_page(&|classic, reads, shared| {
            let scope = Scope {
                theme: &shared.site.theme,
                site: reads,
                request: &shared.request,
                supports: &supported,
            };
            tag(classic, &scope)
        })
    }

    /// `wp_head()` or `wp_footer()` of the page, for a theme with a
    /// `theme.json`; no lock is held while it runs.
    fn printed(&self, head: bool) -> Result<Value, String> {
        let page = lock(&self.0).page.clone().ok_or(NO_PAGE)?;
        let global = page.global.clone().ok_or(UNSERVED)?;
        let supports = lock(&self.0).supports.clone();
        let supported = |feature: &str| supports.iter().any(|s| s == feature);
        let reads = Reads::new(&page.site, page.handle.clone());
        let stylesheet = page.site.option("stylesheet");
        let classic = ClassicPage {
            theme: &page.site.theme,
            global: &global,
            site: &reads,
            request: &page.request,
            title: &page.title,
            supports: &supported,
            stylesheet: &stylesheet,
            enqueued: &page.enqueued,
            hole: &page.hole,
        };
        let printed = if head {
            classic.wp_head()
        } else {
            classic.wp_footer()
        };
        if let Err(failed) = reads.finish() {
            lock(&page.progress).error.get_or_insert(failed);
        }
        Ok(Value::String(printed))
    }

    /// `wp_register_style()` of the extension `slug`: a stylesheet for every
    /// medium that depends on nothing, as far as this host serves them.
    pub(crate) fn register_asset(&self, slug: &str, asset: &Asset) -> Result<(), String> {
        let served = asset.kind == AssetKind::Style
            && asset.deps.is_empty()
            && ["", "all"].contains(&asset.placement.as_str());
        if !served {
            return Err(UNSERVED.into());
        }
        lock(&self.0).assets.push((slug.into(), asset.clone()));
        Ok(())
    }

    /// `wp_enqueue_style()` of a style an extension registered, or
    /// `wp_enqueue_script()` of a script core registers, on the page; as
    /// in PHP, a handle no one registered prints nothing.
    pub(crate) fn enqueue(&self, kind: AssetKind, handle: &str) -> Result<(), String> {
        let (page, registered) = {
            let state = lock(&self.0);
            let found = state.assets.iter().rev().find(|(_, a)| a.handle == handle);
            (state.page.clone().ok_or(NO_PAGE)?, found.cloned())
        };
        let mut enqueued = lock(&page.enqueued);
        match (kind, registered) {
            (AssetKind::Script, _) => enqueued.enqueue_script(handle),
            (AssetKind::Style, Some((slug, asset))) => {
                let siteurl = page.site.option("siteurl");
                let theme = [page.site.option("stylesheet"), page.site.option("template")];
                let folder = if theme.contains(&slug) {
                    "themes"
                } else {
                    "plugins"
                };
                enqueued.enqueue_style(Link {
                    handle: handle.into(),
                    href: format!("{siteurl}/wp-content/{folder}/{slug}/{}", asset.src),
                    version: asset.version,
                });
            }
            (AssetKind::Style, None) => {}
        }
        Ok(())
    }

    /// Starts the page `request` of `site`, whose theme lists `files`, its
    /// document `title` and `global` styles: the theme's calls answer over
    /// it until [`Theme::end`].
    pub(crate) fn begin(
        &self,
        (site, request): (&Arc<Site>, &Request),
        files: &[TemplateFile],
        (title, global): (&str, Option<Arc<GlobalStyles>>),
    ) {
        let theme = (site.option("template"), site.option("stylesheet"));
        let progress = Progress {
            classic: Classic::new((&theme.0, &theme.1)),
            error: None,
        };
        lock(&self.0).page = Some(Arc::new(Page {
            site: site.clone(),
            request: request.clone(),
            files: files.to_vec(),
            handle: Handle::current(),
            progress: Mutex::new(progress),
            global,
            title: title.as_bytes().to_vec(),
            enqueued: Mutex::default(),
            hole: format!("<!-- {} -->", hyscribe_render::head::embed_secret()),
        }));
    }

    /// Ends the page with what the theme `rendered`, its late styles moved
    /// into its head; it fails with the first database error its tags met.
    pub(crate) fn end(
        &self,
        rendered: Result<Vec<u8>, hyscribe_ext::Fault>,
    ) -> Result<Vec<u8>, Failed> {
        let page = lock(&self.0).page.take();
        let Some(page) = page else {
            return rendered.map_err(Failed::Theme);
        };
        if let Some(failed) = lock(&page.progress).error.take() {
            return Err(Failed::Database(failed));
        }
        let rendered = rendered.map_err(Failed::Theme)?;
        Ok(hoist(&rendered, &page.enqueued, &page.hole))
    }
}

/// `get_header()`, `get_footer()`, `get_sidebar()` and `get_template_part()`:
/// their actions fired, the file of the theme's `files` to render, if any.
fn part(
    files: &[TemplateFile],
    (kind, slug, name, args): (Part, &str, &Option<String>, &[Value]),
) -> Value {
    let name = name.as_deref().filter(|name| !name.is_empty());
    let named = name.map_or(Value::Null, |name| Value::String(name.into()));
    let list = args
        .iter()
        .enumerate()
        .map(|(at, arg)| (Key::Int(at as i64), arg.clone()));
    let args = Value::Array(list.collect());
    let (slug, action) = match kind {
        Part::Header => ("header", "get_header"),
        Part::Footer => ("footer", "get_footer"),
        Part::Sidebar => ("sidebar", "get_sidebar"),
        Part::TemplatePart => (slug, ""),
    };
    let mut candidates: Vec<String> = name
        .map(|name| format!("{slug}-{name}"))
        .into_iter()
        .collect();
    candidates.push(slug.to_string());
    match kind {
        Part::TemplatePart => {
            let given = Value::String(slug.into());
            do_action(
                &format!("get_template_part_{slug}"),
                &[&given, &named, &args],
            );
            let templates: Vec<String> = candidates.iter().map(|c| format!("{c}.php")).collect();
            do_action("get_template_part", &[&given, &named, &templates, &args]);
        }
        _ => do_action(action, &[&named, &args]),
    }
    let found = locate(&candidates, files);
    found.map_or(Value::Null, |file| Value::String(file.into()))
}

#[cfg(test)]
mod tests;
