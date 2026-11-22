// SPDX-License-Identifier: AGPL-3.0-or-later

//! What a classic theme's calls reach in a request (ADR 0023): the features
//! it adds, request state as PHP's globals are, and the page it renders,
//! whose template tags `hyscribe_render::classic` answers. No lock is held
//! while a tag runs, so the hooks it fires may call back: such a call sees
//! the page as it was before the tag.

use std::sync::{Arc, Mutex};

use hyscribe_db::DbError;
use hyscribe_ext::{Part, TemplateFile, ThemeCall, locate};
use hyscribe_model::hooks::do_action;
use hyscribe_phpser::{Key, Value};
use hyscribe_render::classic::{Classic, Unserved};
use hyscribe_render::site::Request;
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
    /// `add_theme_support()`'s features, in the order added.
    supports: Vec<String>,
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
    /// The active theme's `call`, answered from the request's state.
    pub(crate) fn answer(&self, call: &ThemeCall) -> Result<Value, String> {
        match call {
            ThemeCall::AddSupport(feature, _) => {
                let supports = &mut lock(&self.0).supports;
                if !supports.contains(feature) {
                    supports.push(feature.clone());
                }
                Ok(Value::Null)
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
            // The global post follows the main loop: no other query sets it.
            ThemeCall::ResetPostdata => self.on_page(&|_, _, _| Ok(Value::Null)),
            ThemeCall::InTheLoop => {
                self.on_page(&|classic, _, _| Ok(Value::Bool(classic.in_the_loop())))
            }
            ThemeCall::Conditional(name, args) => self.on_page(&|classic, reads, shared| {
                classic
                    .is(reads, &shared.request, name, args)
                    .map(Value::Bool)
            }),
            ThemeCall::Print(name, args) => {
                let supports = lock(&self.0).supports.clone();
                let supported = move |feature: &str| supports.iter().any(|s| s == feature);
                self.on_page(&|classic, reads, shared| {
                    let printed = classic.print(reads, &shared.request, (name, args), &supported);
                    printed.map(Value::String)
                })
            }
            ThemeCall::Get(name, args) => self
                .on_page(&|classic, reads, shared| classic.get(reads, &shared.request, name, args)),
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

    /// Starts the page `request` of `site`, whose theme lists `files`: the
    /// theme's calls answer over it until [`Theme::end`].
    pub(crate) fn begin(&self, (site, request): (&Arc<Site>, &Request), files: &[TemplateFile]) {
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
        }));
    }

    /// Ends the page with what the theme `rendered`, which fails with
    /// the first database error its tags met.
    pub(crate) fn end(
        &self,
        rendered: Result<Vec<u8>, hyscribe_ext::Fault>,
    ) -> Result<Vec<u8>, Failed> {
        let page = lock(&self.0).page.take();
        if let Some(failed) = page.and_then(|page| lock(&page.progress).error.take()) {
            return Err(Failed::Database(failed));
        }
        rendered.map_err(Failed::Theme)
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
