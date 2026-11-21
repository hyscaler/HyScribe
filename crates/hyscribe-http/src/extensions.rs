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
use hyscribe_ext::{
    Asset, AssetKind, Caps, Found, Incoming, Message, ObjectKind, Outgoing, Post, Report, Request,
};
use hyscribe_fetch::{Args, Client};
use hyscribe_model::hooks::{self, Dispatch};
use hyscribe_phpser::{Key, Value};
use hyscribe_write::options::{
    Autoload, Meta, add_metadata, add_option, delete_metadata, delete_option,
    delete_site_transient, delete_transient, get_metadata, get_option, get_site_transient,
    get_transient, set_site_transient, set_transient, update_metadata, update_option,
};
use hyscribe_write::roles::{add_role, get_role, remove_role, set_cap};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, oneshot};

use crate::Site;

/// What a host call answers that this host does not serve yet.
const UNSERVED: &str = "not served by this host yet";

/// What a database call answers without a writer: shadow mode (ADR 0010).
const READ_ONLY: &str = "the site is read only";

/// What a call answers once the action it ran in has gone.
const GONE: &str = "the action ended";

/// What a request answers with outbound fetches off (`HTTP_OUTBOUND`), or
/// from an action, which fetches nothing.
const NO_FETCHES: &str = "outbound requests are off";

/// Database work of an extension's host call, run in an action's `Write`.
type Job =
    Box<dyn for<'a, 'w> FnOnce(&'a mut Write<'w>) -> Pin<Box<dyn Future<Output = ()> + 'a>> + Send>;

/// Where an extension's database calls run.
enum Through {
    /// Each in a transaction of its own, committed at once.
    Own(Option<Arc<Writer>>, Handle),
    /// In the action's transaction, which runs the jobs sent while it waits.
    Action(mpsc::Sender<Sent>),
}

/// What the extensions' thread sends an action: a job, or that it is done.
enum Sent {
    Job(Job),
    Done,
}

/// Tells the action the extensions' thread is done, when it ends, also by
/// a panic.
struct Done(mpsc::Sender<Sent>);

impl Drop for Done {
    fn drop(&mut self) {
        let _ = self.0.blocking_send(Sent::Done);
    }
}

/// The client of an extension's requests, the site's `home` and the
/// runtime that runs them.
struct Outbound(Option<Arc<Client>>, String, Handle);

/// The site an extension's host calls reach, once their grants are
/// checked: where their database calls run, and how they fetch.
struct Live(Through, Option<Outbound>);

fn failed(err: impl std::fmt::Debug) -> String {
    format!("{err:?}")
}

fn autoload(on: Option<bool>) -> Option<Autoload> {
    on.map(|on| if on { Autoload::On } else { Autoload::Off })
}

/// An extension's database call, run in a `Write`, its answer kept by
/// the call itself: one instantiation for every kind of answer.
type Call = Box<
    dyn for<'a, 'w> FnOnce(
            &'a mut Write<'w>,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + 'a>>
        + Send,
>;

/// The database work of a host call, answering `T` or an error.
type Work<T, E> = Box<
    dyn for<'a, 'w> FnOnce(&'a mut Write<'w>) -> Pin<Box<dyn Future<Output = Result<T, E>> + 'a>>
        + Send,
>;

fn lock<T>(slot: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    slot.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `get_option()` and the transients answer `false` for none.
fn found(value: Value) -> Option<Value> {
    (value != Value::Bool(false)).then_some(value)
}

/// The meta table of an object kind.
fn meta(kind: ObjectKind) -> Meta {
    match kind {
        ObjectKind::Post => Meta::Post,
        ObjectKind::User => Meta::User,
        ObjectKind::Term => Meta::Term,
        ObjectKind::Comment => Meta::Comment,
    }
}

/// An optional value as PHP's meta functions default it: `''`.
fn or_empty(value: Option<Value>) -> Value {
    value.unwrap_or(Value::String(Vec::new()))
}

impl Live {
    /// `work`'s answer, run where this site's database calls run.
    fn ask<T: Send + 'static, E: std::fmt::Debug + 'static>(
        &self,
        work: Work<T, E>,
    ) -> Result<T, String> {
        let slot = Arc::new(std::sync::Mutex::new(None));
        let kept = slot.clone();
        let ran = self.run(Box::new(move |write| {
            Box::pin(async move {
                let answer = work(write).await.map_err(failed);
                answer.map(|answer| *lock(&kept) = Some(answer))
            })
        }));
        // Without `?`, so no instance has an error branch of its own.
        ran.and_then(|()| lock(&slot).take().ok_or(GONE.to_string()))
    }

    /// Runs `call` where this site's database calls run.
    fn run(&self, call: Call) -> Result<(), String> {
        match &self.0 {
            Through::Own(None, _) => Err(READ_ONLY.into()),
            Through::Own(Some(writer), handle) => tokio::task::block_in_place(|| {
                handle.block_on(async {
                    let mut write = writer.begin().await.map_err(failed)?;
                    call(&mut write).await?;
                    write.commit().await.map_err(failed)
                })
            }),
            Through::Action(jobs) => {
                let (answer, answered) = oneshot::channel();
                let job: Job = Box::new(move |write| {
                    Box::pin(async move {
                        let _ = answer.send(call(write).await);
                    })
                });
                let sent = jobs.blocking_send(Sent::Job(job)).ok();
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
        let got: Work<_, _> =
            Box::new(move |write| Box::pin(async move { get_option(write, &name).await }));
        self.ask(got).map(found)
    }

    fn update_option(&self, name: &str, value: Value, on: Option<bool>) -> Result<bool, String> {
        let name = name.to_string();
        self.ask(Box::new(move |write| {
            Box::pin(async move { update_option(write, &name, &value, autoload(on)).await })
        }))
    }

    fn add_option(&self, name: &str, value: Value, on: Option<bool>) -> Result<bool, String> {
        let name = name.to_string();
        self.ask(Box::new(move |write| {
            Box::pin(async move { add_option(write, &name, &value, autoload(on)).await })
        }))
    }

    fn delete_option(&self, name: &str) -> Result<bool, String> {
        let name = name.to_string();
        self.ask(Box::new(move |write| {
            Box::pin(async move { delete_option(write, &name).await })
        }))
    }

    fn get_transient(&self, name: &str, site: bool) -> Result<Option<Value>, String> {
        let name = name.to_string();
        let got: Work<_, _> = Box::new(move |write| {
            Box::pin(async move {
                if site {
                    get_site_transient(write, &name).await
                } else {
                    get_transient(write, &name).await
                }
            })
        });
        self.ask(got).map(found)
    }

    fn set_transient(
        &self,
        name: &str,
        value: Value,
        expiration: i64,
        site: bool,
    ) -> Result<bool, String> {
        let name = name.to_string();
        self.ask(Box::new(move |write| {
            Box::pin(async move {
                if site {
                    set_site_transient(write, &name, &value, expiration).await
                } else {
                    set_transient(write, &name, &value, expiration).await
                }
            })
        }))
    }

    fn delete_transient(&self, name: &str, site: bool) -> Result<bool, String> {
        let name = name.to_string();
        self.ask(Box::new(move |write| {
            Box::pin(async move {
                if site {
                    delete_site_transient(write, &name).await
                } else {
                    delete_transient(write, &name).await
                }
            })
        }))
    }

    fn fetch(
        &self,
        request: &Outgoing,
        allowed: &(dyn Fn(&str) -> bool + Sync),
    ) -> Result<Incoming, String> {
        let Some(Outbound(Some(client), home, handle)) = &self.1 else {
            return Err(NO_FETCHES.into());
        };
        let mut args = Args::new(format!("WordPress/7.1.3; {home}"));
        args.timeout = std::time::Duration::from_secs(request.timeout.into());
        let outgoing = hyscribe_fetch::Outgoing {
            method: &request.method,
            headers: &request.headers,
            body: &request.body,
        };
        let sent = client.send_within(
            request.url.as_bytes(),
            home.as_bytes(),
            (args, outgoing),
            allowed,
        );
        let fetched = tokio::task::block_in_place(|| handle.block_on(sent));
        let fetched = fetched.map_err(failed)?;
        Ok((fetched.status, fetched.headers, fetched.body))
    }

    fn role(&self, role: &str) -> Result<Option<Caps>, String> {
        let role = role.to_string();
        self.ask(Box::new(move |write| {
            Box::pin(async move { get_role(write, &role).await })
        }))
    }

    fn add_role(
        &self,
        role: &str,
        display_name: &str,
        caps: &[(String, bool)],
    ) -> Result<bool, String> {
        let (role, name, caps) = (role.to_string(), display_name.to_string(), caps.to_vec());
        self.ask(Box::new(move |write| {
            Box::pin(async move { add_role(write, &role, &name, &caps).await })
        }))
    }

    fn remove_role(&self, role: &str) -> Result<bool, String> {
        let role = role.to_string();
        self.ask(Box::new(move |write| {
            Box::pin(async move { remove_role(write, &role).await })
        }))
    }

    fn role_cap(&self, role: &str, cap: &str, grant: Option<bool>) -> Result<bool, String> {
        let (role, cap) = (role.to_string(), cap.to_string());
        self.ask(Box::new(move |write| {
            Box::pin(async move { set_cap(write, &role, &cap, grant).await })
        }))
    }

    fn get_meta(&self, kind: ObjectKind, id: u64, key: &str) -> Result<Vec<Value>, String> {
        let key = key.to_string();
        self.ask(Box::new(move |write| {
            Box::pin(async move { get_metadata(write, meta(kind), id, &key).await })
        }))
    }

    fn add_meta(
        &self,
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: Value,
        unique: bool,
    ) -> Result<Option<u64>, String> {
        let key = key.to_string();
        self.ask(Box::new(move |write| {
            Box::pin(async move { add_metadata(write, meta(kind), id, &key, &value, unique).await })
        }))
    }

    fn update_meta(
        &self,
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: Value,
        previous: Option<Value>,
    ) -> Result<bool, String> {
        let (key, previous) = (key.to_string(), or_empty(previous));
        self.ask(Box::new(move |write| {
            Box::pin(async move {
                update_metadata(write, meta(kind), id, &key, &value, &previous).await
            })
        }))
    }

    fn delete_meta(
        &self,
        kind: ObjectKind,
        id: u64,
        key: &str,
        value: Option<Value>,
    ) -> Result<bool, String> {
        let (key, value) = (key.to_string(), or_empty(value));
        self.ask(Box::new(move |write| {
            Box::pin(
                async move { delete_metadata(write, meta(kind), id, &key, &value, false).await },
            )
        }))
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

    fn theme(&self, _: &str, _: &hyscribe_ext::ThemeCall) -> Result<Value, String> {
        Err(UNSERVED.into())
    }
}

/// The plugin files of `active_plugins` as PHP walks it.
pub(crate) fn files(listed: &[(Key, Value)]) -> Vec<String> {
    listed.iter().filter_map(file).collect()
}

/// An entry's plugin file, when it is text.
fn file((_, entry): &(Key, Value)) -> Option<String> {
    match entry {
        Value::String(file) => String::from_utf8(file.clone()).ok(),
        _ => None,
    }
}

/// Logs what went wrong in the extensions to `out`, the error output as
/// the database error page's errors are logged.
pub(crate) fn log(reports: &[Report], out: &mut dyn std::io::Write) {
    let _ = out.write_all(reports.iter().map(line).collect::<String>().as_bytes());
}

/// A report's line in the log.
fn line(report: &Report) -> String {
    let (extension, hook) = (&report.extension, &report.hook);
    let (callback, fault) = (report.callback, &report.fault);
    format!("extension {extension}: {hook} {callback:?}: {fault:?}\n")
}

/// The extensions of a page view on `site`, if any are active.
pub(crate) fn request(site: &Site) -> Option<Arc<Request>> {
    let runtime = site.extensions.as_ref();
    runtime.and_then(|runtime| {
        let outbound = Outbound(site.fetch.clone(), site.home.clone(), Handle::current());
        let live = Live(
            Through::Own(site.writer.clone(), Handle::current()),
            Some(outbound),
        );
        let stored = site.options.get("active_plugins").map(Vec::as_slice);
        let listed = hyscribe_admin::plugins::option_array(stored).unwrap_or_default();
        let (request, reports) = runtime.request(&files(&listed), Arc::new(live));
        log(&reports, &mut std::io::stderr());
        request.map(Arc::new)
    })
}

/// `request` as the dispatcher of the hooks fired within it.
pub(crate) fn dispatch(request: &Arc<Request>) -> Arc<dyn Dispatch> {
    request.clone()
}

/// What an action runs of its extensions, and what it answers: a lifecycle
/// export's outcome, if it ran one.
pub(crate) type Run = Box<dyn FnOnce(&Request) -> Ran + Send>;
pub(crate) type Ran = Option<Result<(), hyscribe_ext::Fault>>;

/// Extensions run from an action: their database calls run in the action's
/// transaction while it waits for them.
pub(crate) struct Acted {
    request: Arc<Request>,
    sender: mpsc::Sender<Sent>,
    jobs: mpsc::Receiver<Sent>,
}

impl Acted {
    /// The request `make` gives over a site whose calls the action runs.
    pub(crate) fn new<E>(
        make: impl FnOnce(Arc<dyn hyscribe_ext::Site>) -> Result<Request, E>,
    ) -> Result<Acted, E> {
        let (sender, jobs) = mpsc::channel(1);
        let request = make(Arc::new(Live(Through::Action(sender.clone()), None)))?;
        Ok(Acted {
            request: Arc::new(request),
            sender,
            jobs,
        })
    }

    /// Runs `run` on a blocking thread, the request answering its hooks,
    /// while the database calls it makes run in `write`.
    pub(crate) async fn run(&mut self, write: &mut Write<'_>, run: Run) -> Ran {
        let request = self.request.clone();
        let dispatch: Arc<dyn Dispatch> = request.clone();
        let done = Done(self.sender.clone());
        let task = tokio::task::spawn_blocking(move || {
            let _done = done;
            hooks::scoped(&dispatch, || run(&request))
        });
        while let Some(Sent::Job(job)) = self.jobs.recv().await {
            job(write).await;
        }
        task.await
            .unwrap_or_else(|e| std::panic::resume_unwind(e.into_panic()))
    }

    /// What went wrong in the extensions so far.
    pub(crate) fn reports(&self) -> Vec<Report> {
        self.request.reports()
    }
}

#[cfg(test)]
pub(crate) mod tests;
