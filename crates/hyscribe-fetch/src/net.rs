// SPDX-License-Identifier: AGPL-3.0-or-later

//! Name resolution and connections, and the host's certificate store.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
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

/// The bundle macOS ships, which the probe does not know.
const MACOS_BUNDLE: &str = "/etc/ssl/cert.pem";

/// The host's certificate store, where OpenSSL finds it: its bundle file,
/// else each file in its certificate directories. Empty without one.
pub fn host_roots() -> RootCertStore {
    let mut store = RootCertStore::empty();
    store.add_parsable_certificates(host_certificates());
    store
}

/// The certificates of [`host_roots`], each one a trust anchor, for a TLS
/// client of its own, the mail transport's (ADR 0016).
pub fn host_certificates() -> Vec<CertificateDer<'static>> {
    let probe = openssl_probe::probe();
    let file = bundle(probe.cert_file, Path::new(MACOS_BUNDLE));
    roots(file, probe.cert_dir)
}

/// The bundle the probe found, else `fallback` when it is a file.
fn bundle(found: Option<PathBuf>, fallback: &Path) -> Option<PathBuf> {
    found.or_else(|| fallback.is_file().then(|| fallback.to_path_buf()))
}

/// The certificates of `file`, else of each file in `dirs`, that are
/// trust anchors.
fn roots(file: Option<PathBuf>, dirs: Vec<PathBuf>) -> Vec<CertificateDer<'static>> {
    let files = match file {
        Some(file) => vec![file],
        None => dirs
            .iter()
            .flat_map(|dir| std::fs::read_dir(dir).into_iter().flatten().flatten())
            .map(|entry| entry.path())
            .collect(),
    };
    let certificates = files.iter().flat_map(|path| {
        let found = CertificateDer::pem_file_iter(path).into_iter().flatten();
        found.filter_map(Result::ok)
    });
    let anchor = |der: &CertificateDer<'static>| RootCertStore::empty().add(der.clone()).is_ok();
    certificates.filter(anchor).collect()
}

#[cfg(test)]
mod tests;
