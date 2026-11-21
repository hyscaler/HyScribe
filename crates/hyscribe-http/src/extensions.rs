// SPDX-License-Identifier: AGPL-3.0-or-later

//! The extensions a request runs (MVP 11.3, ADR 0022): those `active_plugins`
//! names that are installed and approved, with the hooks of the request's
//! work answered by them, and the site their host calls reach. A page
//! view's database calls each run in a transaction of their own; an
//! action's run in the action's, while it waits for the extensions.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use hyscribe_db::{Write, Writer};
use hyscribe_ext::{Asset, AssetKind, Found, Message, ObjectKind, Post, Report, Request};
use hyscribe_model::hooks::{self, Dispatch};
use hyscribe_phpser::{Key, Value};
use hyscribe_write::options::{Autoload, add_option, delete_option, get_option, update_option};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

use crate::Site;

/// What a host call answers that this host does not serve yet.
const UNSERVED: &str = "not served by this host yet";

/// What a database call answers without a writer: shadow mode (ADR 0010).
const READ_ONLY: &str = "the site is read only";

/// What a call answers once the action it ran in has gone.
const GONE: &str = "the action ended";

/// Database work of an extension's host call, run in an action's `Write`.
type Job =
    Box<dyn for<'a, 'w> FnOnce(&'a mut Write<'w>) -> Pin<Box<dyn Future<Output = ()> + 'a>> + Send>;

/// Where an extension's database calls run.
enum Through {
    /// Each in a transaction of its own, committed at once.
    Own(Option<Arc<Writer>>, Handle),
    /// In the action's transaction, which runs the jobs sent while it waits.
    Action(mpsc::Sender<Job>),
}

/// The site an extension's host calls reach, once their grants are checked.
struct Live(Through);

fn failed(err: impl std::fmt::Debug) -> String {
    format!("{err:?}")
}

fn autoload(on: Option<bool>) -> Option<Autoload> {
    on.map(|on| if on { Autoload::On } else { Autoload::Off })
}

impl Live {
    /// Runs `job` where this site's database calls run.
    fn run<T: Send + 'static>(
        &self,
        job: impl for<'a, 'w> FnOnce(
            &'a mut Write<'w>,
        ) -> Pin<Box<dyn Future<Output = Result<T, String>> + 'a>>
        + Send
        + 'static,
    ) -> Result<T, String> {
        match &self.0 {
            Through::Own(None, _) => Err(READ_ONLY.into()),
            Through::Own(Some(writer), handle) => tokio::task::block_in_place(|| {
                handle.block_on(async {
                    let mut write = writer.begin().await.map_err(failed)?;
                    let done = job(&mut write).await?;
                    write.commit().await.map(|()| done).map_err(failed)
                })
            }),
            Through::Action(jobs) => {
                let (answer, answered) = oneshot::channel();
                let job: Job = Box::new(move |write| {
                    Box::pin(async move {
                        let _ = answer.send(job(write).await);
                    })
                });
                let sent = jobs.blocking_send(job).ok();
                let answer = sent.and_then(|()| answered.blocking_recv().ok());
                answer.unwrap_or(Err(GONE.into()))
            }
        }
    }
}

impl hyscribe_ext::Site for Live {
    fn now(&self) -> u64 {
        u64::try_from(crate::now()).unwrap_or_default()
    }

    fn get_option(&self, name: &str) -> Result<Option<Value>, String> {
        let name = name.to_string();
        self.run(move |write| {
            Box::pin(async move {
                let value = get_option(write, &name).await.map_err(failed)?;
                Ok((value != Value::Bool(false)).then_some(value))
            })
        })
    }

    fn update_option(&self, name: &str, value: Value, on: Option<bool>) -> Result<bool, String> {
        let name = name.to_string();
        self.run(move |write| {
            Box::pin(async move {
                let updated = update_option(write, &name, &value, autoload(on)).await;
                updated.map_err(failed)
            })
        })
    }

    fn add_option(&self, name: &str, value: Value, on: Option<bool>) -> Result<bool, String> {
        let name = name.to_string();
        self.run(move |write| {
            Box::pin(async move {
                let added = add_option(write, &name, &value, autoload(on)).await;
                added.map_err(failed)
            })
        })
    }

    fn delete_option(&self, name: &str) -> Result<bool, String> {
        let name = name.to_string();
        self.run(move |write| {
            Box::pin(async move { delete_option(write, &name).await.map_err(failed) })
        })
    }

    fn get_meta(&self, _: ObjectKind, _: u64, _: &str) -> Result<Vec<Value>, String> {
        Err(UNSERVED.into())
    }

    fn add_meta(
        &self,
        _: ObjectKind,
        _: u64,
        _: &str,
        _: Value,
        _: bool,
    ) -> Result<Option<u64>, String> {
        Err(UNSERVED.into())
    }

    fn update_meta(
        &self,
        _: ObjectKind,
        _: u64,
        _: &str,
        _: Value,
        _: Option<Value>,
    ) -> Result<bool, String> {
        Err(UNSERVED.into())
    }

    fn delete_meta(
        &self,
        _: ObjectKind,
        _: u64,
        _: &str,
        _: Option<Value>,
    ) -> Result<bool, String> {
        Err(UNSERVED.into())
    }

    fn public_post_type(&self, post_type: &str) -> bool {
        ["post", "page", "attachment"].contains(&post_type)
    }

    fn query(&self, _: &Value) -> Result<Found, String> {
        Err(UNSERVED.into())
    }

    fn post(&self, _: u64) -> Result<Option<Post>, String> {
        Err(UNSERVED.into())
    }

    fn schedule(&self, _: i64, _: Option<&str>, _: &str, _: &[Value]) -> Result<(), String> {
        Err(UNSERVED.into())
    }

    fn unschedule(&self, _: &str, _: &[Value]) -> Result<u32, String> {
        Err(UNSERVED.into())
    }

    /// Not served yet: nothing is scheduled.
    fn next_scheduled(&self, _: &str, _: &[Value]) -> Option<i64> {
        None
    }

    fn mail(&self, _: &Message) -> Result<bool, String> {
        Err(UNSERVED.into())
    }

    /// Not served yet: no one is logged in, who can do nothing.
    fn current_user(&self) -> u64 {
        0
    }

    fn user_can(&self, _: &str, _: &[Value]) -> bool {
        false
    }

    fn create_nonce(&self, _: &str) -> String {
        String::new()
    }

    fn verify_nonce(&self, _: &str, _: &str) -> u8 {
        0
    }

    /// `__()` with no translation loaded: the text as it is.
    fn translate(&self, _: &str, text: &str, _: Option<&str>) -> String {
        text.into()
    }

    /// `_n()` with no translation loaded: English's two forms.
    fn translate_plural(
        &self,
        _: &str,
        (single, plural): (&str, &str),
        number: u64,
        _: Option<&str>,
    ) -> String {
        if number == 1 { single } else { plural }.into()
    }

    fn register_asset(&self, _: &str, _: &Asset) -> Result<(), String> {
        Err(UNSERVED.into())
    }

    fn enqueue(&self, _: AssetKind, _: &str) -> Result<(), String> {
        Err(UNSERVED.into())
    }

    fn add_inline(&self, _: AssetKind, _: &str, _: &str, _: bool) -> Result<(), String> {
        Err(UNSERVED.into())
    }
}

/// The plugin files of `active_plugins` as PHP walks it.
pub(crate) fn files(listed: &[(Key, Value)]) -> Vec<String> {
    let files = listed.iter().filter_map(|(_, file)| match file {
        Value::String(file) => String::from_utf8(file.clone()).ok(),
        _ => None,
    });
    files.collect()
}

/// Logs what went wrong in the extensions to `out`, the error output as
/// the database error page's errors are logged.
pub(crate) fn log(reports: &[Report], out: &mut dyn std::io::Write) {
    for report in reports {
        let (extension, hook) = (&report.extension, &report.hook);
        let (callback, fault) = (report.callback, &report.fault);
        let _ = writeln!(out, "extension {extension}: {hook} {callback:?}: {fault:?}");
    }
}

/// The extensions of a page view on `site`, if any are active.
pub(crate) fn request(site: &Site) -> Option<Arc<Request>> {
    let runtime = site.extensions.as_ref()?;
    let live = Live(Through::Own(site.writer.clone(), Handle::current()));
    let stored = site.options.get("active_plugins").map(Vec::as_slice);
    let listed = hyscribe_admin::plugins::option_array(stored).unwrap_or_default();
    let (request, reports) = runtime.request(&files(&listed), Arc::new(live));
    log(&reports, &mut std::io::stderr());
    request.map(Arc::new)
}

/// `request` as the dispatcher of the hooks fired within it.
pub(crate) fn dispatch(request: &Option<Arc<Request>>) -> Option<Arc<dyn Dispatch>> {
    request.clone().map(|request| request as Arc<dyn Dispatch>)
}

/// Extensions run from an action: their database calls run in the action's
/// transaction while it waits for them.
pub(crate) struct Acted {
    request: Arc<Request>,
    jobs: mpsc::Receiver<Job>,
}

impl Acted {
    /// The request `make` gives over a site whose calls the action runs.
    pub(crate) fn new<E>(
        make: impl FnOnce(Arc<dyn hyscribe_ext::Site>) -> Result<Request, E>,
    ) -> Result<Acted, E> {
        let (sender, jobs) = mpsc::channel(1);
        let request = make(Arc::new(Live(Through::Action(sender))))?;
        Ok(Acted {
            request: Arc::new(request),
            jobs,
        })
    }

    /// Runs `run` on a blocking thread, the request answering its hooks,
    /// while the database calls it makes run in `write`.
    pub(crate) async fn run<R: Send + 'static>(
        &mut self,
        write: &mut Write<'_>,
        run: impl FnOnce(&Request) -> R + Send + 'static,
    ) -> R {
        let request = self.request.clone();
        let dispatch: Arc<dyn Dispatch> = request.clone();
        let mut task =
            tokio::task::spawn_blocking(move || hooks::scoped(&dispatch, || run(&request)));
        loop {
            tokio::select! {
                done = &mut task => {
                    return done.unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()));
                }
                Some(job) = self.jobs.recv() => job(write).await,
            }
        }
    }

    /// What went wrong in the extensions so far.
    pub(crate) fn reports(&self) -> Vec<Report> {
        self.request.reports()
    }
}

#[cfg(test)]
pub(crate) mod tests;
