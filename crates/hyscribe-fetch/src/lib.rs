// SPDX-License-Identifier: AGPL-3.0-or-later

//! The outbound HTTP client (ADR 0019): `wp_safe_remote_get()` over `hyper`
//! and `rustls` with the host's certificates. Every hop of a fetch,
//! redirects included, passes `wp_http_validate_url()`, and every address
//! its host resolves to must be public, checked before connecting to the
//! address checked; time, size and redirects are bounded, and nothing but
//! the user agent is sent, but for an extension's own request to a host it
//! is granted (SDK 0.2). With `HTTP_OUTBOUND=off` no client is made.

mod address;
mod net;
mod url;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use http_body_util::{BodyExt, Full};
use hyper::body::Bytes;
use hyper::client::conn::http1::{Connection, SendRequest};
use hyper_util::rt::TokioIo;
use rustls::ClientConfig;
use rustls::pki_types::ServerName;
use tokio::time::{Instant, timeout_at};
use tokio_rustls::TlsConnector;

use address::is_public;
pub use net::{Boxed, Net, System, host_certificates, host_roots};
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

/// A response: its status, its headers and its body, cut at the limit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fetched {
    /// The status code.
    pub status: u16,
    /// The headers, names in lower case, in the order sent.
    pub headers: Vec<(String, Vec<u8>)>,
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
    /// A redirect led to a host the caller does not allow.
    Disallowed,
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

/// Which hosts a redirect may lead to, as the caller judges them.
pub type Allowed<'a> = dyn Fn(&str) -> bool + Sync + 'a;

/// An extension's request (SDK 0.2): its method, the headers it adds and
/// its body. A redirect is followed for `GET` and `HEAD` only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outgoing<'a> {
    /// The method, upper case.
    pub method: &'a str,
    /// Headers added after the client's own; a `User-Agent` among them
    /// replaces the arguments' one. A redirect to another origin sends
    /// none of them.
    pub headers: &'a [(String, String)],
    /// The body, sent with its length.
    pub body: &'a [u8],
}

/// `wp_safe_remote_get()`'s request: a GET, no headers, no body.
const GET: Outgoing<'static> = Outgoing {
    method: "GET",
    headers: &[],
    body: &[],
};

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
        self.get_within(url, home, args, &|_| true).await
    }

    /// [`Client::get`] where every redirect must also lead to a host
    /// `allowed` accepts (the host as the URL gives it), checked before it
    /// is resolved; the URL itself is the caller's to check. Every other
    /// check holds as it does for `get`.
    pub async fn get_within(
        &self,
        url: &[u8],
        home: &[u8],
        args: Args,
        allowed: &Allowed<'_>,
    ) -> Result<Fetched, Error> {
        self.send_within(url, home, (args, GET), allowed).await
    }

    /// `wp_safe_remote_request( $url, $args )` of an extension's request,
    /// as [`Client::get_within`] makes a GET: every check holds, and a
    /// method other than `GET` or `HEAD` follows no redirect.
    pub async fn send_within(
        &self,
        url: &[u8],
        home: &[u8],
        (args, outgoing): (Args, Outgoing<'_>),
        allowed: &Allowed<'_>,
    ) -> Result<Fetched, Error> {
        let mut args = self.filters.http_request_args(args, url);
        if let Some(answer) = self.filters.pre_http_request(&args, url) {
            return answer;
        }
        if !matches!(outgoing.method, "GET" | "HEAD") {
            args.redirection = 0;
        }
        let fail = |kind, hop| Err(Error { kind, hop });
        let deadline = Instant::now() + args.timeout.min(MAX_TIME);
        let limit = args.limit_response_size.unwrap_or(MAX_BYTES).min(MAX_BYTES);
        let origin =
            |url: &[u8]| validate_url(url, home).map(|target| (target.https, target.authority));
        let first = origin(url);
        let mut url = url.to_vec();
        for hop in 0..=args.redirection.min(MAX_REDIRECTS) {
            let check = (hop > 0).then_some(allowed);
            // The extension's headers (its credentials among them) go to
            // the origin it named only: not to another host, nor over
            // plain HTTP after HTTPS, which a redirect may lead to.
            let sent = if origin(&url) == first {
                outgoing
            } else {
                Outgoing {
                    headers: &[],
                    ..outgoing
                }
            };
            let hop_sent = self.hop(&url, home, (&args, sent), (limit, check));
            match timeout_at(deadline, hop_sent).await {
                Err(_) => return fail(Kind::Timeout, hop),
                Ok(Err(kind)) => return fail(kind, hop),
                Ok(Ok(Hop::Done(fetched))) => return Ok(fetched),
                Ok(Ok(Hop::Redirect(location))) => url = make_absolute_url(&location, &url),
            }
        }
        fail(Kind::Redirects, args.redirection.min(MAX_REDIRECTS))
    }

    /// One request: the URL checked, its host resolved and every address
    /// checked, then the request sent to the first address that connects.
    async fn hop(
        &self,
        url: &[u8],
        home: &[u8],
        (args, outgoing): (&Args, Outgoing<'_>),
        (limit, allowed): (usize, Option<&Allowed<'_>>),
    ) -> Result<Hop, Kind> {
        let target = validate_url(url, home).ok_or(Kind::Invalid)?;
        if allowed.is_some_and(|allowed| !allowed(&target.host)) {
            return Err(Kind::Disallowed);
        }
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
        let sent = (args, outgoing);
        if !target.https {
            return request(TokioIo::new(stream), &target, sent, limit).await;
        }
        let tls = self.tls.clone().ok_or(Kind::Tls)?;
        let name = ServerName::try_from(target.host.clone()).map_err(|_| Kind::Tls)?;
        let stream = TlsConnector::from(tls)
            .connect(name, stream)
            .await
            .map_err(|_| Kind::Tls)?;
        request(TokioIo::new(stream), &target, sent, limit).await
    }
}

/// A boxed exchange, as [`request`] awaits it.
type Exchange = Pin<Box<dyn Future<Output = Result<Hop, Kind>> + Send>>;

/// `outgoing` for `target` over `io`: its answer read to `limit`, or the
/// redirect it gives, as WordPress's Requests follows them.
async fn request<T>(
    io: T,
    target: &Target,
    (args, outgoing): (&Args, Outgoing<'_>),
    limit: usize,
) -> Result<Hop, Kind>
where
    T: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
{
    let agent = |(name, _): &&(String, String)| name.eq_ignore_ascii_case("user-agent");
    let (agents, added): (Vec<_>, Vec<_>) = outgoing.headers.iter().partition(agent);
    let user_agent = agents
        .last()
        .map_or(args.user_agent.as_str(), |(_, agent)| agent);
    let mut request = hyper::Request::builder()
        .method(outgoing.method)
        .uri(target.path.as_str())
        .header("Host", target.authority.as_str())
        .header("User-Agent", user_agent)
        .header("Accept-Encoding", "identity")
        .header("Connection", "close");
    for (name, value) in added {
        request = request.header(name.as_str(), value.as_str());
    }
    let body = Full::new(Bytes::copy_from_slice(outgoing.body));
    let request = request.body(body).map_err(|_| Kind::Protocol)?;
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
    mut sender: SendRequest<Full<Bytes>>,
    connection: Connection<T, Full<Bytes>>,
    request: hyper::Request<Full<Bytes>>,
    follow: bool,
    limit: usize,
) -> Result<Hop, Kind>
where
    T: hyper::rt::Read + hyper::rt::Write + Unpin + Send + 'static,
{
    // The connection ends once the sender and the body are dropped.
    tokio::spawn(connection);
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
    let headers = response
        .headers()
        .iter()
        .map(|(name, value)| (name.as_str().to_string(), value.as_bytes().to_vec()))
        .collect();
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
    Ok(Hop::Done(Fetched {
        status,
        headers,
        body: read,
    }))
}

#[cfg(test)]
mod tests;
