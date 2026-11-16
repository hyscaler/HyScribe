// SPDX-License-Identifier: AGPL-3.0-or-later

use std::net::Ipv4Addr;

use crate::address::is_public;

use super::*;

fn unhex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}

/// The golden's lines of `kind`, their fields unhexed but a validation's
/// verdict.
fn golden(kind: &str) -> Vec<Vec<Vec<u8>>> {
    include_str!("../../tests/golden/urls.tsv")
        .lines()
        .filter_map(|line| line.strip_prefix(kind)?.strip_prefix('\t'))
        .map(|line| {
            line.split('\t')
                .map(|field| match field {
                    "0" | "1" => field.as_bytes().to_vec(),
                    hex => unhex(hex),
                })
                .collect()
        })
        .collect()
}

/// The golden's `gethostbyname()` table, and the home's host, which PHP
/// does not resolve.
fn resolve(host: &str) -> Option<Ipv4Addr> {
    let host = host.trim_matches('.').to_ascii_lowercase();
    let address = match host.as_str() {
        "public.test" | "example.org" => "93.184.215.14",
        "private.test" => "10.0.0.5",
        "metadata.test" => "169.254.169.254",
        "loopback.test" => "127.0.0.1",
        literal => literal,
    };
    address.parse().ok()
}

/// Where HyScribe is stricter than `wp_http_validate_url()`: a URL without
/// a scheme, which Requests then refuses anyway.
const STRICTER: [&[u8]; 1] = [b"//public.test/"];

/// The home's own address passes `wp_http_validate_url()` whatever it is;
/// HyScribe refuses a home on an address that is not public.
#[test]
fn a_private_home_is_refused_too() {
    assert!(validate_url(b"http://127.0.0.1:8081/", b"http://127.0.0.1:8081").is_none());
    assert!(validate_url(b"http://10.0.0.1./", b"").is_none());
    assert!(validate_url(b"http://8.8.8.8/", b"").is_some());
}

#[test]
fn urls_are_validated_as_php_validates_them_and_addresses_checked() {
    let mut count = 0;
    for case in golden("validate") {
        let url = &case[0];
        let php = case[1] == b"1";
        let target = validate_url(url, b"http://example.org:8081");
        let ours = target
            .is_some_and(|target| resolve(&target.host).is_some_and(|ip| is_public(ip.into())));
        let expected = php && !STRICTER.contains(&url.as_slice());
        assert_eq!(ours, expected, "{}", String::from_utf8_lossy(url));
        count += 1;
    }
    assert_eq!(count, 56);
}

#[test]
fn relative_urls_are_made_absolute_as_php_makes_them() {
    let mut count = 0;
    for case in golden("absolute") {
        let made = make_absolute_url(&case[0], &case[1]);
        assert_eq!(
            String::from_utf8_lossy(&made),
            String::from_utf8_lossy(&case[2]),
            "{} on {}",
            String::from_utf8_lossy(&case[0]),
            String::from_utf8_lossy(&case[1])
        );
        count += 1;
    }
    assert_eq!(count, 8 * 30);
}

#[test]
fn targets_carry_what_the_request_sends() {
    let target = validate_url(b"HTTPS://Host.test:8080/a b/\xc3\xa9?x=\"1\"#frag", b"").unwrap();
    assert_eq!(
        target,
        Target {
            https: true,
            host: "Host.test".into(),
            port: 8080,
            authority: "Host.test:8080".into(),
            path: "/a%20b/%C3%A9?x=%221%22".into(),
        }
    );
    let target = validate_url(b"http://host.test", b"").unwrap();
    assert_eq!(
        (target.port, target.authority.as_str(), target.path.as_str()),
        (80, "host.test", "/")
    );
    let target = validate_url(b"https://host.test:0", b"").unwrap();
    assert_eq!((target.port, target.authority.as_str()), (443, "host.test"));
    // The home's own port is allowed on its host only.
    assert!(validate_url(b"http://home.test:9000/", b"http://home.test:9000").is_some());
    assert!(validate_url(b"http://home.test:9000/", b"http://home.test:9001").is_none());
    assert!(validate_url(b"http://other.test:9000/", b"http://home.test:9000").is_none());
    assert!(validate_url(b"http://home.test:9000/", b"").is_none());
}

#[test]
fn hostile_dot_segments_stop() {
    let started = std::time::Instant::now();
    for (open, close) in [("a/", "../"), ("a/", "x/../../"), ("ab/c/../", "../")] {
        let path = open.repeat(20_000) + &close.repeat(20_000) + "x";
        let made = make_absolute_url(path.as_bytes(), b"http://h.test/");
        assert!(made.ends_with(b"x"));
    }
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}
