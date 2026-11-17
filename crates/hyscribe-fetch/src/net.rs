// SPDX-License-Identifier: AGPL-3.0-or-later

//! Name resolution and connections, and the host's certificate store.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::pin::Pin;

use rustls::RootCertStore;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::pem::PemObject;
use tokio::net::TcpStream;

/// A boxed future, as a trait object's method returns one.
pub type Boxed<'a, T> = Pin<Box<dyn Future<Output = io::Result<T>> + Send + 'a>>;

/// How the client resolves names and connects. The client checks every
/// address resolved before it connects to one.
pub trait Net: Send + Sync {
    /// The addresses of `host` at `port`.
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> Boxed<'a, Vec<SocketAddr>>;
    /// A connection to `address`.
    fn dial(&self, address: SocketAddr) -> Boxed<'_, TcpStream>;
}

/// The system's resolver and TCP connections.
pub struct System;

impl Net for System {
    fn resolve<'a>(&'a self, host: &'a str, port: u16) -> Boxed<'a, Vec<SocketAddr>> {
        Box::pin(async move { Ok(tokio::net::lookup_host((host, port)).await?.collect()) })
    }

    fn dial(&self, address: SocketAddr) -> Boxed<'_, TcpStream> {
        Box::pin(TcpStream::connect(address))
    }
}

/// The host's certificate store, where OpenSSL finds it: its bundle file,
/// else each file in its certificate directories. Empty without one.
pub fn host_roots() -> RootCertStore {
    let probe = openssl_probe::probe();
    roots(probe.cert_file, probe.cert_dir)
}

/// The certificates of `file`, else of each file in `dirs`.
fn roots(file: Option<PathBuf>, dirs: Vec<PathBuf>) -> RootCertStore {
    let files = match file {
        Some(file) => vec![file],
        None => dirs
            .iter()
            .flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten().flatten())
            .map(|entry| entry.path())
            .collect(),
    };
    let mut store = RootCertStore::empty();
    for path in files {
        let certificates = CertificateDer::pem_file_iter(&path).into_iter().flatten();
        store.add_parsable_certificates(certificates.filter_map(Result::ok));
    }
    store
}

#[cfg(test)]
mod tests;
