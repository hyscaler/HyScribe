// SPDX-License-Identifier: AGPL-3.0-or-later

//! The one `wp_mail()` (ADR 0016): WordPress's filters in its order, the
//! header injection checks, then the relay, through a bounded queue that
//! one background task empties at most 60 messages a minute. A message
//! that cannot be sent is logged and dropped, never failing the request.

use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use hyscribe_config::SmtpTls;
use hyscribe_kses::url_host;
use lettre::Message;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::Mail;
use crate::headers::{breaks_line, parse};
use crate::hooks::Hooks;
use crate::transport::{LIMIT, Relay, message};

/// The one log line of a message: its kind, the recipient's domain, the
/// outcome and the time taken since `started`; never a subject, body or address.
macro_rules! log {
    ($kind:expr, $domain:expr, $outcome:expr, $started:expr $(,)?) => {
        eprintln!(
            "mail {} to {}: {} in {} ms",
            $kind,
            $domain,
            $outcome,
            $started.elapsed().as_millis()
        )
    };
}

/// How many messages leave, wait and are waited for.
#[derive(Clone, Copy)]
struct Limits {
    /// The most messages sent in a `window`; the rest wait in the queue.
    per_window: usize,
    window: Duration,
    /// The most messages waiting; past it a message is refused.
    queue: usize,
    /// The longest a caller waits for its message.
    wait: Duration,
}

/// ADR 0016's: 60 messages a minute, 1,000 waiting, 30 seconds a caller.
const LIMITS: Limits = Limits {
    per_window: 60,
    window: Duration::from_secs(60),
    queue: 1000,
    wait: LIMIT,
};

/// The password reset mails one account may be sent in an hour.
const PER_ACCOUNT: usize = 3;

/// The password reset mails one client address may ask for in an hour.
const PER_ADDRESS: usize = 10;

/// The most accounts or addresses tracked; past it resets are refused.
const TRACKED: usize = 10_000;

const HOUR: Duration = Duration::from_secs(3600);

/// A recipient, `Name <address>` split as `wp_mail()` splits it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipient {
    /// The name shown, possibly empty.
    pub name: String,
    /// The address.
    pub address: String,
}

/// A message past `wp_mail()`'s filters and checks, as the `phpmailer_init`
/// stand-in receives it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outgoing {
    /// The sender address: always `MAIL_FROM` or WordPress's default.
    pub from: String,
    /// The sender's name shown.
    pub from_name: String,
    /// The recipients.
    pub to: Vec<Recipient>,
    /// The `Reply-To` addresses.
    pub reply_to: Vec<Recipient>,
    /// The subject.
    pub subject: String,
    /// The body.
    pub body: String,
    /// Whether the body is `text/html` rather than `text/plain`.
    pub html: bool,
}

/// A message waiting for the relay.
struct Job {
    kind: &'static str,
    domain: String,
    data: Mail,
    message: Message,
    /// Where a waiting caller hears the result.
    done: Option<oneshot::Sender<bool>>,
    queued: Instant,
}

/// The mail transport: `wp_mail()` and the password reset limits.
pub struct Mailer {
    /// The queue to the relay; none while mail is off.
    jobs: Option<mpsc::Sender<Job>>,
    /// The sender address of every message.
    from: String,
    /// How long a caller waits.
    wait: Duration,
    hooks: Arc<dyn Hooks>,
    resets: Mutex<Resets>,
    /// How many messages have been built, for their IDs.
    built: AtomicU64,
}

impl std::fmt::Debug for Mailer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mailer")
            .field("on", &self.jobs.is_some())
            .finish_non_exhaustive()
    }
}

impl Mailer {
    /// The transport of `mail` for the site at `home`, with `hooks`, and
    /// the line the start log says of it. With a relay it sends from a
    /// task of the current runtime.
    pub fn new(
        mail: &hyscribe_config::Mail,
        home: &[u8],
        hooks: Arc<dyn Hooks>,
    ) -> (Mailer, String) {
        // The store is read only for a relay.
        let roots: Vec<Vec<u8>> = match mail.smtp {
            Some(_) => hyscribe_fetch::host_certificates()
                .iter()
                .map(|der| der.to_vec())
                .collect(),
            None => Vec::new(),
        };
        Mailer::start(mail, home, hooks, &roots, LIMITS)
    }

    /// [`Mailer::new`] trusting `roots`, within `limits`.
    fn start(
        mail: &hyscribe_config::Mail,
        home: &[u8],
        hooks: Arc<dyn Hooks>,
        roots: &[Vec<u8>],
        limits: Limits,
    ) -> (Mailer, String) {
        let host = url_host(home).unwrap_or_default();
        let host = String::from_utf8_lossy(&host).into_owned();
        // `wp_mail()`'s default sender: the host without `www.`.
        let sender = format!("wordpress@{}", host.strip_prefix("www.").unwrap_or(&host));
        let mut mailer = Mailer {
            jobs: None,
            from: mail.from.clone().unwrap_or(sender),
            wait: limits.wait,
            hooks: Arc::clone(&hooks),
            resets: Mutex::default(),
            built: AtomicU64::new(0),
        };
        let Some(smtp) = &mail.smtp else {
            return (mailer, "mail off: SMTP_HOST is not set".into());
        };
        if smtp.tls != SmtpTls::None && roots.is_empty() {
            let line = "mail off: no certificate store was found to check the relay's";
            return (mailer, line.into());
        }
        let ehlo = if host.is_empty() { "localhost" } else { &host };
        let relay = match Relay::new(smtp, &smtp.host, roots, ehlo) {
            Ok(relay) => relay,
            Err(err) => return (mailer, format!("mail off: the relay's TLS: {err}")),
        };
        let (jobs, waiting) = mpsc::channel(limits.queue);
        tokio::spawn(work(waiting, relay, hooks, limits));
        mailer.jobs = Some(jobs);
        (
            mailer,
            format!("mail on through {}:{}", smtp.host, smtp.port),
        )
    }

    /// `wp_mail( $to, $subject, $message, $headers )` for a message of
    /// `kind` (for the log): whether it was sent, once the relay has
    /// answered if `wait`, else once it is queued.
    pub async fn wp_mail(&self, mail: Mail, kind: &'static str, wait: bool) -> bool {
        let started = Instant::now();
        let atts = self.hooks.wp_mail(mail);
        if let Some(sent) = self.hooks.pre_wp_mail(&atts) {
            return sent;
        }
        let domain = domain(&atts.to);
        let (out, message) = match self.prepare(&atts, kind) {
            Ok(prepared) => prepared,
            Err(why) => return self.failed(&atts, (kind, &domain), why, started),
        };
        if let Some(sent) = self.hooks.phpmailer_init(&out) {
            if !sent {
                return self.failed(
                    &atts,
                    (kind, &domain),
                    "not sent by phpmailer_init",
                    started,
                );
            }
            log!(kind, &domain, "sent by phpmailer_init", started);
            self.hooks.wp_mail_succeeded(&atts);
            return true;
        }
        let Some(jobs) = &self.jobs else {
            return self.failed(&atts, (kind, &domain), "mail is off", started);
        };
        let (done, result) = oneshot::channel();
        let job = Job {
            kind,
            domain: domain.clone(),
            data: atts.clone(),
            message,
            done: wait.then_some(done),
            queued: started,
        };
        if jobs.try_send(job).is_err() {
            return self.failed(&atts, (kind, &domain), "the queue is full", started);
        }
        // The relay's task reports, and logs, every message it takes.
        !wait || matches!(tokio::time::timeout(self.wait, result).await, Ok(Ok(true)))
    }

    /// `retrieve_password()`'s mail to `account` asked for from `address`,
    /// waited for, within the password reset limits.
    pub async fn retrieve_password(
        &self,
        mail: Mail,
        account: u64,
        address: Option<IpAddr>,
    ) -> bool {
        let kind = "retrieve_password";
        let allowed = self
            .resets
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .allow(account, address, Instant::now());
        if let Err(limit) = allowed {
            log!(
                kind,
                &domain(&mail.to),
                &format!("not sent: {limit}"),
                Instant::now(),
            );
            return false;
        }
        self.wp_mail(mail, kind, true).await
    }

    /// `wp_mail()`'s headers, filters and checks over `atts`: the message
    /// as hooks see it and as the relay takes it, else why it is refused.
    fn prepare(&self, atts: &Mail, kind: &str) -> Result<(Outgoing, Message), &'static str> {
        if breaks_line(&atts.subject) || breaks_line(&atts.to) {
            return Err("refused: a line break in the subject or recipient");
        }
        let headers = parse(&atts.headers).ok_or("refused: a line break in the headers")?;
        if let [_, ..] = headers.dropped.as_slice() {
            eprintln!("mail {kind}: {} headers dropped", headers.dropped.len());
        }
        // The address is always `MAIL_FROM`'s: the filter sets nothing.
        let from = headers
            .from_email
            .unwrap_or_else(|| self.from.clone().into_bytes());
        self.hooks.wp_mail_from(from);
        let from_name = headers.from_name.unwrap_or_else(|| b"WordPress".to_vec());
        let from_name = self.hooks.wp_mail_from_name(from_name);
        let content_type = headers
            .content_type
            .unwrap_or_else(|| b"text/plain".to_vec());
        let html = match &self.hooks.wp_mail_content_type(content_type)[..] {
            b"text/plain" => false,
            b"text/html" => true,
            _ => return Err("refused: a content type other than text/plain or text/html"),
        };
        let charset = headers.charset.unwrap_or_else(|| b"UTF-8".to_vec());
        if !self
            .hooks
            .wp_mail_charset(charset)
            .eq_ignore_ascii_case(b"UTF-8")
        {
            return Err("refused: a charset other than UTF-8");
        }
        let reply_to: Option<Vec<Recipient>> =
            headers.reply_to.iter().map(|r| recipient(r)).collect();
        let to: Option<Vec<Recipient>> = atts.to.split(|&b| b == b',').map(recipient).collect();
        let (Some(to), Some(reply_to)) = (to, reply_to) else {
            return Err("refused: an address that is not UTF-8");
        };
        if breaks_line(&from_name) {
            return Err("refused: a line break in the sender's name");
        }
        let out = Outgoing {
            from: self.from.clone(),
            from_name: text(&from_name),
            to,
            reply_to,
            subject: text(&atts.subject),
            body: text(&atts.message),
            html,
        };
        let id = self.built.fetch_add(1, Ordering::Relaxed);
        let at = self.from.rsplit_once('@').map_or("", |(_, domain)| domain);
        let id = format!("<{}.{id}@{at}>", now_nanos());
        let message = message(&out, &id).ok_or("refused: an address the parser refuses")?;
        Ok((out, message))
    }

    /// A message refused or not sent: logged, `wp_mail_failed` told, false.
    fn failed(
        &self,
        atts: &Mail,
        (kind, domain): (&str, &str),
        why: &str,
        started: Instant,
    ) -> bool {
        log!(kind, domain, why, started);
        self.hooks.wp_mail_failed(atts, why);
        false
    }
}

/// The nanoseconds of the system clock, which make message IDs unique
/// across restarts.
fn now_nanos() -> u128 {
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
    since.unwrap_or_default().as_nanos()
}

/// Bytes as text, as the library takes them.
fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// One address of `wp_mail()`'s list, `Name <address>` split as its
/// `/(.*)<(.+)>/` splits it, both trimmed; none unless UTF-8.
fn recipient(entry: &[u8]) -> Option<Recipient> {
    let open = entry.iter().rposition(|&b| b == b'<');
    let close = entry.iter().rposition(|&b| b == b'>');
    let (name, address) = match (open, close) {
        (Some(open), Some(close)) if close > open + 1 => {
            let name = entry.get(..open).unwrap_or_default();
            (name, entry.get(open + 1..close).unwrap_or_default())
        }
        _ => (&b""[..], entry),
    };
    let trimmed = |bytes: &[u8]| String::from_utf8(hyscribe_phpser::php_trim(bytes).to_vec()).ok();
    Some(Recipient {
        name: trimmed(name)?,
        address: trimmed(address)?,
    })
}

/// The domain of the first recipient of `to`, for the log, with nothing
/// but letters, digits, dots and hyphens.
fn domain(to: &[u8]) -> String {
    let first = to.split(|&b| b == b',').next().unwrap_or_default();
    let after = first
        .iter()
        .rposition(|&b| b == b'@')
        .map_or(&[][..], |at| first.get(at + 1..).unwrap_or_default());
    let kept = after
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric() || b"-.".contains(b));
    kept.map(|&b| char::from(b)).collect()
}

/// The relay's task: each message in turn, at most `limits.per_window` in
/// a window; one whose waiting caller has given up is dropped.
async fn work(mut jobs: mpsc::Receiver<Job>, relay: Relay, hooks: Arc<dyn Hooks>, limits: Limits) {
    let mut recent: VecDeque<Instant> = VecDeque::new();
    while let Some(job) = jobs.recv().await {
        if recent.len() >= limits.per_window
            && let Some(first) = recent.pop_front()
        {
            tokio::time::sleep_until(first + limits.window).await;
        }
        let outcome = if job.done.as_ref().is_some_and(oneshot::Sender::is_closed) {
            Err("not sent in time".to_string())
        } else {
            recent.push_back(Instant::now());
            relay.deliver(&job.message).await
        };
        let sent = outcome.is_ok();
        match outcome {
            Ok(code) => {
                log!(job.kind, &job.domain, &format!("sent, {code}"), job.queued);
                hooks.wp_mail_succeeded(&job.data);
            }
            Err(why) => {
                log!(
                    job.kind,
                    &job.domain,
                    &format!("not sent: {why}"),
                    job.queued,
                );
                hooks.wp_mail_failed(&job.data, &why);
            }
        }
        if let Some(done) = job.done {
            let _ = done.send(sent);
        }
    }
}

/// When each account and client address was last sent a password reset.
#[derive(Default)]
struct Resets {
    accounts: HashMap<u64, Vec<Instant>>,
    addresses: HashMap<IpAddr, Vec<Instant>>,
}

impl Resets {
    /// Counts a reset mail to `account` from `address` at `now`, unless it
    /// is over a limit, which is named.
    fn allow(
        &mut self,
        account: u64,
        address: Option<IpAddr>,
        now: Instant,
    ) -> Result<(), &'static str> {
        prune(&mut self.accounts, now);
        prune(&mut self.addresses, now);
        if self.accounts.len() >= TRACKED || self.addresses.len() >= TRACKED {
            return Err("too many password resets in the hour to track");
        }
        if self.accounts.get(&account).map_or(0, Vec::len) >= PER_ACCOUNT {
            return Err("the account's limit of 3 password resets an hour");
        }
        if let Some(address) = address {
            let times = self.addresses.entry(address).or_default();
            if times.len() >= PER_ADDRESS {
                return Err("the client address's limit of 10 password resets an hour");
            }
            times.push(now);
        }
        self.accounts.entry(account).or_default().push(now);
        Ok(())
    }
}

/// `times` without the times over an hour before `now`, and without the
/// keys left with none.
fn prune<K>(times: &mut HashMap<K, Vec<Instant>>, now: Instant) {
    times.retain(|_, times| {
        times.retain(|&time| now.duration_since(time) < HOUR);
        !times.is_empty()
    });
}

#[cfg(test)]
mod tests;
