// SPDX-License-Identifier: AGPL-3.0-or-later

//! The test site's fetches for render's tests: `pages.test:8080` is
//! `hyscribe-testkit`'s page server, as the oracle served it, and any other
//! host is refused as WordPress refuses one that does not resolve. A
//! plain HTTP/1.1 exchange per hop, redirects followed.

use std::io::{Read, Write};
use std::net::TcpStream;

use crate::outbound::Response;

/// `wp_safe_remote_get( $url )` against the test site.
pub(crate) fn get(url: &[u8], limit: Option<usize>) -> Result<Response, Vec<u8>> {
    let refused = || b"A valid URL was not provided.".to_vec();
    let mut url = String::from_utf8_lossy(url).into_owned();
    for _ in 0..=5 {
        let rest = url
            .strip_prefix("http://pages.test:8080")
            .ok_or_else(refused)?;
        let path = if rest.is_empty() { "/" } else { rest };
        let address = hyscribe_testkit::pages::page_server().ok_or_else(refused)?;
        let mut stream = TcpStream::connect(address).map_err(|_| refused())?;
        let request =
            format!("GET {path} HTTP/1.1\r\nHost: pages.test:8080\r\nUser-Agent: test\r\n\r\n");
        stream
            .write_all(request.as_bytes())
            .map_err(|_| refused())?;
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).map_err(|_| refused())?;
        let split = answer
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or_else(refused)?;
        let head = String::from_utf8_lossy(answer.get(..split).unwrap_or_default()).into_owned();
        let mut body = answer.get(split + 4..).unwrap_or_default().to_vec();
        let mut lines = head.split("\r\n");
        let status: u16 = lines
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .and_then(|code| code.parse().ok())
            .unwrap_or_default();
        let headers: Vec<(String, Vec<u8>)> = lines
            .filter_map(|line| line.split_once(": "))
            .map(|(name, value)| (name.to_ascii_lowercase(), value.as_bytes().to_vec()))
            .collect();
        let location = headers.iter().find(|(name, _)| name == "location");
        if let (300..=399, Some((_, to))) = (status, location) {
            let to = String::from_utf8_lossy(to);
            url = match to.starts_with('/') {
                true => format!("http://pages.test:8080{to}"),
                false => to.into_owned(),
            };
            continue;
        }
        body.truncate(limit.unwrap_or(usize::MAX));
        return Ok(Response {
            status,
            headers,
            body,
        });
    }
    Err(b"Too many redirects.".to_vec())
}
