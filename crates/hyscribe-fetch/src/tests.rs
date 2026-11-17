// SPDX-License-Identifier: AGPL-3.0-or-later

//! The client against local listeners only: the test site of
//! `hyscribe-testkit`, reached through a resolver that answers from a
//! table, so no test makes a real network call.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::*;

/// A resolver answering from a table, and connections that all go to
/// `to`, whatever the address; the addresses connected to are kept.
struct Table {
    hosts: Vec<(&'static str, Vec<&'static str>)>,
    to: SocketAddr,
    dialed: Mutex<Vec<SocketAddr>>,
}

impl Table {
    fn new(to: SocketAddr) -> Arc<Table> {
        Arc::new(Table {
            hosts: vec![
                ("pages.test", vec!["93.184.215.14"]),
                ("two.test", vec!["2606:4700::1111", "93.184.215.14"]),
                ("both.test", vec!["93.184.215.14", "10.0.0.5"]),
                ("private.test", vec!["192.168.1.10"]),
                ("metadata.test", vec!["169.254.169.254"]),
                ("loopback6.test", vec!["::1"]),
                ("mapped.test", vec!["::ffff:127.0.0.1"]),
                ("empty.test", vec![]),
                ("down.test", vec!["93.184.215.15"]),
                ("bad-.test", vec!["93.184.215.14"]),
            ],
            to,
            dialed: Mutex::new(Vec::new()),
        })
    }

    fn dialed(&self) -> Vec<SocketAddr> {
        self.dialed.lock().unwrap().clone()
    }
}

impl Net for Table {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> net::Boxed<'a, Vec<SocketAddr>> {
        let found = self.hosts.iter().find(|(name, _)| *name == host);
        let answer = match (host.parse::<IpAddr>(), found) {
            (Ok(ip), _) => Ok(vec![SocketAddr::new(ip, port)]),
            (_, Some((_, ips))) => Ok(ips
                .iter()
                .map(|ip| SocketAddr::new(ip.parse().unwrap(), port))
                .collect()),
            _ => Err(io::Error::other("unknown host")),
        };
        Box::pin(async move { answer })
    }

    fn dial(&self, address: SocketAddr) -> net::Boxed<'_, TcpStream> {
        self.dialed.lock().unwrap().push(address);
        let refused = address.ip().to_string() == "93.184.215.15";
        let to = self.to;
        Box::pin(async move {
            if refused {
                return Err(io::Error::other("refused"));
            }
            TcpStream::connect(to).await
        })
    }
}

fn site() -> SocketAddr {
    hyscribe_testkit::pages::page_server().unwrap()
}

fn client(net: Arc<Table>) -> Client {
    Client::new(RootCertStore::empty(), net, Arc::new(()))
}

fn args() -> Args {
    Args::new("Tester".into())
}

async fn get(client: &Client, url: &str) -> Result<Fetched, Error> {
    client
        .get(url.as_bytes(), b"http://home.test", args())
        .await
}

fn failed(kind: Kind, hop: u32) -> Result<Fetched, Error> {
    Err(Error { kind, hop })
}

#[tokio::test]
async fn fetches_pages_and_follows_redirects() {
    let table = Table::new(site());
    let client = client(Arc::clone(&table));
    let page = get(&client, "http://pages.test/agent").await.unwrap();
    assert_eq!(page.status, 200);
    assert_eq!(
        page.headers.first(),
        Some(&(
            "content-type".to_string(),
            b"text/html;charset=UTF-8".to_vec()
        ))
    );
    assert_eq!(
        page.body,
        b"<html><head><title>Tester</title></head></html>\n"
    );
    let moved = get(&client, "http://pages.test:8080/redirect-twice")
        .await
        .unwrap();
    assert_eq!(moved.status, 200);
    assert!(moved.body.starts_with(b"<!DOCTYPE html>"));
    assert_eq!(
        get(&client, "http://pages.test/gone").await.unwrap().status,
        410
    );
    assert_eq!(
        get(&client, "http://pages.test/nope").await.unwrap().body,
        b"Not Found"
    );
    // The first address that connects is used.
    assert_eq!(
        get(&client, "http://two.test/empty").await.unwrap().body,
        b""
    );
    let dialed = table.dialed();
    assert_eq!(dialed[0].to_string(), "93.184.215.14:80");
    assert_eq!(dialed[1].to_string(), "93.184.215.14:8080");
    assert_eq!(dialed.last().unwrap().to_string(), "[2606:4700::1111]:80");
}

#[tokio::test]
async fn refuses_addresses_that_are_not_public() {
    let table = Table::new(site());
    let client = client(Arc::clone(&table));
    for url in [
        "http://private.test/",
        "http://metadata.test/latest/meta-data/",
        "http://loopback6.test/",
        "http://mapped.test/",
        // A name that answers both a public and a private address.
        "http://both.test/",
    ] {
        assert_eq!(get(&client, url).await, failed(Kind::Refused, 0), "{url}");
    }
    assert!(table.dialed().is_empty());
    // Redirects into them, checked on the hop they lead to.
    let metadata = get(&client, "http://pages.test/redirect-metadata").await;
    assert_eq!(metadata, failed(Kind::Invalid, 1));
    let loopback = get(&client, "http://pages.test/redirect-loopback").await;
    assert_eq!(loopback, failed(Kind::Invalid, 1));
    assert_eq!(table.dialed().len(), 2);
    // A redirect to a name that resolves to a private address.
    let to_private = answering(b"HTTP/1.1 302 Found\r\nLocation: http://private.test/\r\n\r\n");
    let redirecting = Table::new(to_private.await);
    let client = super::tests::client(Arc::clone(&redirecting));
    assert_eq!(
        get(&client, "http://pages.test/").await,
        failed(Kind::Refused, 1)
    );
    assert_eq!(redirecting.dialed().len(), 1);
}

#[tokio::test]
async fn refuses_urls_wordpress_refuses() {
    let client = client(Table::new(site()));
    for url in [
        "ftp://pages.test/",
        "http://user:pass@pages.test/",
        "http://pages.test:22/",
        "http://[::1]/",
        "//pages.test/",
        "http://169.254.169.254/",
        "http://127.0.0.1:8080/page.html",
        "http://0.0.0.0/",
    ] {
        assert_eq!(get(&client, url).await, failed(Kind::Invalid, 0), "{url}");
    }
    assert_eq!(
        get(&client, "http://unknown.test/").await,
        failed(Kind::Resolve, 0)
    );
    assert_eq!(
        get(&client, "http://empty.test/").await,
        failed(Kind::Resolve, 0)
    );
    assert_eq!(
        get(&client, "http://down.test/").await,
        failed(Kind::Connect, 0)
    );
}

#[tokio::test]
async fn bounds_redirects_size_and_time() {
    let client = client(Table::new(site()));
    assert_eq!(
        get(&client, "http://pages.test/loop").await,
        failed(Kind::Redirects, 5)
    );
    let mut few = args();
    few.redirection = 1;
    let twice = client
        .get(b"http://pages.test/redirect-twice", b"", few)
        .await;
    assert_eq!(twice, failed(Kind::Redirects, 1));
    // Told to follow none, the redirect is the answer, as Requests gives it.
    let mut none = args();
    none.redirection = 0;
    let redirect = client.get(b"http://pages.test/redirect", b"", none).await;
    assert_eq!(redirect.unwrap().status, 302);
    let mut many = args();
    many.redirection = 50;
    let capped = client.get(b"http://pages.test/loop", b"", many).await;
    assert_eq!(capped, failed(Kind::Redirects, MAX_REDIRECTS));
    let mut limited = args();
    limited.limit_response_size = Some(100);
    let big = client
        .get(b"http://pages.test/big.html", b"", limited)
        .await
        .unwrap();
    assert_eq!(big.body.len(), 100);
    let mut over = args();
    over.limit_response_size = Some(usize::MAX);
    let whole = client
        .get(b"http://pages.test/big.html", b"", over)
        .await
        .unwrap();
    assert_eq!(whole.body.len(), 160_146);

    let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let client = super::tests::client(Table::new(silent.local_addr().unwrap()));
    let mut quick = args();
    quick.timeout = Duration::from_millis(200);
    let started = Instant::now();
    let slow = client.get(b"http://pages.test/", b"", quick).await;
    assert_eq!(slow, failed(Kind::Timeout, 0));
    assert!(started.elapsed() < Duration::from_secs(5));
    drop(silent);
}

/// A listener answering every connection with `answer`, then closing it.
async fn answering(answer: &'static [u8]) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = [0; 1024];
            let _ = stream.read(&mut buffer).await;
            let _ = stream.write_all(answer).await;
        }
    });
    address
}

/// A body read to its limit stops there: no more is awaited, and the
/// connection is closed, however much more the server says it sends.
#[tokio::test]
async fn reading_stops_at_the_limit_and_closes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (closed, seen) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 1024];
        let _ = stream.read(&mut buffer).await;
        let head = b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n";
        stream
            .write_all(&[&head[..], &[b'x'; 100]].concat())
            .await
            .unwrap();
        let read = tokio::time::timeout(Duration::from_secs(5), stream.read(&mut buffer)).await;
        let _ = closed.send(matches!(read, Ok(Ok(0))));
    });
    let client = client(Table::new(address));
    let mut limited = args();
    limited.limit_response_size = Some(100);
    let page = client
        .get(b"http://pages.test/", b"", limited)
        .await
        .unwrap();
    assert_eq!(page.body, vec![b'x'; 100]);
    assert!(seen.await.unwrap(), "the connection stayed open");
}

#[tokio::test]
async fn answers_that_are_not_http_fail() {
    let garbage = client(Table::new(answering(b"SSH-2.0-OpenSSH\r\n").await));
    assert_eq!(
        get(&garbage, "http://pages.test/").await,
        failed(Kind::Protocol, 0)
    );
    let cut = b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\nshort";
    let cut = client(Table::new(answering(cut).await));
    assert_eq!(
        get(&cut, "http://pages.test/").await,
        failed(Kind::Protocol, 0)
    );
    // A user agent no header can carry is refused before sending.
    let client = client(Table::new(site()));
    let bad = Args::new("bad\nagent".into());
    let refused = client.get(b"http://pages.test/", b"", bad).await;
    assert_eq!(refused, failed(Kind::Protocol, 0));
}

struct Hooks;

impl Filters for Hooks {
    fn http_request_args(&self, mut args: Args, url: &[u8]) -> Args {
        args.user_agent = format!("{} for {}", args.user_agent, String::from_utf8_lossy(url));
        args
    }

    fn pre_http_request(&self, _: &Args, url: &[u8]) -> Option<Result<Fetched, Error>> {
        (url == b"http://answered.test/").then(|| {
            Ok(Fetched {
                status: 204,
                headers: Vec::new(),
                body: Vec::new(),
            })
        })
    }
}

#[tokio::test]
async fn hooks_change_arguments_and_answer_without_connecting() {
    let table = Table::new(site());
    let client = Client::new(RootCertStore::empty(), table.clone(), Arc::new(Hooks));
    let page = get(&client, "http://pages.test/agent").await.unwrap();
    assert_eq!(
        page.body,
        b"<html><head><title>Tester for http://pages.test/agent</title></head></html>\n"
    );
    let answered = get(&client, "http://answered.test/").await.unwrap();
    assert_eq!(answered.status, 204);
    assert_eq!(table.dialed().len(), 1);
}

fn test_roots() -> RootCertStore {
    let mut roots = RootCertStore::empty();
    let ca = CertificateDer::from_pem_slice(include_bytes!("../tests/tls/ca.pem")).unwrap();
    roots.add(ca).unwrap();
    roots
}

/// A TLS listener for `pages.test` answering every request with a page.
async fn tls_site() -> SocketAddr {
    let chain = CertificateDer::pem_slice_iter(include_bytes!("../tests/tls/leaf.pem"))
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let key = PrivateKeyDer::from_pem_slice(include_bytes!("../tests/tls/leaf-key.pem")).unwrap();
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .unwrap();
    let acceptor = tokio_rustls::TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let Ok(mut stream) = acceptor.accept(stream).await else {
                continue;
            };
            let mut buffer = [0; 1024];
            let _ = stream.read(&mut buffer).await;
            let _ = stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecure")
                .await;
            let _ = stream.shutdown().await;
        }
    });
    address
}

#[tokio::test]
async fn https_verifies_the_host_against_the_roots() {
    let secure = tls_site().await;
    let trusting = Client::new(test_roots(), Table::new(secure), Arc::new(()));
    assert_eq!(format!("{trusting:?}"), "Client { roots: true, .. }");
    let page = get(&trusting, "https://pages.test/").await.unwrap();
    assert_eq!((page.status, page.body.as_slice()), (200, &b"secure"[..]));
    // The certificate names pages.test only.
    assert_eq!(
        get(&trusting, "https://two.test/").await,
        failed(Kind::Tls, 0)
    );
    // An address is no name the certificate gives.
    assert_eq!(
        get(&trusting, "https://93.184.215.14/").await,
        failed(Kind::Tls, 0)
    );
    // A plain HTTP server does not speak TLS.
    let plain = answering(b"HTTP/1.1 200 OK\r\n\r\n").await;
    let plain = Client::new(test_roots(), Table::new(plain), Arc::new(()));
    assert_eq!(
        get(&plain, "https://pages.test/").await,
        failed(Kind::Tls, 0)
    );
    // Without a certificate store nothing is fetched over HTTPS.
    let bare = client(Table::new(secure));
    assert!(!bare.has_roots());
    assert_eq!(
        get(&bare, "https://pages.test/").await,
        failed(Kind::Tls, 0)
    );
    // A host TLS cannot name.
    assert_eq!(
        get(&trusting, "https://bad-.test/").await,
        failed(Kind::Tls, 0)
    );
}
