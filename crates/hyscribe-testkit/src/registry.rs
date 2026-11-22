// SPDX-License-Identifier: AGPL-3.0-or-later

//! A hosted registry for tests (ADR 0024): files served over HTTPS for
//! `pages.test` with the certificate of `hyscribe-fetch`'s tests, from a
//! table a test changes as it goes (a file that is a whole HTTP answer is
//! sent as it is); any other path answers 404. One thread
//! per connection, closed after its answer.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};

use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::{ServerConfig, ServerConnection, StreamOwned};

/// The certificate authority that signed the server's certificate.
const CA: &[u8] = include_bytes!("../../hyscribe-fetch/tests/tls/ca.pem");

/// A store trusting the server's certificate authority alone.
pub fn roots() -> rustls::RootCertStore {
    let mut roots = rustls::RootCertStore::empty();
    roots.add_parsable_certificates(CertificateDer::pem_slice_iter(CA).flatten());
    roots
}

const CHAIN: &[u8] = include_bytes!("../../hyscribe-fetch/tests/tls/leaf.pem");
const KEY: &[u8] = include_bytes!("../../hyscribe-fetch/tests/tls/leaf-key.pem");

/// The files served, by path, and the heads of the requests answered.
#[derive(Default)]
struct Table {
    files: HashMap<String, Vec<u8>>,
    heads: Vec<String>,
}

type Files = Arc<Mutex<Table>>;

/// A running registry.
pub struct Served {
    files: Files,
    /// Where it listens.
    pub address: SocketAddr,
}

impl Served {
    /// A registry serving nothing yet, listening on a local port; `None`
    /// if no port could be bound.
    pub fn start() -> Option<Served> {
        let chain = CertificateDer::pem_slice_iter(CHAIN).flatten().collect();
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let config = PrivateKeyDer::from_pem_slice(KEY).ok().and_then(|key| {
            let builder = ServerConfig::builder_with_provider(provider);
            let config = builder.with_safe_default_protocol_versions();
            let config = config.and_then(|c| c.with_no_client_auth().with_single_cert(chain, key));
            config.ok().map(Arc::new)
        });
        let listener = TcpListener::bind("127.0.0.1:0").ok();
        let address = listener.as_ref().and_then(|l| l.local_addr().ok());
        let files = Files::default();
        let served = Arc::clone(&files);
        let listening = listener.zip(config);
        std::thread::spawn(move || {
            let streams = listening.iter().flat_map(|(listener, config)| {
                listener
                    .incoming()
                    .flatten()
                    .map(move |stream| (stream, config))
            });
            for (stream, config) in streams {
                let (config, files) = (Arc::clone(config), Arc::clone(&served));
                std::thread::spawn(move || answer(stream, config, &files));
            }
        });
        address.map(|address| Served { files, address })
    }

    /// Serves `bytes` at `path` (no leading `/`).
    pub fn put(&self, path: &str, bytes: impl Into<Vec<u8>>) {
        let mut table = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        table.files.insert(format!("/{path}"), bytes.into());
    }

    /// Serves nothing at `path` any more.
    pub fn remove(&self, path: &str) {
        let mut table = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        table.files.remove(&format!("/{path}"));
    }

    /// The heads of the requests answered so far, in order.
    pub fn heads(&self) -> Vec<String> {
        let table = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        table.heads.clone()
    }
}

/// Reads one request and answers its path from `files`.
fn answer(stream: TcpStream, config: Arc<ServerConfig>, files: &Files) {
    let tls = ServerConnection::new(config).ok();
    let _ = tls.map(|connection| respond(StreamOwned::new(connection, stream), files));
}

/// The answer on an open connection, once the request's head is read.
fn respond(mut tls: StreamOwned<ServerConnection, TcpStream>, files: &Files) {
    let mut request = Vec::new();
    let mut buffer = [0; 1024];
    let mut more = |request: &mut Vec<u8>| {
        let read = tls.read(&mut buffer).ok().filter(|n| *n > 0);
        let read = read.and_then(|n| buffer.get(..n));
        read.map(|bytes| request.extend_from_slice(bytes)).is_some()
    };
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && more(&mut request) {}
    let head = String::from_utf8_lossy(&request).into_owned();
    let path = head.split(' ').nth(1).unwrap_or_default().to_string();
    let mut table = files.lock().unwrap_or_else(PoisonError::into_inner);
    let found = table.files.get(&path).cloned();
    table.heads.push(head);
    drop(table);
    let (status, body) = match found {
        Some(body) => ("200 OK", body),
        None => ("404 Not Found", b"Not Found".to_vec()),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    // A file that is a whole answer is sent as it is.
    let raw = body.starts_with(b"HTTP/1.1 ");
    let head = if raw { "" } else { head.as_str() };
    let _ = tls.write_all(head.as_bytes());
    let _ = tls.write_all(&body);
    tls.conn.send_close_notify();
    let _ = tls.flush();
}
