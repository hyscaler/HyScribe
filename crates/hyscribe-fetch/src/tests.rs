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
    assert!(started.elapsed() < Duration::from_secs(30));
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

/// A listener redirecting its first request to `http://two.test/` and
/// answering every later one with a page.
async fn redirecting_once() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut first = true;
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = [0; 1024];
            let _ = stream.read(&mut buffer).await;
            let answer: &[u8] = if first {
                b"HTTP/1.1 302 Found\r\nLocation: http://two.test/\r\n\r\n"
            } else {
                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok"
            };
            first = false;
            let _ = stream.write_all(answer).await;
        }
    });
    address
}

/// Every redirect hop must be on the caller's list, checked before the
/// host is resolved or connected to; the other checks still hold.
#[tokio::test]
async fn redirects_stay_on_the_allowed_hosts() {
    let allow = |hosts: &'static [&'static str]| move |host: &str| hosts.contains(&host);
    // To a host off the list: stopped, and only the first host dialed.
    let table = Table::new(redirecting_once().await);
    let client = super::tests::client(Arc::clone(&table));
    let only = allow(&["pages.test"]);
    let stopped = client
        .get_within(b"http://pages.test/", b"", args(), &only)
        .await;
    assert_eq!(stopped, failed(Kind::Disallowed, 1));
    assert_eq!(table.dialed().len(), 1);
    // To another host on it: followed.
    let table = Table::new(redirecting_once().await);
    let client = super::tests::client(Arc::clone(&table));
    let both = allow(&["pages.test", "two.test"]);
    let followed = client
        .get_within(b"http://pages.test/", b"", args(), &both)
        .await;
    assert_eq!(followed.unwrap().body, b"ok");
    assert_eq!(table.dialed().len(), 2);
    // The URL itself is the caller's to check: only hops are judged.
    let table = Table::new(redirecting_once().await);
    let client = super::tests::client(Arc::clone(&table));
    let later = allow(&["two.test"]);
    let started = client
        .get_within(b"http://pages.test/", b"", args(), &later)
        .await;
    assert_eq!(started.unwrap().body, b"ok");
    // Within the same host, on the list: followed as without a list.
    let client = super::tests::client(Table::new(site()));
    let same = client
        .get_within(b"http://pages.test/redirect-twice", b"", args(), &only)
        .await;
    assert_eq!(same.unwrap().status, 200);
    // On the list, a hop to a private address is still refused.
    let to_private = answering(b"HTTP/1.1 302 Found\r\nLocation: http://private.test/\r\n\r\n");
    let client = super::tests::client(Table::new(to_private.await));
    let listed = allow(&["pages.test", "private.test"]);
    let refused = client
        .get_within(b"http://pages.test/", b"", args(), &listed)
        .await;
    assert_eq!(refused, failed(Kind::Refused, 1));
}

/// A listener answering each request with a redirect whose body is the
/// request as it came, so a test sees what was sent.
async fn echoing() -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = [0; 4096];
            let read = stream.read(&mut buffer).await.unwrap_or(0);
            let head =
                format!("HTTP/1.1 302 Found\r\nLocation: /next\r\nContent-Length: {read}\r\n\r\n");
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(&buffer[..read]).await;
        }
    });
    address
}

/// An extension's request carries its method, headers and body after the
/// client's own headers, its user agent in place of the arguments', and
/// follows no redirect but for GET and HEAD.
#[tokio::test]
async fn sends_an_extension_s_request() {
    let client = client(Table::new(echoing().await));
    let headers = [
        ("X-Key".to_string(), "secret".to_string()),
        ("User-Agent".to_string(), "Ext/1".to_string()),
    ];
    let outgoing = Outgoing {
        method: "POST",
        headers: &headers,
        body: b"a=b",
    };
    let sent = client
        .send_within(
            b"http://pages.test/verify",
            b"",
            (args(), outgoing),
            &|_| true,
        )
        .await
        .unwrap();
    assert_eq!(sent.status, 302);
    let request = String::from_utf8(sent.body).unwrap();
    let expected = "POST /verify HTTP/1.1\r\nhost: pages.test\r\nuser-agent: Ext/1\r\n\
                    accept-encoding: identity\r\nconnection: close\r\nx-key: secret\r\n\
                    content-length: 3\r\n\r\na=b";
    assert_eq!(request, expected);
    let head = Outgoing {
        method: "HEAD",
        ..outgoing
    };
    let followed = client
        .send_within(b"http://pages.test/", b"", (args(), head), &|_| true)
        .await;
    assert_eq!(followed, failed(Kind::Redirects, 5));
    let refused = Outgoing {
        method: "NOT A METHOD",
        ..outgoing
    };
    let refused = client
        .send_within(b"http://pages.test/", b"", (args(), refused), &|_| true)
        .await;
    assert_eq!(refused, failed(Kind::Protocol, 0));
}

/// A listener redirecting its first request to `location` and answering
/// every later one with the request as it came.
async fn redirecting_to(location: &'static str) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let mut first = true;
        while let Ok((mut stream, _)) = listener.accept().await {
            let mut buffer = [0; 4096];
            let read = stream.read(&mut buffer).await.unwrap_or(0);
            let head = if first {
                format!("HTTP/1.1 302 Found\r\nLocation: {location}\r\n\r\n")
            } else {
                format!("HTTP/1.1 200 OK\r\nContent-Length: {read}\r\n\r\n")
            };
            first = false;
            let _ = stream.write_all(head.as_bytes()).await;
            let _ = stream.write_all(&buffer[..read]).await;
        }
    });
    address
}

/// An extension's headers follow a redirect within its origin, and are
/// dropped on one to another host: its credentials stay with the host
/// they were meant for.
#[tokio::test]
async fn an_extension_s_headers_stay_with_its_origin() {
    let headers = [("Authorization".to_string(), "Bearer k".to_string())];
    let outgoing = Outgoing {
        method: "GET",
        headers: &headers,
        body: b"",
    };
    let fetch = |location| async move {
        let client = client(Table::new(redirecting_to(location).await));
        let fetched = client
            .send_within(b"http://pages.test/", b"", (args(), outgoing), &|_| true)
            .await;
        String::from_utf8(fetched.unwrap().body).unwrap()
    };
    let same = fetch("/next").await;
    assert!(same.contains("authorization: Bearer k"), "{same}");
    let other = fetch("http://two.test/").await;
    assert!(
        other.starts_with("GET / HTTP/1.1\r\nhost: two.test\r\n"),
        "{other}"
    );
    assert!(!other.contains("authorization"), "{other}");
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
        let read = tokio::time::timeout(Duration::from_secs(30), stream.read(&mut buffer)).await;
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

/// A TLS listener for `pages.test` answering its connections with
/// `answers` in turn, the last one again and again; one ending in a body
/// cut short is held open for a minute.
async fn tls_answering(answers: Vec<&'static [u8]>) -> SocketAddr {
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
        let mut served = 0;
        while let Ok((stream, _)) = listener.accept().await {
            let answer = answers.get(served).or(answers.last()).copied();
            let answer = answer.unwrap_or_default();
            served += 1;
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                let Ok(mut stream) = acceptor.accept(stream).await else {
                    return;
                };
                let mut buffer = [0; 1024];
                let _ = stream.read(&mut buffer).await;
                let _ = stream.write_all(answer).await;
                if answer.ends_with(b"held") {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
                let _ = stream.shutdown().await;
            });
        }
    });
    address
}

/// A TLS listener for `pages.test` answering every request with a page.
async fn tls_site() -> SocketAddr {
    tls_answering(vec![b"HTTP/1.1 200 OK\r\nContent-Length: 6\r\n\r\nsecure"]).await
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

/// A client trusting the test roots, every connection going to a TLS
/// listener answering with `answers`.
async fn downloading(answers: Vec<&'static [u8]>) -> Client {
    let site = tls_answering(answers).await;
    Client::new(test_roots(), Table::new(site), Arc::new(()))
}

const PACKAGE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello";

/// `hello`'s SHA-256.
const HELLO: &str = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";

fn fetch_failed(kind: Kind, hop: u32) -> Result<String, Download> {
    Err(Download::Fetch(Error { kind, hop }))
}

#[tokio::test]
async fn downloads_a_package_to_a_file() {
    let folder = tempfile::tempdir().unwrap();
    let to = folder.path().join(".download-a-0.1.0");
    std::fs::write(&to, b"left over").unwrap();
    let client = downloading(vec![PACKAGE]).await;
    let url = b"https://pages.test/v1/packages/a/a-0.1.0.hyx";
    let saved = client.download(url, "HyScribe/0.1.0", (5, &to)).await;
    assert_eq!(saved.unwrap(), HELLO);
    assert_eq!(std::fs::read(&to).unwrap(), b"hello");
    // A redirect on the same host is followed; to another one it stops.
    let moved = b"HTTP/1.1 302 Found\r\nLocation: /elsewhere\r\n\r\n";
    let client = downloading(vec![moved, PACKAGE]).await;
    assert_eq!(client.download(url, "", (5, &to)).await.unwrap(), HELLO);
    let away = b"HTTP/1.1 302 Found\r\nLocation: https://two.test/\r\n\r\n";
    let client = downloading(vec![away, PACKAGE]).await;
    let stopped = client.download(url, "", (5, &to)).await;
    assert_eq!(stopped, fetch_failed(Kind::Disallowed, 1));
    assert!(!to.exists());
    // Nor is one to plain HTTP on the same host.
    let plain = b"HTTP/1.1 302 Found\r\nLocation: http://pages.test/a.hyx\r\n\r\n";
    let client = downloading(vec![plain, PACKAGE]).await;
    let stopped = client.download(url, "", (5, &to)).await;
    assert_eq!(stopped, fetch_failed(Kind::Disallowed, 1));
}

#[tokio::test]
async fn refuses_a_download_of_another_size_or_status() {
    let folder = tempfile::tempdir().unwrap();
    let to = folder.path().join("package");
    let url = b"https://pages.test/a.hyx";
    let client = downloading(vec![PACKAGE]).await;
    for size in [4, 6, MAX_DOWNLOAD + 1] {
        assert_eq!(
            client.download(url, "", (size, &to)).await,
            Err(Download::Size)
        );
        assert!(!to.exists());
    }
    let missing = b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
    let client = downloading(vec![missing]).await;
    let refused = client.download(url, "", (5, &to)).await;
    assert_eq!(refused, Err(Download::Status(404)));
    let largest = client.download(url, "", (MAX_DOWNLOAD, &to)).await;
    assert_eq!(largest, Err(Download::Status(404)));
    let over = client.download(url, "", (MAX_DOWNLOAD + 1, &to)).await;
    assert_eq!(over, Err(Download::Size));
    // HTTPS only, and no file where none can be written.
    let plain = client
        .download(b"http://pages.test/a.hyx", "", (5, &to))
        .await;
    assert_eq!(plain, fetch_failed(Kind::Invalid, 0));
    let client = downloading(vec![PACKAGE]).await;
    let nowhere = folder.path().join("no/package");
    assert_eq!(
        client.download(url, "", (5, &nowhere)).await,
        Err(Download::Write)
    );
    // A body cut short by the server.
    let cut = b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nhello";
    let client = downloading(vec![cut]).await;
    let refused = client.download(url, "", (9, &to)).await;
    assert_eq!(refused, fetch_failed(Kind::Protocol, 0));
    assert!(!to.exists());
}

#[tokio::test]
async fn a_download_stops_when_reads_stall_or_time_runs_out() {
    let folder = tempfile::tempdir().unwrap();
    let to = folder.path().join("package");
    let url = b"https://pages.test/a.hyx";
    let held = b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\nheld";
    let client = downloading(vec![held]).await;
    let quick = Duration::from_millis(300);
    let started = Instant::now();
    let window = (Duration::from_secs(20), quick);
    let stalled = client.download_within(url, "", (9, &to), window).await;
    assert_eq!(stalled, fetch_failed(Kind::Timeout, 0));
    let window = (quick, Duration::from_secs(20));
    let late = client.download_within(url, "", (9, &to), window).await;
    assert_eq!(late, fetch_failed(Kind::Timeout, 0));
    // A page's body is read within the fetch's time too.
    let mut quick_args = args();
    quick_args.timeout = quick;
    let page = client.get(b"https://pages.test/", b"", quick_args).await;
    assert_eq!(page, failed(Kind::Timeout, 0));
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(!to.exists());
    // A server that never answers runs into the same limits.
    let silent = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let table = Table::new(silent.local_addr().unwrap());
    let client = Client::new(test_roots(), table, Arc::new(()));
    let window = (Duration::from_secs(20), quick);
    let unanswered = client.download_within(url, "", (9, &to), window).await;
    assert_eq!(unanswered, fetch_failed(Kind::Timeout, 0));
    assert!(started.elapsed() < Duration::from_secs(10));
}
