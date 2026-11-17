// SPDX-License-Identifier: AGPL-3.0-or-later

//! Which addresses a fetch may connect to: public unicast only. IPv4 is
//! refused in every range `wp_http_validate_url()` refuses; IPv6 is allowed
//! in global unicast only, and an IPv4 address it carries is checked as one.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// Whether `ip` is a public address a fetch may connect to.
pub fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => public_v4(ip),
        IpAddr::V6(ip) => public_v6(ip),
    }
}

/// The ranges of `wp_http_validate_url()`: this network, private,
/// loopback, link-local (cloud metadata), CGNAT, IETF, documentation,
/// 6to4 relay, benchmarking, multicast and reserved.
fn public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(matches!(a, 0 | 10 | 127 | 224..)
        || (a == 172 && (16..=31).contains(&b))
        || (a == 192 && b == 168)
        || (a == 192 && b == 0 && matches!(c, 0 | 2))
        || (a == 192 && b == 88 && c == 99)
        || (a == 198 && b == 51 && c == 100)
        || (a == 203 && b == 0 && c == 113)
        || (a == 169 && b == 254)
        || (a == 100 && (64..=127).contains(&b))
        || (a == 198 && matches!(b, 18 | 19)))
}

/// Global unicast (`2000::/3`) outside the IETF block with Teredo
/// (`2001::/23`), documentation (`2001:db8::/32`, `3fff::/20`) and 6to4
/// (`2002::/16`); IPv4-mapped and NAT64 (`64:ff9b::/96`) addresses as the
/// IPv4 address they carry. Loopback, unique local (cloud metadata),
/// link-local and multicast fall outside `2000::/3`.
fn public_v6(ip: Ipv6Addr) -> bool {
    let s = ip.segments();
    if let Some(v4) = ip.to_ipv4_mapped() {
        return public_v4(v4);
    }
    if let [0x64, 0xff9b, 0, 0, 0, 0, high, low] = s {
        let [a, b] = high.to_be_bytes();
        let [c, d] = low.to_be_bytes();
        return public_v4(Ipv4Addr::new(a, b, c, d));
    }
    let [first, second, ..] = s;
    first & 0xe000 == 0x2000
        && !(first == 0x2001 && second < 0x0200)
        && !(first == 0x2001 && second == 0x0db8)
        && first != 0x2002
        && !(first == 0x3fff && second < 0x1000)
}

#[cfg(test)]
mod tests;
