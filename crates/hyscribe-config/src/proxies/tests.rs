// SPDX-License-Identifier: AGPL-3.0-or-later

use super::*;

fn ip(text: &str) -> IpAddr {
    text.parse().unwrap()
}

#[test]
fn a_single_address_matches_only_itself() {
    let one = Cidr::parse("192.0.2.1").unwrap();
    assert!(one.contains(ip("192.0.2.1")));
    assert!(!one.contains(ip("192.0.2.2")));
    let six = Cidr::parse("::1").unwrap();
    assert!(six.contains(ip("::1")));
    assert!(!six.contains(ip("::2")));
}

#[test]
fn ranges_match_by_prefix() {
    let net = Cidr::parse("10.0.0.0/8").unwrap();
    assert!(net.contains(ip("10.255.0.1")));
    assert!(!net.contains(ip("11.0.0.1")));
    let host_bits = Cidr::parse("10.1.2.3/8").unwrap();
    assert!(host_bits.contains(ip("10.9.9.9")));
    let v6 = Cidr::parse("fd00::/8").unwrap();
    assert!(v6.contains(ip("fd12::1")));
    assert!(!v6.contains(ip("fe00::1")));
    let all4 = Cidr::parse("0.0.0.0/0").unwrap();
    assert!(all4.contains(ip("203.0.113.9")));
    assert!(!all4.contains(ip("::1")));
    let all6 = Cidr::parse("::/0").unwrap();
    assert!(all6.contains(ip("2001:db8::1")));
    assert!(!all6.contains(ip("192.0.2.1")));
}

#[test]
fn a_mapped_address_counts_as_its_ipv4_address() {
    let net = Cidr::parse("127.0.0.1").unwrap();
    assert!(net.contains(ip("::ffff:127.0.0.1")));
    assert!(!Cidr::parse("::1").unwrap().contains(ip("127.0.0.1")));
}

#[test]
fn bad_entries_are_refused() {
    for entry in [
        "",
        "x",
        "10.0.0.0/33",
        "::/129",
        "10.0.0.0/-1",
        "10.0.0.0/+8",
        "10.0.0.0/",
        "10.0.0.0/8/8",
        "10.0.0.0/ 8",
        "10.0.0.0/99999999999",
        "1.2.3",
        "10.0.0.0:80",
    ] {
        assert_eq!(Cidr::parse(entry), None, "{entry:?}");
    }
}

#[test]
fn a_list_is_comma_separated_and_bounded() {
    assert_eq!(parse_list(""), Some(Vec::new()));
    assert_eq!(parse_list(" 10.0.0.0/8 ,, ::1 ").map(|l| l.len()), Some(2));
    assert_eq!(parse_list("10.0.0.0/8,nope"), None);
    let full = vec!["10.0.0.1"; MAX_PROXIES].join(",");
    assert_eq!(parse_list(&full).map(|l| l.len()), Some(MAX_PROXIES));
    let over = vec!["10.0.0.1"; MAX_PROXIES + 1].join(",");
    assert_eq!(parse_list(&over), None);
    assert_eq!(parse_list(&" ".repeat(MAX_TEXT + 1)), None);
    assert!(parse_list(&" ".repeat(MAX_TEXT)).is_some());
}

#[test]
fn loopback_is_the_default() {
    let list = default_list();
    assert_eq!(list.len(), 2);
    assert!(list.iter().any(|c| c.contains(ip("127.0.0.1"))));
    assert!(list.iter().any(|c| c.contains(ip("::1"))));
    assert!(!list.iter().any(|c| c.contains(ip("192.0.2.1"))));
}
