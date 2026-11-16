// SPDX-License-Identifier: AGPL-3.0-or-later

//! The test site url-details is recorded against, as
//! `reference/url-details/router.php` serves it from `routes.tsv`: a local
//! listener on a thread, one connection at a time, closed after each answer.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::OnceLock;

/// The folder of the routes and pages.
fn folder() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/url-details")
}

/// The address of the test site, started on first use; `None` if no port
/// could be bound.
pub fn page_server() -> Option<SocketAddr> {
    static ADDRESS: OnceLock<Option<SocketAddr>> = OnceLock::new();
    *ADDRESS.get_or_init(|| {
        let listener = TcpListener::bind("127.0.0.1:0").ok();
        let address = listener.as_ref().and_then(|l| l.local_addr().ok());
        std::thread::spawn(move || {
            listener
                .iter()
                .flat_map(TcpListener::incoming)
                .flatten()
                .for_each(answer)
        });
        address
    })
}

/// The status, `Location` and body of the route of `path`, for the user
/// agent `agent`: 404 for a path with no route.
fn route(path: &str, agent: &str) -> (u16, Option<String>, Vec<u8>) {
    let routes = std::fs::read_to_string(folder().join("routes.tsv")).unwrap_or_default();
    let found =
        routes.lines().find_map(
            |line| match line.split('\t').collect::<Vec<_>>().as_slice() {
                [route, status, what] if *route == path => {
                    Some((status.parse().unwrap_or(500), what.to_string()))
                }
                _ => None,
            },
        );
    match found {
        None => (404, None, b"Not Found".to_vec()),
        Some((status, what)) if (300..400).contains(&status) => (status, Some(what), Vec::new()),
        Some((status, what)) if what == "-" => (status, None, Vec::new()),
        Some((status, file)) => {
            let page = std::fs::read(folder().join("pages").join(file)).unwrap_or_default();
            let page = String::from_utf8_lossy(&page).replace("%agent%", agent);
            (status, None, page.into_bytes())
        }
    }
}

/// The answer to one request, read to the end of its headers.
fn answer(mut stream: TcpStream) {
    let mut request = Vec::new();
    let mut buffer = [0; 4096];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).unwrap_or(0);
        if read == 0 {
            return;
        }
        request.extend(buffer.iter().take(read));
    }
    let request = String::from_utf8_lossy(&request).into_owned();
    let path = request.split([' ', '?']).nth(1).unwrap_or_default();
    let agent = request
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(": ")?;
            name.eq_ignore_ascii_case("user-agent").then_some(value)
        })
        .unwrap_or_default();
    let (status, location, body) = route(path, agent);
    let location = location
        .map(|to| format!("Location: {to}\r\n"))
        .unwrap_or_default();
    let head = format!(
        "HTTP/1.1 {status} Test\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n{location}\r\n",
        body.len()
    );
    let _ = stream.write_all(&[head.into_bytes(), body].concat());
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    use super::*;

    fn get(request: &str) -> String {
        let mut stream = TcpStream::connect(page_server().unwrap()).unwrap();
        stream.write_all(request.as_bytes()).unwrap();
        let mut answer = String::new();
        stream.read_to_string(&mut answer).unwrap();
        answer
    }

    #[test]
    fn serves_routes_redirects_and_404() {
        let page = get("GET /agent?x HTTP/1.1\r\nUser-Agent: Tester\r\n\r\n");
        assert!(page.starts_with("HTTP/1.1 200 Test\r\n"), "{page}");
        assert!(
            page.ends_with("<title>Tester</title></head></html>\n"),
            "{page}"
        );
        let moved = get("GET /redirect HTTP/1.1\r\n\r\n");
        assert!(moved.contains("302 Test") && moved.contains("Location: /page.html\r\n"));
        assert!(
            get("GET /empty HTTP/1.1\r\n\r\n")
                .ends_with("Content-Length: 0\r\nConnection: close\r\n\r\n")
        );
        assert!(get("GET /nope HTTP/1.1\r\n\r\n").ends_with("\r\n\r\nNot Found"));
        // A client that closes before its headers end gets nothing.
        let mut early = TcpStream::connect(page_server().unwrap()).unwrap();
        early.write_all(b"GET /").unwrap();
        early.shutdown(std::net::Shutdown::Write).unwrap();
        let mut nothing = Vec::new();
        early.read_to_end(&mut nothing).unwrap();
        assert!(nothing.is_empty());
    }
}
