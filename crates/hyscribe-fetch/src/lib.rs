// SPDX-License-Identifier: AGPL-3.0-or-later

//! The outbound HTTP client (ADR 0019): `wp_safe_remote_get()` over `hyper`
//! and `rustls` with the host's certificates. Every hop of a fetch,
//! redirects included, passes `wp_http_validate_url()`, and every address
//! its host resolves to must be public, checked before connecting to the
//! address checked; time, size and redirects are bounded, and nothing but
//! the user agent is sent. With `HTTP_OUTBOUND=off` no client is made.

mod address;
mod net;
mod url;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Empty};
use hyper::body::Bytes;
use hyper::client::conn::http1::{Connection, SendRequest};
use hyper_util::rt::TokioIo;
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use tokio::time::{Instant, timeout_at};
use tokio_rustls::TlsConnector;

pub use address::is_public;
pub use net::{Boxed, Net, System, host_roots};
pub use rustls::RootCertStore;
pub use url::{Target, make_absolute_url, validate_url};

/// The longest a fetch takes, redirects included, whatever its arguments.
const MAX_TIME: Duration = Duration::from_secs(30);

/// The most a response body is read, whatever its arguments; past it the
/// body is cut, as `limit_response_size` cuts it.
const MAX_BYTES: usize = 4 << 20;

/// The most redirects followed, whatever the arguments.
const MAX_REDIRECTS: u32 = 5;

/// The request arguments of `WP_Http::request()` the client reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Args {
    /// `timeout`, for the whole fetch.
    pub timeout: Duration,
    /// `redirection`: how many redirects to follow.
    pub redirection: u32,
    /// `user-agent`.
    pub user_agent: String,
    /// `limit_response_size`: the body is cut there.
    pub limit_response_size: Option<usize>,
}

impl Args {
    /// WordPress's defaults, 5 seconds and 5 redirects, with `user_agent`.
    pub fn new(user_agent: String) -> Args {
        Args {
            timeout: Duration::from_secs(5),
            redirection: 5,
            user_agent,
            limit_response_size: None,
        }
    }
}

/// A response: its status and its body, cut at the limit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetched {
    /// The status code.
    pub status: u16,
    /// The body.
    pub body: Vec<u8>,
}

/// Why a fetch failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// The URL fails `wp_http_validate_url()`.
    Invalid,
    /// The host resolves to an address that is not public.
    Refused,
    /// The host does not resolve.
    Resolve,
    /// No connection was made.
    Connect,
    /// TLS failed, or the host has no certificate store.
    Tls,
    /// The time ran out.
    Timeout,
    /// More redirects than allowed.
    Redirects,
    /// The server's answer was not HTTP the client reads.
    Protocol,
}

/// A failed fetch: why, and on which hop (0 for the URL itself).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Error {
    /// Why.
    pub kind: Kind,
    /// The hop: how many redirects were followed before it.
    pub hop: u32,
}

/// WordPress's hooks around a request, under their names; a hook changes
/// the arguments or answers in place of a connection, and never skips a
/// check on one.
pub trait Filters: Send + Sync {
    /// `http_request_args`.
    fn http_request_args(&self, args: Args, _url: &[u8]) -> Args {
        args
    }

    /// `pre_http_request`: an answer given without a connection.
    fn pre_http_request(&self, _args: &Args, _url: &[u8]) -> Option<Result<Fetched, Error>> {
        None
    }
}

/// No hooks attached.
impl Filters for () {}

/// The client.
pub struct Client {
    tls: Option<Arc<ClientConfig>>,
    net: Arc<dyn Net>,
    filters: Arc<dyn Filters>,
}

/// The client shows whether it has certificates, not its hooks.
impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("roots", &self.has_roots())
            .finish_non_exhaustive()
    }
}

/// A hop's outcome: the answer, or where it redirects.
enum Hop {
    Done(Fetched),
    Redirect(Vec<u8>),
}

impl Client {
    /// A client trusting `roots` for HTTPS, none if empty, resolving and
    /// connecting through `net`, with `filters` attached.
    pub fn new(roots: RootCertStore, net: Arc<dyn Net>, filters: Arc<dyn Filters>) -> Client {
        let tls = (!roots.is_empty()).then(|| {
            let provider = Arc::new(rustls::crypto::ring::default_provider());
            ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map(|config| Arc::new(config.with_root_certificates(roots).with_no_client_auth()))
        });
        Client {
            tls: tls.and_then(Result::ok),
            net,
            filters,
        }
    }

    /// Whether HTTPS can be fetched: the host's store had certificates.
    pub fn has_roots(&self) -> bool {
        self.tls.is_some()
    }

    /// `wp_safe_remote_get( $url, $args )` on a site whose `home` is `home`.
    pub async fn get(&self, url: &[u8], home: &[u8], args: Args) -> Result<Fetched, Error> {
        let args = self.filters.http_request_args(args, url);
        if let Some(answer) = self.filters.pre_http_request(&args, url) {
            return answer;
        }
        let fail = |kind, hop| Err(Error { kind, hop });
        let deadline = Instant::now() + args.timeout.min(MAX_TIME);
        let limit = args.limit_response_size.unwrap_or(MAX_BYTES).min(MAX_BYTES);
        let mut url = url.to_vec();
        for hop in 0..=args.redirection.min(MAX_REDIRECTS) {
            let fetched = timeout_at(deadline, self.hop(&url, home, &args, limit)).await;
            match fetched {
                Err(_) => return fail(Kind::Timeout, hop),
                Ok(Err(kind)) => return fail(kind, hop),
                Ok(Ok(Hop::Done(fetched))) => return Ok(fetched),
                Ok(Ok(Hop::Redirect(location))) => url = make_absolute_url(&location, &url),
            }
        }
        fail(Kind::Redirects, args.redirection.min(MAX_REDIRECTS))
    }

    /// One request: the URL checked, its host resolved and every address
    /// checked, then a GET to the first address that connects.
    async fn hop(&self, url: &[u8], home: &[u8], args: &Args, limit: usize) -> Result<Hop, Kind> {
        let target = validate_url(url, home).ok_or(Kind::Invalid)?;
        let addresses = self
            .net
            .resolve(&target.host, target.port)
            .await
            .map_err(|_| Kind::Resolve)?;
        if addresses.is_empty() {
            return Err(Kind::Resolve);
        }
        if !addresses.iter().all(|address| is_public(address.ip())) {
            return Err(Kind::Refused);
        }
        let mut stream = Err(Kind::Connect);
        for address in addresses {
            stream = self.net.dial(address).await.map_err(|_| Kind::Connect);
            if stream.is_ok() {
                break;
            }
        }
        let stream = stream?;
        if !target.https {
            return request(TokioIo::new(stream), &target, args, limit).await;
        }
        let tls = self.tls.clone().ok_or(Kind::Tls)?;
        let name = ServerName::try_from(target.host.clone()).map_err(|_| Kind::Tls)?;
        let stream = TlsConnector::from(tls)
            .connect(name, stream)
            .await
            .map_err(|_| Kind::Tls)?;
        request(TokioIo::new(stream), &target, args, limit).await
    }
}

/// A boxed exchange, as [`request`] awaits it.
type Exchange = Pin<Box<dyn Future<Output = Result<Hop, Kind>> + Send>>;

/// A GET for `target` over `io`: its answer read to `limit`, or the
/// redirect it gives, as WordPress's Requests follows them.
async fn request<T>(io: T, target: &Target, args: &Args, limit: usize) -> Result<Hop, Kind>
where
    T: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
{
    let request = hyper::Request::get(target.path.as_str())
        .header("Host", target.authority.as_str())
        .header("User-Agent", args.user_agent.as_str())
        .header("Accept-Encoding", "identity")
        .header("Connection", "close")
        .body(Empty::<Bytes>::new())
        .map_err(|_| Kind::Protocol)?;
    let follow = args.redirection > 0;
    // hyper's handshake makes no I/O and does not fail; a failure would
    // be the server's, as when sending fails.
    let exchange = hyper::client::conn::http1::handshake(io).await.ok().map(
        |(sender, connection)| -> Exchange {
            Box::pin(exchange(sender, connection, request, follow, limit))
        },
    );
    exchange
        .unwrap_or(Box::pin(std::future::ready(Err(Kind::Protocol))))
        .await
}

/// `request` sent on a connection: the redirect when `follow`, else the
/// answer read to `limit`.
async fn exchange<T>(
    mut sender: SendRequest<Empty<Bytes>>,
    connection: Connection<T, Empty<Bytes>>,
    request: hyper::Request<Empty<Bytes>>,
    follow: bool,
    limit: usize,
) -> Result<Hop, Kind>
where
    T: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
{
    let _driver = Driver(tokio::spawn(connection));
    let response = sender
        .send_request(request)
        .await
        .map_err(|_| Kind::Protocol)?;
    let status = response.status().as_u16();
    // Requests returns a redirect as it is when told to follow none.
    let redirect = follow && matches!(status, 300..=303 | 307..=399);
    if let Some(location) = response.headers().get("location").filter(|_| redirect) {
        return Ok(Hop::Redirect(location.as_bytes().to_vec()));
    }
    let mut body = response.into_body();
    let mut read = Vec::new();
    while read.len() < limit {
        match body.frame().await {
            None => break,
            Some(Err(_)) => return Err(Kind::Protocol),
            Some(Ok(frame)) => read.extend(frame.into_data().unwrap_or_default()),
        }
    }
    read.truncate(limit);
    Ok(Hop::Done(Fetched { status, body: read }))
}

/// The task driving a connection, stopped when the request is done.
struct Driver<T>(tokio::task::JoinHandle<T>);

impl<T> Drop for Driver<T> {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests;
